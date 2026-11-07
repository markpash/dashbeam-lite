use crate::progress::{
    duration_ms, split_child_sizes, EmitThrottle, SpeedMeter, PROGRESS_MIN_BYTES,
    PROGRESS_MIN_SECS, SPEED_WINDOW_SECS,
};
use crate::send::METADATA_ALPN;
use crate::time_compat::{sleep, timeout, Duration, Instant};
use crate::types::{
    get_or_create_secret, AppHandle, DiscoveryModeOption, FileMetadata, ReceiveOptions,
};
use iroh::endpoint::presets;
use iroh::{
    address_lookup::{dns::DnsAddressLookup, pkarr::PkarrResolver},
    Endpoint, TransportAddr,
};
use iroh_blobs::{
    api::{remote::GetProgressItem, Store},
    format::collection::Collection,
    get::{request::get_hash_seq_and_sizes, GetError, Stats},
    ticket::BlobTicket,
};
use n0_future::StreamExt;
use std::str::FromStr;
use tokio::io::AsyncReadExt;

// Helper function to emit events through the app handle
fn emit_event(app_handle: &AppHandle, event_name: &str) {
    if let Some(handle) = app_handle {
        if let Err(e) = handle.emit_event(event_name) {
            tracing::warn!("Failed to emit event {}: {}", event_name, e);
        }
    }
}

// Helper function to emit progress events with payload
fn emit_progress_event(
    app_handle: &AppHandle,
    bytes_transferred: u64,
    total_bytes: u64,
    speed_bps: f64,
) {
    if let Some(handle) = app_handle {
        let event_name = "receive-progress";

        let payload =
            crate::progress::format_progress_payload(bytes_transferred, total_bytes, speed_bps);

        // Emit the event with appropriate payload
        if let Err(e) = handle.emit_event_with_payload(event_name, &payload) {
            tracing::warn!("Failed to emit progress event: {}", e);
        }
    }
}

// Helper function to emit events with payload
fn emit_event_with_payload(app_handle: &AppHandle, event_name: &str, payload: &str) {
    if let Some(handle) = app_handle {
        if let Err(e) = handle.emit_event_with_payload(event_name, payload) {
            tracing::warn!("Failed to emit event {} with payload: {}", event_name, e);
        }
    }
}

/// # Description
/// Receives metadata. This function will connect to the sender, request metadata, and return it without downloading
/// the file data.
/// # Returns
/// A `FileMetadata` struct containing the file name, size, thumbnail (if any), and MIME type (if any).
async fn receive_metadata<S: AsyncReadExt + Unpin>(
    stream: &mut S,
    app_handle: &AppHandle,
) -> anyhow::Result<FileMetadata> {
    // Read the length of the metadata (first 4 bytes)
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| anyhow::anyhow!("metadata read length failed: {e}"))?;
    let meta_len = u32::from_be_bytes(len_buf) as usize;
    tracing::debug!(meta_len, "receive_metadata: length prefix received");

    const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
    anyhow::ensure!(
        meta_len > 0 && meta_len <= MAX_METADATA_BYTES,
        "invalid metadata length: {meta_len}"
    );

    // Read the metadata JSON based on the length
    let mut meta_buf = vec![0u8; meta_len];
    stream
        .read_exact(&mut meta_buf)
        .await
        .map_err(|e| anyhow::anyhow!("metadata read body failed: {e}"))?;
    tracing::debug!(bytes = meta_buf.len(), "receive_metadata: body received");

    // Deserialize the metadata from JSON
    let metadata: FileMetadata = serde_json::from_slice(&meta_buf)
        .map_err(|e| anyhow::anyhow!("metadata json decode failed: {e}"))?;

    // Emit event with file metadata
    if let Some(emitter) = app_handle {
        if let Ok(payload) = serde_json::to_string(&metadata) {
            if let Err(e) = emitter.emit_event_with_payload("receive-file-metadata", &payload) {
                tracing::warn!("Failed to emit file metadata event: {}", e);
            }
        } else {
            tracing::warn!("Failed to serialize file metadata for event payload");
        }
    }

    Ok(metadata)
}

