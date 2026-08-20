use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use asset_core::AssetRecord;
use asset_index::{
    AssetIndex, AssetSort, AssetSortDirection, AssetSortField, QueryParseErrorKind, parse_query,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

pub const SAVED_FILTER_SCHEMA_VERSION: u32 = 1;
pub const MAX_SAVED_FILTER_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_SAVED_FILTERS: usize = 512;
const MAX_QUERY_CHARACTERS: usize = 4_096;
const MAX_NAME_CHARACTERS: usize = 128;
const MAX_ROOT_IDS: usize = 64;
const MAX_VALUE_DEPTH: usize = 32;
const MAX_VALUE_NODES: usize = 65_536;

const FILTER_KEYS: &[&str] = &[
    "id",
    "name",
    "query",
    "scope",
    "sort",
    "createdAt",
    "updatedAt",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilterFileVersion {
    pub exists: bool,
    pub size: u64,
    pub modified_unix_ms: Option<u64>,
    pub sha256: Option<String>,
}

impl SavedFilterFileVersion {
    #[must_use]
    pub const fn expected_absent() -> Self {
        Self {
            exists: false,
            size: 0,
            modified_unix_ms: None,
            sha256: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SavedFilterFileIssueKind {
    InvalidFile,
    FileTooLarge,
    UnsupportedSchema,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilterFileIssue {
    pub kind: SavedFilterFileIssueKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SavedFilterEntryIssueKind {
    InvalidEntry,
    DuplicateId,
    DuplicateName,
    InvalidQuery,
    UnknownSort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilterQueryIssue {
    pub kind: QueryParseErrorKind,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidSavedFilterEntry {
    pub index: usize,
    pub id: Option<Uuid>,
    pub issues: BTreeSet<SavedFilterEntryIssueKind>,
    pub query_issue: Option<SavedFilterQueryIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SavedFilterScope {
    AllEnabledRoots,
    SelectedRoots { root_ids: Vec<Uuid> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SavedFilterSortField {
    FileName,
    ModifiedAt,
    CreatedAt,
    FileSize,
    Rating,
    AssetKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SavedFilterSortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilterSort {
    pub field: SavedFilterSortField,
    pub direction: SavedFilterSortDirection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilter {
    pub id: Uuid,
    pub name: String,
    pub query: String,
    pub scope: SavedFilterScope,
    pub sort: SavedFilterSort,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnavailableSavedFilter {
    pub filter: SavedFilter,
    pub missing_root_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilterCatalog {
    pub file_version: SavedFilterFileVersion,
    pub valid_filters: Vec<SavedFilter>,
    pub unavailable_filters: Vec<UnavailableSavedFilter>,
    pub invalid_entries: Vec<InvalidSavedFilterEntry>,
    pub file_issues: Vec<SavedFilterFileIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSavedFilter {
    pub name: String,
    pub query: String,
    pub scope: SavedFilterScope,
    pub sort: SavedFilterSort,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSavedFilter {
    pub name: String,
    pub query: String,
    pub scope: SavedFilterScope,
    pub sort: SavedFilterSort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilterMutation {
    pub file_version: SavedFilterFileVersion,
    pub filter: Option<SavedFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilterExecution {
    pub filter_id: Uuid,
    pub expression: String,
    pub ordered_keys: Vec<String>,
    pub total_assets: usize,
    pub scoped_assets: usize,
    pub matched_assets: usize,
    pub effective_root_ids: Vec<Uuid>,
    pub missing_root_ids: Vec<Uuid>,
    pub sort: SavedFilterSort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Error)]
#[serde(rename_all = "camelCase")]
#[error("saved filter query is invalid at byte offset {offset}")]
pub struct SavedFilterExecutionError {
    pub kind: QueryParseErrorKind,
    pub offset: usize,
}

/// Re-parses and executes a saved expression against current runtime records.
///
/// The returned keys are an ephemeral view. Neither records nor result keys are
/// written to `saved-filters.yml`, so rebuilding the index always starts from the
/// current filesystem scan.
///
/// # Errors
///
/// Returns the current parser's stable kind and UTF-8 byte offset if the saved
/// expression is no longer valid.
pub fn execute_saved_filter(
    filter: &SavedFilter,
    records: &[AssetRecord],
    enabled_root_ids: &BTreeSet<Uuid>,
    available_root_ids: &BTreeSet<Uuid>,
) -> Result<SavedFilterExecution, SavedFilterExecutionError> {
    let query = parse_query(&filter.query).map_err(|error| SavedFilterExecutionError {
        kind: error.kind,
        offset: error.offset,
    })?;
    let requested_root_ids = match &filter.scope {
        SavedFilterScope::AllEnabledRoots => enabled_root_ids.clone(),
        SavedFilterScope::SelectedRoots { root_ids } => root_ids.iter().copied().collect(),
    };
    let enabled_requested_root_ids = requested_root_ids
        .intersection(enabled_root_ids)
        .copied()
        .collect::<BTreeSet<_>>();
    let effective_root_ids = enabled_requested_root_ids
        .intersection(available_root_ids)
        .copied()
        .collect::<BTreeSet<_>>();
    let missing_root_ids = requested_root_ids
        .difference(&effective_root_ids)
        .copied()
        .collect::<Vec<_>>();
    let sort = AssetSort {
        field: match filter.sort.field {
            SavedFilterSortField::FileName => AssetSortField::FileName,
            SavedFilterSortField::ModifiedAt => AssetSortField::ModifiedAt,
            SavedFilterSortField::CreatedAt => AssetSortField::CreatedAt,
            SavedFilterSortField::FileSize => AssetSortField::FileSize,
            SavedFilterSortField::Rating => AssetSortField::Rating,
            SavedFilterSortField::AssetKind => AssetSortField::AssetKind,
        },
        direction: match filter.sort.direction {
            SavedFilterSortDirection::Ascending => AssetSortDirection::Ascending,
            SavedFilterSortDirection::Descending => AssetSortDirection::Descending,
        },
    };
    let scoped_assets = records
        .iter()
        .filter(|record| {
            record
                .root_id
                .is_some_and(|root_id| effective_root_ids.contains(&root_id))
        })
        .count();
    let index = AssetIndex::from_records(records.iter().cloned());
    let ordered_keys = index.query_ordered(&query, &effective_root_ids, sort);
    Ok(SavedFilterExecution {
        filter_id: filter.id,
        expression: filter.query.clone(),
        matched_assets: ordered_keys.len(),
        ordered_keys,
        total_assets: records.len(),
        scoped_assets,
        effective_root_ids: effective_root_ids.into_iter().collect(),
        missing_root_ids,
        sort: filter.sort,
    })
}

#[derive(Debug, Error)]
pub enum SavedFilterStoreError {
    #[error("saved filter I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("saved filter file is not safe to read or replace")]
    UnsafeTarget,
    #[error("saved filter file is invalid: {0:?}")]
    InvalidFile(SavedFilterFileIssueKind),
    #[error("saved filter file changed externally")]
    ExternalChange {
        expected: Box<SavedFilterFileVersion>,
        actual: Box<SavedFilterFileVersion>,
    },
    #[error("saved filter was not found")]
    NotFound,
    #[error("saved filter ID is ambiguous")]
    AmbiguousId,
    #[error("saved filter mutation is invalid")]
    InvalidMutation(Vec<SavedFilterEntryIssueKind>),
    #[error("saved filter catalog already contains {MAX_SAVED_FILTERS} entries")]
    TooManyFilters,
    #[error("cannot serialize saved filters: {0}")]
    Serialize(#[from] serde_yaml_ng::Error),
    #[error("failed to persist saved filters at {path}: {source}")]
    Persist {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub struct SavedFilterStore {
    path: PathBuf,
}

impl SavedFilterStore {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the fixed saved-filter file and isolates individual invalid entries.
    ///
    /// # Errors
    ///
    /// Returns an error only for an I/O failure. YAML and top-level format failures
    /// are represented by `file_issues` so callers cannot mistake them for an empty file.
    pub fn load(
        &self,
        available_root_ids: &BTreeSet<Uuid>,
    ) -> Result<SavedFilterCatalog, SavedFilterStoreError> {
        match self.read_file()? {
            ReadSavedFilterFile::Missing => Ok(analyze_document(
                &default_document(),
                SavedFilterFileVersion::expected_absent(),
                available_root_ids,
            )),
            ReadSavedFilterFile::TooLarge(version) => Ok(file_issue_catalog(
                version,
                SavedFilterFileIssueKind::FileTooLarge,
            )),
            ReadSavedFilterFile::Present { version, bytes } => {
                let document = match parse_document(&bytes) {
                    Ok(document) => document,
                    Err(kind) => return Ok(file_issue_catalog(version, kind)),
                };
                Ok(analyze_document(&document, version, available_root_ids))
            }
        }
    }

    /// Creates one saved expression while preserving unknown YAML fields.
    ///
    /// # Errors
    ///
    /// Rejects invalid input, a stale expected file version, unsafe targets, and
    /// atomic persistence failures.
    pub fn create(
        &self,
        expected: &SavedFilterFileVersion,
        input: CreateSavedFilter,
    ) -> Result<SavedFilterMutation, SavedFilterStoreError> {
        self.create_at(expected, input, Uuid::now_v7(), Utc::now())
    }

    /// Replaces all user-editable fields of one uniquely identified filter.
    ///
    /// # Errors
    ///
    /// Rejects missing or ambiguous IDs, invalid input, external changes, and
    /// persistence failures.
    pub fn update(
        &self,
        expected: &SavedFilterFileVersion,
        id: Uuid,
        input: UpdateSavedFilter,
    ) -> Result<SavedFilterMutation, SavedFilterStoreError> {
        self.update_at(expected, id, input, Utc::now())
    }

    /// Renames one uniquely identified filter without modifying its expression.
    ///
    /// # Errors
    ///
    /// Rejects missing or ambiguous IDs, duplicate/invalid names, external
    /// changes, and persistence failures.
    pub fn rename(
        &self,
        expected: &SavedFilterFileVersion,
        id: Uuid,
        name: String,
    ) -> Result<SavedFilterMutation, SavedFilterStoreError> {
        let (mut document, current) = self.mutable_document(expected)?;
        let index = unique_filter_index(&document, id)?;
        let mapping = filter_mapping_mut(&mut document, index)?;
        mapping.insert(Value::String("name".into()), Value::String(name));
        mapping.insert(
            Value::String("updatedAt".into()),
            Value::String(timestamp(Utc::now())),
        );
        self.validate_and_write(document, &current, Some(id))
    }

    /// Deletes one uniquely identified entry and leaves an explicit empty catalog.
    ///
    /// # Errors
    ///
    /// Rejects missing or ambiguous IDs, external changes, and persistence failures.
    pub fn delete(
        &self,
        expected: &SavedFilterFileVersion,
        id: Uuid,
    ) -> Result<SavedFilterMutation, SavedFilterStoreError> {
        let (mut document, current) = self.mutable_document(expected)?;
        let index = unique_filter_index(&document, id)?;
        filters_mut(&mut document)?.remove(index);
        self.write_document(document, &current, None)
    }

    fn create_at(
        &self,
        expected: &SavedFilterFileVersion,
        input: CreateSavedFilter,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<SavedFilterMutation, SavedFilterStoreError> {
        let (mut document, current) = self.mutable_document(expected)?;
        if filters(&document)?.len() >= MAX_SAVED_FILTERS {
            return Err(SavedFilterStoreError::TooManyFilters);
        }
        let at = timestamp(now);
        let filter = SavedFilter {
            id,
            name: input.name,
            query: input.query,
            scope: input.scope,
            sort: input.sort,
            created_at: at.clone(),
            updated_at: at,
        };
        filters_mut(&mut document)?.push(saved_filter_value(&filter));
        self.validate_and_write(document, &current, Some(id))
    }

    fn update_at(
        &self,
        expected: &SavedFilterFileVersion,
        id: Uuid,
        input: UpdateSavedFilter,
        now: DateTime<Utc>,
    ) -> Result<SavedFilterMutation, SavedFilterStoreError> {
        let (mut document, current) = self.mutable_document(expected)?;
        let index = unique_filter_index(&document, id)?;
        let mapping = filter_mapping_mut(&mut document, index)?;
        mapping.insert(Value::String("name".into()), Value::String(input.name));
        mapping.insert(Value::String("query".into()), Value::String(input.query));
        mapping.insert(
            Value::String("scope".into()),
            saved_filter_scope_value(&input.scope),
        );
        mapping.insert(
            Value::String("sort".into()),
            saved_filter_sort_value(input.sort),
        );
        mapping.insert(
            Value::String("updatedAt".into()),
            Value::String(timestamp(now)),
        );
        self.validate_and_write(document, &current, Some(id))
    }

    fn validate_and_write(
        &self,
        document: Value,
        current: &SavedFilterFileVersion,
        changed_id: Option<Uuid>,
    ) -> Result<SavedFilterMutation, SavedFilterStoreError> {
        let analysis = analyze_document(&document, current.clone(), &BTreeSet::new());
        if !analysis.file_issues.is_empty() {
            return Err(SavedFilterStoreError::InvalidMutation(vec![
                SavedFilterEntryIssueKind::InvalidEntry,
            ]));
        }
        if let Some(id) = changed_id {
            if let Some(entry) = analysis
                .invalid_entries
                .iter()
                .find(|entry| entry.id == Some(id))
            {
                return Err(SavedFilterStoreError::InvalidMutation(
                    entry.issues.iter().copied().collect(),
                ));
            }
        }
        self.write_document(document, current, changed_id)
    }

    fn mutable_document(
        &self,
        expected: &SavedFilterFileVersion,
    ) -> Result<(Value, SavedFilterFileVersion), SavedFilterStoreError> {
        match self.read_file()? {
            ReadSavedFilterFile::Missing => {
                let current = SavedFilterFileVersion::expected_absent();
                ensure_version(expected, &current)?;
                Ok((default_document(), current))
            }
            ReadSavedFilterFile::TooLarge(version) => {
                ensure_version(expected, &version)?;
                Err(SavedFilterStoreError::InvalidFile(
                    SavedFilterFileIssueKind::FileTooLarge,
                ))
            }
            ReadSavedFilterFile::Present { version, bytes } => {
                ensure_version(expected, &version)?;
                let document =
                    parse_document(&bytes).map_err(SavedFilterStoreError::InvalidFile)?;
                Ok((document, version))
            }
        }
    }

    fn write_document(
        &self,
        mut document: Value,
        expected: &SavedFilterFileVersion,
        changed_id: Option<Uuid>,
    ) -> Result<SavedFilterMutation, SavedFilterStoreError> {
        normalize_document_order(&mut document)?;
        let serialized = serde_yaml_ng::to_string(&document)?;
        if u64::try_from(serialized.len()).unwrap_or(u64::MAX) > MAX_SAVED_FILTER_FILE_BYTES {
            return Err(SavedFilterStoreError::InvalidFile(
                SavedFilterFileIssueKind::FileTooLarge,
            ));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| SavedFilterStoreError::Persist {
                path: self.path.clone(),
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "saved filter path has no parent",
                ),
            })?;
        fs::create_dir_all(parent).map_err(|source| SavedFilterStoreError::Persist {
            path: self.path.clone(),
            source,
        })?;
        let mut temporary =
            NamedTempFile::new_in(parent).map_err(|source| SavedFilterStoreError::Persist {
                path: self.path.clone(),
                source,
            })?;
        temporary
            .write_all(serialized.as_bytes())
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|source| SavedFilterStoreError::Persist {
                path: self.path.clone(),
                source,
            })?;

        let actual = self.current_version()?;
        ensure_version(expected, &actual)?;
        temporary
            .persist(&self.path)
            .map_err(|error| SavedFilterStoreError::Persist {
                path: self.path.clone(),
                source: error.error,
            })?;
        sync_directory(parent).map_err(|source| SavedFilterStoreError::Persist {
            path: self.path.clone(),
            source,
        })?;
        let file_version = match self.read_file()? {
            ReadSavedFilterFile::Present { version, .. } => version,
            ReadSavedFilterFile::Missing | ReadSavedFilterFile::TooLarge(_) => {
                return Err(SavedFilterStoreError::Persist {
                    path: self.path.clone(),
                    source: io::Error::other("persisted saved filter file is unavailable"),
                });
            }
        };
        let analysis = analyze_document(&document, file_version.clone(), &BTreeSet::new());
        let filter = changed_id.and_then(|id| {
            analysis
                .valid_filters
                .into_iter()
                .chain(
                    analysis
                        .unavailable_filters
                        .into_iter()
                        .map(|entry| entry.filter),
                )
                .find(|filter| filter.id == id)
        });
        Ok(SavedFilterMutation {
            file_version,
            filter,
        })
    }

    fn current_version(&self) -> Result<SavedFilterFileVersion, SavedFilterStoreError> {
        match self.read_file()? {
            ReadSavedFilterFile::Missing => Ok(SavedFilterFileVersion::expected_absent()),
            ReadSavedFilterFile::TooLarge(version)
            | ReadSavedFilterFile::Present { version, .. } => Ok(version),
        }
    }

    fn read_file(&self) -> Result<ReadSavedFilterFile, SavedFilterStoreError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(ReadSavedFilterFile::Missing);
            }
            Err(source) => {
                return Err(SavedFilterStoreError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SavedFilterStoreError::UnsafeTarget);
        }
        let initial_modified = metadata
            .modified()
            .map_err(|source| SavedFilterStoreError::Io {
                path: self.path.clone(),
                source,
            })?;
        let initial_modified_unix_ms = system_time_unix_ms(initial_modified, &self.path)?;
        if metadata.len() > MAX_SAVED_FILTER_FILE_BYTES {
            return Ok(ReadSavedFilterFile::TooLarge(SavedFilterFileVersion {
                exists: true,
                size: metadata.len(),
                modified_unix_ms: Some(initial_modified_unix_ms),
                sha256: None,
            }));
        }
        let mut file = File::open(&self.path).map_err(|source| SavedFilterStoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        std::io::Read::by_ref(&mut file)
            .take(MAX_SAVED_FILTER_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| SavedFilterStoreError::Io {
                path: self.path.clone(),
                source,
            })?;
        let after =
            fs::symlink_metadata(&self.path).map_err(|source| SavedFilterStoreError::Io {
                path: self.path.clone(),
                source,
            })?;
        if after.file_type().is_symlink() || !after.is_file() {
            return Err(SavedFilterStoreError::UnsafeTarget);
        }
        let after_modified = after
            .modified()
            .map_err(|source| SavedFilterStoreError::Io {
                path: self.path.clone(),
                source,
            })?;
        let after_modified_unix_ms = system_time_unix_ms(after_modified, &self.path)?;
        if metadata.len() != after.len()
            || initial_modified != after_modified
            || u64::try_from(bytes.len()).unwrap_or(u64::MAX) != after.len()
        {
            return Err(SavedFilterStoreError::ExternalChange {
                expected: Box::new(SavedFilterFileVersion {
                    exists: true,
                    size: metadata.len(),
                    modified_unix_ms: Some(initial_modified_unix_ms),
                    sha256: None,
                }),
                actual: Box::new(SavedFilterFileVersion {
                    exists: true,
                    size: after.len(),
                    modified_unix_ms: Some(after_modified_unix_ms),
                    sha256: None,
                }),
            });
        }
        if bytes.len() > usize::try_from(MAX_SAVED_FILTER_FILE_BYTES).unwrap_or(usize::MAX) {
            return Ok(ReadSavedFilterFile::TooLarge(SavedFilterFileVersion {
                exists: true,
                size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                modified_unix_ms: Some(after_modified_unix_ms),
                sha256: None,
            }));
        }
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        Ok(ReadSavedFilterFile::Present {
            version: SavedFilterFileVersion {
                exists: true,
                size,
                modified_unix_ms: Some(after_modified_unix_ms),
                sha256: Some(digest(&bytes)),
            },
            bytes,
        })
    }
}

enum ReadSavedFilterFile {
    Missing,
    TooLarge(SavedFilterFileVersion),
    Present {
        version: SavedFilterFileVersion,
        bytes: Vec<u8>,
    },
}

fn parse_document(bytes: &[u8]) -> Result<Value, SavedFilterFileIssueKind> {
    let text = std::str::from_utf8(bytes).map_err(|_| SavedFilterFileIssueKind::InvalidFile)?;
    if contains_yaml_anchor_or_alias(text) {
        return Err(SavedFilterFileIssueKind::InvalidFile);
    }
    let value: Value =
        serde_yaml_ng::from_slice(bytes).map_err(|_| SavedFilterFileIssueKind::InvalidFile)?;
    let mut nodes = 0;
    validate_plain_value(&value, 0, &mut nodes)?;
    let mapping = value
        .as_mapping()
        .ok_or(SavedFilterFileIssueKind::InvalidFile)?;
    let schema = mapping
        .get("schema")
        .and_then(Value::as_u64)
        .ok_or(SavedFilterFileIssueKind::InvalidFile)?;
    if schema != u64::from(SAVED_FILTER_SCHEMA_VERSION) {
        return Err(SavedFilterFileIssueKind::UnsupportedSchema);
    }
    let entries = mapping
        .get("filters")
        .and_then(Value::as_sequence)
        .ok_or(SavedFilterFileIssueKind::InvalidFile)?;
    if entries.len() > MAX_SAVED_FILTERS {
        return Err(SavedFilterFileIssueKind::InvalidFile);
    }
    Ok(value)
}

fn analyze_document(
    document: &Value,
    file_version: SavedFilterFileVersion,
    available_root_ids: &BTreeSet<Uuid>,
) -> SavedFilterCatalog {
    let Ok(entries) = filters(document) else {
        return file_issue_catalog(file_version, SavedFilterFileIssueKind::InvalidFile);
    };
    let mut id_counts = BTreeMap::<Uuid, usize>::new();
    let mut name_counts = BTreeMap::<String, usize>::new();
    for entry in entries {
        if let Some(mapping) = entry.as_mapping() {
            if let Some(id) = mapping
                .get("id")
                .and_then(Value::as_str)
                .and_then(parse_v7_id)
            {
                *id_counts.entry(id).or_default() += 1;
            }
            if let Some(name) = mapping.get("name").and_then(Value::as_str) {
                if valid_name(name) {
                    *name_counts.entry(fold_name(name)).or_default() += 1;
                }
            }
        }
    }

    let mut valid_filters = Vec::new();
    let mut unavailable_filters = Vec::new();
    let mut invalid_entries = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let (filter, mut issues, query_issue) = decode_filter(entry);
        let id = filter
            .as_ref()
            .map(|filter| filter.id)
            .or_else(|| raw_v7_id(entry));
        if id.is_some_and(|id| id_counts.get(&id).copied().unwrap_or_default() > 1) {
            issues.insert(SavedFilterEntryIssueKind::DuplicateId);
        }
        if let Some(name) = entry
            .as_mapping()
            .and_then(|mapping| mapping.get("name"))
            .and_then(Value::as_str)
        {
            if valid_name(name)
                && name_counts
                    .get(&fold_name(name))
                    .copied()
                    .unwrap_or_default()
                    > 1
            {
                issues.insert(SavedFilterEntryIssueKind::DuplicateName);
            }
        }
        if !issues.is_empty() {
            invalid_entries.push(InvalidSavedFilterEntry {
                index,
                id,
                issues,
                query_issue,
            });
            continue;
        }
        let Some(filter) = filter else {
            invalid_entries.push(InvalidSavedFilterEntry {
                index,
                id,
                issues: BTreeSet::from([SavedFilterEntryIssueKind::InvalidEntry]),
                query_issue,
            });
            continue;
        };
        let missing_root_ids = match &filter.scope {
            SavedFilterScope::AllEnabledRoots => Vec::new(),
            SavedFilterScope::SelectedRoots { root_ids } => root_ids
                .iter()
                .filter(|id| !available_root_ids.contains(id))
                .copied()
                .collect(),
        };
        if missing_root_ids.is_empty() {
            valid_filters.push(filter);
        } else {
            unavailable_filters.push(UnavailableSavedFilter {
                filter,
                missing_root_ids,
            });
        }
    }
    SavedFilterCatalog {
        file_version,
        valid_filters,
        unavailable_filters,
        invalid_entries,
        file_issues: Vec::new(),
    }
}

fn decode_filter(
    value: &Value,
) -> (
    Option<SavedFilter>,
    BTreeSet<SavedFilterEntryIssueKind>,
    Option<SavedFilterQueryIssue>,
) {
    let mut issues = BTreeSet::new();
    let Some(mapping) = value.as_mapping() else {
        issues.insert(SavedFilterEntryIssueKind::InvalidEntry);
        return (None, issues, None);
    };
    let id = mapping
        .get("id")
        .and_then(Value::as_str)
        .and_then(parse_v7_id);
    let name = mapping.get("name").and_then(Value::as_str);
    let query = mapping.get("query").and_then(Value::as_str);
    let scope = mapping.get("scope").and_then(decode_scope);
    let (sort, unknown_sort) = mapping.get("sort").map_or((None, false), decode_sort);
    let created_at = mapping.get("createdAt").and_then(Value::as_str);
    let updated_at = mapping.get("updatedAt").and_then(Value::as_str);

    if id.is_none()
        || name.is_none_or(|name| !valid_name(name))
        || query.is_none_or(|query| !valid_query_text(query))
        || scope.is_none()
        || created_at.is_none_or(|value| !valid_timestamp(value))
        || updated_at.is_none_or(|value| !valid_timestamp(value))
    {
        issues.insert(SavedFilterEntryIssueKind::InvalidEntry);
    }
    if sort.is_none() {
        issues.insert(if unknown_sort {
            SavedFilterEntryIssueKind::UnknownSort
        } else {
            SavedFilterEntryIssueKind::InvalidEntry
        });
    }
    let query_issue = query.and_then(|query| {
        parse_query(query).err().map(|error| {
            issues.insert(SavedFilterEntryIssueKind::InvalidQuery);
            SavedFilterQueryIssue {
                kind: error.kind,
                offset: error.offset,
            }
        })
    });
    if !issues.is_empty() {
        return (None, issues, query_issue);
    }
    (
        Some(SavedFilter {
            id: id.expect("validated ID"),
            name: name.expect("validated name").into(),
            query: query.expect("validated query").into(),
            scope: scope.expect("validated scope"),
            sort: sort.expect("validated sort"),
            created_at: created_at.expect("validated timestamp").into(),
            updated_at: updated_at.expect("validated timestamp").into(),
        }),
        issues,
        query_issue,
    )
}

fn decode_scope(value: &Value) -> Option<SavedFilterScope> {
    let mapping = value.as_mapping()?;
    match mapping.get("kind")?.as_str()? {
        "all-enabled-roots" if !mapping.contains_key("rootIds") => {
            Some(SavedFilterScope::AllEnabledRoots)
        }
        "selected-roots" => {
            let values = mapping.get("rootIds")?.as_sequence()?;
            if values.is_empty() || values.len() > MAX_ROOT_IDS {
                return None;
            }
            let mut unique = BTreeSet::new();
            let mut root_ids = Vec::with_capacity(values.len());
            for value in values {
                let source = value.as_str()?;
                let id = parse_canonical_id(source)?;
                if !unique.insert(id) {
                    return None;
                }
                root_ids.push(id);
            }
            Some(SavedFilterScope::SelectedRoots { root_ids })
        }
        _ => None,
    }
}

fn decode_sort(value: &Value) -> (Option<SavedFilterSort>, bool) {
    let Some(mapping) = value.as_mapping() else {
        return (None, false);
    };
    let Some(field) = mapping.get("field").and_then(Value::as_str) else {
        return (None, false);
    };
    let field = match field {
        "file-name" => SavedFilterSortField::FileName,
        "modified-at" => SavedFilterSortField::ModifiedAt,
        "created-at" => SavedFilterSortField::CreatedAt,
        "file-size" => SavedFilterSortField::FileSize,
        "rating" => SavedFilterSortField::Rating,
        "asset-kind" => SavedFilterSortField::AssetKind,
        _ => return (None, true),
    };
    let direction = match mapping.get("direction").and_then(Value::as_str) {
        Some("ascending") => SavedFilterSortDirection::Ascending,
        Some("descending") => SavedFilterSortDirection::Descending,
        _ => return (None, false),
    };
    (Some(SavedFilterSort { field, direction }), false)
}

fn validate_plain_value(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), SavedFilterFileIssueKind> {
    if depth > MAX_VALUE_DEPTH || *nodes >= MAX_VALUE_NODES {
        return Err(SavedFilterFileIssueKind::InvalidFile);
    }
    *nodes += 1;
    match value {
        Value::Sequence(values) => {
            for value in values {
                validate_plain_value(value, depth + 1, nodes)?;
            }
        }
        Value::Mapping(mapping) => {
            for (key, value) in mapping {
                if !matches!(key, Value::String(_)) {
                    return Err(SavedFilterFileIssueKind::InvalidFile);
                }
                validate_plain_value(value, depth + 1, nodes)?;
            }
        }
        Value::Tagged(_) => return Err(SavedFilterFileIssueKind::InvalidFile),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn contains_yaml_anchor_or_alias(text: &str) -> bool {
    let mut block_parent_indent = None;
    for line in text.lines() {
        let indent = line.bytes().take_while(|byte| *byte == b' ').count();
        if let Some(parent_indent) = block_parent_indent {
            if line.trim().is_empty() || indent > parent_indent {
                continue;
            }
            block_parent_indent = None;
        }
        let mut single_quoted = false;
        let mut double_quoted = false;
        let mut escaped = false;
        let mut escaped_single_quote = false;
        let mut previous = None;
        let characters = line.char_indices().collect::<Vec<_>>();
        for (position, (_, character)) in characters.iter().enumerate() {
            if double_quoted && escaped {
                escaped = false;
                previous = Some(*character);
                continue;
            }
            if double_quoted && *character == '\\' {
                escaped = true;
                previous = Some(*character);
                continue;
            }
            if !double_quoted && *character == '\'' {
                if escaped_single_quote {
                    escaped_single_quote = false;
                    previous = Some(*character);
                    continue;
                }
                if single_quoted
                    && characters
                        .get(position + 1)
                        .is_some_and(|(_, next)| *next == '\'')
                {
                    escaped_single_quote = true;
                    previous = Some(*character);
                    continue;
                }
                single_quoted = !single_quoted;
                previous = Some(*character);
                continue;
            }
            if !single_quoted && *character == '"' {
                double_quoted = !double_quoted;
                previous = Some(*character);
                continue;
            }
            if single_quoted || double_quoted {
                previous = Some(*character);
                continue;
            }
            if *character == '#' && previous.is_none_or(char::is_whitespace) {
                break;
            }
            if matches!(*character, '&' | '*')
                && previous.is_none_or(|value| {
                    value.is_whitespace() || matches!(value, '[' | '{' | ',' | ':')
                })
                && characters
                    .get(position + 1)
                    .is_some_and(|(_, next)| next.is_alphanumeric() || matches!(*next, '_' | '-'))
            {
                return true;
            }
            if matches!(*character, '|' | '>')
                && previous.is_none_or(|value| value.is_whitespace() || matches!(value, ':' | '-'))
            {
                let remainder = characters[position + 1..]
                    .iter()
                    .map(|(_, value)| *value)
                    .collect::<String>();
                let header = remainder.split('#').next().unwrap_or_default().trim();
                if header
                    .chars()
                    .all(|value| value.is_ascii_digit() || matches!(value, '+' | '-'))
                {
                    block_parent_indent = Some(indent);
                    break;
                }
            }
            previous = Some(*character);
        }
    }
    false
}

fn unique_filter_index(document: &Value, id: Uuid) -> Result<usize, SavedFilterStoreError> {
    let matches = filters(document)?
        .iter()
        .enumerate()
        .filter(|(_, entry)| raw_v7_id(entry) == Some(id))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(SavedFilterStoreError::NotFound),
        [index] => Ok(*index),
        _ => Err(SavedFilterStoreError::AmbiguousId),
    }
}

fn filter_mapping_mut(
    document: &mut Value,
    index: usize,
) -> Result<&mut Mapping, SavedFilterStoreError> {
    filters_mut(document)?
        .get_mut(index)
        .and_then(Value::as_mapping_mut)
        .ok_or_else(|| {
            SavedFilterStoreError::InvalidMutation(vec![SavedFilterEntryIssueKind::InvalidEntry])
        })
}

fn filters(document: &Value) -> Result<&Vec<Value>, SavedFilterStoreError> {
    document
        .as_mapping()
        .and_then(|mapping| mapping.get("filters"))
        .and_then(Value::as_sequence)
        .ok_or(SavedFilterStoreError::InvalidFile(
            SavedFilterFileIssueKind::InvalidFile,
        ))
}

fn filters_mut(document: &mut Value) -> Result<&mut Vec<Value>, SavedFilterStoreError> {
    document
        .as_mapping_mut()
        .and_then(|mapping| mapping.get_mut("filters"))
        .and_then(Value::as_sequence_mut)
        .ok_or(SavedFilterStoreError::InvalidFile(
            SavedFilterFileIssueKind::InvalidFile,
        ))
}

fn raw_v7_id(value: &Value) -> Option<Uuid> {
    value
        .as_mapping()?
        .get("id")?
        .as_str()
        .and_then(parse_v7_id)
}

fn parse_v7_id(value: &str) -> Option<Uuid> {
    let id = parse_canonical_id(value)?;
    (id.get_version_num() == 7).then_some(id)
}

fn parse_canonical_id(value: &str) -> Option<Uuid> {
    let id = Uuid::parse_str(value).ok()?;
    (id.to_string() == value).then_some(id)
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= MAX_NAME_CHARACTERS
        && !value.chars().any(char::is_control)
}

fn fold_name(value: &str) -> String {
    let normalized = value.trim().nfc().collect::<String>();
    normalized
        .as_str()
        .case_fold()
        .collect::<String>()
        .nfc()
        .collect()
}

fn valid_query_text(value: &str) -> bool {
    value.chars().count() <= MAX_QUERY_CHARACTERS
        && !value.contains(['\r', '\n'])
        && !value
            .chars()
            .any(|character| character.is_control() && character != '\t')
}

fn valid_timestamp(value: &str) -> bool {
    value.len() <= 64 && DateTime::parse_from_rfc3339(value).is_ok()
}

fn default_document() -> Value {
    let mut mapping = Mapping::new();
    mapping.insert(
        Value::String("schema".into()),
        Value::Number(SAVED_FILTER_SCHEMA_VERSION.into()),
    );
    mapping.insert(Value::String("filters".into()), Value::Sequence(Vec::new()));
    Value::Mapping(mapping)
}

fn saved_filter_value(filter: &SavedFilter) -> Value {
    let mut mapping = Mapping::new();
    mapping.insert(
        Value::String("id".into()),
        Value::String(filter.id.to_string()),
    );
    mapping.insert(
        Value::String("name".into()),
        Value::String(filter.name.clone()),
    );
    mapping.insert(
        Value::String("query".into()),
        Value::String(filter.query.clone()),
    );
    mapping.insert(
        Value::String("scope".into()),
        saved_filter_scope_value(&filter.scope),
    );
    mapping.insert(
        Value::String("sort".into()),
        saved_filter_sort_value(filter.sort),
    );
    mapping.insert(
        Value::String("createdAt".into()),
        Value::String(filter.created_at.clone()),
    );
    mapping.insert(
        Value::String("updatedAt".into()),
        Value::String(filter.updated_at.clone()),
    );
    Value::Mapping(mapping)
}

fn saved_filter_scope_value(scope: &SavedFilterScope) -> Value {
    let mut mapping = Mapping::new();
    match scope {
        SavedFilterScope::AllEnabledRoots => {
            mapping.insert(
                Value::String("kind".into()),
                Value::String("all-enabled-roots".into()),
            );
        }
        SavedFilterScope::SelectedRoots { root_ids } => {
            mapping.insert(
                Value::String("kind".into()),
                Value::String("selected-roots".into()),
            );
            mapping.insert(
                Value::String("rootIds".into()),
                Value::Sequence(
                    root_ids
                        .iter()
                        .map(|id| Value::String(id.to_string()))
                        .collect(),
                ),
            );
        }
    }
    Value::Mapping(mapping)
}

fn saved_filter_sort_value(sort: SavedFilterSort) -> Value {
    let field = match sort.field {
        SavedFilterSortField::FileName => "file-name",
        SavedFilterSortField::ModifiedAt => "modified-at",
        SavedFilterSortField::CreatedAt => "created-at",
        SavedFilterSortField::FileSize => "file-size",
        SavedFilterSortField::Rating => "rating",
        SavedFilterSortField::AssetKind => "asset-kind",
    };
    let direction = match sort.direction {
        SavedFilterSortDirection::Ascending => "ascending",
        SavedFilterSortDirection::Descending => "descending",
    };
    let mut mapping = Mapping::new();
    mapping.insert(Value::String("field".into()), Value::String(field.into()));
    mapping.insert(
        Value::String("direction".into()),
        Value::String(direction.into()),
    );
    Value::Mapping(mapping)
}

fn normalize_document_order(document: &mut Value) -> Result<(), SavedFilterStoreError> {
    let root = document
        .as_mapping_mut()
        .ok_or(SavedFilterStoreError::InvalidFile(
            SavedFilterFileIssueKind::InvalidFile,
        ))?;
    *root = reordered_mapping(std::mem::take(root), &["schema", "filters"]);
    for entry in filters_mut(document)? {
        if let Some(mapping) = entry.as_mapping_mut() {
            *mapping = reordered_mapping(std::mem::take(mapping), FILTER_KEYS);
            if let Some(scope) = mapping.get_mut("scope").and_then(Value::as_mapping_mut) {
                *scope = reordered_mapping(std::mem::take(scope), &["kind", "rootIds"]);
            }
            if let Some(sort) = mapping.get_mut("sort").and_then(Value::as_mapping_mut) {
                *sort = reordered_mapping(std::mem::take(sort), &["field", "direction"]);
            }
            for key in ["createdAt", "updatedAt"] {
                let canonical = mapping
                    .get(key)
                    .and_then(Value::as_str)
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                    .map(|parsed| timestamp(parsed.with_timezone(&Utc)));
                if let Some(canonical) = canonical {
                    mapping.insert(Value::String(key.into()), Value::String(canonical));
                }
            }
        }
    }
    Ok(())
}

fn reordered_mapping(mut source: Mapping, known_keys: &[&str]) -> Mapping {
    let mut ordered = Mapping::new();
    for key in known_keys {
        if let Some(value) = source.shift_remove(*key) {
            ordered.insert(Value::String((*key).into()), value);
        }
    }
    for (key, value) in source {
        ordered.insert(key, value);
    }
    ordered
}

fn file_issue_catalog(
    file_version: SavedFilterFileVersion,
    kind: SavedFilterFileIssueKind,
) -> SavedFilterCatalog {
    SavedFilterCatalog {
        file_version,
        valid_filters: Vec::new(),
        unavailable_filters: Vec::new(),
        invalid_entries: Vec::new(),
        file_issues: vec![SavedFilterFileIssue { kind }],
    }
}

fn ensure_version(
    expected: &SavedFilterFileVersion,
    actual: &SavedFilterFileVersion,
) -> Result<(), SavedFilterStoreError> {
    if expected == actual {
        Ok(())
    } else {
        Err(SavedFilterStoreError::ExternalChange {
            expected: Box::new(expected.clone()),
            actual: Box::new(actual.clone()),
        })
    }
}

fn system_time_unix_ms(
    modified: std::time::SystemTime,
    path: &Path,
) -> Result<u64, SavedFilterStoreError> {
    modified
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .map_err(|source| SavedFilterStoreError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

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
    use std::fmt::Write as _;
    use std::fs;

    use chrono::TimeZone;
    use tempfile::tempdir;

    use super::*;

    const FIRST_ID: &str = "0198a7c2-8341-7a31-b842-f15d39f33c18";
    const SECOND_ID: &str = "0198a7c2-8342-7a31-b842-f15d39f33c19";
    const ROOT_ID: &str = "0198a7c2-9001-7a31-b842-f15d39f33c20";

    fn all_enabled_input(name: &str, query: &str) -> CreateSavedFilter {
        CreateSavedFilter {
            name: name.into(),
            query: query.into(),
            scope: SavedFilterScope::AllEnabledRoots,
            sort: SavedFilterSort {
                field: SavedFilterSortField::ModifiedAt,
                direction: SavedFilterSortDirection::Descending,
            },
        }
    }

    fn fixed_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, 0, 0, 0)
            .single()
            .expect("fixed time")
    }

    #[test]
    fn missing_file_creates_and_reloads_only_the_expression() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("saved-filters.yml");
        let store = SavedFilterStore::new(path.clone());
        let empty = store.load(&BTreeSet::new()).expect("empty catalog");
        assert_eq!(
            empty.file_version,
            SavedFilterFileVersion::expected_absent()
        );

        let id = Uuid::parse_str(FIRST_ID).expect("ID");
        let created = store
            .create_at(
                &empty.file_version,
                all_enabled_input("Wide images", "type:image width:>=1920"),
                id,
                fixed_time(),
            )
            .expect("create filter");
        assert_eq!(created.filter.as_ref().map(|filter| filter.id), Some(id));
        let bytes = fs::read_to_string(&path).expect("saved YAML");
        assert!(bytes.starts_with("schema: 1\nfilters:\n"));
        for forbidden in ["assetKey", "relativePath", "result", "thumbnail", "index"] {
            assert!(
                !bytes.contains(forbidden),
                "unexpected snapshot field {forbidden}"
            );
        }
        let loaded = store.load(&BTreeSet::new()).expect("reload");
        assert_eq!(loaded.valid_filters.len(), 1);
        assert!(loaded.invalid_entries.is_empty());
        assert_eq!(loaded.file_version.sha256, Some(digest(bytes.as_bytes())));
    }

    #[test]
    fn isolates_duplicates_invalid_query_unknown_sort_and_unavailable_root() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("saved-filters.yml");
        fs::write(
            &path,
            format!(
                "schema: 1\nfilters:\n  - id: {FIRST_ID}\n    name: Straße\n    query: type:image\n    scope: {{kind: all-enabled-roots}}\n    sort: {{field: file-name, direction: ascending}}\n    createdAt: '2026-08-20T00:00:00.000Z'\n    updatedAt: '2026-08-20T00:00:00.000Z'\n  - id: {SECOND_ID}\n    name: STRASSE\n    query: kind:image\n    scope: {{kind: all-enabled-roots}}\n    sort: {{field: future-order, direction: ascending}}\n    createdAt: '2026-08-20T00:00:00.000Z'\n    updatedAt: '2026-08-20T00:00:00.000Z'\n  - id: 0198a7c2-8343-7a31-b842-f15d39f33c1a\n    name: Offline\n    query: favorite:true\n    scope:\n      kind: selected-roots\n      rootIds: [{ROOT_ID}]\n    sort: {{field: rating, direction: descending}}\n    createdAt: '2026-08-20T00:00:00.000Z'\n    updatedAt: '2026-08-20T00:00:00.000Z'\n"
            ),
        )
        .expect("write catalog");
        let catalog = SavedFilterStore::new(path)
            .load(&BTreeSet::new())
            .expect("load catalog");
        assert!(catalog.valid_filters.is_empty());
        assert_eq!(catalog.invalid_entries.len(), 2);
        assert!(
            catalog.invalid_entries[0]
                .issues
                .contains(&SavedFilterEntryIssueKind::DuplicateName)
        );
        assert!(
            catalog.invalid_entries[1]
                .issues
                .is_superset(&BTreeSet::from([
                    SavedFilterEntryIssueKind::DuplicateName,
                    SavedFilterEntryIssueKind::InvalidQuery,
                    SavedFilterEntryIssueKind::UnknownSort,
                ]))
        );
        assert_eq!(catalog.unavailable_filters.len(), 1);
        assert_eq!(catalog.unavailable_filters[0].missing_root_ids.len(), 1);
    }

    #[test]
    fn preserves_unknown_fields_and_blocks_external_change() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("saved-filters.yml");
        fs::write(
            &path,
            format!(
                "futureTop:\n  keep: true\nschema: 1\nfilters:\n  - futureBefore: alpha\n    id: {FIRST_ID}\n    name: Original\n    query: type:image\n    scope: {{kind: all-enabled-roots}}\n    sort: {{field: file-name, direction: ascending}}\n    createdAt: '2026-08-20T00:00:00.000Z'\n    updatedAt: '2026-08-20T00:00:00.000Z'\n    futureAfter: beta\n"
            ),
        )
        .expect("write catalog");
        let store = SavedFilterStore::new(path.clone());
        let loaded = store.load(&BTreeSet::new()).expect("load");
        fs::write(
            &path,
            fs::read_to_string(&path).expect("read") + "external: true\n",
        )
        .expect("external edit");
        assert!(matches!(
            store.rename(
                &loaded.file_version,
                Uuid::parse_str(FIRST_ID).expect("ID"),
                "Changed".into()
            ),
            Err(SavedFilterStoreError::ExternalChange { .. })
        ));
        let current = store.load(&BTreeSet::new()).expect("reload");
        store
            .rename(
                &current.file_version,
                Uuid::parse_str(FIRST_ID).expect("ID"),
                "Changed".into(),
            )
            .expect("rename");
        let output = fs::read_to_string(path).expect("output");
        assert!(output.starts_with("schema: 1\nfilters:\n"));
        assert!(output.contains("futureBefore: alpha"));
        assert!(output.contains("futureAfter: beta"));
        assert!(output.contains("futureTop:\n  keep: true"));
        assert!(output.contains("external: true"));
    }

    #[test]
    fn invalid_whole_files_are_reported_and_never_replaced() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("saved-filters.yml");
        let cases = [
            "schema: 1\nfilters: &items []\ncopy: *items\n",
            "schema: 1\nfilters: !future []\n",
            "schema: 1\nfilters: []\nschema: 1\n",
            "schema: 2\nfilters: []\n",
            "schema: 1\nfilters: []\n1: invalid-key\n",
        ];
        for source in cases {
            fs::write(&path, source).expect("write invalid file");
            let store = SavedFilterStore::new(path.clone());
            let catalog = store.load(&BTreeSet::new()).expect("load issue");
            assert_eq!(catalog.file_issues.len(), 1, "{source}");
            let before = fs::read(&path).expect("before");
            assert!(matches!(
                store.create(
                    &catalog.file_version,
                    all_enabled_input("New", "type:image")
                ),
                Err(SavedFilterStoreError::InvalidFile(_))
            ));
            assert_eq!(fs::read(&path).expect("after"), before);
        }
    }

    #[test]
    fn accepts_anchor_like_text_but_rejects_depth_and_oversized_files() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("saved-filters.yml");
        let valid = "schema: 1\nfilters: []\nfutureQuoted: 'it''s *literal'\nfutureBlock: |\n  * bullet & note\n";
        fs::write(&path, valid).expect("write valid scalars");
        let store = SavedFilterStore::new(path.clone());
        assert!(
            store
                .load(&BTreeSet::new())
                .expect("load valid scalars")
                .file_issues
                .is_empty()
        );

        let mut deep = String::from("schema: 1\nfilters: []\nfuture:\n");
        for depth in 0..=MAX_VALUE_DEPTH {
            writeln!(deep, "{}level{depth}:", "  ".repeat(depth + 1)).expect("deep YAML");
        }
        writeln!(deep, "{}value", "  ".repeat(MAX_VALUE_DEPTH + 2)).expect("deep value");
        fs::write(&path, deep).expect("write deep YAML");
        let deep_catalog = store.load(&BTreeSet::new()).expect("load deep YAML");
        assert_eq!(
            deep_catalog.file_issues[0].kind,
            SavedFilterFileIssueKind::InvalidFile
        );

        fs::write(
            &path,
            vec![b' '; usize::try_from(MAX_SAVED_FILTER_FILE_BYTES + 1).expect("size")],
        )
        .expect("write oversized YAML");
        let large_catalog = store.load(&BTreeSet::new()).expect("load oversized YAML");
        assert_eq!(
            large_catalog.file_issues[0].kind,
            SavedFilterFileIssueKind::FileTooLarge
        );
        assert!(large_catalog.file_version.sha256.is_none());
    }

    #[test]
    fn duplicate_ids_isolate_every_matching_entry() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("saved-filters.yml");
        fs::write(
            &path,
            format!(
                "schema: 1\nfilters:\n  - id: {FIRST_ID}\n    name: First\n    query: type:image\n    scope: {{kind: all-enabled-roots}}\n    sort: {{field: file-name, direction: ascending}}\n    createdAt: '2026-08-20T00:00:00.000Z'\n    updatedAt: '2026-08-20T00:00:00.000Z'\n  - id: {FIRST_ID}\n    name: Second\n    query: type:video\n    scope: {{kind: all-enabled-roots}}\n    sort: {{field: file-name, direction: ascending}}\n    createdAt: '2026-08-20T00:00:00.000Z'\n    updatedAt: '2026-08-20T00:00:00.000Z'\n"
            ),
        )
        .expect("write duplicate IDs");
        let store = SavedFilterStore::new(path);
        let catalog = store.load(&BTreeSet::new()).expect("load duplicates");
        assert_eq!(catalog.invalid_entries.len(), 2);
        assert!(catalog.invalid_entries.iter().all(|entry| {
            entry
                .issues
                .contains(&SavedFilterEntryIssueKind::DuplicateId)
        }));
        assert!(matches!(
            store.rename(
                &catalog.file_version,
                Uuid::parse_str(FIRST_ID).expect("ID"),
                "Ambiguous".into()
            ),
            Err(SavedFilterStoreError::AmbiguousId)
        ));
    }

    #[test]
    fn update_delete_and_duplicate_id_are_precise() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("saved-filters.yml");
        let store = SavedFilterStore::new(path);
        let id = Uuid::parse_str(FIRST_ID).expect("ID");
        let created = store
            .create_at(
                &SavedFilterFileVersion::expected_absent(),
                all_enabled_input("First", "type:image"),
                id,
                fixed_time(),
            )
            .expect("create");
        let updated = store
            .update_at(
                &created.file_version,
                id,
                UpdateSavedFilter {
                    name: "Updated".into(),
                    query: "rating:>=4".into(),
                    scope: SavedFilterScope::AllEnabledRoots,
                    sort: SavedFilterSort {
                        field: SavedFilterSortField::Rating,
                        direction: SavedFilterSortDirection::Descending,
                    },
                },
                fixed_time() + chrono::Duration::minutes(1),
            )
            .expect("update");
        assert_eq!(
            updated.filter.as_ref().map(|filter| filter.name.as_str()),
            Some("Updated")
        );
        let deleted = store.delete(&updated.file_version, id).expect("delete");
        assert!(deleted.filter.is_none());
        let catalog = store.load(&BTreeSet::new()).expect("empty reload");
        assert!(catalog.valid_filters.is_empty());
        assert!(catalog.file_version.exists);
    }

    #[test]
    fn execution_rebuilds_current_scoped_and_sorted_results() {
        let root_a = Uuid::parse_str(ROOT_ID).expect("root A");
        let root_b = Uuid::parse_str("0198a7c2-9002-7a31-b842-f15d39f33c21").expect("root B");
        let mut high = AssetRecord::untagged(
            "high".into(),
            PathBuf::from("/root/high.png"),
            "image/png".into(),
            10,
            20,
        );
        high.root_id = Some(root_a);
        high.tags.insert("project/eagle".into());
        high.rating = 5;
        let mut medium = high.clone();
        medium.key = "medium".into();
        medium.file_name = "medium.png".into();
        medium.rating = 4;
        let mut offline = high.clone();
        offline.key = "offline".into();
        offline.root_id = Some(root_b);
        let filter = SavedFilter {
            id: Uuid::parse_str(FIRST_ID).expect("filter ID"),
            name: "Review".into(),
            query: "project/eagle rating:>=4".into(),
            scope: SavedFilterScope::SelectedRoots {
                root_ids: vec![root_a, root_b],
            },
            sort: SavedFilterSort {
                field: SavedFilterSortField::Rating,
                direction: SavedFilterSortDirection::Descending,
            },
            created_at: "2026-08-20T00:00:00.000Z".into(),
            updated_at: "2026-08-20T00:00:00.000Z".into(),
        };
        let records = vec![medium, offline, high];
        let enabled = BTreeSet::from([root_a, root_b]);
        let available = BTreeSet::from([root_a]);
        let first =
            execute_saved_filter(&filter, &records, &enabled, &available).expect("first execution");
        let rebuilt = execute_saved_filter(&filter, &records.clone(), &enabled, &available)
            .expect("rebuilt execution");
        assert_eq!(first.ordered_keys, ["high", "medium"]);
        assert_eq!(first.missing_root_ids, [root_b]);
        assert_eq!(first.scoped_assets, 2);
        assert_eq!(first, rebuilt);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_target_without_touching_it() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("temp directory");
        let target = directory.path().join("target.yml");
        let path = directory.path().join("saved-filters.yml");
        fs::write(&target, "do not touch").expect("target");
        symlink(&target, &path).expect("symlink");
        let store = SavedFilterStore::new(path);
        assert!(matches!(
            store.load(&BTreeSet::new()),
            Err(SavedFilterStoreError::UnsafeTarget)
        ));
        assert_eq!(
            fs::read_to_string(target).expect("target bytes"),
            "do not touch"
        );
    }
}
