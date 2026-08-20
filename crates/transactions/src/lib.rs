use std::collections::BTreeSet;
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use asset_core::AssetRecord;
use chrono::{SecondsFormat, Utc};
use metadata::{
    AssetSidecar, ExpectedVersion, MetadataPatch, SidecarError, SidecarFileVersion, digest_file,
    fingerprint_asset, inspect_sidecar_version, prepare_asset_metadata_edit,
    prepare_asset_metadata_edit_versioned, remove_sidecar_if_version,
    restore_sidecar_content_atomic, sidecar_path_for,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

const TRANSACTION_SCHEMA: u32 = 1;
const JOURNAL_CHECKPOINT_ITEMS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionTarget {
    pub key: String,
    pub root_id: Uuid,
    pub asset_path: PathBuf,
    pub expected_sidecar_digest: Option<String>,
    pub expected_sidecar_size: Option<u64>,
    pub expected_sidecar_modified_unix_ms: Option<i64>,
}

impl TransactionTarget {
    #[must_use]
    pub fn from_record(
        record: &AssetRecord,
        expected_sidecar_digest: Option<String>,
        expected_sidecar_size: Option<u64>,
        expected_sidecar_modified_unix_ms: Option<i64>,
    ) -> Option<Self> {
        Some(Self {
            key: record.key.clone(),
            root_id: record.root_id?,
            asset_path: record.path.clone(),
            expected_sidecar_digest,
            expected_sidecar_size,
            expected_sidecar_modified_unix_ms,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionState {
    Active,
    Completed,
    Restored,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TransactionItemState {
    Planned,
    Applied,
    Failed,
    Restored,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSummary {
    pub id: Uuid,
    pub state: TransactionState,
    pub created_at: String,
    pub updated_at: String,
    pub item_count: usize,
    pub applied_count: usize,
    pub failed_count: usize,
    pub conflict_count: usize,
    pub restored_count: usize,
    pub root_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionScopeItem {
    pub root_id: Uuid,
    pub asset_path: PathBuf,
    pub sidecar_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionFailureKind {
    Conflict,
    InvalidInput,
    WriteFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionItemFailure {
    pub key: String,
    pub kind: TransactionFailureKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommittedSidecar {
    pub key: String,
    pub sidecar_path: PathBuf,
    pub sidecar: AssetSidecar,
    pub version: SidecarFileVersion,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransactionExecution {
    pub summary: TransactionSummary,
    pub committed: Vec<CommittedSidecar>,
    pub failures: Vec<TransactionItemFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRecoveryResult {
    pub summary: TransactionSummary,
    pub failures: Vec<TransactionItemFailure>,
}

#[derive(Debug, Clone)]
pub struct MetadataTransactionStore {
    directory: PathBuf,
}

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("metadata transaction requires at least one target")]
    NoTargets,
    #[error("metadata transaction requires at least two targets")]
    TooFewTargets,
    #[error("metadata transaction was not found: {0}")]
    NotFound(Uuid),
    #[error("invalid metadata transaction at {path}: {message}")]
    InvalidJournal { path: PathBuf, message: String },
    #[error("metadata transaction I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot serialize metadata transaction: {0}")]
    Serialize(#[from] serde_yaml_ng::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionJournal {
    schema: u32,
    id: Uuid,
    created_at: String,
    updated_at: String,
    state: TransactionState,
    patch: MetadataPatch,
    items: Vec<TransactionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionItem {
    key: String,
    root_id: Uuid,
    asset_path: PathBuf,
    sidecar_path: PathBuf,
    original_digest: Option<String>,
    original_content: Option<String>,
    planned_digest: Option<String>,
    planned_content: Option<String>,
    state: TransactionItemState,
    failure_kind: Option<TransactionFailureKind>,
    failure: Option<String>,
}

#[derive(Debug)]
struct OperationFailure {
    kind: TransactionFailureKind,
    message: String,
}

impl MetadataTransactionStore {
    /// Opens the pure-file transaction journal directory.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] when the directory cannot be created.
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, TransactionError> {
        let directory = directory.into();
        fs::create_dir_all(&directory).map_err(|source| TransactionError::Io {
            path: directory.clone(),
            source,
        })?;
        Ok(Self { directory })
    }

    /// Creates a durable plan before applying any Sidecar and records every item result.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] when the journal cannot be persisted.
    pub fn execute(
        &self,
        targets: &[TransactionTarget],
        patch: &MetadataPatch,
    ) -> Result<TransactionExecution, TransactionError> {
        self.execute_with_limit(targets, patch, None)
    }

    /// Persists a durable Sidecar plan without applying any item.
    ///
    /// Plan-only mode accepts one or more targets because a single Sidecar may
    /// still need coordination with `saved-filters.yml`. The returned journal
    /// can be applied through [`Self::continue_transaction`] after a separate
    /// coordinator record is durable.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] for an empty target set or when the journal
    /// cannot be prepared and persisted.
    pub fn plan(
        &self,
        targets: &[TransactionTarget],
        patch: &MetadataPatch,
    ) -> Result<TransactionSummary, TransactionError> {
        if targets.is_empty() {
            return Err(TransactionError::NoTargets);
        }
        let journal = Self::prepare_journal(targets, patch);
        self.persist(&journal)?;
        Ok(journal.summary())
    }

    /// Reconciles one retained journal with current Sidecar bytes and returns
    /// its latest summary without applying or restoring any item.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] when the journal is missing, invalid, or
    /// cannot persist its reconciled state.
    pub fn summary(&self, id: Uuid) -> Result<TransactionSummary, TransactionError> {
        let mut journal = self.load(id)?;
        if journal.state == TransactionState::Active {
            reconcile_actual_states(&mut journal);
            journal.finish_from_items();
            self.persist(&journal)?;
        }
        Ok(journal.summary())
    }

    /// Lists every retained transaction journal in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] when a journal cannot be read or parsed.
    pub fn list(&self) -> Result<Vec<TransactionSummary>, TransactionError> {
        let mut paths = fs::read_dir(&self.directory)
            .map_err(|source| TransactionError::Io {
                path: self.directory.clone(),
                source,
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_transaction_path(path))
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .iter()
            .map(|path| {
                let mut journal = read_journal(path)?;
                validate_journal_identity(path, &journal, transaction_id_from_path(path)?)?;
                if journal.state == TransactionState::Active {
                    reconcile_actual_states(&mut journal);
                    self.persist(&journal)?;
                }
                Ok(journal.summary())
            })
            .collect()
    }

    /// Returns the file scope recorded by a transaction for authorization checks.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] when the journal cannot be read.
    pub fn scope(&self, id: Uuid) -> Result<Vec<TransactionScopeItem>, TransactionError> {
        Ok(self
            .load(id)?
            .items
            .into_iter()
            .map(|item| TransactionScopeItem {
                root_id: item.root_id,
                asset_path: item.asset_path,
                sidecar_path: item.sidecar_path,
            })
            .collect())
    }

    /// Continues every item that is still provably at its original version.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] when the journal cannot be read or updated.
    pub fn continue_transaction(
        &self,
        id: Uuid,
    ) -> Result<TransactionRecoveryResult, TransactionError> {
        let mut journal = self.load(id)?;
        reconcile_actual_states(&mut journal);
        self.persist(&journal)?;
        let mut failures = Vec::new();
        let mut processed = 0;
        let mut applied = 0;
        for index in 0..journal.items.len() {
            if journal.items[index].state != TransactionItemState::Planned {
                continue;
            }
            match apply_planned_item(&journal.items[index]) {
                Ok(()) => {
                    journal.items[index].state = TransactionItemState::Applied;
                    applied += 1;
                    inject_fault_after_applied(applied);
                }
                Err(message) => {
                    journal.items[index].state = TransactionItemState::Conflict;
                    journal.items[index].failure_kind = Some(message.kind);
                    journal.items[index].failure = Some(message.message.clone());
                    failures.push(TransactionItemFailure {
                        key: journal.items[index].key.clone(),
                        kind: message.kind,
                        message: message.message,
                    });
                }
            }
            journal.touch();
            processed += 1;
            if processed % JOURNAL_CHECKPOINT_ITEMS == 0 {
                self.persist(&journal)?;
            }
        }
        journal.finish_from_items();
        self.persist(&journal)?;
        Ok(TransactionRecoveryResult {
            summary: journal.summary(),
            failures,
        })
    }

    /// Restores applied items only when their current digest still equals the plan.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] when the journal cannot be read or updated.
    pub fn restore_transaction(
        &self,
        id: Uuid,
    ) -> Result<TransactionRecoveryResult, TransactionError> {
        let mut journal = self.load(id)?;
        reconcile_actual_states(&mut journal);
        self.persist(&journal)?;
        let mut failures = Vec::new();
        let mut processed = 0;
        for index in 0..journal.items.len() {
            let result = match journal.items[index].state {
                TransactionItemState::Applied => restore_applied_item(&journal.items[index]),
                TransactionItemState::Planned | TransactionItemState::Failed => Ok(()),
                TransactionItemState::Restored => continue,
                TransactionItemState::Conflict => {
                    let message = journal.items[index]
                        .failure
                        .clone()
                        .unwrap_or_else(|| "Sidecar 在事务后发生了外部修改".into());
                    failures.push(TransactionItemFailure {
                        key: journal.items[index].key.clone(),
                        kind: journal.items[index]
                            .failure_kind
                            .unwrap_or(TransactionFailureKind::Conflict),
                        message,
                    });
                    continue;
                }
            };
            match result {
                Ok(()) => journal.items[index].state = TransactionItemState::Restored,
                Err(message) => {
                    journal.items[index].state = TransactionItemState::Conflict;
                    journal.items[index].failure_kind = Some(message.kind);
                    journal.items[index].failure = Some(message.message.clone());
                    failures.push(TransactionItemFailure {
                        key: journal.items[index].key.clone(),
                        kind: message.kind,
                        message: message.message,
                    });
                }
            }
            journal.touch();
            processed += 1;
            if processed % JOURNAL_CHECKPOINT_ITEMS == 0 {
                self.persist(&journal)?;
            }
        }
        journal.state = if journal
            .items
            .iter()
            .any(|item| item.state == TransactionItemState::Conflict)
        {
            TransactionState::Conflict
        } else {
            TransactionState::Restored
        };
        journal.touch();
        self.persist(&journal)?;
        Ok(TransactionRecoveryResult {
            summary: journal.summary(),
            failures,
        })
    }

    /// Deletes only the selected transaction journal, never an asset or Sidecar.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] when the journal does not exist or cannot be removed.
    pub fn dismiss(&self, id: Uuid) -> Result<(), TransactionError> {
        let path = self.path(id);
        if !path.is_file() {
            return Err(TransactionError::NotFound(id));
        }
        fs::remove_file(&path).map_err(|source| TransactionError::Io { path, source })?;
        sync_directory(&self.directory)
    }

    fn execute_with_limit(
        &self,
        targets: &[TransactionTarget],
        patch: &MetadataPatch,
        maximum_applied: Option<usize>,
    ) -> Result<TransactionExecution, TransactionError> {
        if targets.len() < 2 {
            return Err(TransactionError::TooFewTargets);
        }
        let mut journal = Self::prepare_journal(targets, patch);
        self.persist(&journal)?;
        let mut committed = Vec::new();
        let mut failures = journal
            .items
            .iter()
            .filter(|item| item.state == TransactionItemState::Failed)
            .map(|item| TransactionItemFailure {
                key: item.key.clone(),
                kind: item
                    .failure_kind
                    .unwrap_or(TransactionFailureKind::WriteFailed),
                message: item.failure.clone().unwrap_or_else(|| "计划失败".into()),
            })
            .collect::<Vec<_>>();
        let mut applied = 0;
        let mut processed = 0;
        for index in 0..journal.items.len() {
            if journal.items[index].state != TransactionItemState::Planned {
                continue;
            }
            if maximum_applied.is_some_and(|maximum| applied >= maximum) {
                break;
            }
            match apply_planned_item(&journal.items[index]) {
                Ok(()) => {
                    journal.items[index].state = TransactionItemState::Applied;
                    applied += 1;
                    if let Some(committed_sidecar) = journal.items[index].committed_sidecar() {
                        committed.push(committed_sidecar);
                    }
                    inject_fault_after_applied(applied);
                }
                Err(message) => {
                    journal.items[index].state = TransactionItemState::Failed;
                    journal.items[index].failure_kind = Some(message.kind);
                    journal.items[index].failure = Some(message.message.clone());
                    failures.push(TransactionItemFailure {
                        key: journal.items[index].key.clone(),
                        kind: message.kind,
                        message: message.message,
                    });
                }
            }
            journal.touch();
            processed += 1;
            if processed % JOURNAL_CHECKPOINT_ITEMS == 0 {
                self.persist(&journal)?;
            }
        }
        if maximum_applied.is_none() {
            journal.finish_from_items();
            self.persist(&journal)?;
        }
        Ok(TransactionExecution {
            summary: journal.summary(),
            committed,
            failures,
        })
    }

    fn prepare_journal(targets: &[TransactionTarget], patch: &MetadataPatch) -> TransactionJournal {
        let now = now_rfc3339();
        TransactionJournal {
            schema: TRANSACTION_SCHEMA,
            id: Uuid::now_v7(),
            created_at: now.clone(),
            updated_at: now,
            state: TransactionState::Active,
            patch: patch.clone(),
            items: targets
                .iter()
                .map(|target| prepare_item(target, patch))
                .collect(),
        }
    }

    fn load(&self, id: Uuid) -> Result<TransactionJournal, TransactionError> {
        let path = self.path(id);
        if !path.is_file() {
            return Err(TransactionError::NotFound(id));
        }
        let journal = read_journal(&path)?;
        validate_journal_identity(&path, &journal, id)?;
        Ok(journal)
    }

    fn persist(&self, journal: &TransactionJournal) -> Result<(), TransactionError> {
        let path = self.path(journal.id);
        let mut content = serde_yaml_ng::to_string(journal)?;
        if !content.ends_with('\n') {
            content.push('\n');
        }
        let mut temp =
            NamedTempFile::new_in(&self.directory).map_err(|source| TransactionError::Io {
                path: self.directory.clone(),
                source,
            })?;
        temp.write_all(content.as_bytes())
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|source| TransactionError::Io {
                path: temp.path().to_path_buf(),
                source,
            })?;
        temp.persist(&path).map_err(|error| TransactionError::Io {
            path: path.clone(),
            source: error.error,
        })?;
        sync_directory(&self.directory)
    }

    fn path(&self, id: Uuid) -> PathBuf {
        self.directory.join(format!("{id}.transaction.yml"))
    }
}

impl TransactionJournal {
    fn summary(&self) -> TransactionSummary {
        let root_ids = self
            .items
            .iter()
            .map(|item| item.root_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        TransactionSummary {
            id: self.id,
            state: self.state,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            item_count: self.items.len(),
            applied_count: count_items(&self.items, TransactionItemState::Applied),
            failed_count: count_items(&self.items, TransactionItemState::Failed),
            conflict_count: count_items(&self.items, TransactionItemState::Conflict),
            restored_count: count_items(&self.items, TransactionItemState::Restored),
            root_ids,
        }
    }

    fn touch(&mut self) {
        self.updated_at = now_rfc3339();
    }

    fn finish_from_items(&mut self) {
        self.state = if self
            .items
            .iter()
            .any(|item| item.state == TransactionItemState::Conflict)
        {
            TransactionState::Conflict
        } else if self
            .items
            .iter()
            .any(|item| item.state == TransactionItemState::Planned)
        {
            TransactionState::Active
        } else {
            TransactionState::Completed
        };
        self.touch();
    }
}

impl TransactionItem {
    fn committed_sidecar(&self) -> Option<CommittedSidecar> {
        let content = self.planned_content.as_deref()?;
        Some(CommittedSidecar {
            key: self.key.clone(),
            sidecar_path: self.sidecar_path.clone(),
            sidecar: serde_yaml_ng::from_str(content).ok()?,
            version: inspect_sidecar_version(&self.sidecar_path).ok()?,
        })
    }
}

fn prepare_item(target: &TransactionTarget, patch: &MetadataPatch) -> TransactionItem {
    let sidecar_path = sidecar_path_for(&target.asset_path);
    let original_content = fs::read_to_string(&sidecar_path).ok();
    let original_digest = original_content.as_deref().map(digest_content);
    let expected_version = target.expected_sidecar_digest.as_ref().and_then(|digest| {
        Some(SidecarFileVersion {
            digest: digest.clone(),
            size: target.expected_sidecar_size?,
            modified_unix_ms: target.expected_sidecar_modified_unix_ms?,
        })
    });
    let prepared = if expected_version.is_some() || target.expected_sidecar_digest.is_none() {
        prepare_asset_metadata_edit_versioned(&target.asset_path, expected_version.as_ref(), patch)
    } else {
        prepare_asset_metadata_edit(
            &target.asset_path,
            target.expected_sidecar_digest.as_deref(),
            patch,
        )
    };
    match prepared {
        Ok(prepared) => {
            let planned_content = if prepared.changed {
                prepared.planned_content
            } else {
                original_content.clone().unwrap_or_default()
            };
            TransactionItem {
                key: target.key.clone(),
                root_id: target.root_id,
                asset_path: target.asset_path.clone(),
                sidecar_path,
                original_digest,
                original_content,
                planned_digest: Some(digest_content(&planned_content)),
                planned_content: Some(planned_content),
                state: TransactionItemState::Planned,
                failure_kind: None,
                failure: None,
            }
        }
        Err(error) => TransactionItem {
            key: target.key.clone(),
            root_id: target.root_id,
            asset_path: target.asset_path.clone(),
            sidecar_path,
            original_digest,
            original_content,
            planned_digest: None,
            planned_content: None,
            state: TransactionItemState::Failed,
            failure_kind: Some(classify_sidecar_error(&error)),
            failure: Some(error.to_string()),
        },
    }
}

fn apply_planned_item(item: &TransactionItem) -> Result<(), OperationFailure> {
    let content = item
        .planned_content
        .as_deref()
        .ok_or_else(|| invalid_operation("事务缺少计划 Sidecar 内容"))?;
    let planned_digest = item
        .planned_digest
        .as_deref()
        .ok_or_else(|| invalid_operation("事务缺少计划摘要"))?;
    if current_digest(&item.sidecar_path).as_deref() == Some(planned_digest) {
        return Ok(());
    }
    let planned_sidecar: AssetSidecar =
        serde_yaml_ng::from_str(content).map_err(|error| invalid_operation(error.to_string()))?;
    let actual_fingerprint =
        fingerprint_asset(&item.asset_path).map_err(|error| operation_from_sidecar(&error))?;
    if planned_sidecar.fingerprint.as_ref() != Some(&actual_fingerprint) {
        return Err(OperationFailure {
            kind: TransactionFailureKind::Conflict,
            message: "素材内容在事务计划后发生了变化".into(),
        });
    }
    restore_sidecar_content_atomic(
        &item.sidecar_path,
        content,
        &expected_original_version(item),
    )
    .map(|_| ())
    .map_err(|error| operation_from_sidecar(&error))
}

fn restore_applied_item(item: &TransactionItem) -> Result<(), OperationFailure> {
    let planned_digest = item
        .planned_digest
        .clone()
        .ok_or_else(|| invalid_operation("事务缺少计划摘要"))?;
    let expected = ExpectedVersion::Digest(planned_digest);
    if let Some(original_content) = &item.original_content {
        restore_sidecar_content_atomic(&item.sidecar_path, original_content, &expected)
            .map(|_| ())
            .map_err(|error| operation_from_sidecar(&error))
    } else {
        remove_sidecar_if_version(&item.sidecar_path, &expected)
            .map_err(|error| operation_from_sidecar(&error))
    }
}

fn reconcile_actual_states(journal: &mut TransactionJournal) {
    for item in &mut journal.items {
        if matches!(
            item.state,
            TransactionItemState::Failed | TransactionItemState::Restored
        ) {
            continue;
        }
        let actual = current_digest(&item.sidecar_path);
        if actual == item.planned_digest {
            item.state = TransactionItemState::Applied;
            item.failure_kind = None;
            item.failure = None;
        } else if actual == item.original_digest {
            item.state = TransactionItemState::Planned;
            item.failure_kind = None;
            item.failure = None;
        } else {
            item.state = TransactionItemState::Conflict;
            item.failure_kind = Some(TransactionFailureKind::Conflict);
            item.failure = Some("Sidecar 在事务后发生了外部修改".into());
        }
    }
    journal.finish_from_items();
}

const fn classify_sidecar_error(error: &SidecarError) -> TransactionFailureKind {
    match error {
        SidecarError::Conflict { .. } => TransactionFailureKind::Conflict,
        SidecarError::InvalidRating(_)
        | SidecarError::EmptyTag
        | SidecarError::TagTooLong
        | SidecarError::EmptyAlias
        | SidecarError::AliasTooLong
        | SidecarError::NoteTooLong
        | SidecarError::EmptyEdit
        | SidecarError::AmbiguousTagEdit
        | SidecarError::ConflictingTagEdit(_) => TransactionFailureKind::InvalidInput,
        _ => TransactionFailureKind::WriteFailed,
    }
}

fn operation_from_sidecar(error: &SidecarError) -> OperationFailure {
    OperationFailure {
        kind: classify_sidecar_error(error),
        message: error.to_string(),
    }
}

fn invalid_operation(message: impl Into<String>) -> OperationFailure {
    OperationFailure {
        kind: TransactionFailureKind::InvalidInput,
        message: message.into(),
    }
}

fn expected_original_version(item: &TransactionItem) -> ExpectedVersion {
    item.original_digest
        .clone()
        .map_or(ExpectedVersion::Missing, ExpectedVersion::Digest)
}

fn current_digest(path: &Path) -> Option<String> {
    path.is_file().then(|| digest_file(path).ok()).flatten()
}

fn digest_content(content: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(content.as_bytes());
    format!("{:x}", digest.finalize())
}

fn count_items(items: &[TransactionItem], state: TransactionItemState) -> usize {
    items.iter().filter(|item| item.state == state).count()
}

fn is_transaction_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".transaction.yml"))
}

fn transaction_id_from_path(path: &Path) -> Result<Uuid, TransactionError> {
    let id = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".transaction.yml"))
        .and_then(|name| Uuid::parse_str(name).ok())
        .ok_or_else(|| TransactionError::InvalidJournal {
            path: path.to_path_buf(),
            message: "transaction filename must contain a UUID".into(),
        })?;
    Ok(id)
}

fn validate_journal_identity(
    path: &Path,
    journal: &TransactionJournal,
    expected_id: Uuid,
) -> Result<(), TransactionError> {
    if journal.id == expected_id {
        return Ok(());
    }
    Err(TransactionError::InvalidJournal {
        path: path.to_path_buf(),
        message: format!(
            "journal id {} does not match filename id {expected_id}",
            journal.id
        ),
    })
}

fn read_journal(path: &Path) -> Result<TransactionJournal, TransactionError> {
    let bytes = fs::read(path).map_err(|source| TransactionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let journal: TransactionJournal =
        serde_yaml_ng::from_slice(&bytes).map_err(|error| TransactionError::InvalidJournal {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if journal.schema != TRANSACTION_SCHEMA {
        return Err(TransactionError::InvalidJournal {
            path: path.to_path_buf(),
            message: format!("unsupported schema {}", journal.schema),
        });
    }
    Ok(journal)
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(feature = "fault-injection")]
fn inject_fault_after_applied(applied: usize) {
    let abort_after = std::env::var("EAGLE_TRANSACTION_ABORT_AFTER_APPLIED")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    if abort_after == Some(applied) {
        std::process::abort();
    }
}

#[cfg(not(feature = "fault-injection"))]
const fn inject_fault_after_applied(_applied: usize) {}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), TransactionError> {
    let directory = File::open(path).map_err(|source| TransactionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    directory.sync_all().map_err(|source| TransactionError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), TransactionError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use metadata::{MetadataPatch, edit_asset_metadata, read_sidecar, sidecar_path_for};
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{
        MetadataTransactionStore, TransactionFailureKind, TransactionState, TransactionTarget,
    };

    #[test]
    fn plan_only_persists_one_target_before_any_sidecar_is_applied() {
        let directory = tempdir().expect("tempdir");
        let journal_directory = directory.path().join("transactions");
        let store = MetadataTransactionStore::open(&journal_directory).expect("store");
        let asset_path = directory.path().join("single.png");
        fs::write(&asset_path, b"asset").expect("asset");
        let target = TransactionTarget {
            key: "single".into(),
            root_id: Uuid::now_v7(),
            asset_path: asset_path.clone(),
            expected_sidecar_digest: None,
            expected_sidecar_size: None,
            expected_sidecar_modified_unix_ms: None,
        };

        let planned = store
            .plan(
                &[target],
                &MetadataPatch {
                    add_tags: BTreeSet::from(["renamed".into()]),
                    ..MetadataPatch::default()
                },
            )
            .expect("plan");

        assert_eq!(planned.state, TransactionState::Active);
        assert_eq!(planned.item_count, 1);
        assert_eq!(planned.applied_count, 0);
        assert!(!sidecar_path_for(&asset_path).exists());
        let recovered = store.summary(planned.id).expect("summary");
        assert_eq!(recovered.id, planned.id);
        assert_eq!(recovered.state, TransactionState::Active);
        assert_eq!(recovered.applied_count, 0);
        let completed = store
            .continue_transaction(planned.id)
            .expect("continue plan");
        assert_eq!(completed.summary.state, TransactionState::Completed);
        assert!(
            read_sidecar(&sidecar_path_for(&asset_path))
                .expect("sidecar")
                .0
                .tags
                .contains("renamed")
        );
    }

    #[test]
    fn interrupted_thousand_item_transaction_is_discovered_and_continued() {
        let directory = tempdir().expect("tempdir");
        let journal_directory = directory.path().join("transactions");
        let store = MetadataTransactionStore::open(&journal_directory).expect("store");
        let root_id = Uuid::now_v7();
        let targets = (0..1_000)
            .map(|index| {
                let path = directory.path().join(format!("asset-{index:04}.png"));
                fs::write(&path, format!("asset {index}")).expect("asset");
                TransactionTarget {
                    key: path.to_string_lossy().into_owned(),
                    root_id,
                    asset_path: path,
                    expected_sidecar_digest: None,
                    expected_sidecar_size: None,
                    expected_sidecar_modified_unix_ms: None,
                }
            })
            .collect::<Vec<_>>();
        let interrupted = store
            .execute_with_limit(
                &targets,
                &MetadataPatch {
                    add_tags: BTreeSet::from(["batch/recovered".into()]),
                    ..MetadataPatch::default()
                },
                Some(317),
            )
            .expect("interrupted execution");
        assert_eq!(interrupted.summary.state, TransactionState::Active);
        drop(store);

        let reopened = MetadataTransactionStore::open(&journal_directory).expect("reopen");
        let discovered = reopened.list().expect("list");
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].applied_count, 317);
        let continued = reopened
            .continue_transaction(discovered[0].id)
            .expect("continue");

        assert_eq!(continued.summary.state, TransactionState::Completed);
        assert_eq!(continued.summary.applied_count, 1_000);
        assert!(continued.failures.is_empty());
        for target in &targets {
            let (sidecar, _) =
                read_sidecar(&sidecar_path_for(&target.asset_path)).expect("sidecar");
            assert!(sidecar.tags.contains("batch/recovered"));
        }
    }

    #[test]
    fn restore_never_overwrites_a_sidecar_edited_after_the_transaction() {
        let directory = tempdir().expect("tempdir");
        let store =
            MetadataTransactionStore::open(directory.path().join("transactions")).expect("store");
        let root_id = Uuid::now_v7();
        let mut targets = (0..3)
            .map(|index| {
                let path = directory.path().join(format!("asset-{index}.png"));
                fs::write(&path, format!("asset {index}")).expect("asset");
                TransactionTarget {
                    key: path.to_string_lossy().into_owned(),
                    root_id,
                    asset_path: path,
                    expected_sidecar_digest: None,
                    expected_sidecar_size: None,
                    expected_sidecar_modified_unix_ms: None,
                }
            })
            .collect::<Vec<_>>();
        let existing = edit_asset_metadata(
            &targets[2].asset_path,
            None,
            &MetadataPatch {
                add_tags: BTreeSet::from(["before/original".into()]),
                ..MetadataPatch::default()
            },
        )
        .expect("existing sidecar");
        targets[2].expected_sidecar_digest = Some(existing.digest);
        targets[2].expected_sidecar_size = Some(existing.size);
        targets[2].expected_sidecar_modified_unix_ms = Some(existing.modified_unix_ms);
        let original_content =
            fs::read_to_string(sidecar_path_for(&targets[2].asset_path)).expect("original");
        let execution = store
            .execute(
                &targets,
                &MetadataPatch {
                    add_tags: BTreeSet::from(["batch/new".into()]),
                    ..MetadataPatch::default()
                },
            )
            .expect("execute");
        let externally_edited = sidecar_path_for(&targets[0].asset_path);
        let mut external = fs::read_to_string(&externally_edited).expect("read sidecar");
        external.push_str("external: true\n");
        fs::write(&externally_edited, &external).expect("external edit");

        let restored = store
            .restore_transaction(execution.summary.id)
            .expect("restore");

        assert_eq!(restored.summary.state, TransactionState::Conflict);
        assert_eq!(restored.summary.conflict_count, 1);
        assert_eq!(restored.summary.restored_count, 2);
        assert_eq!(
            fs::read_to_string(externally_edited).expect("external"),
            external
        );
        assert!(!sidecar_path_for(&targets[1].asset_path).exists());
        assert_eq!(
            fs::read_to_string(sidecar_path_for(&targets[2].asset_path))
                .expect("restored original"),
            original_content
        );
    }

    #[test]
    fn stale_item_keeps_conflict_semantics_without_blocking_the_batch_plan() {
        let directory = tempdir().expect("tempdir");
        let store =
            MetadataTransactionStore::open(directory.path().join("transactions")).expect("store");
        let root_id = Uuid::now_v7();
        let targets = (0..2)
            .map(|index| {
                let path = directory.path().join(format!("asset-{index}.png"));
                fs::write(&path, format!("asset {index}")).expect("asset");
                TransactionTarget {
                    key: path.to_string_lossy().into_owned(),
                    root_id,
                    asset_path: path,
                    expected_sidecar_digest: None,
                    expected_sidecar_size: None,
                    expected_sidecar_modified_unix_ms: None,
                }
            })
            .collect::<Vec<_>>();
        edit_asset_metadata(
            &targets[0].asset_path,
            None,
            &MetadataPatch {
                add_tags: BTreeSet::from(["external/current".into()]),
                ..MetadataPatch::default()
            },
        )
        .expect("external sidecar");
        let mut stale_targets = targets.clone();
        stale_targets[0].expected_sidecar_digest = Some("stale-digest".into());

        let execution = store
            .execute(
                &stale_targets,
                &MetadataPatch {
                    add_tags: BTreeSet::from(["batch/new".into()]),
                    ..MetadataPatch::default()
                },
            )
            .expect("execute batch");

        assert_eq!(execution.summary.failed_count, 1);
        assert_eq!(execution.summary.applied_count, 1);
        assert_eq!(execution.failures[0].kind, TransactionFailureKind::Conflict);
        let (first, _) = read_sidecar(&sidecar_path_for(&targets[0].asset_path)).expect("first");
        let (second, _) = read_sidecar(&sidecar_path_for(&targets[1].asset_path)).expect("second");
        assert!(first.tags.contains("external/current"));
        assert!(!first.tags.contains("batch/new"));
        assert!(second.tags.contains("batch/new"));
    }
}
