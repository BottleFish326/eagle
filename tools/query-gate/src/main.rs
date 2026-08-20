use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use asset_core::{AssetDimensions, AssetKind, AssetRecord, MediaProperties};
use asset_index::{AssetIndex, parse_query};
use clap::Parser;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "query-gate",
    about = "Runs the product query parser and index over fixed logical records"
)]
struct Cli {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long, default_value_t = 0)]
    performance_records: usize,
    #[arg(long, default_value_t = 0)]
    performance_iterations: usize,
    #[arg(long)]
    performance_case: Option<String>,
    #[arg(long)]
    wait_for_sampler: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    records: Vec<LogicalRecord>,
    valid_cases: Vec<QueryCase>,
    invalid_cases: Vec<QueryCase>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogicalRecord {
    key: String,
    root_id: Option<Uuid>,
    relative_path: PathBuf,
    kind: AssetKind,
    extension: Option<String>,
    size: Option<u64>,
    created_unix_ms: Option<i64>,
    modified_unix_ms: Option<i64>,
    width: Option<u32>,
    height: Option<u32>,
    display_quarter_turns: u8,
    duration_ms: Option<u64>,
    page_count: Option<u32>,
    color_space: Option<String>,
    has_alpha: Option<bool>,
    rating: u8,
    favorite: bool,
    note: String,
    tags: BTreeSet<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryCase {
    id: String,
    expression: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema: u32,
    record_count: usize,
    valid_cases: Vec<CaseResult>,
    invalid_cases: Vec<CaseResult>,
    performance: Option<PerformanceReport>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseResult {
    id: String,
    elapsed_nanoseconds: u64,
    keys: Option<Vec<String>>,
    error: Option<ProductQueryError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductQueryError {
    kind: asset_index::QueryParseErrorKind,
    offset: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PerformanceReport {
    case_id: String,
    record_count: usize,
    iterations: usize,
    result_count: usize,
    index_build_nanoseconds: u64,
    p50_nanoseconds: u64,
    p95_nanoseconds: u64,
    max_nanoseconds: u64,
    samples_nanoseconds: Vec<u64>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let manifest = read_manifest(&cli.manifest)?;
    validate_performance_options(&cli)?;
    if cli.wait_for_sampler {
        eprintln!("QUERY_GATE_READY");
        let mut acknowledgement = String::new();
        std::io::stdin()
            .read_line(&mut acknowledgement)
            .context("wait for query sampler acknowledgement")?;
        if acknowledgement.trim() != "START" {
            bail!("query sampler acknowledgement was not START");
        }
    }
    let record_count = manifest.records.len();
    let index = AssetIndex::from_records(manifest.records.iter().cloned().map(logical_record));
    let valid_cases = manifest
        .valid_cases
        .iter()
        .cloned()
        .map(|case| execute_case(&index, case))
        .collect();
    let invalid_cases = manifest
        .invalid_cases
        .iter()
        .cloned()
        .map(|case| execute_case(&index, case))
        .collect();
    drop(index);
    let performance = run_performance(&manifest, &cli)?;
    serde_json::to_writer_pretty(
        std::io::stdout().lock(),
        &Report {
            schema: 1,
            record_count,
            valid_cases,
            invalid_cases,
            performance,
        },
    )
    .context("serialize query gate report")?;
    println!();
    Ok(())
}

fn validate_performance_options(cli: &Cli) -> Result<()> {
    let any = cli.performance_records > 0
        || cli.performance_iterations > 0
        || cli.performance_case.is_some();
    let all = cli.performance_records > 0
        && cli.performance_iterations > 0
        && cli.performance_case.is_some();
    if any && !all {
        bail!(
            "performance-records, performance-iterations, and performance-case are required together"
        );
    }
    if cli.performance_records > 100_000 {
        bail!("performance-records exceeds 100000");
    }
    if cli.performance_iterations > 1_000 {
        bail!("performance-iterations exceeds 1000");
    }
    Ok(())
}

fn run_performance(manifest: &Manifest, cli: &Cli) -> Result<Option<PerformanceReport>> {
    let Some(case_id) = &cli.performance_case else {
        return Ok(None);
    };
    let case = manifest
        .valid_cases
        .iter()
        .find(|entry| &entry.id == case_id)
        .with_context(|| format!("performance case is undeclared: {case_id}"))?;
    let query = parse_query(&case.expression).context("performance case did not parse")?;
    let build_started = Instant::now();
    let records = (0..cli.performance_records).map(|index| {
        let mut record = manifest.records[index % manifest.records.len()].clone();
        record.key = format!("perf-{index:06}");
        record.relative_path = PathBuf::from(format!(
            "perf/{index:06}/{}",
            record.relative_path.to_string_lossy()
        ));
        logical_record(record)
    });
    let index = AssetIndex::from_records(records);
    let index_build_nanoseconds = elapsed_nanoseconds(build_started);
    let mut samples = Vec::with_capacity(cli.performance_iterations);
    let mut result_count = None;
    for _ in 0..cli.performance_iterations {
        let started = Instant::now();
        let current_count = index.query(&query).len();
        samples.push(elapsed_nanoseconds(started));
        if result_count.is_some_and(|expected| expected != current_count) {
            bail!("performance query result count changed between iterations");
        }
        result_count = Some(current_count);
    }
    let mut sorted = samples.clone();
    sorted.sort_unstable();
    Ok(Some(PerformanceReport {
        case_id: case_id.clone(),
        record_count: cli.performance_records,
        iterations: cli.performance_iterations,
        result_count: result_count.unwrap_or(0),
        index_build_nanoseconds,
        p50_nanoseconds: percentile(&sorted, 50),
        p95_nanoseconds: percentile(&sorted, 95),
        max_nanoseconds: sorted.last().copied().unwrap_or(0),
        samples_nanoseconds: samples,
    }))
}

fn elapsed_nanoseconds(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    let rank = sorted
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    sorted.get(rank).copied().unwrap_or(0)
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    let file = File::open(path).with_context(|| format!("open manifest {}", path.display()))?;
    serde_json::from_reader(BufReader::new(file)).context("parse query manifest")
}

fn logical_record(record: LogicalRecord) -> AssetRecord {
    let mime = match record.kind {
        AssetKind::Image => "image/query-fixture",
        AssetKind::Video => "video/query-fixture",
        AssetKind::Audio => "audio/query-fixture",
        AssetKind::Pdf => "application/pdf",
        AssetKind::Other => "application/octet-stream",
    };
    let mut asset = AssetRecord::untagged(
        record.key,
        record.relative_path.clone(),
        mime.into(),
        record.size.unwrap_or(0),
        record.modified_unix_ms.unwrap_or(0),
    );
    asset.root_id = record.root_id;
    asset.relative_path = record.relative_path;
    asset.kind = record.kind;
    asset.extension = record.extension;
    asset.size = record.size;
    asset.created_unix_ms = record.created_unix_ms;
    asset.modified_unix_ms = record.modified_unix_ms;
    asset.dimensions = record
        .width
        .zip(record.height)
        .map(|(width, height)| AssetDimensions { width, height });
    asset.media = Some(MediaProperties {
        duration_ms: record.duration_ms,
        display_quarter_turns: Some(record.display_quarter_turns),
        page_count: record.page_count,
        color_space: record.color_space,
        has_alpha: record.has_alpha,
        ..MediaProperties::default()
    });
    asset.tags = record.tags;
    asset.rating = record.rating;
    asset.favorite = record.favorite;
    asset.note = record.note;
    asset
}

fn execute_case(index: &AssetIndex, case: QueryCase) -> CaseResult {
    let started = Instant::now();
    let parsed = parse_query(&case.expression);
    let elapsed = || elapsed_nanoseconds(started);
    match parsed {
        Ok(query) => CaseResult {
            id: case.id,
            elapsed_nanoseconds: elapsed(),
            keys: Some(index.query(&query).into_iter().collect()),
            error: None,
        },
        Err(error) => CaseResult {
            id: case.id,
            elapsed_nanoseconds: elapsed(),
            keys: None,
            error: Some(ProductQueryError {
                kind: error.kind,
                offset: error.offset,
            }),
        },
    }
}
