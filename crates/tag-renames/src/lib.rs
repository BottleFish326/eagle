use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use asset_saved_filters::{
    SavedFilterFileVersion, SavedFilterStore, SavedFilterStoreError, SavedFilterTagChoice,
    SavedFilterTagRewriteInput, SavedFilterTagRewritePlan, SavedFilterTagRewritePlanState,
    TagRenameMode,
};
use asset_transactions::{
    MetadataTransactionStore, TransactionError, TransactionState, TransactionSummary,
    TransactionTarget,
};
use chrono::{DateTime, SecondsFormat, Utc};
use metadata::MetadataPatch;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

const TAG_RENAME_SCHEMA: u32 = 1;
const MAX_JOURNAL_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TagRenameState {
    Planned,
    SidecarsActive,
    FiltersPending,
    Completed,
    Conflict,
    Restored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TagRenameFilterOutcome {
    Pending,
    Updated,
    Retained,
    Restored,
}

#[derive(Debug, Clone)]
pub struct TagRenameRequest {
    pub old_tag: String,
    pub new_tag: String,
    pub catalog_revision: u64,
    pub targets: Vec<TransactionTarget>,
    pub saved_filter_version: SavedFilterFileVersion,
    pub saved_filter_choices: Vec<SavedFilterTagChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRenameSummary {
    pub id: Uuid,
    pub state: TagRenameState,
    pub filter_outcome: TagRenameFilterOutcome,
    pub transaction_id: Uuid,
    pub catalog_revision: u64,
    pub root_ids: Vec<Uuid>,
    pub item_count: usize,
    pub filter_update_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Error)]
pub enum TagRenameError {
    #[error("Tag rename requires distinct non-empty exact Tag values")]
    InvalidTagRename,
    #[error("Tag rename coordinator was not found: {0}")]
    NotFound(Uuid),
    #[error("invalid Tag rename journal at {path}: {message}")]
    InvalidJournal { path: PathBuf, message: String },
    #[error("Tag rename coordinator I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("metadata transaction failed: {0}")]
    Transaction(#[from] TransactionError),
    #[error("Tag rename coordinator scope no longer matches transaction: {0}")]
    TransactionBindingMismatch(Uuid),
    #[error("saved filter coordination failed: {0}")]
    SavedFilters(#[from] SavedFilterStoreError),
    #[error("cannot serialize Tag rename coordinator: {0}")]
    Serialize(#[from] serde_yaml_ng::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TagRenameJournal {
    schema: u32,
    id: Uuid,
    created_at: String,
    updated_at: String,
    state: TagRenameState,
    filter_outcome: TagRenameFilterOutcome,
    old_tag: String,
    new_tag: String,
    catalog_revision: u64,
    root_ids: Vec<Uuid>,
    transaction_id: Uuid,
    item_count: usize,
    filter_plan: SavedFilterTagRewritePlan,
}

#[derive(Debug, Clone)]
pub struct TagRenameCoordinator {
    directory: PathBuf,
}

impl TagRenameCoordinator {
    /// Opens the private, pure-file `tag-renames-v1` directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be created.
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, TagRenameError> {
        let directory = directory.into();
        fs::create_dir_all(&directory).map_err(|source| TagRenameError::Io {
            path: directory.clone(),
            source,
        })?;
        Ok(Self { directory })
    }

    /// Persists both full-file plans before applying Sidecars, then advances the
    /// two-phase operation as far as current versions permit.
    ///
    /// # Errors
    ///
    /// Rejects invalid Tags, empty targets, stale files, transaction failures,
    /// and unsafe coordinator persistence.
    pub fn start(
        &self,
        transactions: &MetadataTransactionStore,
        filters: &SavedFilterStore,
        request: TagRenameRequest,
    ) -> Result<TagRenameSummary, TagRenameError> {
        validate_exact_rename(&request.old_tag, &request.new_tag)?;
        let filter_plan = filters.plan_tag_rewrite(
            &request.saved_filter_version,
            SavedFilterTagRewriteInput {
                old_tag: request.old_tag.clone(),
                new_tag: request.new_tag.clone(),
                mode: TagRenameMode::Exact,
                choices: request.saved_filter_choices,
            },
        )?;
        let patch = MetadataPatch {
            add_tags: BTreeSet::from([request.new_tag.clone()]),
            remove_tags: BTreeSet::from([request.old_tag.clone()]),
            ..MetadataPatch::default()
        };
        let transaction = transactions.plan(&request.targets, &patch)?;
        let now = now_rfc3339();
        let mut journal = TagRenameJournal {
            schema: TAG_RENAME_SCHEMA,
            id: Uuid::now_v7(),
            created_at: now.clone(),
            updated_at: now,
            state: TagRenameState::Planned,
            filter_outcome: TagRenameFilterOutcome::Pending,
            old_tag: request.old_tag,
            new_tag: request.new_tag,
            catalog_revision: request.catalog_revision,
            root_ids: transaction.root_ids.clone(),
            transaction_id: transaction.id,
            item_count: transaction.item_count,
            filter_plan,
        };
        self.persist(&journal)?;
        inject_fault("coordinator-written");
        self.continue_loaded(transactions, filters, &mut journal)
    }

    /// Reconciles retained journals against current transaction and filter bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when any retained journal is malformed or cannot be read.
    pub fn list(
        &self,
        transactions: &MetadataTransactionStore,
        filters: &SavedFilterStore,
    ) -> Result<Vec<TagRenameSummary>, TagRenameError> {
        let mut paths = fs::read_dir(&self.directory)
            .map_err(|source| TagRenameError::Io {
                path: self.directory.clone(),
                source,
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_journal_path(path))
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .iter()
            .map(|path| {
                let mut journal = read_journal(path)?;
                validate_identity(path, &journal, id_from_path(path)?)?;
                Self::reconcile(transactions, filters, &mut journal)?;
                self.persist(&journal)?;
                Ok(journal.summary())
            })
            .collect()
    }

    /// Continues an interrupted Sidecar/filter operation from current full-file
    /// identities rather than trusting the last recorded checkpoint.
    ///
    /// # Errors
    ///
    /// Returns an error for missing/invalid journals or persistence failures.
    pub fn continue_operation(
        &self,
        transactions: &MetadataTransactionStore,
        filters: &SavedFilterStore,
        id: Uuid,
    ) -> Result<TagRenameSummary, TagRenameError> {
        let mut journal = self.load(id)?;
        Self::reconcile(transactions, filters, &mut journal)?;
        self.persist(&journal)?;
        self.continue_loaded(transactions, filters, &mut journal)
    }

    /// Keeps original saved-filter queries after Sidecars completed.
    ///
    /// # Errors
    ///
    /// Rejects incomplete Sidecars and any external saved-filter modification.
    pub fn retain_filters(
        &self,
        transactions: &MetadataTransactionStore,
        filters: &SavedFilterStore,
        id: Uuid,
    ) -> Result<TagRenameSummary, TagRenameError> {
        let mut journal = self.load(id)?;
        let transaction = transactions.summary(journal.transaction_id)?;
        validate_transaction_binding(&journal, &transaction)?;
        if !transaction_is_complete(&transaction)
            || filters.tag_rewrite_plan_state(&journal.filter_plan)?
                != SavedFilterTagRewritePlanState::Original
        {
            journal.state = TagRenameState::Conflict;
            journal.touch();
            self.persist(&journal)?;
            return Ok(journal.summary());
        }
        journal.filter_outcome = TagRenameFilterOutcome::Retained;
        journal.state = TagRenameState::Completed;
        journal.touch();
        self.persist(&journal)?;
        Ok(journal.summary())
    }

    /// Conditionally restores filters and Sidecars. Neither file class is
    /// overwritten after an external change.
    ///
    /// # Errors
    ///
    /// Returns an error when recovery state cannot be inspected or persisted.
    pub fn restore(
        &self,
        transactions: &MetadataTransactionStore,
        filters: &SavedFilterStore,
        id: Uuid,
    ) -> Result<TagRenameSummary, TagRenameError> {
        let mut journal = self.load(id)?;
        match filters.tag_rewrite_plan_state(&journal.filter_plan)? {
            SavedFilterTagRewritePlanState::Planned => {
                filters.restore_tag_rewrite_plan(&journal.filter_plan)?;
            }
            SavedFilterTagRewritePlanState::Original => {}
            SavedFilterTagRewritePlanState::External => {
                journal.state = TagRenameState::Conflict;
                journal.touch();
                self.persist(&journal)?;
                return Ok(journal.summary());
            }
        }
        let result = transactions.restore_transaction(journal.transaction_id)?;
        validate_transaction_binding(&journal, &result.summary)?;
        if result.summary.state == TransactionState::Restored && result.failures.is_empty() {
            journal.state = TagRenameState::Restored;
            journal.filter_outcome = TagRenameFilterOutcome::Restored;
        } else {
            journal.state = TagRenameState::Conflict;
        }
        journal.touch();
        self.persist(&journal)?;
        Ok(journal.summary())
    }

    /// Deletes only the selected private recovery journal.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal is missing or cannot be removed.
    pub fn dismiss(&self, id: Uuid) -> Result<(), TagRenameError> {
        let path = self.path(id);
        if !path.is_file() {
            return Err(TagRenameError::NotFound(id));
        }
        fs::remove_file(&path).map_err(|source| TagRenameError::Io {
            path: path.clone(),
            source,
        })?;
        sync_directory(&self.directory).map_err(|source| TagRenameError::Io {
            path: self.directory.clone(),
            source,
        })
    }

    fn continue_loaded(
        &self,
        transactions: &MetadataTransactionStore,
        filters: &SavedFilterStore,
        journal: &mut TagRenameJournal,
    ) -> Result<TagRenameSummary, TagRenameError> {
        if matches!(
            journal.state,
            TagRenameState::Completed | TagRenameState::Restored | TagRenameState::Conflict
        ) {
            return Ok(journal.summary());
        }
        journal.state = TagRenameState::SidecarsActive;
        journal.touch();
        self.persist(journal)?;
        let transaction = transactions.continue_transaction(journal.transaction_id)?;
        validate_transaction_binding(journal, &transaction.summary)?;
        if !transaction_is_complete(&transaction.summary) || !transaction.failures.is_empty() {
            journal.state = TagRenameState::Conflict;
            journal.touch();
            self.persist(journal)?;
            return Ok(journal.summary());
        }
        inject_fault("sidecars-completed");

        journal.state = TagRenameState::FiltersPending;
        journal.touch();
        self.persist(journal)?;
        if journal.filter_plan.planned_content.is_some() {
            match filters.tag_rewrite_plan_state(&journal.filter_plan)? {
                SavedFilterTagRewritePlanState::Original => {
                    inject_fault("filter-before-replace");
                    match filters.apply_tag_rewrite_plan(&journal.filter_plan) {
                        Ok(_) => {}
                        Err(SavedFilterStoreError::ExternalChange { .. }) => {
                            journal.state = TagRenameState::Conflict;
                            journal.touch();
                            self.persist(journal)?;
                            return Ok(journal.summary());
                        }
                        Err(error) => return Err(error.into()),
                    }
                    inject_fault("filter-after-replace");
                }
                SavedFilterTagRewritePlanState::Planned => {}
                SavedFilterTagRewritePlanState::External => {
                    journal.state = TagRenameState::Conflict;
                    journal.touch();
                    self.persist(journal)?;
                    return Ok(journal.summary());
                }
            }
            journal.filter_outcome = TagRenameFilterOutcome::Updated;
        } else {
            journal.filter_outcome = TagRenameFilterOutcome::Retained;
        }
        journal.state = TagRenameState::Completed;
        journal.touch();
        self.persist(journal)?;
        Ok(journal.summary())
    }

    fn reconcile(
        transactions: &MetadataTransactionStore,
        filters: &SavedFilterStore,
        journal: &mut TagRenameJournal,
    ) -> Result<(), TagRenameError> {
        if journal.state == TagRenameState::Restored {
            return Ok(());
        }
        let transaction = transactions.summary(journal.transaction_id)?;
        validate_transaction_binding(journal, &transaction)?;
        let filter_state = filters.tag_rewrite_plan_state(&journal.filter_plan)?;
        journal.state = if transaction.state == TransactionState::Restored {
            if filter_state == SavedFilterTagRewritePlanState::Original {
                TagRenameState::Restored
            } else {
                TagRenameState::Conflict
            }
        } else if transaction.state == TransactionState::Conflict
            || transaction.failed_count > 0
            || transaction.conflict_count > 0
            || filter_state == SavedFilterTagRewritePlanState::External
        {
            TagRenameState::Conflict
        } else if transaction.state == TransactionState::Active {
            TagRenameState::SidecarsActive
        } else if journal.filter_outcome == TagRenameFilterOutcome::Retained {
            TagRenameState::Completed
        } else if filter_state == SavedFilterTagRewritePlanState::Planned
            || journal.filter_plan.planned_content.is_none()
        {
            journal.filter_outcome = if journal.filter_plan.planned_content.is_some() {
                TagRenameFilterOutcome::Updated
            } else {
                TagRenameFilterOutcome::Retained
            };
            TagRenameState::Completed
        } else {
            TagRenameState::FiltersPending
        };
        journal.touch();
        Ok(())
    }

    fn load(&self, id: Uuid) -> Result<TagRenameJournal, TagRenameError> {
        let path = self.path(id);
        if !path.is_file() {
            return Err(TagRenameError::NotFound(id));
        }
        let journal = read_journal(&path)?;
        validate_identity(&path, &journal, id)?;
        Ok(journal)
    }

    fn persist(&self, journal: &TagRenameJournal) -> Result<(), TagRenameError> {
        let path = self.path(journal.id);
        let content = serde_yaml_ng::to_string(journal)?;
        if u64::try_from(content.len()).unwrap_or(u64::MAX) > MAX_JOURNAL_BYTES {
            return Err(TagRenameError::InvalidJournal {
                path,
                message: "journal exceeds the 4 MiB limit".into(),
            });
        }
        let mut temporary =
            NamedTempFile::new_in(&self.directory).map_err(|source| TagRenameError::Io {
                path: self.directory.clone(),
                source,
            })?;
        temporary
            .write_all(content.as_bytes())
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|source| TagRenameError::Io {
                path: temporary.path().to_path_buf(),
                source,
            })?;
        temporary
            .persist(&path)
            .map_err(|error| TagRenameError::Io {
                path: path.clone(),
                source: error.error,
            })?;
        sync_directory(&self.directory).map_err(|source| TagRenameError::Io {
            path: self.directory.clone(),
            source,
        })
    }

    fn path(&self, id: Uuid) -> PathBuf {
        self.directory.join(format!("{id}.tag-rename.yml"))
    }
}

impl TagRenameJournal {
    fn touch(&mut self) {
        self.updated_at = now_rfc3339();
    }

    fn summary(&self) -> TagRenameSummary {
        TagRenameSummary {
            id: self.id,
            state: self.state,
            filter_outcome: self.filter_outcome,
            transaction_id: self.transaction_id,
            catalog_revision: self.catalog_revision,
            root_ids: self.root_ids.clone(),
            item_count: self.item_count,
            filter_update_count: self.filter_plan.updated_filter_ids.len(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

fn validate_exact_rename(old_tag: &str, new_tag: &str) -> Result<(), TagRenameError> {
    if old_tag.trim().is_empty()
        || new_tag.trim().is_empty()
        || old_tag != old_tag.trim()
        || new_tag != new_tag.trim()
        || old_tag == new_tag
        || old_tag.chars().count() > 128
        || new_tag.chars().count() > 128
    {
        return Err(TagRenameError::InvalidTagRename);
    }
    Ok(())
}

fn transaction_is_complete(summary: &TransactionSummary) -> bool {
    summary.state == TransactionState::Completed
        && summary.failed_count == 0
        && summary.conflict_count == 0
        && summary.applied_count == summary.item_count
}

fn validate_transaction_binding(
    journal: &TagRenameJournal,
    summary: &TransactionSummary,
) -> Result<(), TagRenameError> {
    if summary.id == journal.transaction_id
        && summary.item_count == journal.item_count
        && summary.root_ids == journal.root_ids
    {
        return Ok(());
    }
    Err(TagRenameError::TransactionBindingMismatch(journal.id))
}

fn read_journal(path: &Path) -> Result<TagRenameJournal, TagRenameError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| TagRenameError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_JOURNAL_BYTES
    {
        return Err(TagRenameError::InvalidJournal {
            path: path.to_path_buf(),
            message: "journal is unsafe or exceeds the 4 MiB limit".into(),
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or_default());
    File::open(path)
        .and_then(|file| file.take(MAX_JOURNAL_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|source| TagRenameError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let after = fs::symlink_metadata(path).map_err(|source| TagRenameError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || metadata.len() != after.len()
        || metadata.modified().ok() != after.modified().ok()
        || u64::try_from(bytes.len()).unwrap_or(u64::MAX) != after.len()
    {
        return Err(TagRenameError::InvalidJournal {
            path: path.to_path_buf(),
            message: "journal changed while it was being read".into(),
        });
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_JOURNAL_BYTES {
        return Err(TagRenameError::InvalidJournal {
            path: path.to_path_buf(),
            message: "journal exceeds the 4 MiB limit".into(),
        });
    }
    let journal: TagRenameJournal =
        serde_yaml_ng::from_slice(&bytes).map_err(|error| TagRenameError::InvalidJournal {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    if journal.schema != TAG_RENAME_SCHEMA
        || Uuid::parse_str(&journal.id.to_string())
            .ok()
            .is_none_or(|id| id.get_version_num() != 7)
        || journal.transaction_id.get_version_num() != 7
        || DateTime::parse_from_rfc3339(&journal.created_at).is_err()
        || DateTime::parse_from_rfc3339(&journal.updated_at).is_err()
        || journal.root_ids.is_empty()
        || journal
            .root_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != journal.root_ids.len()
        || journal.item_count == 0
        || validate_exact_rename(&journal.old_tag, &journal.new_tag).is_err()
    {
        return Err(TagRenameError::InvalidJournal {
            path: path.to_path_buf(),
            message: "journal fields are invalid".into(),
        });
    }
    Ok(journal)
}

fn validate_identity(
    path: &Path,
    journal: &TagRenameJournal,
    expected_id: Uuid,
) -> Result<(), TagRenameError> {
    if journal.id == expected_id {
        return Ok(());
    }
    Err(TagRenameError::InvalidJournal {
        path: path.to_path_buf(),
        message: "journal ID does not match its filename".into(),
    })
}

fn is_journal_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".tag-rename.yml"))
}

fn id_from_path(path: &Path) -> Result<Uuid, TagRenameError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".tag-rename.yml"))
        .and_then(|name| Uuid::parse_str(name).ok())
        .ok_or_else(|| TagRenameError::InvalidJournal {
            path: path.to_path_buf(),
            message: "journal filename must contain a UUID".into(),
        })
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(feature = "fault-injection")]
fn inject_fault(point: &str) {
    if std::env::var("EAGLE_TAG_RENAME_ABORT_AT").as_deref() == Ok(point) {
        std::process::abort();
    }
}

#[cfg(not(feature = "fault-injection"))]
fn inject_fault(_point: &str) {}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::fs;

    use asset_saved_filters::{
        CreateSavedFilter, SavedFilterFileVersion, SavedFilterScope, SavedFilterSort,
        SavedFilterSortDirection, SavedFilterSortField, SavedFilterTagChoiceAction,
    };
    use metadata::{MetadataPatch, edit_asset_metadata, read_sidecar, sidecar_path_for};
    use tempfile::tempdir;

    use super::*;

    struct Fixture {
        root_id: Uuid,
        targets: Vec<TransactionTarget>,
        originals: HashMap<PathBuf, String>,
    }

    fn fixture(directory: &Path) -> Fixture {
        let root_id = Uuid::now_v7();
        let mut targets = Vec::new();
        let mut originals = HashMap::new();
        for (index, tags) in [
            BTreeSet::from(["old".to_owned()]),
            BTreeSet::from(["old".to_owned(), "new".to_owned()]),
        ]
        .into_iter()
        .enumerate()
        {
            let asset_path = directory.join(format!("asset-{index}.png"));
            fs::write(&asset_path, format!("asset {index}")).expect("asset");
            let edit = edit_asset_metadata(
                &asset_path,
                None,
                &MetadataPatch {
                    set_tags: Some(tags),
                    ..MetadataPatch::default()
                },
            )
            .expect("initial sidecar");
            let sidecar_path = sidecar_path_for(&asset_path);
            originals.insert(
                sidecar_path,
                fs::read_to_string(sidecar_path_for(&asset_path)).expect("original sidecar"),
            );
            targets.push(TransactionTarget {
                key: format!("asset-{index}"),
                root_id,
                asset_path,
                expected_sidecar_digest: Some(edit.digest),
                expected_sidecar_size: Some(edit.size),
                expected_sidecar_modified_unix_ms: Some(edit.modified_unix_ms),
            });
        }
        Fixture {
            root_id,
            targets,
            originals,
        }
    }

    fn saved_filter(store: &SavedFilterStore) -> (Uuid, SavedFilterFileVersion, Vec<u8>) {
        let mutation = store
            .create(
                &SavedFilterFileVersion::expected_absent(),
                CreateSavedFilter {
                    name: "Exact rename".into(),
                    query: "old -old any:(old|other) path:old".into(),
                    scope: SavedFilterScope::AllEnabledRoots,
                    sort: SavedFilterSort {
                        field: SavedFilterSortField::FileName,
                        direction: SavedFilterSortDirection::Ascending,
                    },
                },
            )
            .expect("saved filter");
        (
            mutation.filter.expect("created filter").id,
            mutation.file_version,
            fs::read(store.path()).expect("filter bytes"),
        )
    }

    fn request(
        fixture: &Fixture,
        filter_id: Uuid,
        file_version: SavedFilterFileVersion,
    ) -> TagRenameRequest {
        TagRenameRequest {
            old_tag: "old".into(),
            new_tag: "new".into(),
            catalog_revision: 42,
            targets: fixture.targets.clone(),
            saved_filter_version: file_version,
            saved_filter_choices: vec![SavedFilterTagChoice {
                filter_id,
                action: SavedFilterTagChoiceAction::Update,
            }],
        }
    }

    fn planned_journal(
        coordinator: &TagRenameCoordinator,
        transactions: &MetadataTransactionStore,
        filters: &SavedFilterStore,
        request: TagRenameRequest,
    ) -> TagRenameJournal {
        let filter_plan = filters
            .plan_tag_rewrite(
                &request.saved_filter_version,
                SavedFilterTagRewriteInput {
                    old_tag: request.old_tag.clone(),
                    new_tag: request.new_tag.clone(),
                    mode: TagRenameMode::Exact,
                    choices: request.saved_filter_choices,
                },
            )
            .expect("filter plan");
        let transaction = transactions
            .plan(
                &request.targets,
                &MetadataPatch {
                    add_tags: BTreeSet::from([request.new_tag.clone()]),
                    remove_tags: BTreeSet::from([request.old_tag.clone()]),
                    ..MetadataPatch::default()
                },
            )
            .expect("transaction plan");
        let now = now_rfc3339();
        let journal = TagRenameJournal {
            schema: TAG_RENAME_SCHEMA,
            id: Uuid::now_v7(),
            created_at: now.clone(),
            updated_at: now,
            state: TagRenameState::Planned,
            filter_outcome: TagRenameFilterOutcome::Pending,
            old_tag: request.old_tag,
            new_tag: request.new_tag,
            catalog_revision: request.catalog_revision,
            root_ids: transaction.root_ids,
            transaction_id: transaction.id,
            item_count: transaction.item_count,
            filter_plan,
        };
        coordinator.persist(&journal).expect("coordinator plan");
        journal
    }

    #[test]
    fn completes_exact_rename_and_conditionally_restores_both_file_classes() {
        let directory = tempdir().expect("temp directory");
        let fixture = fixture(directory.path());
        let transactions = MetadataTransactionStore::open(directory.path().join("transactions"))
            .expect("transactions");
        let filters = SavedFilterStore::new(directory.path().join("saved-filters.yml"));
        let (filter_id, version, original_filters) = saved_filter(&filters);
        let coordinator = TagRenameCoordinator::open(directory.path().join("tag-renames-v1"))
            .expect("coordinator");

        let completed = coordinator
            .start(
                &transactions,
                &filters,
                request(&fixture, filter_id, version),
            )
            .expect("complete rename");

        assert_eq!(completed.state, TagRenameState::Completed);
        assert_eq!(completed.filter_outcome, TagRenameFilterOutcome::Updated);
        assert_eq!(completed.root_ids, vec![fixture.root_id]);
        for target in &fixture.targets {
            let (sidecar, _) =
                read_sidecar(&sidecar_path_for(&target.asset_path)).expect("renamed sidecar");
            assert!(!sidecar.tags.contains("old"));
            assert!(sidecar.tags.contains("new"));
            assert_eq!(sidecar.tags.len(), 1, "target Tag must merge as a set");
        }
        let filter_catalog = filters.load(&BTreeSet::new()).expect("rewritten filter");
        assert_eq!(
            filter_catalog.valid_filters[0].query,
            "new -tag:new any:(new|other) path:old"
        );
        let journal_output = fs::read_to_string(coordinator.path(completed.id)).expect("journal");
        assert!(journal_output.contains("originalContent:"));
        assert!(journal_output.contains("plannedSha256:"));

        let restored = coordinator
            .restore(&transactions, &filters, completed.id)
            .expect("restore");
        assert_eq!(restored.state, TagRenameState::Restored);
        assert_eq!(
            fs::read(filters.path()).expect("restored filters"),
            original_filters
        );
        for (path, content) in fixture.originals {
            assert_eq!(fs::read_to_string(path).expect("restored sidecar"), content);
        }
        coordinator.dismiss(completed.id).expect("dismiss");
        assert!(transactions.summary(completed.transaction_id).is_ok());
    }

    #[test]
    fn reconstructs_coordinator_written_filters_pending_and_post_replace_states() {
        let directory = tempdir().expect("temp directory");
        let fixture = fixture(directory.path());
        let transactions = MetadataTransactionStore::open(directory.path().join("transactions"))
            .expect("transactions");
        let filters = SavedFilterStore::new(directory.path().join("saved-filters.yml"));
        let (filter_id, version, _) = saved_filter(&filters);
        let coordinator = TagRenameCoordinator::open(directory.path().join("tag-renames-v1"))
            .expect("coordinator");
        let mut journal = planned_journal(
            &coordinator,
            &transactions,
            &filters,
            request(&fixture, filter_id, version),
        );

        let discovered = coordinator
            .list(&transactions, &filters)
            .expect("discover plan");
        assert_eq!(discovered[0].state, TagRenameState::SidecarsActive);
        transactions
            .continue_transaction(journal.transaction_id)
            .expect("complete Sidecars");
        journal.state = TagRenameState::FiltersPending;
        journal.touch();
        coordinator.persist(&journal).expect("filters checkpoint");
        let pending = coordinator
            .list(&transactions, &filters)
            .expect("reconstruct pending");
        assert_eq!(pending[0].state, TagRenameState::FiltersPending);

        filters
            .apply_tag_rewrite_plan(&journal.filter_plan)
            .expect("simulate replacement before checkpoint");
        let after_replace = coordinator
            .list(&transactions, &filters)
            .expect("reconstruct replace");
        assert_eq!(after_replace[0].state, TagRenameState::Completed);
        assert_eq!(
            after_replace[0].filter_outcome,
            TagRenameFilterOutcome::Updated
        );
    }

    #[test]
    fn retain_and_external_changes_never_trigger_overwrite() {
        let directory = tempdir().expect("temp directory");
        let fixture = fixture(directory.path());
        let transactions = MetadataTransactionStore::open(directory.path().join("transactions"))
            .expect("transactions");
        let filters = SavedFilterStore::new(directory.path().join("saved-filters.yml"));
        let (filter_id, version, original_filters) = saved_filter(&filters);
        let coordinator = TagRenameCoordinator::open(directory.path().join("tag-renames-v1"))
            .expect("coordinator");
        let journal = planned_journal(
            &coordinator,
            &transactions,
            &filters,
            request(&fixture, filter_id, version),
        );
        transactions
            .continue_transaction(journal.transaction_id)
            .expect("complete Sidecars");
        let retained = coordinator
            .retain_filters(&transactions, &filters, journal.id)
            .expect("retain filters");
        assert_eq!(retained.state, TagRenameState::Completed);
        assert_eq!(retained.filter_outcome, TagRenameFilterOutcome::Retained);
        assert_eq!(
            fs::read(filters.path()).expect("unchanged filter"),
            original_filters
        );

        let first = &fixture.targets[0];
        let sidecar_path = sidecar_path_for(&first.asset_path);
        let (_, current) = read_sidecar(&sidecar_path).expect("current sidecar");
        edit_asset_metadata(
            &first.asset_path,
            Some(&current),
            &MetadataPatch {
                add_tags: BTreeSet::from(["external".into()]),
                ..MetadataPatch::default()
            },
        )
        .expect("external sidecar edit");
        let conflict = coordinator
            .restore(&transactions, &filters, journal.id)
            .expect("conditional restore result");
        assert_eq!(conflict.state, TagRenameState::Conflict);
        let (preserved, _) = read_sidecar(&sidecar_path).expect("preserved sidecar");
        assert!(preserved.tags.contains("external"));
    }
}
