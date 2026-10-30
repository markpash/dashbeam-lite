//! The app's single source of truth. Engine events land here on the UI thread
//! and are folded into plain state the views render.

use crate::backend::{self, Event, Network};
use crate::settings::Settings;
use crate::updater;
use engine::{
    sanitize_folder_name, short_fingerprint, unix_now_ms, DeviceInfo, Discoverability,
    FileMetadata, NearbyDevice, NodeService, PairedDeviceInfo, PairingStatus, RelayStatusResponse,
    SendResult, TransferDirection, TransferHistoryStore, TransferPathType, TransferPeer,
    TransferRecord, TransferStatus,
};
use gpui_kit::*;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

const PAIRING_TTL_SECS: u64 = 180;

pub struct DashGlobal(pub Entity<Dash>);
impl Global for DashGlobal {}

/// Something worth telling the user about, shown as a toast in the window or
/// as a system notification when the window isn't focused.
pub struct Toast(pub SharedString);

#[derive(Clone, Copy, Default)]
pub struct Progress {
    pub bytes: u64,
    pub total: u64,
    pub speed_bps: f64,
}

impl Progress {
    fn parse(payload: Option<&str>) -> Option<Self> {
        let mut parts = payload?.split(':');
        Some(Self {
            bytes: parts.next()?.parse().ok()?,
            total: parts.next()?.parse().ok()?,
            speed_bps: parts.next()?.parse::<f64>().ok()? / 1000.0,
        })
    }

    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.bytes as f64 / self.total as f64) as f32
        }
    }

    /// Remaining time at the current speed, e.g. "2 min 30 sec left".
    pub fn eta(&self) -> Option<String> {
        let remaining = self.total.saturating_sub(self.bytes);
        if self.speed_bps <= 0.0 || remaining == 0 {
            return None;
        }
        let secs = (remaining as f64 / self.speed_bps) as u64;
        let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
        Some(match (h, m) {
            (0, 0) => format!("{s} sec left"),
            (0, _) => format!("{m} min {s} sec left"),
            _ => format!("{h} h {m} min left"),
        })
    }
}

pub enum NodeState {
    Starting,
    Ready(Arc<NodeService>),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq)]
pub enum ShareStatus {
    /// Hashing the files into the blob store.
    Preparing,
    Waiting,
    Transferring,
    Done,
    Failed,
}

pub struct Share {
    pub tag: u64,
    pub title: String,
    /// Known once the files are hashed.
    pub meta: Option<FileMetadata>,
    pub ticket: Option<String>,
    result: Option<SendResult>,
    abort: Option<tokio::task::AbortHandle>,
    started: std::time::Instant,
    pub status: ShareStatus,
    pub progress: Progress,
    peers: u32,
    record: Option<String>,
    target: Option<TransferPeer>,
    /// Device the share was offered to, and the outcome so far.
    pub invite: Option<(String, SharedString)>,
}

impl Share {
    pub fn active(&self) -> bool {
        !matches!(self.status, ShareStatus::Done | ShareStatus::Failed)
    }
}

#[derive(PartialEq)]
pub enum DownloadStatus {
    Running,
    Done,
    Failed(String),
}

pub struct Download {
    pub tag: u64,
    ticket: String,
    /// Content hash; two downloads of the same content would share a store.
    hash: String,
    sub_folder: Option<String>,
    pub name: Option<String>,
    pub status: DownloadStatus,
    pub progress: Progress,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    output_dir: PathBuf,
    record: Option<String>,
    file_names: Vec<String>,
    pub conflicts: u32,
    peer: Option<TransferPeer>,
    /// What "Show in Finder" selects once done.
    pub reveal: Option<PathBuf>,
}

#[derive(Clone)]
pub enum Invite {
    /// Files offered by a paired device, or (when `paired` is false) by an
    /// unpaired nearby device whose code must be checked first.
    Files {
        endpoint_id: String,
        sender: String,
        ticket: String,
        file_count: u32,
        total_size: u64,
        paired: bool,
    },
    Pair {
        endpoint_id: String,
        sender: String,
        device_type: String,
    },
}

impl Invite {
    pub fn endpoint_id(&self) -> &str {
        match self {
            Invite::Files { endpoint_id, .. } | Invite::Pair { endpoint_id, .. } => endpoint_id,
        }
    }

    pub fn code(&self) -> String {
        short_fingerprint(self.endpoint_id()).unwrap_or_default()
    }
}

pub struct Dash {
    pub data_dir: PathBuf,
    pub settings: Settings,
    history: Arc<TransferHistoryStore>,
    events: UnboundedSender<Event>,
    next_tag: u64,
    pub node: NodeState,
    pub network_ready: bool,
    pub device: Option<DeviceInfo>,
    pub paired: Vec<PairedDeviceInfo>,
    pub nearby: Vec<NearbyDevice>,
    pub nearby_unavailable: Option<String>,
    /// Newest first.
    pub shares: Vec<Share>,
    /// Newest first.
    pub downloads: Vec<Download>,
    /// Why the last pasted ticket didn't start.
    pub ticket_error: Option<String>,
    pub invites: Vec<Invite>,
    pub pairing_code: Option<String>,
    /// Sender side of a nearby handshake: the device asked, and our own code.
    pub verifying: Option<(String, String)>,
    /// Which relay this device reaches; `None` while a check is running.
    pub relay: Option<RelayStatusResponse>,
    pub connectivity: Option<ConnectivityReport>,
    pub connectivity_checking: bool,
    pub update: Update,
}

