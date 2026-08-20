use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use asset_core::{AssetIssue, AssetRecord};
use asset_filesystem::{
    LibraryRootStatus, OrphanSidecarState, ReconciliationReport, RootAccessStatus,
};
use metadata::{read_sidecar_versioned, sidecar_path_for};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_CONSISTENCY_FINDINGS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SupportSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsistencyFinding {
    pub severity: SupportSeverity,
    pub code: String,
    pub root_id: Option<Uuid>,
    pub asset_id: Option<Uuid>,
    pub relative_path: Option<PathBuf>,
    pub path_fingerprint: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootConsistencySummary {
    pub root_id: Uuid,
    pub enabled: bool,
    pub access_status: RootAccessStatus,
    pub catalog_assets: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryConsistencySummary {
    pub configured_roots: usize,
    pub catalog_assets: usize,
    pub findings: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryConsistencyReport {
    pub generated_unix_ms: i64,
    pub authoritative: bool,
    pub summary: LibraryConsistencySummary,
    pub roots: Vec<RootConsistencySummary>,
    pub findings: Vec<ConsistencyFinding>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TraceOutcome {
    Passed,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetTraceStep {
    pub match_index: Option<usize>,
    pub stage: String,
    pub outcome: TraceOutcome,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetTraceMatch {
    pub root_id: Option<Uuid>,
    pub root_access_status: Option<RootAccessStatus>,
    pub relative_path: PathBuf,
    pub path_fingerprint: String,
    pub asset_present: bool,
    pub sidecar_present: bool,
    pub sidecar_id_matches: Option<bool>,
    pub mime: String,
    pub issue_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetTraceReport {
    pub generated_unix_ms: i64,
    pub asset_id: Uuid,
    pub match_count: usize,
    pub matches: Vec<AssetTraceMatch>,
    pub steps: Vec<AssetTraceStep>,
}

#[must_use]
pub fn inspect_library_consistency(
    roots: &[LibraryRootStatus],
    records: &[AssetRecord],
    authoritative: bool,
) -> LibraryConsistencyReport {
    let mut report = LibraryConsistencyReport {
        generated_unix_ms: now_unix_ms(),
        authoritative,
        summary: LibraryConsistencySummary {
            configured_roots: roots.len(),
            catalog_assets: records.len(),
            findings: 0,
            warnings: 0,
            errors: 0,
        },
        roots: roots
            .iter()
            .map(|root| RootConsistencySummary {
                root_id: root.root.id,
                enabled: root.root.enabled,
                access_status: root.access_status,
                catalog_assets: records
                    .iter()
                    .filter(|record| record.root_id == Some(root.root.id))
                    .count(),
                warnings: 0,
                errors: 0,
            })
            .collect(),
        findings: Vec::new(),
        truncated: false,
    };

    for root in roots {
        if root.root.enabled && root.access_status != RootAccessStatus::Available {
            push_finding(
                &mut report,
                ConsistencyFinding {
                    severity: SupportSeverity::Error,
                    code: "root-unavailable".into(),
                    root_id: Some(root.root.id),
                    asset_id: None,
                    relative_path: None,
                    path_fingerprint: Some(path_fingerprint(&root.root.path)),
                    message: format!("enabled root is {}", root.access_status),
                },
            );
        }
    }

    let roots_by_id = roots
        .iter()
        .map(|root| (root.root.id, root))
        .collect::<BTreeMap<_, _>>();
    let mut records_by_id = BTreeMap::<Uuid, Vec<&AssetRecord>>::new();
    for record in records {
        if let Some(id) = record.id {
            records_by_id.entry(id).or_default().push(record);
        }
        inspect_record(&mut report, &roots_by_id, record);
    }
    for (id, matches) in records_by_id {
        if matches.len() > 1 {
            push_finding(
                &mut report,
                ConsistencyFinding {
                    severity: SupportSeverity::Error,
                    code: "duplicate-stable-id".into(),
                    root_id: matches.first().and_then(|record| record.root_id),
                    asset_id: Some(id),
                    relative_path: None,
                    path_fingerprint: None,
                    message: format!(
                        "stable ID is associated with {} catalog records",
                        matches.len()
                    ),
                },
            );
        }
    }
    report
}

pub fn append_reconciliation_findings(
    report: &mut LibraryConsistencyReport,
    root: &LibraryRootStatus,
    reconciliation: &ReconciliationReport,
) {
    for orphan in &reconciliation.orphan_sidecars {
        push_finding(
            report,
            ConsistencyFinding {
                severity: SupportSeverity::Error,
                code: match orphan.state {
                    OrphanSidecarState::Ready => "orphan-sidecar",
                    OrphanSidecarState::MissingFingerprint => "orphan-sidecar-missing-fingerprint",
                    OrphanSidecarState::Invalid => "invalid-orphan-sidecar",
                }
                .into(),
                root_id: Some(root.root.id),
                asset_id: orphan.sidecar_id,
                relative_path: root_relative_path(&root.root.path, &orphan.sidecar_path),
                path_fingerprint: Some(path_fingerprint(&orphan.sidecar_path)),
                message: "Sidecar exists but its adjacent asset is missing".into(),
            },
        );
    }
    for candidate in &reconciliation.pending_moves {
        push_finding(
            report,
            ConsistencyFinding {
                severity: if candidate.ambiguous {
                    SupportSeverity::Error
                } else {
                    SupportSeverity::Warning
                },
                code: if candidate.ambiguous {
                    "ambiguous-relink-candidate"
                } else {
                    "relink-candidate"
                }
                .into(),
                root_id: Some(root.root.id),
                asset_id: Some(candidate.sidecar_id),
                relative_path: root_relative_path(&root.root.path, &candidate.asset_path),
                path_fingerprint: Some(path_fingerprint(&candidate.asset_path)),
                message: "orphan Sidecar has a content-confirmed asset candidate".into(),
            },
        );
    }
    for conflict in &reconciliation.sync_conflict_copies {
        push_finding(
            report,
            ConsistencyFinding {
                severity: SupportSeverity::Warning,
                code: "sync-conflict-copy".into(),
                root_id: Some(root.root.id),
                asset_id: conflict.sidecar_id,
                relative_path: root_relative_path(&root.root.path, &conflict.path),
                path_fingerprint: Some(path_fingerprint(&conflict.path)),
                message: "synchronization conflict copy requires explicit review".into(),
            },
        );
    }
}

pub fn append_reconciliation_failure(
    report: &mut LibraryConsistencyReport,
    root: &LibraryRootStatus,
) {
    push_finding(
        report,
        ConsistencyFinding {
            severity: SupportSeverity::Error,
            code: "reconciliation-inspection-failed".into(),
            root_id: Some(root.root.id),
            asset_id: None,
            relative_path: None,
            path_fingerprint: Some(path_fingerprint(&root.root.path)),
            message: "root reconciliation inspection could not be completed".into(),
        },
    );
}

fn root_relative_path(root: &Path, path: &Path) -> Option<PathBuf> {
    path.strip_prefix(root).ok().map(Path::to_path_buf)
}

fn inspect_record(
    report: &mut LibraryConsistencyReport,
    roots_by_id: &BTreeMap<Uuid, &LibraryRootStatus>,
    record: &AssetRecord,
) {
    let Some(root_id) = record.root_id else {
        push_record_finding(
            report,
            record,
            SupportSeverity::Error,
            "missing-root-association",
            "catalog record is not associated with a configured root",
        );
        return;
    };
    let Some(root) = roots_by_id.get(&root_id) else {
        push_record_finding(
            report,
            record,
            SupportSeverity::Error,
            "unknown-root-association",
            "catalog record refers to a root that is not configured",
        );
        return;
    };
    if !record.path.starts_with(&root.root.path) {
        push_record_finding(
            report,
            record,
            SupportSeverity::Error,
            "asset-outside-root",
            "catalog path is outside its configured root",
        );
    }
    match fs::symlink_metadata(&record.path) {
        Ok(metadata) if metadata.file_type().is_symlink() => push_record_finding(
            report,
            record,
            SupportSeverity::Error,
            "asset-symlink",
            "catalog asset is a symbolic link and is not a supported source",
        ),
        Ok(metadata) if !metadata.is_file() => push_record_finding(
            report,
            record,
            SupportSeverity::Error,
            "asset-not-file",
            "catalog asset path is no longer a regular file",
        ),
        Err(_) => push_record_finding(
            report,
            record,
            SupportSeverity::Error,
            "asset-missing",
            "catalog asset is missing or unreadable",
        ),
        Ok(_) => {}
    }

    let expected_sidecar = sidecar_path_for(&record.path);
    if record
        .sidecar_path
        .as_ref()
        .is_some_and(|path| path != &expected_sidecar)
    {
        push_record_finding(
            report,
            record,
            SupportSeverity::Error,
            "sidecar-association-mismatch",
            "catalog Sidecar path is not adjacent to the asset",
        );
    }
    if record.sidecar_state.is_some() {
        match fs::symlink_metadata(&expected_sidecar) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                push_record_finding(
                    report,
                    record,
                    SupportSeverity::Error,
                    "sidecar-unsafe",
                    "catalog Sidecar is not a regular non-symlink file",
                );
            }
            Err(_) => push_record_finding(
                report,
                record,
                SupportSeverity::Error,
                "sidecar-missing",
                "catalog metadata refers to a Sidecar that is missing",
            ),
            Ok(_) => {}
        }
    }
    for issue in &record.issues {
        let (severity, code, message) = issue_finding(issue);
        push_record_finding(report, record, severity, code, message);
    }
}

fn push_record_finding(
    report: &mut LibraryConsistencyReport,
    record: &AssetRecord,
    severity: SupportSeverity,
    code: &str,
    message: &str,
) {
    push_finding(
        report,
        ConsistencyFinding {
            severity,
            code: code.into(),
            root_id: record.root_id,
            asset_id: record.id,
            relative_path: Some(record.relative_path.clone()),
            path_fingerprint: Some(path_fingerprint(&record.path)),
            message: message.into(),
        },
    );
}

fn push_finding(report: &mut LibraryConsistencyReport, finding: ConsistencyFinding) {
    report.summary.findings += 1;
    match finding.severity {
        SupportSeverity::Warning => report.summary.warnings += 1,
        SupportSeverity::Error => report.summary.errors += 1,
    }
    if let Some(root_id) = finding.root_id
        && let Some(root) = report.roots.iter_mut().find(|root| root.root_id == root_id)
    {
        match finding.severity {
            SupportSeverity::Warning => root.warnings += 1,
            SupportSeverity::Error => root.errors += 1,
        }
    }
    if report.findings.len() < MAX_CONSISTENCY_FINDINGS {
        report.findings.push(finding);
    } else {
        report.truncated = true;
    }
}

fn issue_finding(issue: &AssetIssue) -> (SupportSeverity, &'static str, &'static str) {
    match issue {
        AssetIssue::InvalidSidecar(_) => (
            SupportSeverity::Error,
            "invalid-sidecar",
            "Sidecar could not be parsed",
        ),
        AssetIssue::MismatchedSidecar(_) => (
            SupportSeverity::Error,
            "mismatched-sidecar",
            "Sidecar fingerprint does not match the adjacent asset",
        ),
        AssetIssue::UnreadableFile(_) => (
            SupportSeverity::Error,
            "unreadable-file",
            "asset metadata could not be read",
        ),
        AssetIssue::InvalidImageMetadata(_) => (
            SupportSeverity::Warning,
            "invalid-image-metadata",
            "image dimensions could not be parsed",
        ),
        AssetIssue::InvalidNativeMetadata(_) => (
            SupportSeverity::Warning,
            "invalid-native-metadata",
            "optional native metadata could not be parsed",
        ),
        AssetIssue::MimeMismatch(_) => (
            SupportSeverity::Warning,
            "mime-mismatch",
            "content signature overrides the file extension",
        ),
        AssetIssue::UnsafeEmbeddedContent(_) => (
            SupportSeverity::Warning,
            "unsafe-embedded-content",
            "active or external embedded content was isolated",
        ),
        AssetIssue::ResourceLimited(_) => (
            SupportSeverity::Warning,
            "resource-limited",
            "optional parsing stopped at a configured resource boundary",
        ),
        AssetIssue::MissingAsset => (
            SupportSeverity::Error,
            "missing-asset",
            "associated asset is missing",
        ),
        AssetIssue::UnsupportedFormat => (
            SupportSeverity::Warning,
            "unsupported-format",
            "asset format is not fully supported",
        ),
    }
}

#[must_use]
pub fn trace_asset(
    asset_id: Uuid,
    roots: &[LibraryRootStatus],
    records: &[AssetRecord],
) -> AssetTraceReport {
    let roots_by_id = roots
        .iter()
        .map(|root| (root.root.id, root))
        .collect::<BTreeMap<_, _>>();
    let matched_records = records
        .iter()
        .filter(|record| record.id == Some(asset_id))
        .collect::<Vec<_>>();
    let mut steps = vec![AssetTraceStep {
        match_index: None,
        stage: "catalog-lookup".into(),
        outcome: if matched_records.is_empty() {
            TraceOutcome::Error
        } else if matched_records.len() > 1 {
            TraceOutcome::Warning
        } else {
            TraceOutcome::Passed
        },
        code: if matched_records.is_empty() {
            "not-found"
        } else if matched_records.len() > 1 {
            "duplicate-id"
        } else {
            "unique-match"
        }
        .into(),
        message: format!(
            "catalog lookup returned {} record(s)",
            matched_records.len()
        ),
    }];
    let trace_matches = matched_records
        .iter()
        .enumerate()
        .map(|(index, record)| trace_match(index, asset_id, record, &roots_by_id, &mut steps))
        .collect::<Vec<_>>();
    AssetTraceReport {
        generated_unix_ms: now_unix_ms(),
        asset_id,
        match_count: trace_matches.len(),
        matches: trace_matches,
        steps,
    }
}

#[allow(clippy::too_many_lines)]
fn trace_match(
    index: usize,
    asset_id: Uuid,
    record: &AssetRecord,
    roots_by_id: &BTreeMap<Uuid, &LibraryRootStatus>,
    steps: &mut Vec<AssetTraceStep>,
) -> AssetTraceMatch {
    let root = record.root_id.and_then(|id| roots_by_id.get(&id).copied());
    steps.push(AssetTraceStep {
        match_index: Some(index),
        stage: "root-association".into(),
        outcome: if root.is_some() {
            TraceOutcome::Passed
        } else {
            TraceOutcome::Error
        },
        code: if root.is_some() {
            "configured-root"
        } else {
            "missing-root"
        }
        .into(),
        message: root.map_or_else(
            || "record is not associated with a configured root".into(),
            |root| format!("root access is {}", root.access_status),
        ),
    });

    let asset_present = fs::symlink_metadata(&record.path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
    steps.push(AssetTraceStep {
        match_index: Some(index),
        stage: "asset-file".into(),
        outcome: if asset_present {
            TraceOutcome::Passed
        } else {
            TraceOutcome::Error
        },
        code: if asset_present {
            "regular-file"
        } else {
            "missing-or-unsafe"
        }
        .into(),
        message: if asset_present {
            "asset is a readable regular file".into()
        } else {
            "asset is missing, unreadable, or a symbolic link".into()
        },
    });

    let sidecar_path = sidecar_path_for(&record.path);
    let sidecar_present = fs::symlink_metadata(&sidecar_path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
    let sidecar_id_matches = if sidecar_present {
        if let Ok((sidecar, _)) = read_sidecar_versioned(&sidecar_path) {
            let matches = sidecar.id == asset_id;
            steps.push(AssetTraceStep {
                match_index: Some(index),
                stage: "sidecar-parse".into(),
                outcome: if matches {
                    TraceOutcome::Passed
                } else {
                    TraceOutcome::Error
                },
                code: if matches { "id-matched" } else { "id-mismatch" }.into(),
                message: if matches {
                    "adjacent Sidecar parsed and supplied the requested stable ID".into()
                } else {
                    "adjacent Sidecar parsed but supplied a different stable ID".into()
                },
            });
            Some(matches)
        } else {
            steps.push(AssetTraceStep {
                match_index: Some(index),
                stage: "sidecar-parse".into(),
                outcome: TraceOutcome::Error,
                code: "invalid-sidecar".into(),
                message: "adjacent Sidecar exists but could not be parsed".into(),
            });
            None
        }
    } else {
        steps.push(AssetTraceStep {
            match_index: Some(index),
            stage: "sidecar-parse".into(),
            outcome: TraceOutcome::Error,
            code: "sidecar-missing".into(),
            message: "adjacent Sidecar is missing".into(),
        });
        None
    };
    let issue_codes = record
        .issues
        .iter()
        .map(|issue| issue_finding(issue).1.to_owned())
        .collect::<Vec<_>>();
    steps.push(AssetTraceStep {
        match_index: Some(index),
        stage: "parser-outcome".into(),
        outcome: if issue_codes.is_empty() {
            TraceOutcome::Passed
        } else {
            TraceOutcome::Warning
        },
        code: if issue_codes.is_empty() {
            "clean"
        } else {
            "issues-present"
        }
        .into(),
        message: format!(
            "catalog record contains {} parser issue(s)",
            issue_codes.len()
        ),
    });

    AssetTraceMatch {
        root_id: record.root_id,
        root_access_status: root.map(|root| root.access_status),
        relative_path: record.relative_path.clone(),
        path_fingerprint: path_fingerprint(&record.path),
        asset_present,
        sidecar_present,
        sidecar_id_matches,
        mime: record.mime.clone(),
        issue_codes,
    }
}

fn path_fingerprint(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    format!("{digest:x}")[..16].to_owned()
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use asset_core::{AssetIssue, AssetRecord};
    use asset_filesystem::{
        LibraryRoot, LibraryRootStatus, OrphanSidecar, OrphanSidecarState, ReconciliationReport,
        RootAccessStatus, RootScanSettings,
    };
    use metadata::{AssetSidecar, ExpectedVersion, sidecar_path_for, write_sidecar_atomic};
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{
        SupportSeverity, append_reconciliation_findings, inspect_library_consistency, trace_asset,
    };

    #[test]
    fn consistency_report_is_read_only_bounded_and_path_redacted() {
        let temp = tempdir().expect("tempdir");
        let root_id = Uuid::now_v7();
        let asset_id = Uuid::now_v7();
        let path = temp.path().join("asset.png");
        fs::write(&path, b"asset").expect("write asset");
        let mut record = AssetRecord::untagged(
            path.to_string_lossy().into_owned(),
            path.clone(),
            "image/png".into(),
            5,
            1,
        );
        record.root_id = Some(root_id);
        record.id = Some(asset_id);
        record.relative_path = "asset.png".into();
        record.issues.push(AssetIssue::InvalidNativeMetadata(
            temp.path().display().to_string(),
        ));
        let roots = vec![root(root_id, temp.path())];

        let report = inspect_library_consistency(&roots, &[record], true);

        assert_eq!(report.summary.catalog_assets, 1);
        assert_eq!(report.summary.warnings, 1);
        assert_eq!(report.findings[0].severity, SupportSeverity::Warning);
        let json = serde_json::to_string(&report).expect("json");
        assert!(!json.contains(&temp.path().display().to_string()));
        assert!(path.is_file());
    }

    #[test]
    fn trace_replays_adjacent_sidecar_identity_without_exposing_absolute_path() {
        let temp = tempdir().expect("tempdir");
        let root_id = Uuid::now_v7();
        let asset_id = Uuid::now_v7();
        let path = temp.path().join("asset.png");
        fs::write(&path, b"asset").expect("write asset");
        let mut sidecar = AssetSidecar::new();
        sidecar.id = asset_id;
        let receipt = write_sidecar_atomic(
            &sidecar_path_for(&path),
            &sidecar,
            &ExpectedVersion::Missing,
        )
        .expect("write Sidecar");
        let mut record = AssetRecord::untagged(
            path.to_string_lossy().into_owned(),
            path.clone(),
            "image/png".into(),
            5,
            1,
        );
        record.root_id = Some(root_id);
        record.id = Some(asset_id);
        record.relative_path = "asset.png".into();
        record.sidecar_path = Some(sidecar_path_for(&path));
        record.sidecar_state = Some(asset_core::SidecarState {
            schema: 1,
            digest: receipt.digest,
            size: receipt.size,
            modified_unix_ms: receipt.modified_unix_ms,
            updated_at: sidecar.updated_at,
        });

        let report = trace_asset(asset_id, &[root(root_id, temp.path())], &[record]);

        assert_eq!(report.match_count, 1);
        assert_eq!(report.matches[0].sidecar_id_matches, Some(true));
        assert!(report.steps.iter().any(|step| step.code == "id-matched"));
        let json = serde_json::to_string(&report).expect("json");
        assert!(!json.contains(&temp.path().display().to_string()));
    }

    #[test]
    fn duplicate_stable_ids_are_reported_without_guessing() {
        let temp = tempdir().expect("tempdir");
        let root_id = Uuid::now_v7();
        let asset_id = Uuid::now_v7();
        let records = ["one.png", "two.png"].map(|name| {
            let path = temp.path().join(name);
            fs::write(&path, b"asset").expect("write asset");
            let mut record = AssetRecord::untagged(
                path.to_string_lossy().into_owned(),
                path,
                "image/png".into(),
                5,
                1,
            );
            record.root_id = Some(root_id);
            record.id = Some(asset_id);
            record.relative_path = name.into();
            record
        });

        let report = inspect_library_consistency(&[root(root_id, temp.path())], &records, true);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == "duplicate-stable-id")
        );
        assert_eq!(
            trace_asset(asset_id, &[root(root_id, temp.path())], &records).match_count,
            2
        );
    }

    #[test]
    fn consistency_finding_details_are_bounded_while_totals_remain_exact() {
        let temp = tempdir().expect("tempdir");
        let root_id = Uuid::now_v7();
        let path = temp.path().join("asset.png");
        fs::write(&path, b"asset").expect("write asset");
        let records = (0..600)
            .map(|index| {
                let mut record = AssetRecord::untagged(
                    format!("asset-{index}"),
                    path.clone(),
                    "image/png".into(),
                    5,
                    1,
                );
                record.root_id = Some(root_id);
                record.relative_path = format!("asset-{index}.png").into();
                record
                    .issues
                    .push(AssetIssue::InvalidNativeMetadata("damaged".into()));
                record
            })
            .collect::<Vec<_>>();

        let report = inspect_library_consistency(&[root(root_id, temp.path())], &records, true);
        assert_eq!(report.summary.findings, 600);
        assert_eq!(report.findings.len(), 512);
        assert!(report.truncated);
    }

    #[test]
    fn consistency_report_includes_orphan_sidecars_as_relative_findings() {
        let temp = tempdir().expect("tempdir");
        let root_id = Uuid::now_v7();
        let root = root(root_id, temp.path());
        let sidecar_path = temp.path().join("lost.png.asset.yml");
        let mut report = inspect_library_consistency(std::slice::from_ref(&root), &[], true);
        append_reconciliation_findings(
            &mut report,
            &root,
            &ReconciliationReport {
                root_id,
                orphan_sidecars: vec![OrphanSidecar {
                    sidecar_id: None,
                    sidecar_path,
                    expected_asset_path: temp.path().join("lost.png"),
                    state: OrphanSidecarState::Invalid,
                    message: Some(temp.path().display().to_string()),
                    candidate_count: 0,
                }],
                missing_assets: Vec::new(),
                pending_moves: Vec::new(),
                sync_conflict_copies: Vec::new(),
            },
        );

        assert_eq!(report.findings[0].code, "invalid-orphan-sidecar");
        assert_eq!(
            report.findings[0].relative_path.as_deref(),
            Some(std::path::Path::new("lost.png.asset.yml"))
        );
        assert!(
            !serde_json::to_string(&report)
                .expect("json")
                .contains(&temp.path().display().to_string())
        );
    }

    fn root(id: Uuid, path: &std::path::Path) -> LibraryRootStatus {
        LibraryRootStatus {
            root: LibraryRoot {
                id,
                path: path.to_path_buf(),
                name: "Library".into(),
                enabled: true,
                scan: RootScanSettings::default(),
                extra: BTreeMap::default(),
            },
            access_status: RootAccessStatus::Available,
            access_message: None,
        }
    }
}
