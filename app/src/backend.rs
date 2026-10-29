//! Thin async layer over the engine. Everything here runs on the tokio
//! runtime; the UI awaits the results through [`run`].

use engine::{
    build_discovery_mode, resolve_relay_mode_with_fallback, AddrInfoOptions, Discoverability,
    DiscoveryConfigArg, EventEmitter, FileMetadata, FilePreviewItem, NodeService, ReceiveOptions,
    RelayConfigArg, SendOptions, SendResult,
};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use tokio::sync::mpsc::UnboundedSender;

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
});

/// Runs `fut` on the tokio runtime and resolves on whichever executor awaits it.
pub fn run<T: Send + 'static>(
    fut: impl Future<Output = T> + Send + 'static,
) -> impl Future<Output = T> {
    let handle = RUNTIME.spawn(fut);
    async move { handle.await.expect("backend task panicked") }
}

/// Fire-and-forget on the tokio runtime.
pub fn spawn(fut: impl Future<Output = impl Send + 'static> + Send + 'static) {
    RUNTIME.spawn(fut);
}

pub fn block_on<T>(fut: impl Future<Output = T>) -> T {
    RUNTIME.block_on(fut)
}

/// An engine event, forwarded verbatim to the UI thread. `tag` names the
/// transfer that produced it (0 for the device node), so events from a
/// cancelled or replaced transfer can be told apart.
pub struct Event {
    pub tag: u64,
    pub name: String,
    pub payload: Option<String>,
}

pub struct Emitter {
    pub tx: UnboundedSender<Event>,
    pub tag: u64,
}

pub fn handle(tx: &UnboundedSender<Event>, tag: u64) -> Handle {
    Some(Arc::new(Emitter {
        tx: tx.clone(),
        tag,
    }))
}

impl EventEmitter for Emitter {
    fn emit_event(&self, name: &str) -> Result<(), String> {
        self.emit(name, None)
    }

    fn emit_event_with_payload(&self, name: &str, payload: &str) -> Result<(), String> {
        self.emit(name, Some(payload.to_string()))
    }
}

impl Emitter {
    fn emit(&self, name: &str, payload: Option<String>) -> Result<(), String> {
        self.tx
            .send(Event {
                tag: self.tag,
                name: name.to_string(),
                payload,
            })
            .map_err(|e| e.to_string())
    }
}

pub type Handle = engine::AppHandle;

#[derive(Clone)]
pub struct Network {
    pub relay: Option<RelayConfigArg>,
    pub discovery: Option<DiscoveryConfigArg>,
}

pub async fn start_node(
    data_dir: PathBuf,
    network: Network,
    discoverability: Discoverability,
    handle: Handle,
) -> Result<Arc<NodeService>, String> {
    let (relay_mode, _) = resolve_relay_mode_with_fallback(network.relay).await?;
    let discovery_mode = build_discovery_mode(network.discovery)?;
    NodeService::start(
        &data_dir,
        relay_mode.into(),
        discovery_mode,
        discoverability,
        handle,
    )
    .await
    .map(Arc::new)
    .map_err(|e| format!("Failed to start device node: {e}"))
}

pub async fn reconfigure_node(node: Arc<NodeService>, network: Network) -> Result<(), String> {
    let (relay_mode, _) = resolve_relay_mode_with_fallback(network.relay).await?;
    let discovery_mode = build_discovery_mode(network.discovery)?;
    node.reconfigure_network(relay_mode.into(), discovery_mode)
        .await
        .map_err(|e| format!("Failed to update network settings: {e}"))
}

pub async fn share(
    paths: Vec<PathBuf>,
    network: Network,
    handle: Handle,
) -> Result<(SendResult, FileMetadata), String> {
    let metadata = {
        let paths = paths.clone();
        tokio::task::spawn_blocking(move || send_metadata(&paths))
            .await
            .map_err(|e| e.to_string())??
    };
    let (relay_mode, _) = resolve_relay_mode_with_fallback(network.relay).await?;
    let options = SendOptions {
        relay_mode,
        discovery_mode: build_discovery_mode(network.discovery)?,
        ticket_type: AddrInfoOptions::RelayAndAddresses,
        ..Default::default()
    };
    let result = engine::start_share_items(paths, options, &handle, Some(metadata.clone()))
        .await
        .map_err(|e| format!("Failed to start sharing: {e}"))?;
    Ok((result, metadata))
}

pub async fn stop_share(result: SendResult) {
    let router = result.router;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), router.shutdown()).await;
    router.endpoint().close().await;
}

pub async fn receive(
    ticket: String,
    output_dir: PathBuf,
    network: Network,
    handle: Handle,
    cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), String> {
    std::fs::create_dir_all(&output_dir).map_err(|e| format!("Download folder: {e}"))?;
    let (relay_mode, _) = resolve_relay_mode_with_fallback(network.relay).await?;
    let options = ReceiveOptions {
        output_dir: Some(output_dir),
        relay_mode,
        discovery_mode: build_discovery_mode(network.discovery)?,
        ..Default::default()
    };
    engine::download(ticket, options, handle, cancel)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Clears blob stores a crash or cancel left in the temp dir.
pub fn cleanup_orphaned_stores() {
    let Ok(entries) = std::fs::read_dir(engine::storage::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(engine::storage::SEND_DIR_PREFIX)
            || name.starts_with(engine::storage::RECV_DIR_PREFIX)
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn mime_of(path: &Path) -> String {
    if path.is_dir() {
        "inode/directory".into()
    } else {
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .essence_str()
            .to_string()
    }
}

fn total_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "item".into())
}

fn send_metadata(paths: &[PathBuf]) -> Result<FileMetadata, String> {
    let [first, ..] = paths else {
        return Err("Nothing to send".into());
    };
    let items: Vec<FilePreviewItem> = paths
        .iter()
        .map(|path| FilePreviewItem {
            file_name: file_name(path),
            size: total_size(path),
            thumbnail: None,
            mime_type: Some(mime_of(path)),
        })
        .collect();
    let size = items.iter().map(|i| i.size).sum();
    Ok(if paths.len() == 1 {
        FileMetadata {
            file_name: file_name(first),
            item_count: 1,
            size,
            thumbnail: None,
            mime_type: Some(mime_of(first)),
            items: None,
        }
    } else {
        FileMetadata {
            file_name: file_name(first),
            item_count: paths.len() as u32,
            size,
            thumbnail: None,
            mime_type: Some("application/x-iroh-collection".into()),
            items: Some(items),
        }
    })
}