pub struct DownloadToStoreResult {
    pub collection: Collection,
    pub total_files: u64,
    pub payload_size: u64,
    pub stats: Stats,
    /// Wire time for the payload, excluding connection setup and disk export.
    pub download_duration_ms: u64,
}

/// How long a download keeps trying to reach the sender again after losing
/// it, counted from the last time data arrived.
pub const RECONNECT_GIVE_UP: Duration = Duration::from_secs(10 * 60);

/// Waits between reconnect attempts; the last one repeats.
const RECONNECT_BACKOFF: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// A stalled transfer (a paused sender, say) still reports progress this
/// often, so the speed shown decays to zero instead of freezing.
const STALL_TICK: Duration = Duration::from_secs(1);

/// What one connection's worth of downloading achieved.
struct Attempt {
    stats: Stats,
    total_files: u64,
    payload_size: u64,
    wire_ms: u64,
}

/// Why an attempt stopped short.
struct AttemptError {
    error: anyhow::Error,
    /// The sender was reached before it failed.
    connected: bool,
    /// Trying again may help: the connection broke, rather than the data
    /// being bad or the local store failing.
    retryable: bool,
    /// Time spent transferring before it failed.
    wire_ms: u64,
}

impl AttemptError {
    fn get(error: GetError, wire_ms: u64) -> Self {
        let retryable = is_retryable(&error);
        Self {
            error: show_get_error(error).into(),
            connected: true,
            retryable,
            wire_ms,
        }
    }
}

/// Whether a failed get is worth retrying on a new connection. Bad data and
/// a failing local store would fail the same way again; a dropped connection,
/// or a sender that has stopped serving for now, may not.
fn is_retryable(error: &GetError) -> bool {
    use iroh_blobs::get::fsm::DecodeError;
    match error {
        GetError::BadRequest { .. } | GetError::LocalFailure { .. } => false,
        GetError::Decode { source, .. } => !matches!(
            source,
            DecodeError::ParentHashMismatch { .. }
                | DecodeError::LeafHashMismatch { .. }
                | DecodeError::Write { .. }
        ),
        _ => true,
    }
}

/// Download ticket payload into the blob store (no filesystem export).
///
/// Only what the store is missing is requested, so a download picks up where
/// an earlier one stopped. If the connection breaks after the sender was
/// reached (in this call or, judging by data already in the store, an earlier
/// one), it reconnects with backoff for up to [`RECONNECT_GIVE_UP`], emitting
/// `receive-reconnecting` before each wait and `receive-reconnected` once
/// connected again.
pub async fn download_to_store(
    ticket: BlobTicket,
    addr: iroh::EndpointAddr,
    endpoint: &Endpoint,
    db: &Store,
    app_handle: &AppHandle,
) -> anyhow::Result<DownloadToStoreResult> {
    let hash_and_format = ticket.hash_and_format();
    let local = db.remote().local(hash_and_format).await?;

    let (stats, total_files, payload_size, download_duration_ms) = if !local.is_complete() {
        emit_event(app_handle, "receive-started");

        // Data already stored means this sender was reached before.
        let mut reached = local.local_bytes() > 0;
        let mut lost_since: Option<Instant> = None;
        let mut failures = 0usize;
        // Index into `RECONNECT_BACKOFF`; starts over once data flows again.
        let mut backoff = 0usize;
        let mut wire_ms = 0u64;
        let mut stored = local.local_bytes();
        loop {
            let local = db.remote().local(hash_and_format).await?;
            // New data since the last failure: the connection was working.
            if local.local_bytes() > stored {
                stored = local.local_bytes();
                lost_since = None;
                backoff = 0;
            }
            match download_attempt(
                &addr,
                endpoint,
                db,
                hash_and_format.hash,
                &local,
                failures > 0,
                app_handle,
            )
            .await
            {
                Ok(attempt) => {
                    break (
                        attempt.stats,
                        attempt.total_files,
                        attempt.payload_size,
                        wire_ms.saturating_add(attempt.wire_ms),
                    );
                }
                Err(failure) => {
                    wire_ms = wire_ms.saturating_add(failure.wire_ms);
                    reached |= failure.connected;
                    let since = *lost_since.get_or_insert_with(Instant::now);
                    if !failure.retryable || !reached || since.elapsed() >= RECONNECT_GIVE_UP {
                        return Err(failure.error);
                    }
                    let wait = RECONNECT_BACKOFF[backoff.min(RECONNECT_BACKOFF.len() - 1)];
                    backoff += 1;
                    failures += 1;
                    tracing::warn!(
                        "Download interrupted ({}); reconnecting in {wait:?}",
                        failure.error
                    );
                    let payload = serde_json::json!({
                        "attempt": failures,
                        "retryInMs": wait.as_millis() as u64,
                        "reason": failure.error.to_string(),
                    });
                    emit_event_with_payload(
                        app_handle,
                        "receive-reconnecting",
                        &payload.to_string(),
                    );
                    sleep(wait).await;
                }
            }
        }
    } else {
        let total_files = local.children().unwrap() - 1;
        let payload_bytes = 0;

        emit_event(app_handle, "receive-started");
        emit_event(app_handle, "receive-completed");

        (Stats::default(), total_files, payload_bytes, 0)
    };

    let collection = Collection::load(hash_and_format.hash, db).await?;

    let mut file_names: Vec<String> = Vec::new();
    for (name, _hash) in collection.iter() {
        file_names.push(name.to_string());
    }

    if !file_names.is_empty() {
        let file_names_json =
            serde_json::to_string(&file_names).unwrap_or_else(|_| "[]".to_string());
        emit_event_with_payload(app_handle, "receive-file-names", &file_names_json);
    }

    Ok(DownloadToStoreResult {
        collection,
        total_files,
        payload_size,
        stats,
        download_duration_ms,
    })
}

