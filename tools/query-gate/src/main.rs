use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    records: Vec<LogicalRecord>,
    valid_cases: Vec<QueryCase>,
    invalid_cases: Vec<QueryCase>,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let manifest = read_manifest(&cli.manifest)?;
    let record_count = manifest.records.len();
    let index = AssetIndex::from_records(manifest.records.into_iter().map(logical_record));
    let valid_cases = manifest
        .valid_cases
        .into_iter()
        .map(|case| execute_case(&index, case))
        .collect();
    let invalid_cases = manifest
        .invalid_cases
        .into_iter()
        .map(|case| execute_case(&index, case))
        .collect();
    serde_json::to_writer_pretty(
        std::io::stdout().lock(),
        &Report {
            schema: 1,
            record_count,
            valid_cases,
            invalid_cases,
        },
    )
    .context("serialize query gate report")?;
    println!();
    Ok(())
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
    let elapsed = || u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
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
