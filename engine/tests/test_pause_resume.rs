mod common;

use common::{MockEventEmitter, TestFixture};
use engine::{download, download_to_store, start_share, ReceiveOptions};
use iroh::endpoint::{presets, Connection, RelayMode};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr};
use iroh_blobs::format::collection::Collection;
use iroh_blobs::store::mem::MemStore;
use iroh_blobs::ticket::BlobTicket;
use iroh_blobs::{BlobFormat, BlobsProtocol};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A paused share holds back every byte, keeps the receiver connected, and
/// lets the download finish once resumed.
#[tokio::test]
async fn e2e_paused_share_holds_data_until_resumed() {
    let fixture = TestFixture::new();
    let content = vec![7u8; 2 * 1024 * 1024];
    let source = fixture.create_file("paused.bin", &content);
    let recv_dir = fixture.output_dir();

    let share = start_share(source, common::send_options(), None, None)
        .await
        .unwrap();
    share.pause.set_paused(true);

    let emitter = MockEventEmitter::new();
    let (_cancel_tx, cancel_rx) = common::no_cancel();
    let recv_task = tokio::spawn(download(
        share.ticket.clone(),
        ReceiveOptions {
            output_dir: Some(recv_dir.clone()),
            ..Default::default()
        },
        Some(emitter.clone()),
        cancel_rx,
    ));

    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(!recv_task.is_finished(), "a paused share must not complete");
    assert!(!recv_dir.join("paused.bin").exists());
    assert!(
        !emitter.has_event("receive-reconnecting"),
        "pausing must not drop the connection"
    );

    share.pause.set_paused(false);
    tokio::time::timeout(Duration::from_secs(60), recv_task)
        .await
        .expect("download should finish after resuming")
        .expect("receive task panicked")
        .expect("download should succeed");
    assert_eq!(std::fs::read(recv_dir.join("paused.bin")).unwrap(), content);
    assert!(!emitter.has_event("receive-reconnecting"));

    drop(share);
}

/// Serves blobs, but closes the first connection shortly after accepting it,
/// as a network drop would.
#[derive(Debug, Clone)]
struct DropFirstConnection {
    blobs: BlobsProtocol,
    dropped: Arc<AtomicBool>,
}

impl ProtocolHandler for DropFirstConnection {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        if !self.dropped.swap(true, Ordering::SeqCst) {
            // Late enough that the receiver counts as connected.
            tokio::time::sleep(Duration::from_millis(200)).await;
            connection.close(1u32.into(), b"simulated drop");
            return Ok(());
        }
        self.blobs.accept(connection).await
    }
}

async fn local_endpoint(alpns: Vec<Vec<u8>>) -> Endpoint {
    Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .alpns(alpns)
        .bind()
        .await
        .expect("bind endpoint")
}

fn loopback_addr(endpoint: &Endpoint) -> EndpointAddr {
    let port = endpoint
        .bound_sockets()
        .into_iter()
        .find(SocketAddr::is_ipv4)
        .expect("an IPv4 socket")
        .port();
    EndpointAddr::new(endpoint.id()).with_ip_addr(SocketAddr::from(([127, 0, 0, 1], port)))
}

/// When the connection drops after the sender was reached, the download
/// reconnects on its own and finishes.
#[tokio::test]
async fn download_reconnects_after_connection_drop() {
    let content = vec![42u8; 512 * 1024];
    let sender_store = MemStore::new();
    let blob = sender_store.add_bytes(content.clone()).await.unwrap();
    let collection: Collection = [("data.bin".to_string(), blob.hash)].into_iter().collect();
    let tag = collection.store(&sender_store).await.unwrap();

    let sender = local_endpoint(vec![iroh_blobs::ALPN.to_vec()]).await;
    let addr = loopback_addr(&sender);
    let router = Router::builder(sender)
        .accept(
            iroh_blobs::ALPN,
            DropFirstConnection {
                blobs: BlobsProtocol::new(&sender_store, None),
                dropped: Arc::new(AtomicBool::new(false)),
            },
        )
        .spawn();

    let receiver = local_endpoint(vec![]).await;
    let receiver_store = MemStore::new();
    let ticket = BlobTicket::new(addr.clone(), tag.hash(), BlobFormat::HashSeq);
    let emitter = MockEventEmitter::new();
    let app_handle: engine::AppHandle = Some(emitter.clone());

    let result = tokio::time::timeout(
        Duration::from_secs(60),
        download_to_store(ticket, addr, &receiver, &receiver_store, &app_handle),
    )
    .await
    .expect("download should finish")
    .expect("download should reconnect and succeed");

    assert_eq!(result.payload_size, content.len() as u64);
    assert!(emitter.has_event("receive-reconnecting"));
    assert!(emitter.has_event("receive-reconnected"));
    let names = emitter.event_names();
    let reconnecting = names.iter().position(|n| n == "receive-reconnecting");
    let reconnected = names.iter().position(|n| n == "receive-reconnected");
    assert!(reconnecting < reconnected, "events out of order: {names:?}");

    router.shutdown().await.ok();
    receiver.close().await;
}

/// A pause longer than QUIC's idle timeout still doesn't drop the receiver:
/// keep-alives hold the connection open. Slow, so it's not run in CI:
///   cargo test -p engine --test test_pause_resume long_pause -- --ignored
#[tokio::test]
#[ignore = "takes about a minute; run manually"]
async fn e2e_long_pause_keeps_the_connection() {
    let fixture = TestFixture::new();
    let content = vec![9u8; 1024 * 1024];
    let source = fixture.create_file("long.bin", &content);
    let recv_dir = fixture.output_dir();

    let share = start_share(source, common::send_options(), None, None)
        .await
        .unwrap();
    share.pause.set_paused(true);

    let emitter = MockEventEmitter::new();
    let (_cancel_tx, cancel_rx) = common::no_cancel();
    let recv_task = tokio::spawn(download(
        share.ticket.clone(),
        ReceiveOptions {
            output_dir: Some(recv_dir.clone()),
            ..Default::default()
        },
        Some(emitter.clone()),
        cancel_rx,
    ));

    tokio::time::sleep(Duration::from_secs(45)).await;
    assert!(!recv_task.is_finished());
    assert!(
        !emitter.has_event("receive-reconnecting"),
        "the connection dropped during the pause"
    );

    share.pause.set_paused(false);
    tokio::time::timeout(Duration::from_secs(60), recv_task)
        .await
        .expect("download should finish after resuming")
        .expect("receive task panicked")
        .expect("download should succeed");
    assert_eq!(std::fs::read(recv_dir.join("long.bin")).unwrap(), content);

    drop(share);
}