/// Connects once and fetches what `local` lacks. `reconnecting` says an
/// earlier attempt failed, so a successful connection is announced.
async fn download_attempt(
    addr: &iroh::EndpointAddr,
    endpoint: &Endpoint,
    db: &Store,
    hash: iroh_blobs::Hash,
    local: &iroh_blobs::api::remote::LocalInfo,
    reconnecting: bool,
    app_handle: &AppHandle,
) -> Result<Attempt, AttemptError> {
    let connection = match endpoint
        .connect(addr.clone(), iroh_blobs::protocol::ALPN)
        .await
    {
        Ok(conn) => conn,
        Err(e) => {
            tracing::error!("Connection failed: {}", e);
            tracing::error!("Error details: {:?}", e);
            tracing::error!("Tried to connect to node: {}", addr.id);
            tracing::error!("With relay: {:?}", addr.relay_urls().collect::<Vec<_>>());
            tracing::error!(
                "With direct addrs: {:?}",
                addr.ip_addrs().collect::<Vec<_>>()
            );
            return Err(AttemptError {
                error: anyhow::anyhow!("Connection failed: {}", e),
                connected: false,
                retryable: true,
                wire_ms: 0,
            });
        }
    };

    let (hash_seq, sizes) =
        match get_hash_seq_and_sizes(&connection, &hash, 1024 * 1024 * 32, None).await {
            Ok(found) => found,
            Err(e) => {
                tracing::error!("Failed to get sizes: {:?}", e);
                return Err(AttemptError::get(e, 0));
            }
        };
    if reconnecting {
        emit_event(app_handle, "receive-reconnected");
    }
    // `sizes` holds children only (entry 0 is the collection metadata), but
    // the get stream counts the hash-seq root and that blob too — subtract
    // them to leave the same "file bytes" the sender reports.
    let root_bytes = (hash_seq.len() as u64).saturating_mul(32);
    let split = split_child_sizes(root_bytes, &sizes);
    let payload_size = split.payload_bytes;
    let total_files = (sizes.len().saturating_sub(1)) as u64;

    // Bytes a previous attempt stored count toward progress; the get stream
    // only counts what this one fetches.
    let stored = local.local_bytes();
    let received_with = |fetched: u64| {
        stored
            .saturating_add(fetched)
            .saturating_sub(split.overhead_bytes)
            .min(payload_size)
    };
    let mut received = received_with(0);
    emit_progress_event(app_handle, received, payload_size, 0.0);

    let get = db.remote().execute_get(connection, local.missing());
    let mut stream = get.stream();
    let transfer_start_time = Instant::now();
    let mut speed = SpeedMeter::new(SPEED_WINDOW_SECS);
    speed.record(0.0, received);
    let mut throttle = EmitThrottle::new(PROGRESS_MIN_BYTES, PROGRESS_MIN_SECS);
    let wire_ms = || duration_ms(transfer_start_time.elapsed().as_secs_f64());

    loop {
        let item = match timeout(STALL_TICK, stream.next()).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_) => {
                let elapsed = transfer_start_time.elapsed().as_secs_f64();
                emit_progress_event(
                    app_handle,
                    received,
                    payload_size,
                    speed.bytes_per_sec_at(elapsed),
                );
                continue;
            }
        };
        match item {
            GetProgressItem::Progress(offset) => {
                let elapsed = transfer_start_time.elapsed().as_secs_f64();
                received = received_with(offset);
                speed.record(elapsed, received);
                if throttle.should_emit(elapsed, received) {
                    emit_progress_event(
                        app_handle,
                        received,
                        payload_size,
                        speed.bytes_per_sec_at(elapsed),
                    );
                }
            }
            GetProgressItem::Done(stats) => {
                let elapsed = transfer_start_time.elapsed().as_secs_f64();
                speed.record(elapsed, payload_size);
                emit_progress_event(
                    app_handle,
                    payload_size,
                    payload_size,
                    speed.bytes_per_sec_at(elapsed),
                );
                return Ok(Attempt {
                    stats,
                    total_files,
                    payload_size,
                    wire_ms: duration_ms(elapsed),
                });
            }
            GetProgressItem::Error(cause) => {
                tracing::error!("Download error: {:?}", cause);
                return Err(AttemptError::get(cause, wire_ms()));
            }
        }
    }
    Err(AttemptError {
        error: anyhow::anyhow!("download ended before it finished"),
        connected: true,
        retryable: true,
        wire_ms: wire_ms(),
    })
}

