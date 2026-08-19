use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use asset_core::AssetRecord;
use asset_filesystem::{
    ScanCancellation, ScanOptions, WatchSession, scan_root_incremental_controlled,
};
use asset_preview::{CacheStats, ThumbnailService};
use clap::Parser;
use resource_control::{ResourceController, ResourceMode, ResourceSnapshot, WorkKind};
use serde::Serialize;

const EVENT_FILE_NAME: &str = ".material-eagle-p2-soak-event";

#[derive(Debug, Parser)]
#[command(about = "Exercise and report bounded scan, hash, decode, watcher, and cache resources")]
struct Cli {
    root: PathBuf,
    cache: PathBuf,
    #[arg(long, default_value_t = 28_800)]
    duration_seconds: u64,
    #[arg(long, default_value_t = 5_000)]
    sample_interval_ms: u64,
    #[arg(long, default_value_t = 250)]
    event_interval_ms: u64,
    #[arg(long, default_value_t = 25_000)]
    thumbnail_window: usize,
    #[arg(long, default_value_t = 60)]
    mode_interval_seconds: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SoakSample {
    status: &'static str,
    elapsed_ms: u128,
    source_assets: usize,
    scan_passes: u64,
    watcher_batches: u64,
    generated_events: u64,
    thumbnail_requests: u64,
    hash_requests: u64,
    scheduler: ResourceSnapshot,
    cache: CacheStats,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate(&cli)?;
    let root = cli
        .root
        .canonicalize()
        .with_context(|| format!("canonicalize fixture root {}", cli.root.display()))?;
    let resources = ResourceController::with_defaults();
    let records = initial_scan(&root, &resources)?;
    if records.is_empty() {
        bail!("resource soak requires at least one supported image");
    }
    let previews = ThumbnailService::open_with_resources(&cli.cache, resources.clone())?;
    let watcher = WatchSession::start(&root)?;
    let stop = Arc::new(AtomicBool::new(false));
    let scan_cancellation = ScanCancellation::new();
    let scan_passes = Arc::new(AtomicU64::new(0));
    let scan_thread = spawn_scan_loop(
        root.clone(),
        resources.clone(),
        Arc::clone(&stop),
        scan_cancellation.clone(),
        Arc::clone(&scan_passes),
    )?;
    let event_path = root.join(EVENT_FILE_NAME);
    let result = run_loop(
        &cli,
        &records,
        &previews,
        &watcher,
        &resources,
        &scan_passes,
        &event_path,
    );
    stop.store(true, Ordering::Release);
    scan_cancellation.cancel();
    scan_thread
        .join()
        .map_err(|_| anyhow::anyhow!("background scan worker panicked"))??;
    if event_path.is_file() {
        fs::remove_file(&event_path)
            .with_context(|| format!("remove owned event file {}", event_path.display()))?;
    }
    result
}

fn validate(cli: &Cli) -> Result<()> {
    if !cli.root.is_dir() {
        bail!("fixture root is not a directory: {}", cli.root.display());
    }
    if cli.duration_seconds == 0
        || cli.sample_interval_ms == 0
        || cli.event_interval_ms == 0
        || cli.thumbnail_window == 0
        || cli.mode_interval_seconds == 0
    {
        bail!("all duration, interval, and window arguments must be positive");
    }
    Ok(())
}

fn initial_scan(root: &Path, resources: &ResourceController) -> Result<Vec<AssetRecord>> {
    let cancellation = ScanCancellation::new();
    let mut records = Vec::new();
    scan_root_incremental_controlled(
        None,
        root,
        &ScanOptions::default(),
        &cancellation,
        resources,
        |batch| records.extend(batch.assets),
    )?;
    Ok(records)
}

fn spawn_scan_loop(
    root: PathBuf,
    resources: ResourceController,
    stop: Arc<AtomicBool>,
    cancellation: ScanCancellation,
    passes: Arc<AtomicU64>,
) -> Result<thread::JoinHandle<Result<()>>> {
    thread::Builder::new()
        .name("resource-soak-scan".into())
        .spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let summary = scan_root_incremental_controlled(
                    None,
                    &root,
                    &ScanOptions::default(),
                    &cancellation,
                    &resources,
                    |_| {},
                )?;
                if summary.completion == asset_filesystem::ScanCompletion::Completed {
                    passes.fetch_add(1, Ordering::AcqRel);
                }
                if stop.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
            Ok(())
        })
        .context("spawn background scan worker")
}

