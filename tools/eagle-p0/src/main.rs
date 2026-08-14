use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use asset_core::AssetKind;
use asset_filesystem::{FsChangeKind, ScanOptions, WatchSession, scan_root};
use asset_index::{AssetIndex, AssetQuery, parse_query};
use clap::{Parser, Subcommand, ValueEnum};
use metadata::{
    AssetSidecar, ExpectedVersion, SidecarError, digest_file, read_sidecar, sidecar_path_for,
    write_sidecar_atomic,
};

#[derive(Debug, Parser)]
#[command(name = "eagle-p0", about = "Phase-zero filesystem material prototypes")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Scan {
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Query {
        root: PathBuf,
        #[arg(long = "all-tag")]
        all_tags: Vec<String>,
        #[arg(long = "any-tag")]
        any_tags: Vec<String>,
        #[arg(long = "exclude-tag")]
        excluded_tags: Vec<String>,
        #[arg(long)]
        kind: Option<KindArg>,
        #[arg(long)]
        favorite: Option<bool>,
    },
    Search {
        root: PathBuf,
        expression: String,
    },
    Tag {
        asset: PathBuf,
        #[arg(long = "tag", required = true)]
        tags: Vec<String>,
        #[arg(long, value_enum, default_value = "abort")]
        on_conflict: ConflictAction,
    },
    Watch {
        root: PathBuf,
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        #[arg(long)]
        summary: bool,
    },
    Benchmark {
        root: PathBuf,
        #[arg(long, default_value_t = 1000)]
        iterations: usize,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum KindArg {
    Image,
    Video,
    Audio,
    Pdf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ConflictAction {
    Abort,
    Reload,
    Merge,
}

impl From<KindArg> for AssetKind {
    fn from(value: KindArg) -> Self {
        match value {
            KindArg::Image => Self::Image,
            KindArg::Video => Self::Video,
            KindArg::Audio => Self::Audio,
            KindArg::Pdf => Self::Pdf,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan { root, json } => run_scan(&root, json),
        Command::Query {
            root,
            all_tags,
            any_tags,
            excluded_tags,
            kind,
            favorite,
        } => run_query(
            &root,
            &AssetQuery {
                all_tags: all_tags.into_iter().collect(),
                any_tag_groups: if any_tags.is_empty() {
                    Vec::new()
                } else {
                    vec![any_tags.into_iter().collect()]
                },
                excluded_tags: excluded_tags.into_iter().collect(),
                kinds: kind.map(Into::into).into_iter().collect(),
                favorite,
                ..AssetQuery::default()
            },
        ),
        Command::Search { root, expression } => {
            let query = parse_query(&expression).context("invalid query expression")?;
            run_query(&root, &query)
        }
        Command::Tag {
            asset,
            tags,
            on_conflict,
        } => run_tag(&asset, tags, on_conflict),
        Command::Watch {
            root,
            seconds,
            summary,
        } => run_watch(&root, seconds, summary),
        Command::Benchmark { root, iterations } => run_benchmark(&root, iterations),
    }
}

fn run_scan(root: &std::path::Path, json: bool) -> Result<()> {
    let report = scan_root(root, &ScanOptions::default()).context("scan failed")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("root: {}", report.root.display());
        println!("visited files: {}", report.visited_files);
        println!("assets: {}", report.assets.len());
        println!("problems: {}", report.problems.len());
        println!("elapsed: {} ms", report.elapsed_ms);
    }
    Ok(())
}

fn run_query(root: &std::path::Path, query: &AssetQuery) -> Result<()> {
    let report = scan_root(root, &ScanOptions::default()).context("scan failed")?;
    let index = AssetIndex::from_records(report.assets);
    let matches = index.query(query);
    for key in &matches {
        println!("{key}");
    }
    eprintln!("matched {} of {} assets", matches.len(), index.len());
    Ok(())
}

fn run_tag(asset: &std::path::Path, tags: Vec<String>, on_conflict: ConflictAction) -> Result<()> {
    if !asset.is_file() {
        bail!("asset is not a readable file: {}", asset.display());
    }
    let asset_digest_before = digest_file(asset).context("hash asset before write")?;
    let sidecar_path = sidecar_path_for(asset);
    let (mut sidecar, expected) = if sidecar_path.is_file() {
        let (sidecar, digest) = read_sidecar(&sidecar_path).context("read sidecar")?;
        (sidecar, ExpectedVersion::Digest(digest))
    } else {
        (AssetSidecar::new(), ExpectedVersion::Missing)
    };
    let requested_tags = tags.into_iter().collect::<BTreeSet<_>>();
    sidecar.tags.extend(requested_tags.iter().cloned());
    sidecar.touch();
    let receipt = match write_sidecar_atomic(&sidecar_path, &sidecar, &expected) {
        Ok(receipt) => Some(receipt),
        Err(error @ SidecarError::Conflict { .. }) => match on_conflict {
            ConflictAction::Abort => return Err(error).context("write sidecar atomically"),
            ConflictAction::Reload => {
                let (latest, digest) =
                    read_sidecar(&sidecar_path).context("reload conflicting sidecar")?;
                println!("conflict: reloaded sidecar digest {digest}");
                println!("current tags: {}", serde_json::to_string(&latest.tags)?);
                None
            }
            ConflictAction::Merge => {
                let (mut latest, digest) =
                    read_sidecar(&sidecar_path).context("reload sidecar before merge")?;
                latest.tags.extend(requested_tags);
                latest.touch();
                Some(
                    write_sidecar_atomic(&sidecar_path, &latest, &ExpectedVersion::Digest(digest))
                        .context("merge sidecar after conflict")?,
                )
            }
        },
        Err(error) => return Err(error).context("write sidecar atomically"),
    };
    let asset_digest_after = digest_file(asset).context("hash asset after write")?;
    if asset_digest_before != asset_digest_after {
        bail!("asset content changed while writing metadata");
    }
    if let Some(receipt) = receipt {
        println!("sidecar: {}", receipt.path.display());
        println!("sidecar digest: {}", receipt.digest);
    }
    println!("asset digest unchanged: {asset_digest_after}");
    Ok(())
}

fn run_watch(root: &std::path::Path, seconds: u64, summary: bool) -> Result<()> {
    let session = WatchSession::start(root).context("start watcher")?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut counts = [0_u64; 5];
    while Instant::now() < deadline {
        if let Some(change) = session
            .next_timeout(Duration::from_millis(250))
            .context("watch event")?
        {
            counts[match change.kind {
                FsChangeKind::Create => 0,
                FsChangeKind::Modify => 1,
                FsChangeKind::Move => 2,
                FsChangeKind::Delete => 3,
                FsChangeKind::RescanRequired => 4,
            }] += 1;
            if !summary {
                println!("{}", serde_json::to_string(&change)?);
            }
        }
    }
    println!("events_total: {}", counts.iter().sum::<u64>());
    println!("events_create: {}", counts[0]);
    println!("events_modify: {}", counts[1]);
    println!("events_move: {}", counts[2]);
    println!("events_delete: {}", counts[3]);
    println!("events_rescan_required: {}", counts[4]);
    Ok(())
}

fn run_benchmark(root: &std::path::Path, iterations: usize) -> Result<()> {
    if iterations == 0 {
        bail!("iterations must be greater than zero");
    }
    let scan_started = Instant::now();
    let report = scan_root(root, &ScanOptions::default()).context("scan failed")?;
    let scan_elapsed = scan_started.elapsed();
    let index = AssetIndex::from_records(report.assets);
    let query = parse_query("group/* any:(group/even|group/odd) -state/draft type:image ext:png")
        .context("parse benchmark query")?;
    let query_matches = index.query(&query).len();
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        let _ = index.query(&query);
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let p50 = samples[percentile_index(samples.len(), 50)];
    let p95 = samples[percentile_index(samples.len(), 95)];

    println!("assets: {}", index.len());
    println!("scan_ms: {}", scan_elapsed.as_millis());
    println!("query_iterations: {iterations}");
    println!("query_matches: {query_matches}");
    println!("query_p50_us: {}", p50.as_micros());
    println!("query_p95_us: {}", p95.as_micros());
    Ok(())
}

fn percentile_index(len: usize, percentile: usize) -> usize {
    ((len - 1) * percentile / 100).min(len - 1)
}
