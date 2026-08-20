use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use asset_catalog::{AssetCatalog, QueryAssetsInput};
use asset_core::AssetRecord;
use asset_filesystem::{ScanCancellation, ScanOptions, scan_root_incremental};
use clap::Parser;
use metadata::{
    AssetSidecar, ExpectedVersion, digest_file, fingerprint_asset, sidecar_path_for,
    write_sidecar_atomic,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use uuid::Uuid;

const ROOT_ID: &str = "019b76c0-0000-7000-8000-000000000001";
const FIXED_UPDATED_AT: &str = "2026-08-21T00:00:00Z";

#[derive(Debug, Parser)]
#[command(
    name = "query-scan-gate",
    about = "Scans fixed ordinary files and Sidecars through the product query path"
)]
struct Cli {
    #[arg(long)]
    source_root: PathBuf,
    #[arg(long)]
    git_commit: String,
}

#[derive(Debug, Clone, Copy)]
struct Fixture {
    relative_path: &'static str,
    id: &'static str,
    rating: u8,
    favorite: bool,
    note: &'static str,
    extra_tag: &'static str,
}

const FIXTURES: [Fixture; 8] = [
    Fixture {
        relative_path: "svg/minimal.svg",
        id: "019b76c0-1000-7000-8000-000000000001",
        rating: 5,
        favorite: true,
        note: "vector reference",
        extra_tag: "usage/hero",
    },
    Fixture {
        relative_path: "video/minimal.mp4",
        id: "019b76c0-1000-7000-8000-000000000002",
        rating: 4,
        favorite: true,
        note: "video reference",
        extra_tag: "media/video",
    },
    Fixture {
        relative_path: "video/minimal.mov",
        id: "019b76c0-1000-7000-8000-000000000003",
        rating: 3,
        favorite: false,
        note: "",
        extra_tag: "media/video",
    },
    Fixture {
        relative_path: "video/minimal.webm",
        id: "019b76c0-1000-7000-8000-000000000004",
        rating: 2,
        favorite: false,
        note: "",
        extra_tag: "media/video",
    },
    Fixture {
        relative_path: "audio/minimal.mp3",
        id: "019b76c0-1000-7000-8000-000000000005",
        rating: 1,
        favorite: false,
        note: "",
        extra_tag: "media/audio",
    },
    Fixture {
        relative_path: "audio/minimal.wav",
        id: "019b76c0-1000-7000-8000-000000000006",
        rating: 4,
        favorite: true,
        note: "audio reference",
        extra_tag: "media/audio",
    },
    Fixture {
        relative_path: "audio/minimal.flac",
        id: "019b76c0-1000-7000-8000-000000000007",
        rating: 0,
        favorite: false,
        note: "",
        extra_tag: "media/audio",
    },
    Fixture {
        relative_path: "pdf/minimal.pdf",
        id: "019b76c0-1000-7000-8000-000000000008",
        rating: 5,
        favorite: true,
        note: "document reference",
        extra_tag: "media/document",
    },
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema: u32,
    accepted: bool,
    git_commit: String,
    root_id: Uuid,
    source_digest_before: String,
    source_digest_after: String,
    copied_asset_digest_before: String,
    copied_asset_digest_after: String,
    source_files: Vec<SourceFileReceipt>,
    sidecar_count: usize,
    scanned_record_count: usize,
    scan_problem_count: usize,
    records: Vec<ScannedRecord>,
    query_cases: Vec<QueryCaseReceipt>,
    invalid_cases: Vec<InvalidCaseReceipt>,
    failures: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceFileReceipt {
    relative_path: String,
    sha256_before: String,
    sha256_after: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScannedRecord {
    relative_path: String,
    stable_id: Option<Uuid>,
    kind: asset_core::AssetKind,
    size: Option<u64>,
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    page_count: Option<u32>,
    rating: u8,
    favorite: bool,
    has_note: bool,
    tags: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryCaseReceipt {
    id: String,
    expression: String,
    expected_relative_paths: Vec<String>,
    actual_relative_paths: Vec<String>,
    accepted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InvalidCaseReceipt {
    id: String,
    expression: String,
    expected_kind: asset_index::QueryParseErrorKind,
    expected_offset: usize,
    actual_kind: Option<asset_index::QueryParseErrorKind>,
    actual_offset: Option<usize>,
    accepted: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if !is_commit(&cli.git_commit) {
        bail!("git-commit must be a lowercase 40-character SHA-1");
    }
    let report = run_gate(&cli.source_root, cli.git_commit)?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &report)
        .context("serialize query scan report")?;
    println!();
    if !report.accepted {
        bail!("query scan gate rejected: {}", report.failures.join("; "));
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_gate(source_root: &Path, git_commit: String) -> Result<Report> {
    let source_root = source_root
        .canonicalize()
        .with_context(|| format!("canonicalize source root {}", source_root.display()))?;
    let root_id = Uuid::parse_str(ROOT_ID).context("parse fixed root ID")?;
    let source_before = source_receipts(&source_root)?;
    let source_digest_before = aggregate_digest(&source_before);
    let workspace = tempdir().context("create isolated scan root")?;
    let scan_root = workspace.path().join("library");
    fs::create_dir(&scan_root).context("create isolated library")?;
    let mut copied_before = Vec::new();

    for fixture in FIXTURES {
        let source = source_root.join(fixture.relative_path);
        let destination = scan_root.join(fixture.relative_path);
        fs::create_dir_all(destination.parent().context("fixture has no parent")?)
            .context("create fixture parent")?;
        fs::copy(&source, &destination)
            .with_context(|| format!("copy fixed fixture {}", fixture.relative_path))?;
        let digest = digest_file(&destination).context("digest copied fixture")?;
        copied_before.push((fixture.relative_path.to_owned(), digest));

        let mut sidecar =
            AssetSidecar::with_id(Uuid::parse_str(fixture.id).context("parse fixed asset ID")?);
        sidecar.tags = BTreeSet::from(["project/eagle".into(), fixture.extra_tag.into()]);
        sidecar.rating = fixture.rating;
        sidecar.favorite = fixture.favorite;
        sidecar.note = fixture.note.into();
        sidecar.fingerprint = Some(fingerprint_asset(&destination)?);
        sidecar.updated_at = FIXED_UPDATED_AT.into();
        write_sidecar_atomic(
            &sidecar_path_for(&destination),
            &sidecar,
            &ExpectedVersion::Missing,
        )
        .context("write isolated Sidecar")?;
    }
    let copied_asset_digest_before = aggregate_pairs(&copied_before);

    let mut scanned = Vec::new();
    let mut scan_problem_count = 0;
    let summary = scan_root_incremental(
        Some(root_id),
        &scan_root,
        &ScanOptions::default(),
        &ScanCancellation::new(),
        |batch| {
            scanned.extend(batch.assets);
            scan_problem_count += batch.problems.len();
        },
    )
    .context("scan isolated ordinary files and Sidecars")?;
    scanned.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut catalog = AssetCatalog::default();
    catalog.ingest(scanned.iter().cloned());

    let mut failures = Vec::new();
    if summary.asset_count != FIXTURES.len() || scanned.len() != FIXTURES.len() {
        failures.push(format!(
            "scanner returned {} records for {} fixtures",
            scanned.len(),
            FIXTURES.len()
        ));
    }
    if scan_problem_count != 0 {
        failures.push(format!("scanner reported {scan_problem_count} problems"));
    }
    for record in &scanned {
        if record.sidecar_state.is_none() || record.id.is_none() {
            failures.push(format!(
                "Sidecar was not merged for {}",
                normalized_relative(record)
            ));
        }
    }

    let query_cases = execute_query_cases(&catalog, &mut failures);
    let invalid_cases = execute_invalid_cases(&catalog, &mut failures);
    let records = scanned.iter().map(record_receipt).collect();

    let copied_after = FIXTURES
        .iter()
        .map(|fixture| {
            Ok((
                fixture.relative_path.to_owned(),
                digest_file(&scan_root.join(fixture.relative_path))?,
            ))
        })
        .collect::<Result<Vec<_>, metadata::SidecarError>>()?;
    let copied_asset_digest_after = aggregate_pairs(&copied_after);
    if copied_asset_digest_before != copied_asset_digest_after {
        failures.push("copied asset bytes changed during scan/query execution".into());
    }

    let source_after = source_receipts(&source_root)?;
    let source_digest_after = aggregate_digest(&source_after);
    if source_digest_before != source_digest_after {
        failures.push("tracked source fixture bytes changed during the gate".into());
    }
    let source_files = source_before
        .into_iter()
        .zip(source_after)
        .map(
            |((relative_path, sha256_before), (after_path, sha256_after))| {
                if relative_path != after_path {
                    failures.push("source receipt ordering changed".into());
                }
                SourceFileReceipt {
                    relative_path,
                    sha256_before,
                    sha256_after,
                }
            },
        )
        .collect();

    Ok(Report {
        schema: 1,
        accepted: failures.is_empty(),
        git_commit,
        root_id,
        source_digest_before,
        source_digest_after,
        copied_asset_digest_before,
        copied_asset_digest_after,
        source_files,
        sidecar_count: FIXTURES.len(),
        scanned_record_count: scanned.len(),
        scan_problem_count,
        records,
        query_cases,
        invalid_cases,
        failures,
    })
}

fn source_receipts(source_root: &Path) -> Result<Vec<(String, String)>> {
    FIXTURES
        .iter()
        .map(|fixture| {
            let path = source_root.join(fixture.relative_path);
            if !path.is_file() || path.is_symlink() {
                bail!("source fixture is not a regular non-symlink file");
            }
            Ok((fixture.relative_path.into(), digest_file(&path)?))
        })
        .collect()
}

fn aggregate_digest(receipts: &[(String, String)]) -> String {
    aggregate_pairs(receipts)
}

fn aggregate_pairs(receipts: &[(String, String)]) -> String {
    let mut digest = Sha256::new();
    for (path, sha256) in receipts {
        digest.update(path.as_bytes());
        digest.update([0]);
        digest.update(sha256.as_bytes());
        digest.update(b"\n");
    }
    format!("{:x}", digest.finalize())
}

fn execute_query_cases(
    catalog: &AssetCatalog,
    failures: &mut Vec<String>,
) -> Vec<QueryCaseReceipt> {
    let all = expected_paths(|_| true);
    let video = expected_paths(|fixture| fixture.relative_path.starts_with("video/"));
    let audio = expected_paths(|fixture| fixture.relative_path.starts_with("audio/"));
    let pdf = vec!["pdf/minimal.pdf".into()];
    let image = vec!["svg/minimal.svg".into()];
    let cases = vec![
        ("all", String::new(), all.clone()),
        ("tag", "project/eagle".into(), all.clone()),
        (
            "rating",
            "rating:>=4".into(),
            expected_paths(|fixture| fixture.rating >= 4),
        ),
        (
            "favorite",
            "favorite:true".into(),
            expected_paths(|fixture| fixture.favorite),
        ),
        (
            "note",
            "has-note:true".into(),
            expected_paths(|fixture| !fixture.note.is_empty()),
        ),
        ("image", "type:image".into(), image.clone()),
        ("video", "type:video".into(), video.clone()),
        ("audio", "type:audio".into(), audio.clone()),
        ("pdf", "type:pdf".into(), pdf.clone()),
        (
            "width",
            "width:>=300".into(),
            [video.clone(), pdf.clone()].concat(),
        ),
        ("landscape", "orientation:landscape".into(), video.clone()),
        ("aspect", "aspect:16/9".into(), video.clone()),
        (
            "duration",
            "duration:>=1s".into(),
            vec![
                "audio/minimal.flac".into(),
                "audio/minimal.wav".into(),
                "video/minimal.mov".into(),
                "video/minimal.mp4".into(),
                "video/minimal.webm".into(),
            ],
        ),
        ("pages", "pages:>=2".into(), pdf),
        ("root", format!("root:{ROOT_ID}"), all.clone()),
        ("path", "path:\"video/\"".into(), video),
        ("size", "size:>0".into(), all.clone()),
        (
            "modified",
            "modified:<2100-01-01T00:00:00Z".into(),
            all.clone(),
        ),
        ("color-unknown", "color-space:unknown".into(), all.clone()),
        ("alpha-unknown", "has-alpha:unknown".into(), all),
    ];
    cases
        .into_iter()
        .map(|(id, expression, mut expected)| {
            expected.sort();
            let actual = catalog
                .query_assets(&QueryAssetsInput {
                    expression: expression.clone(),
                })
                .map(|result| {
                    result
                        .keys
                        .iter()
                        .filter_map(|key| catalog.get(key))
                        .map(normalized_relative)
                        .collect::<Vec<_>>()
                });
            let (mut actual, query_failed) = match actual {
                Ok(actual) => (actual, false),
                Err(error) => {
                    failures.push(format!("query {id} did not parse: {error}"));
                    (Vec::new(), true)
                }
            };
            actual.sort();
            let accepted = !query_failed && actual == expected;
            if !accepted && !query_failed {
                failures.push(format!("query {id} did not match its independent path set"));
            }
            QueryCaseReceipt {
                id: id.into(),
                expression,
                expected_relative_paths: expected,
                actual_relative_paths: actual,
                accepted,
            }
        })
        .collect()
}

fn execute_invalid_cases(
    catalog: &AssetCatalog,
    failures: &mut Vec<String>,
) -> Vec<InvalidCaseReceipt> {
    use asset_index::QueryParseErrorKind;
    let cases = [
        ("unit", "size:10MB", QueryParseErrorKind::InvalidUnit, 0),
        (
            "path",
            "path:\"../escape\"",
            QueryParseErrorKind::InvalidPath,
            0,
        ),
        (
            "conflict",
            "width:unknown width:>=1",
            QueryParseErrorKind::ConflictingValue,
            14,
        ),
    ];
    cases
        .into_iter()
        .map(|(id, expression, expected_kind, expected_offset)| {
            let error = catalog
                .query_assets(&QueryAssetsInput {
                    expression: expression.into(),
                })
                .expect_err("invalid fixed query must fail");
            let accepted = error.kind == expected_kind && error.offset == expected_offset;
            if !accepted {
                failures.push(format!("invalid query {id} returned the wrong error"));
            }
            InvalidCaseReceipt {
                id: id.into(),
                expression: expression.into(),
                expected_kind,
                expected_offset,
                actual_kind: Some(error.kind),
                actual_offset: Some(error.offset),
                accepted,
            }
        })
        .collect()
}

fn expected_paths(predicate: impl Fn(&Fixture) -> bool) -> Vec<String> {
    FIXTURES
        .iter()
        .filter(|fixture| predicate(fixture))
        .map(|fixture| fixture.relative_path.into())
        .collect()
}

fn record_receipt(record: &AssetRecord) -> ScannedRecord {
    ScannedRecord {
        relative_path: normalized_relative(record),
        stable_id: record.id,
        kind: record.kind,
        size: record.size,
        width: record
            .dimensions
            .as_ref()
            .map(|dimensions| dimensions.width),
        height: record
            .dimensions
            .as_ref()
            .map(|dimensions| dimensions.height),
        duration_ms: record.media.as_ref().and_then(|media| media.duration_ms),
        page_count: record.media.as_ref().and_then(|media| media.page_count),
        rating: record.rating,
        favorite: record.favorite,
        has_note: !record.note.trim().is_empty(),
        tags: record.tags.clone(),
    }
}

fn normalized_relative(record: &AssetRecord) -> String {
    record.relative_path.to_string_lossy().replace('\\', "/")
}

fn is_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::run_gate;

    #[test]
    fn scans_real_files_and_sidecars_without_source_drift() {
        let source_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/formats/sources");
        let report = run_gate(&source_root, "0".repeat(40)).expect("run query scan gate");

        assert!(report.accepted, "{:?}", report.failures);
        assert_eq!(report.scanned_record_count, 8);
        assert_eq!(report.sidecar_count, 8);
        assert_eq!(report.source_digest_before, report.source_digest_after);
        assert_eq!(
            report.copied_asset_digest_before,
            report.copied_asset_digest_after
        );
        assert!(report.query_cases.iter().all(|case| case.accepted));
        assert!(report.invalid_cases.iter().all(|case| case.accepted));
    }
}