#[allow(clippy::too_many_arguments)]
fn run_loop(
    cli: &Cli,
    records: &[AssetRecord],
    previews: &ThumbnailService,
    watcher: &WatchSession,
    resources: &ResourceController,
    scan_passes: &AtomicU64,
    event_path: &Path,
) -> Result<()> {
    let started = Instant::now();
    let duration = Duration::from_secs(cli.duration_seconds);
    let sample_interval = Duration::from_millis(cli.sample_interval_ms);
    let event_interval = Duration::from_millis(cli.event_interval_ms);
    let mode_interval = Duration::from_secs(cli.mode_interval_seconds);
    let mut next_sample = Instant::now();
    let mut next_event = Instant::now();
    let mut next_mode = Instant::now() + mode_interval;
    let mut background = false;
    let mut watcher_batches = 0;
    let mut generated_events = 0;
    let mut thumbnail_requests = 0;
    let mut hash_requests = 0;
    let window = cli.thumbnail_window.min(records.len());
    while started.elapsed() < duration {
        let now = Instant::now();
        if now >= next_event {
            generate_event(event_path, generated_events)?;
            generated_events += 1;
            next_event = now + event_interval;
        }
        if let Some(_batch) = watcher.next_batch_timeout(Duration::from_millis(25))? {
            watcher_batches += 1;
        }
        let record = &records[usize::try_from(thumbnail_requests).unwrap_or(0) % window];
        let _ = previews.request(record, 64)?;
        thumbnail_requests += 1;
        {
            let _permit = resources.acquire(WorkKind::Hash)?;
            let _ = metadata::quick_fingerprint_file(&record.path)?;
            hash_requests += 1;
        }
        if now >= next_mode {
            background = !background;
            resources.set_mode(if background {
                ResourceMode::Background
            } else {
                ResourceMode::Foreground
            })?;
            next_mode = now + mode_interval;
        }
        if now >= next_sample {
            emit_sample(
                "running",
                started,
                records.len(),
                scan_passes.load(Ordering::Acquire),
                watcher_batches,
                generated_events,
                thumbnail_requests,
                hash_requests,
                resources,
                previews,
            )?;
            next_sample = now + sample_interval;
        }
    }
    resources.set_mode(ResourceMode::Foreground)?;
    emit_sample(
        "complete",
        started,
        records.len(),
        scan_passes.load(Ordering::Acquire),
        watcher_batches,
        generated_events,
        thumbnail_requests,
        hash_requests,
        resources,
        previews,
    )
}

fn generate_event(path: &Path, sequence: u64) -> Result<()> {
    if path.metadata().is_ok_and(|metadata| metadata.len() > 4_096) {
        fs::write(path, b"")?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{sequence}")?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_sample(
    status: &'static str,
    started: Instant,
    source_assets: usize,
    scan_passes: u64,
    watcher_batches: u64,
    generated_events: u64,
    thumbnail_requests: u64,
    hash_requests: u64,
    resources: &ResourceController,
    previews: &ThumbnailService,
) -> Result<()> {
    let sample = SoakSample {
        status,
        elapsed_ms: started.elapsed().as_millis(),
        source_assets,
        scan_passes,
        watcher_batches,
        generated_events,
        thumbnail_requests,
        hash_requests,
        scheduler: resources.snapshot()?,
        cache: previews.cache_stats()?,
    };
    println!("{}", serde_json::to_string(&sample)?);
    Ok(())
}
