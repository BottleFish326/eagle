use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use asset_catalog::AssetCatalog;
use asset_core::{AssetRecord, SidecarState};
use asset_selection::{SelectionError, SelectionSessionStore};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use metadata::{
    MetadataPatch, SidecarFileVersion, fingerprint_asset, read_sidecar_versioned, sidecar_path_for,
    validate_metadata_patch,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_PREFLIGHT_OPERATIONS: usize = 16;
pub const MAX_PREFLIGHT_TARGETS: usize = 100_000;
pub const MAX_PREFLIGHT_FAILURES: usize = 10_000;
pub const PREFLIGHT_TTL_MINUTES: i64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootRuntimeState {
    Available,
    Disabled,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRootAuthorization {
    pub id: Uuid,
    pub path: PathBuf,
    pub state: RootRuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataPreflightInput {
    pub snapshot_id: Uuid,
    pub patch: MetadataPatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BatchFailureKind {
    AssetMissing,
    AssetMovedAmbiguous,
    RootDisabled,
    RootOffline,
    AuthorizationLost,
    SourceChanged,
    SidecarConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPreflightFailure {
    pub key: String,
    pub kind: BatchFailureKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPreflightSummary {
    pub operation_id: Uuid,
    pub snapshot_id: Uuid,
    pub catalog_revision: u64,
    pub requested_count: usize,
    pub executable_count: usize,
    pub requires_stable_id_count: usize,
    pub unavailable_count: usize,
    pub conflict_count: usize,
    pub failure_count: usize,
    pub failures_truncated: bool,
    pub failures: Vec<BatchPreflightFailure>,
    pub confirmation_digest: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPreflightConfirmation {
    pub operation_id: Uuid,
    pub snapshot_id: Uuid,
    pub catalog_revision: u64,
    pub requested_count: usize,
    pub executable_count: usize,
    pub confirmation_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFileVersion {
    pub digest: String,
    pub size: u64,
    pub modified_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightTarget {
    pub key: String,
    pub stable_id: Option<Uuid>,
    pub root_id: Uuid,
    pub asset_path: PathBuf,
    pub source_version: SourceFileVersion,
    pub sidecar_version: Option<SidecarFileVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMetadataPreflight {
    pub summary: BatchPreflightSummary,
    pub patch: MetadataPatch,
    pub targets: Vec<PreflightTarget>,
}

#[derive(Debug, Error)]
pub enum BatchPreflightError {
    #[error("selection snapshot could not be resolved: {0}")]
    Selection(#[from] SelectionError),
    #[error("metadata operation is invalid")]
    InvalidOperation,
    #[error("batch preflight session state is unavailable")]
    StateUnavailable,
    #[error("batch preflight session budget is exhausted")]
    SessionBudgetExceeded,
    #[error("batch preflight operation was not found")]
    OperationNotFound,
    #[error("batch preflight operation expired")]
    OperationExpired,
    #[error("batch preflight confirmation no longer matches")]
    PreflightStale,
}

#[derive(Debug, Default)]
struct PreflightSessions {
    operations: BTreeMap<Uuid, StoredPreflight>,
    total_targets: usize,
}

#[derive(Debug, Clone)]
struct StoredPreflight {
    summary: BatchPreflightSummary,
    patch: MetadataPatch,
    targets: Vec<PreflightTarget>,
    expires_unix_ms: i64,
}

#[derive(Debug, Default)]
pub struct BatchPreflightStore {
    sessions: Mutex<PreflightSessions>,
}

impl BatchPreflightStore {
    /// Resolves an opaque selection against current files and stores a bounded,
    /// read-only execution plan. No Sidecar is created or changed.
    ///
    /// # Errors
    ///
    /// Rejects an invalid patch, missing snapshot, or exhausted runtime budget.
    pub fn prepare_metadata(
        &self,
        selections: &SelectionSessionStore,
        catalog: &AssetCatalog,
        roots: &[BatchRootAuthorization],
        input: &MetadataPreflightInput,
    ) -> Result<BatchPreflightSummary, BatchPreflightError> {
        validate_metadata_patch(&input.patch).map_err(|_| BatchPreflightError::InvalidOperation)?;
        let snapshot = selections.resolve(input.snapshot_id)?;
        let requested_count = snapshot.ordered_items.len();
        let root_map = roots
            .iter()
            .map(|root| (root.id, root))
            .collect::<BTreeMap<_, _>>();
        let mut targets = Vec::with_capacity(requested_count);
        let mut failures = Vec::new();
        let mut failure_count = 0;
        let mut unavailable_count = 0;
        let mut conflict_count = 0;
        let mut requires_stable_id_count = 0;

        for item in snapshot.ordered_items {
            let (record, moved) = match resolve_record(catalog, &item.key, item.stable_id) {
                Ok(value) => value,
                Err(kind) => {
                    push_failure(&mut failures, &mut failure_count, &item.key, kind);
                    if matches!(kind, BatchFailureKind::AssetMovedAmbiguous) {
                        conflict_count += 1;
                    } else {
                        unavailable_count += 1;
                    }
                    continue;
                }
            };
            if moved {
                requires_stable_id_count += 1;
            }
            match inspect_target(&record, &root_map) {
                Ok(target) => targets.push(target),
                Err(kind) => {
                    push_failure(&mut failures, &mut failure_count, &item.key, kind);
                    if matches!(
                        kind,
                        BatchFailureKind::SourceChanged
                            | BatchFailureKind::SidecarConflict
                            | BatchFailureKind::AssetMovedAmbiguous
                    ) {
                        conflict_count += 1;
                    } else {
                        unavailable_count += 1;
                    }
                }
            }
        }

        self.store(
            input,
            catalog.revision(),
            requested_count,
            requires_stable_id_count,
            unavailable_count,
            conflict_count,
            failure_count,
            failures,
            targets,
            Utc::now(),
        )
    }

    /// Resolves a confirmed preflight without exposing its paths over IPC.
    ///
    /// # Errors
    ///
    /// Rejects missing, expired, or mismatched confirmations.
    pub fn resolve_metadata(
        &self,
        confirmation: &BatchPreflightConfirmation,
    ) -> Result<ResolvedMetadataPreflight, BatchPreflightError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| BatchPreflightError::StateUnavailable)?;
        let now = Utc::now().timestamp_millis();
        let Some(stored) = sessions.operations.get(&confirmation.operation_id) else {
            return Err(BatchPreflightError::OperationNotFound);
        };
        if stored.expires_unix_ms <= now {
            remove_operation(&mut sessions, confirmation.operation_id);
            return Err(BatchPreflightError::OperationExpired);
        }
        if stored.summary.snapshot_id != confirmation.snapshot_id
            || stored.summary.catalog_revision != confirmation.catalog_revision
            || stored.summary.requested_count != confirmation.requested_count
            || stored.summary.executable_count != confirmation.executable_count
            || stored.summary.confirmation_digest != confirmation.confirmation_digest
        {
            return Err(BatchPreflightError::PreflightStale);
        }
        Ok(ResolvedMetadataPreflight {
            summary: stored.summary.clone(),
            patch: stored.patch.clone(),
            targets: stored.targets.clone(),
        })
    }

    /// Releases one ephemeral preflight plan.
    ///
    /// # Errors
    ///
    /// Returns a state error if the bounded store is unavailable.
    pub fn release(&self, operation_id: Uuid) -> Result<bool, BatchPreflightError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| BatchPreflightError::StateUnavailable)?;
        Ok(remove_operation(&mut sessions, operation_id))
    }

    #[allow(clippy::too_many_arguments)]
    fn store(
        &self,
        input: &MetadataPreflightInput,
        catalog_revision: u64,
        requested_count: usize,
        requires_stable_id_count: usize,
        unavailable_count: usize,
        conflict_count: usize,
        failure_count: usize,
        failures: Vec<BatchPreflightFailure>,
        targets: Vec<PreflightTarget>,
        now: DateTime<Utc>,
    ) -> Result<BatchPreflightSummary, BatchPreflightError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| BatchPreflightError::StateUnavailable)?;
        prune_expired(&mut sessions, now.timestamp_millis());
        if sessions.operations.len() >= MAX_PREFLIGHT_OPERATIONS
            || sessions.total_targets.saturating_add(targets.len()) > MAX_PREFLIGHT_TARGETS
        {
            return Err(BatchPreflightError::SessionBudgetExceeded);
        }
        let operation_id = Uuid::now_v7();
        let expires = now + Duration::minutes(PREFLIGHT_TTL_MINUTES);
        let digest = confirmation_digest(
            operation_id,
            input.snapshot_id,
            catalog_revision,
            requested_count,
            &input.patch,
            &targets,
        );
        let summary = BatchPreflightSummary {
            operation_id,
            snapshot_id: input.snapshot_id,
            catalog_revision,
            requested_count,
            executable_count: targets.len(),
            requires_stable_id_count,
            unavailable_count,
            conflict_count,
            failure_count,
            failures_truncated: failure_count > failures.len(),
            failures,
            confirmation_digest: digest,
            created_at: timestamp(now),
            expires_at: timestamp(expires),
        };
        sessions.total_targets += targets.len();
        sessions.operations.insert(
            operation_id,
            StoredPreflight {
                summary: summary.clone(),
                patch: input.patch.clone(),
                targets,
                expires_unix_ms: expires.timestamp_millis(),
            },
        );
        Ok(summary)
    }
}

fn resolve_record(
    catalog: &AssetCatalog,
    key: &str,
    stable_id: Option<Uuid>,
) -> Result<(AssetRecord, bool), BatchFailureKind> {
    if let Some(record) = catalog.get(key) {
        return Ok((record.clone(), false));
    }
    let Some(stable_id) = stable_id else {
        return Err(BatchFailureKind::AssetMissing);
    };
    let matches = catalog.records_with_id(stable_id);
    match matches.as_slice() {
        [record] => Ok((record.clone(), true)),
        [] => Err(BatchFailureKind::AssetMissing),
        _ => Err(BatchFailureKind::AssetMovedAmbiguous),
    }
}

fn inspect_target(
    record: &AssetRecord,
    roots: &BTreeMap<Uuid, &BatchRootAuthorization>,
) -> Result<PreflightTarget, BatchFailureKind> {
    let root_id = record.root_id.ok_or(BatchFailureKind::AuthorizationLost)?;
    let root = roots
        .get(&root_id)
        .ok_or(BatchFailureKind::AuthorizationLost)?;
    match root.state {
        RootRuntimeState::Disabled => return Err(BatchFailureKind::RootDisabled),
        RootRuntimeState::Offline => return Err(BatchFailureKind::RootOffline),
        RootRuntimeState::Available => {}
    }
    let canonical_root = root
        .path
        .canonicalize()
        .map_err(|_| BatchFailureKind::RootOffline)?;
    let canonical_asset = record
        .path
        .canonicalize()
        .map_err(|_| BatchFailureKind::AssetMissing)?;
    if !canonical_asset.starts_with(&canonical_root) || !canonical_asset.is_file() {
        return Err(BatchFailureKind::AuthorizationLost);
    }
    let metadata = fs::metadata(&canonical_asset).map_err(|_| BatchFailureKind::AssetMissing)?;
    let modified_unix_ms = modified_unix_ms(&metadata).ok_or(BatchFailureKind::SourceChanged)?;
    if record.size != Some(metadata.len()) || record.modified_unix_ms != Some(modified_unix_ms) {
        return Err(BatchFailureKind::SourceChanged);
    }
    let fingerprint =
        fingerprint_asset(&canonical_asset).map_err(|_| BatchFailureKind::SourceChanged)?;
    let sidecar_path = sidecar_path_for(&canonical_asset);
    let sidecar_version = if sidecar_path.is_file() {
        let (_, version) =
            read_sidecar_versioned(&sidecar_path).map_err(|_| BatchFailureKind::SidecarConflict)?;
        if !sidecar_matches(record.sidecar_state.as_ref(), Some(&version)) {
            return Err(BatchFailureKind::SidecarConflict);
        }
        Some(version)
    } else {
        if record.sidecar_state.is_some() {
            return Err(BatchFailureKind::SidecarConflict);
        }
        None
    };
    Ok(PreflightTarget {
        key: record.key.clone(),
        stable_id: record.id,
        root_id,
        asset_path: canonical_asset,
        source_version: SourceFileVersion {
            digest: fingerprint.value,
            size: metadata.len(),
            modified_unix_ms,
        },
        sidecar_version,
    })
}

fn sidecar_matches(expected: Option<&SidecarState>, actual: Option<&SidecarFileVersion>) -> bool {
    match (expected, actual) {
        (None, None) => true,
        (Some(expected), Some(actual)) => {
            expected.digest == actual.digest
                && expected.size == actual.size
                && expected.modified_unix_ms == actual.modified_unix_ms
        }
        _ => false,
    }
}

fn push_failure(
    failures: &mut Vec<BatchPreflightFailure>,
    failure_count: &mut usize,
    key: &str,
    kind: BatchFailureKind,
) {
    *failure_count += 1;
    if failures.len() < MAX_PREFLIGHT_FAILURES {
        failures.push(BatchPreflightFailure {
            key: key.to_owned(),
            kind,
            message: failure_message(kind).into(),
        });
    }
}

const fn failure_message(kind: BatchFailureKind) -> &'static str {
    match kind {
        BatchFailureKind::AssetMissing => "asset is no longer available",
        BatchFailureKind::AssetMovedAmbiguous => "stable ID resolves to multiple assets",
        BatchFailureKind::RootDisabled => "library root is disabled",
        BatchFailureKind::RootOffline => "library root is offline",
        BatchFailureKind::AuthorizationLost => "asset is outside the authorized root",
        BatchFailureKind::SourceChanged => "source file changed since cataloging",
        BatchFailureKind::SidecarConflict => "Sidecar changed since cataloging",
    }
}

fn confirmation_digest(
    operation_id: Uuid,
    snapshot_id: Uuid,
    catalog_revision: u64,
    requested_count: usize,
    patch: &MetadataPatch,
    targets: &[PreflightTarget],
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"material-eagle-batch-preflight-v1\0");
    digest.update(operation_id.as_bytes());
    digest.update(snapshot_id.as_bytes());
    digest.update(catalog_revision.to_le_bytes());
    digest.update(requested_count.to_le_bytes());
    digest.update(format!("{patch:?}").as_bytes());
    for target in targets {
        digest.update(target.key.as_bytes());
        digest.update([0]);
        digest.update(target.root_id.as_bytes());
        digest.update(target.source_version.digest.as_bytes());
        if let Some(sidecar) = &target.sidecar_version {
            digest.update(sidecar.digest.as_bytes());
            digest.update(sidecar.size.to_le_bytes());
            digest.update(sidecar.modified_unix_ms.to_le_bytes());
        }
    }
    format!("{:x}", digest.finalize())
}

fn modified_unix_ms(metadata: &fs::Metadata) -> Option<i64> {
    let duration = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(duration.as_millis()).ok()
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn prune_expired(sessions: &mut PreflightSessions, now_unix_ms: i64) {
    let expired = sessions
        .operations
        .iter()
        .filter_map(|(id, operation)| (operation.expires_unix_ms <= now_unix_ms).then_some(*id))
        .collect::<Vec<_>>();
    for id in expired {
        remove_operation(sessions, id);
    }
}

fn remove_operation(sessions: &mut PreflightSessions, id: Uuid) -> bool {
    let Some(operation) = sessions.operations.remove(&id) else {
        return false;
    };
    sessions.total_targets = sessions
        .total_targets
        .saturating_sub(operation.targets.len());
    true
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use asset_core::{AssetRecord, SidecarState};
    use asset_selection::ExplicitSelectionInput;
    use metadata::{MetadataPatch, edit_asset_metadata};
    use tempfile::tempdir;

    use super::*;

    #[test]
    #[allow(clippy::too_many_lines)]
    fn preflight_converges_one_move_and_reports_missing_ambiguity_and_conflict() {
        let directory = tempdir().expect("temp root");
        let root_id = Uuid::now_v7();
        let good = write_record(directory.path(), root_id, "good", Uuid::now_v7());
        let moved_id = Uuid::now_v7();
        let moved = write_record(directory.path(), root_id, "moved-old", moved_id);
        let missing = write_record(directory.path(), root_id, "missing", Uuid::now_v7());
        let ambiguous_id = Uuid::now_v7();
        let ambiguous = write_record(directory.path(), root_id, "ambiguous-old", ambiguous_id);
        let mut conflict = write_record(directory.path(), root_id, "conflict", Uuid::now_v7());
        let edit = edit_asset_metadata(
            &conflict.path,
            None,
            &MetadataPatch {
                favorite: Some(true),
                ..MetadataPatch::default()
            },
        )
        .expect("initial conflict sidecar");
        conflict.id = Some(edit.sidecar.id);
        conflict.sidecar_path = Some(edit.sidecar_path.clone());
        conflict.sidecar_state = Some(SidecarState {
            schema: edit.sidecar.schema,
            digest: edit.digest,
            size: edit.size,
            modified_unix_ms: edit.modified_unix_ms,
            updated_at: edit.sidecar.updated_at,
        });

        let mut catalog = AssetCatalog::default();
        catalog.ingest([
            good.clone(),
            moved.clone(),
            missing.clone(),
            ambiguous.clone(),
            conflict.clone(),
        ]);
        let selections = SelectionSessionStore::default();
        let snapshot = selections
            .create_explicit_snapshot(
                &catalog,
                &ExplicitSelectionInput {
                    expected_catalog_revision: catalog.revision(),
                    keys: vec![
                        good.key.clone(),
                        moved.key.clone(),
                        missing.key.clone(),
                        ambiguous.key.clone(),
                        conflict.key.clone(),
                    ],
                },
            )
            .expect("selection snapshot");

        let moved_path = directory.path().join("moved-new.png");
        fs::rename(&moved.path, &moved_path).expect("move asset");
        let mut moved_new = record_for_path(root_id, "moved-new", moved_path, moved_id);
        moved_new.relative_path = PathBuf::from("moved-new.png");
        let ambiguous_a = write_record(directory.path(), root_id, "ambiguous-a", ambiguous_id);
        let ambiguous_b = write_record(directory.path(), root_id, "ambiguous-b", ambiguous_id);
        fs::remove_file(&missing.path).expect("remove missing asset");
        fs::write(
            conflict.sidecar_path.as_ref().expect("conflict sidecar"),
            b"external: invalid\n",
        )
        .expect("change sidecar externally");
        catalog.clear();
        catalog.ingest([
            good.clone(),
            moved_new,
            ambiguous_a,
            ambiguous_b,
            conflict.clone(),
        ]);

        let source_before = fs::read(&good.path).expect("good source before");
        let preflights = BatchPreflightStore::default();
        let summary = preflights
            .prepare_metadata(
                &selections,
                &catalog,
                &[BatchRootAuthorization {
                    id: root_id,
                    path: directory.path().to_path_buf(),
                    state: RootRuntimeState::Available,
                }],
                &MetadataPreflightInput {
                    snapshot_id: snapshot.id,
                    patch: MetadataPatch {
                        add_tags: BTreeSet::from(["batch/ready".into()]),
                        ..MetadataPatch::default()
                    },
                },
            )
            .expect("read-only preflight");

        assert_eq!(summary.requested_count, 5);
        assert_eq!(summary.executable_count, 2);
        assert_eq!(summary.requires_stable_id_count, 1);
        assert_eq!(summary.unavailable_count, 1);
        assert_eq!(summary.conflict_count, 2);
        assert_eq!(summary.failure_count, 3);
        assert_eq!(
            summary
                .failures
                .iter()
                .map(|failure| failure.kind)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                BatchFailureKind::AssetMissing,
                BatchFailureKind::AssetMovedAmbiguous,
                BatchFailureKind::SidecarConflict,
            ])
        );
        assert_eq!(
            fs::read(&good.path).expect("good source after"),
            source_before
        );
        assert!(!sidecar_path_for(&good.path).exists());

        let confirmation = confirmation(&summary);
        let resolved = preflights
            .resolve_metadata(&confirmation)
            .expect("confirmed plan");
        assert_eq!(resolved.targets.len(), 2);
        assert_eq!(resolved.targets[1].key, "moved-new");

        let mut stale = confirmation;
        stale.executable_count += 1;
        assert!(matches!(
            preflights.resolve_metadata(&stale),
            Err(BatchPreflightError::PreflightStale)
        ));
        assert!(preflights.release(summary.operation_id).expect("release"));
    }

    #[test]
    fn disabled_root_and_invalid_patch_never_create_an_operation() {
        let directory = tempdir().expect("temp root");
        let root_id = Uuid::now_v7();
        let record = write_record(directory.path(), root_id, "disabled", Uuid::now_v7());
        let mut catalog = AssetCatalog::default();
        catalog.ingest([record.clone()]);
        let selections = SelectionSessionStore::default();
        let snapshot = selections
            .create_explicit_snapshot(
                &catalog,
                &ExplicitSelectionInput {
                    expected_catalog_revision: catalog.revision(),
                    keys: vec![record.key],
                },
            )
            .expect("snapshot");
        let preflights = BatchPreflightStore::default();
        assert!(matches!(
            preflights.prepare_metadata(
                &selections,
                &catalog,
                &[],
                &MetadataPreflightInput {
                    snapshot_id: snapshot.id,
                    patch: MetadataPatch::default(),
                },
            ),
            Err(BatchPreflightError::InvalidOperation)
        ));
        let summary = preflights
            .prepare_metadata(
                &selections,
                &catalog,
                &[BatchRootAuthorization {
                    id: root_id,
                    path: directory.path().to_path_buf(),
                    state: RootRuntimeState::Disabled,
                }],
                &MetadataPreflightInput {
                    snapshot_id: snapshot.id,
                    patch: MetadataPatch {
                        rating: Some(4),
                        ..MetadataPatch::default()
                    },
                },
            )
            .expect("disabled-root summary");
        assert_eq!(summary.executable_count, 0);
        assert_eq!(summary.unavailable_count, 1);
        assert_eq!(summary.failures[0].kind, BatchFailureKind::RootDisabled);
    }

    fn confirmation(summary: &BatchPreflightSummary) -> BatchPreflightConfirmation {
        BatchPreflightConfirmation {
            operation_id: summary.operation_id,
            snapshot_id: summary.snapshot_id,
            catalog_revision: summary.catalog_revision,
            requested_count: summary.requested_count,
            executable_count: summary.executable_count,
            confirmation_digest: summary.confirmation_digest.clone(),
        }
    }

    fn write_record(root: &std::path::Path, root_id: Uuid, key: &str, id: Uuid) -> AssetRecord {
        let path = root.join(format!("{key}.png"));
        fs::write(&path, format!("source-{key}")).expect("write asset");
        let mut record = record_for_path(root_id, key, path, id);
        record.relative_path = PathBuf::from(format!("{key}.png"));
        record
    }

    fn record_for_path(root_id: Uuid, key: &str, path: PathBuf, id: Uuid) -> AssetRecord {
        let metadata = fs::metadata(&path).expect("asset metadata");
        let modified = modified_unix_ms(&metadata).expect("asset mtime");
        let mut record = AssetRecord::untagged(
            key.into(),
            path,
            "image/png".into(),
            metadata.len(),
            modified,
        );
        record.root_id = Some(root_id);
        record.id = Some(id);
        record
    }
}