/// # Description
/// Fetches metadata for a given ticket without downloading the file data. This is used to display file information (name, size, thumbnail) in the UI before the user decides to download.
/// # Returns
/// A `FileMetadata` struct containing the file name, size, and preview metadata (if any).
pub async fn fetch_metadata(
    ticket_str: String,
    options: ReceiveOptions,
) -> anyhow::Result<FileMetadata> {
    // parse ticket and extract address
    let ticket = BlobTicket::from_str(&ticket_str)?;
    let addr = ticket.addr().clone();

    // Create a temporary endpoint to connect and fetch metadata
    let secret_key = get_or_create_secret()?;

    let discovery_mode = options.discovery_mode.clone();
    let builder = match &discovery_mode {
        DiscoveryModeOption::Custom { .. } => Endpoint::builder(presets::Minimal),
        DiscoveryModeOption::Default => Endpoint::builder(presets::N0),
    };
    let custom_infra = crate::tls_config::uses_custom_infra(&discovery_mode, &options.relay_mode);
    let mut builder = crate::tls_config::with_system_ca_if_custom(builder, custom_infra)
        // METADATA_ALPN only to indicate a metadata fetch
        .alpns(vec![METADATA_ALPN.to_vec()])
        .secret_key(secret_key)
        .relay_mode(options.relay_mode.into());

    {
        if ticket.addr().relay_urls().count() == 0 && ticket.addr().ip_addrs().count() == 0 {
            builder = match &discovery_mode {
                DiscoveryModeOption::Custom {
                    pkarr_relay_url,
                    dns_origin,
                } => {
                    let mut builder =
                        builder.address_lookup(PkarrResolver::builder(pkarr_relay_url.clone()));
                    if let Some(origin) = dns_origin {
                        builder = builder.address_lookup(DnsAddressLookup::builder(origin.clone()));
                    }
                    builder
                }
                DiscoveryModeOption::Default => builder.address_lookup(DnsAddressLookup::n0_dns()),
            };
        }
        if let Some(addr) = options.magic_ipv4_addr {
            builder = builder.bind_addr(addr)?;
        }
        if let Some(addr) = options.magic_ipv6_addr {
            builder = builder.bind_addr(addr)?;
        }
    }

    let endpoint = builder.bind().await?;

    // Attempt connection and metadata fetch up to 3 times
    let mut attempt_plan: Vec<(usize, &'static str, iroh::EndpointAddr)> = vec![
        (1, "default", addr.clone()),
        (2, "default", addr.clone()),
        (3, "default", addr.clone()),
    ];

    // Relay-only attempt if relay addresses are avaliable
    let mut relay_only_addr = addr.clone();
    relay_only_addr
        .addrs
        .retain(|transport_addr| matches!(transport_addr, TransportAddr::Relay(_)));
    if !relay_only_addr.addrs.is_empty() {
        attempt_plan[2] = (3, "relay-only", relay_only_addr);
    }

    let mut last_error: Option<anyhow::Error> = None;
    let mut attempt_errors: Vec<(usize, &'static str, String)> = Vec::new();

    for (attempt, path, target_addr) in attempt_plan {
        tracing::info!(attempt, path, "fetch_metadata: connecting to sender");

        let result: anyhow::Result<FileMetadata> = async {
            let connection = timeout(
                Duration::from_secs(15),
                endpoint.connect(target_addr, METADATA_ALPN),
            )
            .await
            .map_err(|_| anyhow::anyhow!("metadata connect timeout"))??;

            tracing::debug!(attempt, path, "fetch_metadata: connection established");

            let (mut send_stream, mut recv_stream) =
                timeout(Duration::from_secs(20), connection.open_bi())
                    .await
                    .map_err(|_| anyhow::anyhow!("metadata open_bi timeout"))??;

            tracing::debug!(attempt, path, "fetch_metadata: bi stream opened");

            // Send 1 byte as a marker to indicate metadata request
            timeout(Duration::from_secs(10), send_stream.write_all(&[1]))
                .await
                .map_err(|_| anyhow::anyhow!("metadata request write timeout"))??;

            tracing::debug!(attempt, path, "fetch_metadata: request marker sent");

            let metadata = timeout(
                Duration::from_secs(20),
                receive_metadata(&mut recv_stream, &None),
            )
            .await
            .map_err(|_| anyhow::anyhow!("metadata read timeout"))??;

            // Finish send_stream only AFTER receiving the metadata.
            // signals the server that we are safely done and it can drop the connection.
            let _ = send_stream.finish();

            Ok(metadata)
        }
        .await;

        match result {
            Ok(metadata) => {
                tracing::info!(
                    attempt,
                    path,
                    retries = attempt_errors.len(),
                    file_name = %metadata.file_name,
                    size = metadata.size,
                    "fetch_metadata: received metadata"
                );
                endpoint.close().await;
                return Ok(metadata);
            }
            Err(err) => {
                let will_retry = attempt < 3;
                tracing::debug!(
                    attempt,
                    path,
                    will_retry,
                    error = %err,
                    "fetch_metadata attempt failed"
                );
                attempt_errors.push((attempt, path, err.to_string()));
                last_error = Some(err);
                if will_retry {
                    sleep(Duration::from_millis(300)).await;
                }
            }
        }
    }

    endpoint.close().await;

    if !attempt_errors.is_empty() {
        let failure_summary = attempt_errors
            .iter()
            .map(|(attempt, path, err)| format!("#{attempt}({path}): {err}"))
            .collect::<Vec<_>>()
            .join(" | ");

        if let Some(ref err) = last_error {
            tracing::warn!(
                attempts = attempt_errors.len(),
                error = %err,
                failure_summary = %failure_summary,
                "fetch_metadata: failed to connect to sender"
            );
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("metadata fetch failed")))
}

fn show_get_error(e: GetError) -> GetError {
    match &e {
        GetError::InitialNext { source, .. } => {
            tracing::error!("initial connection error: {source}");
        }
        GetError::ConnectedNext { source, .. } => {
            tracing::error!("connected error: {source}");
        }
        GetError::AtBlobHeaderNext { source, .. } => {
            tracing::error!("reading blob header error: {source}");
        }
        GetError::Decode { source, .. } => {
            tracing::error!("decoding error: {source}");
        }
        GetError::IrpcSend { source, .. } => {
            tracing::error!("error sending over irpc: {source}");
        }
        GetError::AtClosingNext { source, .. } => {
            tracing::error!("error at closing: {source}");
        }
        GetError::BadRequest { .. } => {
            tracing::error!("bad request");
        }
        GetError::LocalFailure { source, .. } => {
            tracing::error!("local failure {source:?}");
        }
    }
    e
}