pub enum Update {
    None,
    Available(updater::Release),
    Installing(u64),
}

/// Result of the Connectivity tab's checks.
pub struct ConnectivityReport {
    pub net: engine::Connectivity,
    /// Whether other devices can find this one by its id; `Err` says why not.
    /// `None` while the (slower) lookup is still running.
    pub findable: Option<Result<(), String>>,
}

impl gpui_kit::EventEmitter<Toast> for Dash {}

impl Dash {
    pub fn new(data_dir: PathBuf, cx: &mut Context<Self>) -> Self {
        backend::cleanup_orphaned_stores();
        let history = Arc::new(TransferHistoryStore::new(&data_dir));
        let _ = history.mark_interrupted();
        let _ = history.forget_missing_partial_stores();
        let settings = Settings::load(&data_dir);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                if this
                    .update(cx, |this, cx| this.on_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let mut this = Self {
            data_dir,
            settings,
            history,
            events: tx,
            next_tag: 1,
            node: NodeState::Starting,
            network_ready: false,
            device: None,
            paired: Vec::new(),
            nearby: Vec::new(),
            nearby_unavailable: None,
            shares: Vec::new(),
            downloads: Vec::new(),
            ticket_error: None,
            invites: Vec::new(),
            pairing_code: None,
            verifying: None,
            relay: None,
            connectivity: None,
            connectivity_checking: false,
            update: Update::None,
        };
        this.start_node(cx);
        this.check_relay(cx);
        this.schedule_update_checks(cx);
        this
    }

    fn network(&self) -> Network {
        Network {
            relay: self.settings.relay.clone(),
            discovery: self.settings.discovery.clone(),
        }
    }

    fn tag(&mut self) -> u64 {
        self.next_tag += 1;
        self.next_tag
    }

    pub fn node(&self) -> Option<Arc<NodeService>> {
        match &self.node {
            NodeState::Ready(node) => Some(node.clone()),
            _ => None,
        }
    }

    fn toast(&self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        cx.emit(Toast(message.into()));
    }

    pub fn save_settings(&self) {
        self.settings.save(&self.data_dir);
    }

    /// Runs `fut` against the node on the backend and reports a failure as a
    /// toast. `then` sees the success value back on the UI thread.
    fn with_node<T: Send + 'static, F>(
        &self,
        cx: &mut Context<Self>,
        op: impl FnOnce(Arc<NodeService>) -> F + 'static,
        then: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) where
        F: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
    {
        let Some(node) = self.node() else {
            self.toast("Device networking is not ready yet", cx);
            return;
        };
        let task = backend::run(op(node));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(value) => then(this, value, cx),
                    Err(error) => this.toast(error.to_string(), cx),
                }
                cx.notify();
            })
        })
        .detach();
    }

    // Node lifecycle

    fn start_node(&self, cx: &mut Context<Self>) {
        let task = backend::run(backend::start_node(
            self.data_dir.clone(),
            self.network(),
            self.settings.discoverability,
            backend::handle(&self.events, 0),
        ));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(node) => {
                        this.device = Some(node.device_info());
                        this.node = NodeState::Ready(node);
                        this.refresh_devices(cx);
                    }
                    Err(error) => {
                        tracing::error!(%error, "device node failed");
                        this.node = NodeState::Failed(error);
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    pub fn shutdown(&mut self) {
        for share in self.shares.drain(..) {
            if let Some(result) = share.result {
                backend::block_on(backend::stop_share(result));
            }
        }
        if let Some(node) = self.node() {
            let _ = backend::block_on(node.shutdown());
        }
    }

    pub fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.node() else { return };
        self.paired = node.list_paired().unwrap_or_default();
        self.paired.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then_with(|| a.display_name.cmp(&b.display_name))
        });
        self.nearby_unavailable = node.nearby_unavailable_reason();
        let task = backend::run(async move { node.list_nearby().await });
        cx.spawn(async move |this, cx| {
            let nearby = task.await;
            this.update(cx, |this, cx| {
                this.nearby = nearby;
                cx.notify();
            })
        })
        .detach();
        cx.defer(crate::tray::refresh);
    }

    pub fn presence(&self) -> (usize, usize) {
        let active = self
            .paired
            .iter()
            .filter(|d| d.pairing_status == PairingStatus::Active);
        let total = active.clone().count();
        (active.filter(|d| d.online).count(), total)
    }

    fn paired_device(&self, endpoint_id: &str) -> Option<&PairedDeviceInfo> {
        self.paired.iter().find(|d| {
            d.endpoint_id.eq_ignore_ascii_case(endpoint_id)
                && d.pairing_status == PairingStatus::Active
        })
    }

    fn peer(&self, endpoint_id: &str) -> TransferPeer {
        let paired = self.paired_device(endpoint_id);
        let nearby = self
            .nearby
            .iter()
            .find(|d| d.endpoint_id.eq_ignore_ascii_case(endpoint_id));
        TransferPeer {
            endpoint_id: endpoint_id.to_string(),
            display_name: paired
                .map(|d| d.display_name.clone())
                .or_else(|| nearby.and_then(|d| d.display_name.clone())),
            device_type: paired
                .map(|d| d.device_type.clone())
                .or_else(|| nearby.and_then(|d| d.device_type.clone())),
        }
    }

    // Transfers

    /// True while anything is mid-transfer; network changes and updates wait.
    pub fn busy(&self) -> bool {
        self.active_sends() > 0 || self.active_receives() > 0
    }

    pub fn active_sends(&self) -> usize {
        self.shares.iter().filter(|s| s.active()).count()
    }

    pub fn active_receives(&self) -> usize {
        self.downloads
            .iter()
            .filter(|d| d.status == DownloadStatus::Running)
            .count()
    }

    /// Refreshes everything that summarizes transfers outside the window.
    fn transfers_changed(&self, cx: &mut Context<Self>) {
        cx.defer(crate::tray::refresh);
        cx.notify();
    }

    fn share_mut(&mut self, tag: u64) -> Option<&mut Share> {
        self.shares.iter_mut().find(|s| s.tag == tag)
    }

    fn download_mut(&mut self, tag: u64) -> Option<&mut Download> {
        self.downloads.iter_mut().find(|d| d.tag == tag)
    }

    // Sending

    pub fn start_share(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let tag = self.tag();
        let names: Vec<String> = paths
            .iter()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        let (abort, task) = backend::run_abortable(backend::share(
            paths,
            self.network(),
            backend::handle(&self.events, tag),
        ));
        self.shares.insert(
            0,
            Share {
                tag,
                title: summarize(&names),
                meta: None,
                ticket: None,
                result: None,
                abort: Some(abort),
                started: std::time::Instant::now(),
                status: ShareStatus::Preparing,
                progress: Progress::default(),
                peers: 0,
                record: None,
                target: None,
                invite: None,
            },
        );
        self.transfers_changed(cx);
        cx.spawn(async move |this, cx| {
            let Some(result) = task.await else { return };
            this.update(cx, |this, cx| {
                let Some(share) = this.share_mut(tag) else {
                    // Cancelled just as it finished.
                    if let Ok((result, _)) = result {
                        backend::spawn(backend::stop_share(result));
                    }
                    return;
                };
                match result {
                    Ok((result, meta)) => {
                        share.ticket = Some(result.ticket.clone());
                        share.title = meta_title(&meta);
                        share.meta = Some(meta);
                        share.result = Some(result);
                        share.abort = None;
                        share.status = ShareStatus::Waiting;
                    }
                    Err(error) => {
                        this.shares.retain(|s| s.tag != tag);
                        this.toast(error, cx);
                    }
                }
                this.transfers_changed(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Stops a share (cancelling it while it still prepares) and removes it.
    pub fn stop_share(&mut self, tag: u64, cx: &mut Context<Self>) {
        let Some(index) = self.shares.iter().position(|s| s.tag == tag) else {
            return;
        };
        let mut share = self.shares.remove(index);
        if let Some(abort) = share.abort.take() {
            abort.abort();
        }
        if let Some(id) = share.record.take() {
            if share.active() {
                finish_record(&self.history, &id, TransferStatus::Cancelled, None, None);
            }
        }
        if let Some(result) = share.result.take() {
            backend::spawn(backend::stop_share(result));
        }
        let target = share.invite.map(|(id, _)| id);
        if self
            .verifying
            .as_ref()
            .is_some_and(|(id, _)| Some(id) == target.as_ref())
        {
            self.verifying = None;
        }
        self.transfers_changed(cx);
    }

    /// Offers a share to a paired or nearby device.
    pub fn send_to(&mut self, tag: u64, endpoint_id: String, cx: &mut Context<Self>) {
        let peer = self.peer(&endpoint_id);
        let paired = self.paired_device(&endpoint_id).is_some();
        let Some(share) = self.share_mut(tag) else {
            return;
        };
        let (Some(ticket), Some(meta)) = (share.ticket.clone(), share.meta.as_ref()) else {
            return;
        };
        let (count, size) = (meta.item_count, meta.size);
        share.target = Some(peer);
        share.invite = Some((endpoint_id.clone(), "Sending…".into()));
        cx.notify();
        let id = endpoint_id.clone();
        self.with_node(
            cx,
            move |node| async move {
                if paired {
                    node.invite_paired_device(&id, &ticket, count, size).await
                } else {
                    node.invite_nearby_device(&id, &ticket, count, size).await
                }
            },
            move |this, delivered, _cx| {
                if let Some(share) = this.share_mut(tag) {
                    share.invite = Some((
                        endpoint_id.clone(),
                        if delivered {
                            "Waiting for them to accept…"
                        } else {
                            "Device unreachable"
                        }
                        .into(),
                    ));
                }
                if delivered && !paired {
                    this.start_verifying(endpoint_id);
                }
            },
        );
    }

    fn start_verifying(&mut self, endpoint_id: String) {
        let code = self
            .device
            .as_ref()
            .and_then(|d| short_fingerprint(&d.endpoint_id))
            .unwrap_or_default();
        self.verifying = Some((endpoint_id, code));
    }

    // Receiving

    /// Starts a download. `sender` names the peer for history when known from
    /// an invite; tickets only carry a per-share key. Returns whether it
    /// started.
    pub fn receive(
        &mut self,
        ticket: String,
        sub_folder: Option<String>,
        sender: Option<TransferPeer>,
        cx: &mut Context<Self>,
    ) -> bool {
        let ticket = ticket.trim().to_string();
        let ticket = ticket
            .split_once("ticket=")
            .map(|(_, t)| t.split('&').next().unwrap_or(t).to_string())
            .unwrap_or(ticket);
        if ticket.is_empty() {
            return false;
        }
        let parsed = match ticket.parse::<iroh_blobs::ticket::BlobTicket>() {
            Ok(parsed) => parsed,
            Err(error) => {
                self.ticket_error = Some(format!("Invalid ticket: {error}"));
                cx.notify();
                return false;
            }
        };
        let hash = parsed.hash().to_hex().to_string();
        if self
            .downloads
            .iter()
            .any(|d| d.hash == hash && d.status == DownloadStatus::Running)
        {
            self.ticket_error = Some("That ticket is already downloading.".into());
            cx.notify();
            return false;
        }
        self.ticket_error = None;
        let sender_id = parsed.addr().id.to_string();
        let mut output_dir = self.settings.download_dir();
        if let Some(name) = &sub_folder {
            output_dir = output_dir.join(sanitize_folder_name(name, &sender_id[..12]));
        }
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let tag = self.tag();
        let task = backend::run(backend::receive(
            ticket.clone(),
            output_dir.clone(),
            self.network(),
            backend::handle(&self.events, tag),
            cancel_rx,
        ));
        self.downloads.insert(
            0,
            Download {
                tag,
                ticket,
                hash,
                sub_folder,
                name: None,
                status: DownloadStatus::Running,
                progress: Progress::default(),
                cancel: Some(cancel_tx),
                output_dir,
                record: None,
                file_names: Vec::new(),
                conflicts: 0,
                peer: Some(sender.unwrap_or_else(|| self.peer(&sender_id))),
                reveal: None,
            },
        );
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| this.receive_finished(tag, result, cx))
        })
        .detach();
        self.transfers_changed(cx);
        true
    }

    pub fn cancel_download(&mut self, tag: u64, cx: &mut Context<Self>) {
        if let Some(cancel) = self.download_mut(tag).and_then(|d| d.cancel.take()) {
            let _ = cancel.send(());
        }
        cx.notify();
    }

    pub fn dismiss_download(&mut self, tag: u64, cx: &mut Context<Self>) {
        self.downloads
            .retain(|d| d.tag != tag || d.status == DownloadStatus::Running);
        cx.notify();
    }

    pub fn retry_download(&mut self, tag: u64, cx: &mut Context<Self>) {
        let Some(index) = self
            .downloads
            .iter()
            .position(|d| d.tag == tag && matches!(d.status, DownloadStatus::Failed(_)))
        else {
            return;
        };
        let old = self.downloads.remove(index);
        self.receive(old.ticket, old.sub_folder, old.peer, cx);
    }

    /// Drops finished items from the Send (`sends`) or Receive list.
    pub fn clear_finished(&mut self, sends: bool, cx: &mut Context<Self>) {
        if sends {
            let done: Vec<u64> = self
                .shares
                .iter()
                .filter(|s| !s.active())
                .map(|s| s.tag)
                .collect();
            for tag in done {
                self.stop_share(tag, cx);
            }
        } else {
            self.downloads
                .retain(|d| d.status == DownloadStatus::Running);
        }
        cx.notify();
    }

    fn receive_finished(&mut self, tag: u64, result: Result<(), String>, cx: &mut Context<Self>) {
        let history = self.history.clone();
        let Some(download) = self.download_mut(tag) else {
            return;
        };
        let record = download.record.take();
        download.cancel = None;
        match result {
            Ok(()) => {
                let name = download
                    .name
                    .clone()
                    .unwrap_or_else(|| summarize(&download.file_names));
                download.reveal = Some(reveal_path(&download.output_dir, &download.file_names));
                download.name = Some(name.clone());
                download.status = DownloadStatus::Done;
                // Normally `receive-completed` closed the row already.
                if let Some(id) = record {
                    finish_record(&history, &id, TransferStatus::Completed, None, None);
                }
                cx.emit(Toast(format!("Received {name}").into()));
            }
            Err(error) if error == "cancelled" => {
                if let Some(id) = record {
                    finish_record(&history, &id, TransferStatus::Cancelled, None, None);
                }
                self.downloads.retain(|d| d.tag != tag);
            }
            Err(error) => {
                if let Some(id) = record {
                    finish_record(
                        &history,
                        &id,
                        TransferStatus::Failed,
                        None,
                        Some(error.clone()),
                    );
                }
                download.status = DownloadStatus::Failed(error);
            }
        }
        self.transfers_changed(cx);
    }

    // Invites

    pub fn answer_invite(&mut self, endpoint_id: &str, accept: bool, cx: &mut Context<Self>) {
        let Some(index) = self
            .invites
            .iter()
            .position(|i| i.endpoint_id().eq_ignore_ascii_case(endpoint_id))
        else {
            return;
        };
        let invite = self.invites.remove(index);
        cx.dismiss_system_notification(&format!("invite:{endpoint_id}"));
        let id = endpoint_id.to_string();
        match invite.clone() {
            Invite::Files { paired: true, .. } => self.with_node(
                cx,
                move |node| async move { node.respond_paired_invite(&id, accept).await },
                |_, _, _| {},
            ),
            _ => self.with_node(
                cx,
                move |node| async move {
                    if accept {
                        node.accept_nearby_invite(&id).await
                    } else {
                        node.decline_nearby_invite(&id, false).await
                    }
                },
                |this, _, cx| this.refresh_devices(cx),
            ),
        }
        if let (true, Invite::Files { ticket, .. }) = (accept, invite) {
            let sender = self.peer(endpoint_id);
            self.receive(ticket, None, Some(sender), cx);
        }
        cx.notify();
    }

    fn on_invite(&mut self, invite: Invite, cx: &mut Context<Self>) {
        let trusted = self
            .paired_device(invite.endpoint_id())
            .filter(|d| d.trusted)
            .map(|d| d.display_name.clone());
        if let (
            Some(folder),
            Invite::Files {
                endpoint_id,
                ticket,
                sender,
                ..
            },
        ) = (trusted, &invite)
        {
            // Trusted devices skip the prompt and file under their own folder.
            let id = endpoint_id.clone();
            self.with_node(
                cx,
                move |node| async move { node.respond_paired_invite(&id, true).await },
                |_, _, _| {},
            );
            self.toast(format!("Receiving from {sender}"), cx);
            let peer = self.peer(endpoint_id);
            self.receive(ticket.clone(), Some(folder), Some(peer), cx);
            return;
        }
        let (title, body) = match &invite {
            Invite::Files {
                sender,
                file_count,
                total_size,
                ..
            } => (
                format!("{sender} wants to send you files"),
                format!("{} · {}", items(*file_count), format_size(*total_size)),
            ),
            Invite::Pair { sender, .. } => (
                format!("{sender} wants to pair"),
                format!("Code {}", invite.code()),
            ),
        };
        cx.show_system_notification(SystemNotification {
            tag: format!("invite:{}", invite.endpoint_id()).into(),
            title: title.into(),
            body: body.into(),
            actions: vec![
                SystemNotificationAction {
                    id: "accept".into(),
                    label: "Accept".into(),
                },
                SystemNotificationAction {
                    id: "decline".into(),
                    label: "Decline".into(),
                },
            ],
        });
        self.invites
            .retain(|i| !i.endpoint_id().eq_ignore_ascii_case(invite.endpoint_id()));
        self.invites.push(invite);
    }

    // Pairing

    pub fn open_pairing(&mut self, cx: &mut Context<Self>) {
        self.with_node(
            cx,
            |node| async move { node.start_pairing_host(Some(PAIRING_TTL_SECS)).await },
            |this, code, _| this.pairing_code = Some(code),
        );
    }

    pub fn close_pairing(&mut self, cx: &mut Context<Self>) {
        self.pairing_code = None;
        if let Some(node) = self.node() {
            backend::spawn(async move { node.stop_pairing_host().await });
        }
        cx.notify();
    }

    pub fn join_pairing(&mut self, code: String, cx: &mut Context<Self>) {
        let code = code.trim().to_string();
        if code.is_empty() {
            return;
        }
        self.with_node(
            cx,
            move |node| async move { node.join_pairing(&code).await },
            |this, _, cx| this.refresh_devices(cx),
        );
    }

    pub fn pair_nearby(&mut self, endpoint_id: String, cx: &mut Context<Self>) {
        let id = endpoint_id.clone();
        self.with_node(
            cx,
            move |node| async move { node.request_nearby_pair(&id).await },
            move |this, delivered, cx| {
                if delivered {
                    this.start_verifying(endpoint_id);
                } else {
                    this.toast("Device unreachable", cx);
                }
            },
        );
    }

    pub fn trust(&mut self, endpoint_id: String, trust: bool, cx: &mut Context<Self>) {
        self.with_node(
            cx,
            move |node| async move { node.trust_paired(&endpoint_id, trust) },
            |this, _, cx| this.refresh_devices(cx),
        );
    }

    pub fn rename_paired(&mut self, endpoint_id: String, name: String, cx: &mut Context<Self>) {
        self.with_node(
            cx,
            move |node| async move { node.rename_paired(&endpoint_id, &name) },
            |this, _, cx| this.refresh_devices(cx),
        );
    }

    pub fn forget(&mut self, endpoint_id: String, cx: &mut Context<Self>) {
        self.with_node(
            cx,
            move |node| async move { node.forget_paired(&endpoint_id).await },
            |this, _, cx| this.refresh_devices(cx),
        );
    }

    pub fn rename_device(&mut self, name: String, cx: &mut Context<Self>) {
        self.with_node(
            cx,
            move |node| async move { node.set_device_display_name(&name) },
            |this, info, _| this.device = Some(info),
        );
    }

    // Settings

    pub fn set_discoverability(&mut self, setting: Discoverability, cx: &mut Context<Self>) {
        self.with_node(
            cx,
            move |node| async move { node.set_discoverability(setting).await },
            move |this, _, cx| {
                this.settings.discoverability = setting;
                this.save_settings();
                this.refresh_devices(cx);
            },
        );
    }

    pub fn apply_network(&mut self, settings: Settings, cx: &mut Context<Self>) {
        if self.busy() {
            self.toast("Finish the current transfer first", cx);
            return;
        }
        let network = Network {
            relay: settings.relay.clone(),
            discovery: settings.discovery.clone(),
        };
        let Some(node) = self.node() else { return };
        let task = backend::run(backend::reconfigure_node(node, network));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.settings.relay = settings.relay;
                    this.settings.discovery = settings.discovery;
                    this.save_settings();
                    this.toast("Network settings applied", cx);
                }
                Err(error) => this.toast(error, cx),
            })
        })
        .detach();
    }

    pub fn check_connectivity(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.node() else { return };
        if self.connectivity_checking {
            return;
        }
        self.connectivity_checking = true;
        self.check_relay(cx);
        let discovery = self.settings.discovery.clone();
        let snapshot = backend::run({
            let node = node.clone();
            async move { node.connectivity().await }
        });
        let lookup = backend::run(async move {
            match discovery.filter(|d| d.mode == "custom") {
                Some(d) if d.dns_origin.is_none() => engine::verify_discovery(d).await.map(|_| ()),
                Some(d) => node
                    .lookup_self(d.dns_origin)
                    .await
                    .map_err(|e| e.to_string()),
                None => node.lookup_self(None).await.map_err(|e| e.to_string()),
            }
        });
        cx.spawn(async move |this, cx| {
            let net = snapshot.await;
            this.update(cx, |this, cx| {
                this.connectivity = Some(ConnectivityReport {
                    net,
                    findable: None,
                });
                cx.notify();
            })?;
            let findable = lookup.await;
            this.update(cx, |this, cx| {
                if let Some(report) = this.connectivity.as_mut() {
                    report.findable = Some(findable);
                }
                this.connectivity_checking = false;
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    // Updates

    fn schedule_update_checks(&self, cx: &mut Context<Self>) {
        if !updater::enabled() {
            return;
        }
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(10))
                .await;
            let found = backend::run(updater::check()).await;
            if this
                .update(cx, |this, cx| this.found_update(found, false, cx))
                .is_err()
            {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_secs(6 * 60 * 60))
                .await;
        })
        .detach();
    }

    pub fn check_for_update(&mut self, cx: &mut Context<Self>) {
        let task = backend::run(updater::check());
        cx.spawn(async move |this, cx| {
            let found = task.await;
            this.update(cx, |this, cx| this.found_update(found, true, cx))
        })
        .detach();
    }

    fn found_update(
        &mut self,
        found: Result<Option<updater::Release>, String>,
        manual: bool,
        cx: &mut Context<Self>,
    ) {
        match found {
            Ok(Some(release)) if !matches!(self.update, Update::Installing(_)) => {
                self.update = Update::Available(release);
                cx.notify();
            }
            Ok(_) if manual => self.toast("You're on the latest build", cx),
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "update check failed");
                if manual {
                    self.toast(format!("Update check failed: {error}"), cx);
                }
            }
        }
    }

    pub fn install_update(&mut self, cx: &mut Context<Self>) {
        let Update::Available(release) = &self.update else {
            return;
        };
        if self.busy() {
            self.toast("Finish the current transfer first", cx);
            return;
        }
        let release = release.clone();
        self.update = Update::Installing(release.build);
        let task = backend::run(updater::install(release.clone()));
        cx.spawn(async move |this, cx| match task.await {
            Ok(bundle) => {
                updater::relaunch(&bundle);
                cx.update(|cx| cx.quit());
            }
            Err(error) => {
                let _ = this.update(cx, |this, cx| {
                    this.update = Update::Available(release);
                    this.toast(format!("Update failed: {error}"), cx);
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    pub fn check_relay(&mut self, cx: &mut Context<Self>) {
        self.relay = None;
        let task = backend::run(engine::get_relay_status(self.settings.relay.clone()));
        cx.spawn(async move |this, cx| {
            let status = task.await.unwrap_or_else(|_| RelayStatusResponse {
                kind: "unavailable".into(),
                url: None,
                connected: false,
                fell_back_to_public: false,
            });
            this.update(cx, |this, cx| {
                this.relay = Some(status);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    // History

    pub fn history(&self) -> Vec<TransferRecord> {
        self.history.list().unwrap_or_default()
    }

    pub fn delete_record(&mut self, id: &str, cx: &mut Context<Self>) {
        let _ = self.history.delete(id);
        cx.notify();
    }

    pub fn clear_history(&mut self, cx: &mut Context<Self>) {
        let _ = self.history.clear();
        cx.notify();
    }
}

fn finish_record(
    history: &TransferHistoryStore,
    id: &str,
    status: TransferStatus,
    done: Option<Completion>,
    error: Option<String>,
) {
    let _ = history.update(id, |r| {
        r.status = status;
        r.ended_at = Some(unix_now_ms());
        r.error = error;
        if let Some(done) = done {
            r.duration_ms = done.duration_ms;
            r.export_ms = done.export_ms;
            if let Some(bytes) = done.bytes {
                r.payload_bytes = bytes;
                r.bytes_transferred = bytes;
            }
            r.avg_speed_bps = done
                .duration_ms
                .filter(|ms| *ms > 0)
                .map(|ms| r.payload_bytes as f64 * 1000.0 / ms as f64);
            r.resumable_store_path = None;
        }
    });
}

impl Dash {
    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let payload = event.payload.as_deref();
        if matches!(
            event.name.as_str(),
            "transfer-completed" | "transfer-failed"
        ) {
            cx.defer(crate::tray::refresh);
        }
        match event.name.as_str() {
            "import-progress" => {
                let progress = Progress::parse(payload);
                let share = self
                    .share_mut(event.tag)
                    .filter(|s| s.status == ShareStatus::Preparing);
                match (share, progress) {
                    (Some(share), Some(mut progress)) => {
                        let secs = share.started.elapsed().as_secs_f64();
                        if secs > 0.0 {
                            progress.speed_bps = progress.bytes as f64 / secs;
                        }
                        share.progress = progress;
                    }
                    _ => return,
                }
            }
            "transfer-started"
            | "transfer-progress"
            | "transfer-completed"
            | "transfer-failed"
            | "share-peer-connected" => self.on_share_event(event.tag, &event.name, payload, cx),
            "receive-started"
            | "receive-progress"
            | "receive-file-metadata"
            | "receive-file-names"
            | "receive-conflicts"
            | "receive-completed" => self.on_receive_event(event.tag, &event.name, payload),
            "device-node-network-ready" => self.network_ready = true,
            "device-node-network-warming" => self.network_ready = false,
            "device-paired" => {
                let name = json::<Named>(payload).and_then(|n| n.display_name);
                self.toast(
                    format!("Paired with {}", name.as_deref().unwrap_or("a device")),
                    cx,
                );
                self.pairing_code = None;
                self.verifying = None;
                self.refresh_devices(cx);
            }
            "device-unpaired" => {
                if let Some(d) = json::<Unpaired>(payload).filter(|u| u.reason == "remote") {
                    let name = d.display_name.unwrap_or_else(|| "A device".into());
                    self.toast(format!("{name} unpaired this device"), cx);
                }
                self.refresh_devices(cx);
            }
            "paired-device-presence"
            | "identity-rotated"
            | "nearby-device-found"
            | "nearby-device-identified"
            | "nearby-device-lost" => self.refresh_devices(cx),
            "nearby-unavailable" => {
                self.nearby_unavailable = json::<Reason>(payload).and_then(|r| r.reason);
            }
            "pairing-host-expired" => self.pairing_code = None,
            "paired-invite-response" => {
                if let Some(r) = json::<InviteResponse>(payload) {
                    if self
                        .verifying
                        .as_ref()
                        .is_some_and(|(id, _)| id.eq_ignore_ascii_case(&r.endpoint_id))
                    {
                        self.verifying = None;
                    }
                    let name = r.display_name.unwrap_or_else(|| "The device".into());
                    let text = if r.response == "accepted" {
                        format!("{name} accepted")
                    } else {
                        format!("{name} declined")
                    };
                    let offered = self.shares.iter_mut().find(|s| {
                        s.invite
                            .as_ref()
                            .is_some_and(|(id, _)| id.eq_ignore_ascii_case(&r.endpoint_id))
                    });
                    if let Some(share) = offered {
                        share.invite = Some((r.endpoint_id, text.into()));
                    }
                }
            }
            "paired-invite-received" => {
                if let Some(i) = json::<IncomingFiles>(payload) {
                    let paired = self.paired_device(&i.remote_endpoint_id).is_some();
                    self.on_invite(
                        Invite::Files {
                            endpoint_id: i.remote_endpoint_id,
                            sender: i.sender_name,
                            ticket: i.blob_ticket,
                            file_count: i.file_count,
                            total_size: i.total_size,
                            paired,
                        },
                        cx,
                    );
                }
            }
            "nearby-pair-request-received" => {
                if let Some(r) = json::<PairRequest>(payload) {
                    self.on_invite(
                        Invite::Pair {
                            endpoint_id: r.remote_endpoint_id,
                            sender: r.sender_name,
                            device_type: r.device_type,
                        },
                        cx,
                    );
                }
            }
            _ => return,
        }
        cx.notify();
    }

    fn on_share_event(
        &mut self,
        tag: u64,
        name: &str,
        payload: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let history = &self.history;
        let Some(share) = self.shares.iter_mut().find(|s| s.tag == tag) else {
            return;
        };
        match name {
            "share-peer-connected" => share.peers += 1,
            "transfer-started" => {
                share.status = ShareStatus::Transferring;
                if let (None, Some(meta)) = (&share.record, &share.meta) {
                    let mut record = TransferRecord::new(
                        TransferDirection::Send,
                        meta.file_name.clone(),
                        meta.size,
                    );
                    record.item_count = meta.item_count;
                    record.path_type = Some(match meta.mime_type.as_deref() {
                        Some("inode/directory") => TransferPathType::Directory,
                        _ => TransferPathType::File,
                    });
                    record.peer = share.target.clone();
                    share.record = history.open(record).ok();
                }
            }
            "transfer-progress" => {
                if let Some(progress) = Progress::parse(payload) {
                    share.progress = progress;
                }
            }
            "transfer-completed" => {
                share.status = ShareStatus::Done;
                if let Some(id) = share.record.take() {
                    let peers = share.peers.max(1);
                    let _ = history.update(&id, |r| r.peer_count = peers);
                    finish_record(history, &id, TransferStatus::Completed, json(payload), None);
                }
                cx.emit(Toast(format!("Sent {}", share.title).into()));
            }
            "transfer-failed" => {
                share.status = ShareStatus::Failed;
                if let Some(id) = share.record.take() {
                    finish_record(history, &id, TransferStatus::Failed, None, None);
                }
            }
            _ => {}
        }
    }

    fn on_receive_event(&mut self, tag: u64, name: &str, payload: Option<&str>) {
        let history = &self.history;
        let Some(Download {
            name: file_name,
            progress,
            output_dir,
            record,
            file_names,
            conflicts,
            peer,
            ..
        }) = self
            .downloads
            .iter_mut()
            .find(|d| d.tag == tag && d.status == DownloadStatus::Running)
        else {
            return;
        };
        match name {
            "receive-file-metadata" => {
                if let Some(meta) = json::<FileMetadata>(payload) {
                    *file_name = Some(meta.file_name);
                }
            }
            "receive-progress" => {
                if let Some(p) = Progress::parse(payload) {
                    *progress = p;
                }
            }
            "receive-file-names" => {
                *file_names = json(payload).unwrap_or_default();
            }
            "receive-conflicts" => {
                *conflicts = json::<Vec<serde_json::Value>>(payload).map_or(0, |c| c.len() as u32);
            }
            "receive-started" if record.is_none() => {
                let mut r = TransferRecord::new(
                    TransferDirection::Receive,
                    file_name.clone().unwrap_or_default(),
                    progress.total,
                );
                r.save_path = Some(output_dir.to_string_lossy().into_owned());
                r.peer = peer.clone();
                *record = history.open(r).ok();
            }
            "receive-completed" => {
                if let Some(id) = record.take() {
                    let names = std::mem::take(file_names);
                    let summary = summarize(&names);
                    let count = *conflicts;
                    let done = json::<Completion>(payload);
                    if let Some(dir) = done.as_ref().and_then(|d| d.output_dir.clone()) {
                        *output_dir = dir;
                    }
                    *file_names = names.clone();
                    let _ = history.update(&id, |r| {
                        r.root_name = summary;
                        r.item_count = top_level(&names).len() as u32;
                        r.conflict_count = count;
                        r.set_file_names(names);
                    });
                    finish_record(history, &id, TransferStatus::Completed, done, None);
                }
            }
            _ => {}
        }
    }
}

fn json<T: for<'de> Deserialize<'de>>(payload: Option<&str>) -> Option<T> {
    serde_json::from_str(payload?).ok()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Completion {
    duration_ms: Option<u64>,
    export_ms: Option<u64>,
    bytes: Option<u64>,
    output_dir: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Named {
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct Unpaired {
    display_name: Option<String>,
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
struct Reason {
    reason: Option<String>,
}

#[derive(Deserialize)]
struct InviteResponse {
    endpoint_id: String,
    display_name: Option<String>,
    response: String,
}

#[derive(Deserialize)]
struct IncomingFiles {
    blob_ticket: String,
    file_count: u32,
    total_size: u64,
    sender_name: String,
    remote_endpoint_id: String,
}

#[derive(Deserialize)]
struct PairRequest {
    remote_endpoint_id: String,
    sender_name: String,
    device_type: String,
}

/// "photo.jpg", or "photo.jpg and 3 more" for a multi-item share.
fn meta_title(meta: &FileMetadata) -> String {
    match meta.item_count.saturating_sub(1) {
        0 => meta.file_name.clone(),
        more => format!("{} and {more} more", meta.file_name),
    }
}

fn top_level(file_names: &[String]) -> Vec<&str> {
    let mut roots: Vec<&str> = file_names
        .iter()
        .filter_map(|n| n.split('/').next())
        .collect();
    roots.dedup();
    roots
}

fn summarize(file_names: &[String]) -> String {
    match top_level(file_names).as_slice() {
        [] => "files".into(),
        [one] => one.to_string(),
        [first, rest @ ..] => format!("{first} and {} more", rest.len()),
    }
}

/// What "Show in Finder" should select: the single top-level item, or the
/// folder everything landed in.
fn reveal_path(output_dir: &std::path::Path, file_names: &[String]) -> PathBuf {
    match top_level(file_names).as_slice() {
        [one] => output_dir.join(one),
        _ => output_dir.to_path_buf(),
    }
}

pub fn items(count: u32) -> String {
    if count == 1 {
        "1 item".into()
    } else {
        format!("{count} items")
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
