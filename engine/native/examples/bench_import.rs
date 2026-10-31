//! Benchmark the "Preparing transfer…" step: importing paths into a fresh
//! send store, which BLAKE3-hashes every file and writes its bao outboard.
//!
//! Each dataset is timed three ways, all with a warm page cache:
//!   walk    directory walk + stat (what `send_metadata` and `import_paths` do first)
//!   read    plain sequential read of every byte, 1 MiB buffers, one thread
//!   blake3  plain `blake3::Hasher` over every byte, 1 MiB buffers, one thread
//!   import  `native::import::import_paths` into a new FsStore, exactly as a send does
//!
//! Usage:
//!   cargo run --release --example bench_import --manifest-path engine/native/Cargo.toml -- \
//!     [--data DIR] [--iters N] [--only big|medium|small] [--import-only]
//!
//! Generated data (~3.1 GiB) is kept in DIR between runs; delete it when done.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use protocol::EventEmitter;
use walkdir::WalkDir;

const MIB: u64 = 1024 * 1024;

struct Dataset {
    name: &'static str,
    files: u64,
    file_size: u64,
}

const DATASETS: &[Dataset] = &[
    Dataset {
        name: "big",
        files: 1,
        file_size: 2048 * MIB,
    },
    Dataset {
        name: "medium",
        files: 512,
        file_size: 2 * MIB,
    },
    Dataset {
        name: "small",
        files: 20_000,
        file_size: 4 * 1024,
    },
];

#[derive(Default)]
struct CountingEmitter {
    events: AtomicU64,
}

impl EventEmitter for CountingEmitter {
    fn emit_event(&self, _: &str) -> Result<(), String> {
        self.events.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn emit_event_with_payload(&self, _: &str, _: &str) -> Result<(), String> {
        self.events.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut data_root = std::env::temp_dir().join("dashbeam-bench-data");
    let mut iters = 3usize;
    let mut only: Option<String> = None;
    let mut import_only = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => data_root = PathBuf::from(args.next().expect("--data DIR")),
            "--iters" => iters = args.next().expect("--iters N").parse()?,
            "--only" => only = args.next(),
            "--import-only" => import_only = true,
            other => anyhow::bail!("unknown argument {other}"),
        }
    }

    println!(
        "profile: {}, cpus: {}, data: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        num_cpus::get(),
        data_root.display()
    );
    println!();
    println!(
        "{:<8} {:>7} {:>10} | {:>10} {:>12} {:>12} {:>12} {:>8}",
        "dataset", "files", "size", "walk", "read", "blake3", "import", "events"
    );

    for ds in DATASETS {
        if only.as_deref().is_some_and(|o| o != ds.name) {
            continue;
        }
        let dir = data_root.join(ds.name);
        ensure_dataset(&dir, ds)?;
        let total = ds.files * ds.file_size;

        // Warm the page cache so every measurement sees the same state.
        read_all(&dir)?;

        let (walk, read, hash) = if import_only {
            Default::default()
        } else {
            (
                median(iters, || walk(&dir).map(|_| ()))?,
                median(iters, || read_all(&dir))?,
                median(iters, || blake3_all(&dir))?,
            )
        };

        let mut import_times = Vec::with_capacity(iters);
        let mut events = 0;
        for _ in 0..iters {
            let (elapsed, n) = import_once(&dir).await?;
            import_times.push(elapsed);
            events = n;
        }
        import_times.sort();
        let import = import_times[iters / 2];

        println!(
            "{:<8} {:>7} {:>10} | {:>10} {:>12} {:>12} {:>12} {:>8}",
            ds.name,
            ds.files,
            fmt_size(total),
            fmt_dur(walk),
            fmt_rate(total, read),
            fmt_rate(total, hash),
            fmt_rate(total, import),
            events,
        );
        println!(
            "{:<8} {:>7} {:>10} | {:>10} {:>12} {:>12} {:>12}",
            "",
            "",
            "",
            "",
            fmt_dur(read),
            fmt_dur(hash),
            fmt_dur(import),
        );
    }
    Ok(())
}

async fn import_once(dir: &Path) -> anyhow::Result<(Duration, u64)> {
    let emitter = Arc::new(CountingEmitter::default());
    let handle: protocol::AppHandle = Some(emitter.clone());
    let store_dir = native::storage::new_send_blobs_dir();
    let store = native::storage::create_send_store(&store_dir).await?;
    let paths = native::import::canonicalize_input_paths(vec![dir.to_path_buf()])?;

    let started = Instant::now();
    let (tag, _size, _collection) = native::import::import_paths(paths, &store, &handle).await?;
    let elapsed = started.elapsed();

    drop(tag);
    store.shutdown().await?;
    std::fs::remove_dir_all(&store_dir)?;
    Ok((elapsed, emitter.events.load(Ordering::Relaxed)))
}

fn median(iters: usize, mut f: impl FnMut() -> anyhow::Result<()>) -> anyhow::Result<Duration> {
    let mut times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let started = Instant::now();
        f()?;
        times.push(started.elapsed());
    }
    times.sort();
    Ok(times[iters / 2])
}

fn files(dir: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
}

fn walk(dir: &Path) -> anyhow::Result<u64> {
    let mut total = 0;
    for file in files(dir) {
        total += std::fs::metadata(file)?.len();
    }
    Ok(total)
}

fn read_all(dir: &Path) -> anyhow::Result<()> {
    let mut buf = vec![0u8; MIB as usize];
    for file in files(dir) {
        let mut f = std::fs::File::open(file)?;
        while f.read(&mut buf)? != 0 {}
    }
    Ok(())
}

fn blake3_all(dir: &Path) -> anyhow::Result<()> {
    let mut buf = vec![0u8; MIB as usize];
    for file in files(dir) {
        let mut f = std::fs::File::open(file)?;
        let mut hasher = blake3::Hasher::new();
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        std::hint::black_box(hasher.finalize());
    }
    Ok(())
}

fn ensure_dataset(dir: &Path, ds: &Dataset) -> anyhow::Result<()> {
    let marker = dir.join(".complete");
    if marker.exists() {
        return Ok(());
    }
    eprintln!(
        "generating {} ({} x {})…",
        ds.name,
        ds.files,
        fmt_size(ds.file_size)
    );
    let _ = std::fs::remove_dir_all(dir);
    let mut rng = 0x9e37_79b9_7f4a_7c15u64;
    let mut buf = vec![0u8; ds.file_size.min(MIB) as usize];
    for i in 0..ds.files {
        // 100 files per subdirectory, like a real folder tree.
        let sub = dir.join(format!("d{:03}", i / 100));
        std::fs::create_dir_all(&sub)?;
        let mut f = std::fs::File::create(sub.join(format!("f{i:05}.bin")))?;
        let mut left = ds.file_size;
        while left > 0 {
            for word in buf.as_chunks_mut::<8>().0 {
                // xorshift64: incompressible, and cheap enough not to matter.
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                *word = rng.to_le_bytes();
            }
            let n = left.min(buf.len() as u64) as usize;
            f.write_all(&buf[..n])?;
            left -= n as u64;
        }
    }
    std::fs::write(marker, b"")?;
    Ok(())
}

fn fmt_size(bytes: u64) -> String {
    if bytes >= 1024 * MIB {
        format!("{:.1} GiB", bytes as f64 / (1024 * MIB) as f64)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

fn fmt_rate(bytes: u64, d: Duration) -> String {
    if d.is_zero() {
        return "-".into();
    }
    format!("{:.0} MiB/s", bytes as f64 / MIB as f64 / d.as_secs_f64())
}

fn fmt_dur(d: Duration) -> String {
    format!("{:.0} ms", d.as_secs_f64() * 1000.0)
}
