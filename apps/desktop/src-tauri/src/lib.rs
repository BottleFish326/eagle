use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use app_config::{
    APPLICATION_CONFIG_SCHEMA_VERSION, ApplicationConfig, ApplicationConfigManager,
    DiagnosticAccessSummary, DiagnosticBuild, DiagnosticCacheSummary, DiagnosticCatalogSummary,
    DiagnosticConfigurationSummary, DiagnosticExportReport, DiagnosticLevel,
    DiagnosticPerformanceSummary, DiagnosticRuntime, DiagnosticService, DiagnosticSnapshot,
    UpdateUiPreferences,
};
use asset_batch_workflows::{
    BatchPreflightError, BatchPreflightStore, BatchPreflightSummary, BatchRootAuthorization,
    MetadataPreflightInput, RootRuntimeState,
};
use asset_catalog::{
    AssetCatalog, BatchMetadataEdit, BatchMetadataEditResult, CatalogRootReconciliation,
    EditFailureKind, MetadataEditFailure, QueryAssetsInput, QueryAssetsResult,
};
use asset_filesystem::{
    AddLibraryRoot, FsChangeBatch, FsRescanReason, LibraryRoot, LibraryRootManager,
    LibraryRootStatus, ReconciliationReport, RelinkCandidate, RelinkReceipt, RootAccessStatus,
    ScanBatch, ScanCancellation, ScanCompletion, ScanOptions, ScanSummary, UpdateLibraryRoot,
    WatchSession, apply_relink, inspect_reconciliation, inspect_root_access,
    scan_root_incremental_controlled,
};
use asset_index::QueryParseError;
use asset_link_resolver::{
    AddVaultRoot, UpdateVaultRoot, VaultError, VaultManager, VaultReference, VaultRoot,
    VaultRootStatus,
};
use asset_preview::{
    CacheClearReport, CacheMaintenanceReport, CacheStartupReport, CacheStats, PreviewError,
    ThumbnailOutcome, ThumbnailRequest, ThumbnailService,
};
use asset_saved_filters::{
    CreateSavedFilter, SavedFilterCatalog, SavedFilterEntryIssueKind, SavedFilterExecution,
    SavedFilterExecutionError, SavedFilterFileIssueKind, SavedFilterFileVersion,
    SavedFilterMutation, SavedFilterStore, SavedFilterStoreError, UpdateSavedFilter,
    execute_saved_filter_at_revision as execute_saved_filter_view,
};
use asset_selection::{
    ExplicitSelectionInput, QuerySelectionInput, RangeSelectionInput, SelectionError,
    SelectionSessionStats, SelectionSessionStore, SelectionSnapshotSummary,
};
use asset_transactions::{
    MetadataTransactionStore, TransactionFailureKind, TransactionRecoveryResult,
    TransactionScopeItem, TransactionSummary, TransactionTarget,
};
use format_worker::{WORKER_BUNDLE_MANIFEST, open_libheif_worker_bundle};
use resource_control::{ResourceController, ResourceMode, ResourceSnapshot, WorkKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Manager, State, ipc::Channel, ipc::Response};
use uuid::Uuid;

mod metadata_conflicts;
mod support;

use metadata_conflicts::{
    MetadataConflictStore, MetadataConflictView, ResolveMetadataConflictInput,
};
use support::{
    AssetTraceReport, LibraryConsistencyReport, append_reconciliation_failure,
    append_reconciliation_findings,
    inspect_library_consistency as build_library_consistency_report,
    trace_asset as build_asset_trace,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildInfo {
    version: &'static str,
    git_commit: &'static str,
    build_target: &'static str,
    build_profile: &'static str,
    rustc_version: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ApplicationPaths {
    #[serde(rename = "configDirectory")]
    config: PathBuf,
    #[serde(rename = "cacheDirectory")]
    cache: PathBuf,
    #[serde(rename = "logDirectory")]
    log: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeRecoveryStatus {
    paths: ApplicationPaths,
    cache_startup: CacheStartupReport,
    cache_stats: CacheStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeResourceStatus {
    scheduler: ResourceSnapshot,
    active_scans: usize,
    active_watches: usize,
    cache: CacheStats,
    scan_batch_queue_capacity: usize,
    pending_scan_batches: usize,
    max_active_scans: usize,
    max_active_watches: usize,
}

const SCAN_BATCH_QUEUE_CAPACITY: usize = 8;
const MAX_ACTIVE_SCANS: usize = 8;
const MAX_ACTIVE_WATCHES: usize = 64;

enum ScanPipelineMessage {
    Batch(ScanBatch),
    Finished(Result<ScanSummary, String>),
}

fn scan_pipeline_channel() -> (
    SyncSender<ScanPipelineMessage>,
    Receiver<ScanPipelineMessage>,
) {
    std::sync::mpsc::sync_channel(SCAN_BATCH_QUEUE_CAPACITY)
}

#[derive(Debug, Clone)]
struct RuntimeState {
    paths: ApplicationPaths,
    cache_startup: CacheStartupReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DerivedStateResetReport {
    cache: CacheClearReport,
    catalog_assets_removed: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchMetadataEditCommandResult {
    updated: Vec<asset_core::AssetRecord>,
    failures: Vec<MetadataEditFailure>,
    transaction: Option<TransactionSummary>,
    conflicts: Vec<MetadataConflictView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
enum LibraryScanEvent {
    Started {
        scan_id: Uuid,
        root_id: Uuid,
        root: PathBuf,
    },
    Batch {
        scan_id: Uuid,
        batch: ScanBatch,
    },
    Finished {
        scan_id: Uuid,
        summary: ScanSummary,
        reconciliation: CatalogRootReconciliation,
    },
    Failed {
        scan_id: Uuid,
        message: String,
        removed_keys: Vec<String>,
        restored_records: Vec<asset_core::AssetRecord>,
        root_access_status: Option<RootAccessStatus>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
enum LibraryWatchEvent {
    Started {
        watch_id: Uuid,
        root_id: Uuid,
    },
    Changes {
        watch_id: Uuid,
        root_id: Uuid,
        batch: FsChangeBatch,
    },
    Failed {
        watch_id: Uuid,
        root_id: Uuid,
        message: String,
        root_access_status: Option<RootAccessStatus>,
    },
    Stopped {
        watch_id: Uuid,
        root_id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
enum QueryAssetsError {
    Parse { error: QueryParseError },
    Internal { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SelectionCommandErrorKind {
    SnapshotNotFound,
    SnapshotExpired,
    CatalogChanged,
    AssetMissing,
    RootDisabled,
    RootOffline,
    AuthorizationLost,
    InvalidOperation,
    OutputTooLarge,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionCommandError {
    kind: SelectionCommandErrorKind,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_kind: Option<asset_index::QueryParseErrorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_id: Option<Uuid>,
}

impl SelectionCommandError {
    fn simple(kind: SelectionCommandErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            actual_revision: None,
            query_kind: None,
            query_offset: None,
            root_id: None,
        }
    }

    fn root(kind: SelectionCommandErrorKind, root_id: Uuid) -> Self {
        Self {
            kind,
            message: "selection root is not currently authorized and available".into(),
            actual_revision: None,
            query_kind: None,
            query_offset: None,
            root_id: Some(root_id),
        }
    }
}

impl From<SelectionError> for SelectionCommandError {
    fn from(error: SelectionError) -> Self {
        match error {
            SelectionError::SnapshotNotFound => Self::simple(
                SelectionCommandErrorKind::SnapshotNotFound,
                "selection snapshot was not found",
            ),
            SelectionError::SnapshotExpired => Self::simple(
                SelectionCommandErrorKind::SnapshotExpired,
                "selection snapshot expired",
            ),
            SelectionError::CatalogChanged { actual_revision } => Self {
                kind: SelectionCommandErrorKind::CatalogChanged,
                message: "asset catalog changed; refresh the view before selecting".into(),
                actual_revision: Some(actual_revision),
                query_kind: None,
                query_offset: None,
                root_id: None,
            },
            SelectionError::InvalidQuery { kind, offset } => Self {
                kind: SelectionCommandErrorKind::InvalidOperation,
                message: "selection query is invalid".into(),
                actual_revision: None,
                query_kind: Some(kind),
                query_offset: Some(offset),
                root_id: None,
            },
            SelectionError::AssetMissing => Self::simple(
                SelectionCommandErrorKind::AssetMissing,
                "selection contains an asset that is no longer in the catalog",
            ),
            SelectionError::TooManyItems
            | SelectionError::TooManyExplicitItems
            | SelectionError::SessionBudgetExceeded => Self::simple(
                SelectionCommandErrorKind::OutputTooLarge,
                "selection exceeds the bounded runtime session budget",
            ),
            SelectionError::EmptyRootScope
            | SelectionError::EmptySelection
            | SelectionError::AnchorMissing
            | SelectionError::TargetMissing => Self::simple(
                SelectionCommandErrorKind::InvalidOperation,
                error.to_string(),
            ),
            SelectionError::StateUnavailable => Self::simple(
                SelectionCommandErrorKind::Internal,
                "selection session state is unavailable",
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum BatchCommandErrorKind {
    SnapshotNotFound,
    SnapshotExpired,
    InvalidOperation,
    OutputTooLarge,
    PreflightNotFound,
    PreflightExpired,
    PreflightStale,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchCommandError {
    kind: BatchCommandErrorKind,
    message: String,
}

impl BatchCommandError {
    fn new(kind: BatchCommandErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<BatchPreflightError> for BatchCommandError {
    fn from(error: BatchPreflightError) -> Self {
        match error {
            BatchPreflightError::Selection(SelectionError::SnapshotNotFound) => Self::new(
                BatchCommandErrorKind::SnapshotNotFound,
                "selection snapshot was not found",
            ),
            BatchPreflightError::Selection(SelectionError::SnapshotExpired) => Self::new(
                BatchCommandErrorKind::SnapshotExpired,
                "selection snapshot expired",
            ),
            BatchPreflightError::Selection(_) | BatchPreflightError::InvalidOperation => Self::new(
                BatchCommandErrorKind::InvalidOperation,
                "batch operation is invalid",
            ),
            BatchPreflightError::SessionBudgetExceeded => Self::new(
                BatchCommandErrorKind::OutputTooLarge,
                "batch preflight exceeds the bounded runtime budget",
            ),
            BatchPreflightError::OperationNotFound => Self::new(
                BatchCommandErrorKind::PreflightNotFound,
                "batch preflight was not found",
            ),
            BatchPreflightError::OperationExpired => Self::new(
                BatchCommandErrorKind::PreflightExpired,
                "batch preflight expired",
            ),
            BatchPreflightError::PreflightStale => Self::new(
                BatchCommandErrorKind::PreflightStale,
                "batch confirmation does not match the prepared operation",
            ),
            BatchPreflightError::StateUnavailable => Self::new(
                BatchCommandErrorKind::Internal,
                "batch preflight state is unavailable",
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SavedFilterCommandErrorKind {
    InvalidFile,
    FileTooLarge,
    UnsupportedSchema,
    InvalidEntry,
    DuplicateId,
    DuplicateName,
    InvalidQuery,
    UnknownSort,
    RewriteFailed,
    ExternalChange,
    NotFound,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedFilterCommandError {
    kind: SavedFilterCommandErrorKind,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual_version: Option<SavedFilterFileVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_kind: Option<asset_index::QueryParseErrorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
enum ThumbnailCommandError {
    AssetNotFound {
        #[serde(rename = "assetKey")]
        asset_key: String,
    },
    InvalidRequest {
        message: String,
    },
    Cache {
        message: String,
    },
    Internal {
        message: String,
    },
    RecoveryBusy {
        #[serde(rename = "activeScans")]
        active_scans: usize,
        message: String,
    },
    RecoveryIncomplete {
        #[serde(rename = "pendingRoots")]
        pending_roots: usize,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveVaultReferencesInput {
    vault_id: Uuid,
    asset_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedVaultReference {
    asset_key: String,
    #[serde(flatten)]
    reference: VaultReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum VaultReferenceFailureKind {
    AssetNotFound,
    VaultNotFound,
    VaultDisabled,
    VaultUnavailable,
    AssetUnavailable,
    OutsideVault,
    UnsafeWikilink,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultReferenceFailure {
    asset_key: String,
    kind: VaultReferenceFailureKind,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolveVaultReferencesResult {
    resolved: Vec<ResolvedVaultReference>,
    failures: Vec<VaultReferenceFailure>,
}

impl From<PreviewError> for ThumbnailCommandError {
    fn from(error: PreviewError) -> Self {
        match error {
            PreviewError::InvalidMaxEdge(_) | PreviewError::InvalidCacheKey(_) => {
                Self::InvalidRequest {
                    message: error.to_string(),
                }
            }
            PreviewError::UnsafeCacheRoot(_)
            | PreviewError::CacheIo { .. }
            | PreviewError::CacheMetadata { .. }
            | PreviewError::MissingCacheEntry(_) => Self::Cache {
                message: error.to_string(),
            },
            PreviewError::InvalidConcurrency(_)
            | PreviewError::InvalidCachePolicy(_)
            | PreviewError::InvalidWorkerIdentity
            | PreviewError::PoisonedLock(_)
            | PreviewError::Resource(_) => Self::Internal {
                message: error.to_string(),
            },
        }
    }
}

impl From<SavedFilterStoreError> for SavedFilterCommandError {
    fn from(error: SavedFilterStoreError) -> Self {
        match error {
            SavedFilterStoreError::InvalidFile(kind) => Self::file_issue(kind),
            SavedFilterStoreError::ExternalChange { actual, .. } => Self {
                kind: SavedFilterCommandErrorKind::ExternalChange,
                message: "saved filters changed outside Material Eagle; reload before saving"
                    .into(),
                actual_version: Some(*actual),
                query_kind: None,
                query_offset: None,
            },
            SavedFilterStoreError::NotFound => Self::simple(
                SavedFilterCommandErrorKind::NotFound,
                "saved filter is missing or stale",
            ),
            SavedFilterStoreError::AmbiguousId => Self::simple(
                SavedFilterCommandErrorKind::DuplicateId,
                "duplicate saved filter IDs must be repaired before editing",
            ),
            SavedFilterStoreError::InvalidMutation(issues) => {
                let kind = issues
                    .into_iter()
                    .map(command_kind_for_entry_issue)
                    .min_by_key(|kind| saved_filter_error_priority(*kind))
                    .unwrap_or(SavedFilterCommandErrorKind::InvalidEntry);
                Self::simple(
                    kind,
                    "saved filter input is invalid or conflicts with another entry",
                )
            }
            SavedFilterStoreError::TagRewrite(error) => match error {
                asset_index::QueryTagRewriteError::InvalidQuery(_)
                | asset_index::QueryTagRewriteError::RewriteInvalid(_) => Self::simple(
                    SavedFilterCommandErrorKind::InvalidQuery,
                    "saved filter query cannot be rewritten",
                ),
                asset_index::QueryTagRewriteError::InvalidTag => Self::simple(
                    SavedFilterCommandErrorKind::InvalidEntry,
                    "Tag rename input is invalid",
                ),
                asset_index::QueryTagRewriteError::EquivalenceFailed => Self::simple(
                    SavedFilterCommandErrorKind::RewriteFailed,
                    "saved filter rewrite did not preserve query semantics",
                ),
            },
            SavedFilterStoreError::TooManyFilters => Self::simple(
                SavedFilterCommandErrorKind::InvalidEntry,
                "saved filter limit of 512 entries was reached",
            ),
            SavedFilterStoreError::Io { .. }
            | SavedFilterStoreError::UnsafeTarget
            | SavedFilterStoreError::Serialize(_)
            | SavedFilterStoreError::Persist { .. } => Self::simple(
                SavedFilterCommandErrorKind::Internal,
                "saved filter storage is unavailable",
            ),
        }
    }
}

impl From<SavedFilterExecutionError> for SavedFilterCommandError {
    fn from(error: SavedFilterExecutionError) -> Self {
        Self {
            kind: SavedFilterCommandErrorKind::InvalidQuery,
            message: "saved filter query no longer parses".into(),
            actual_version: None,
            query_kind: Some(error.kind),
            query_offset: Some(error.offset),
        }
    }
}

impl SavedFilterCommandError {
    fn simple(kind: SavedFilterCommandErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.into(),
            actual_version: None,
            query_kind: None,
            query_offset: None,
        }
    }

    fn file_issue(kind: SavedFilterFileIssueKind) -> Self {
        match kind {
            SavedFilterFileIssueKind::InvalidFile => Self::simple(
                SavedFilterCommandErrorKind::InvalidFile,
                "saved filter YAML is invalid and was left unchanged",
            ),
            SavedFilterFileIssueKind::FileTooLarge => Self::simple(
                SavedFilterCommandErrorKind::FileTooLarge,
                "saved filter YAML exceeds the 1 MiB limit",
            ),
            SavedFilterFileIssueKind::UnsupportedSchema => Self::simple(
                SavedFilterCommandErrorKind::UnsupportedSchema,
                "saved filter YAML uses an unsupported schema",
            ),
        }
    }
}

const fn command_kind_for_entry_issue(
    issue: SavedFilterEntryIssueKind,
) -> SavedFilterCommandErrorKind {
    match issue {
        SavedFilterEntryIssueKind::InvalidEntry => SavedFilterCommandErrorKind::InvalidEntry,
        SavedFilterEntryIssueKind::DuplicateId => SavedFilterCommandErrorKind::DuplicateId,
        SavedFilterEntryIssueKind::DuplicateName => SavedFilterCommandErrorKind::DuplicateName,
        SavedFilterEntryIssueKind::InvalidQuery => SavedFilterCommandErrorKind::InvalidQuery,
        SavedFilterEntryIssueKind::UnknownSort => SavedFilterCommandErrorKind::UnknownSort,
    }
}

const fn saved_filter_error_priority(kind: SavedFilterCommandErrorKind) -> u8 {
    match kind {
        SavedFilterCommandErrorKind::DuplicateId => 0,
        SavedFilterCommandErrorKind::DuplicateName => 1,
        SavedFilterCommandErrorKind::InvalidQuery => 2,
        SavedFilterCommandErrorKind::UnknownSort => 3,
        _ => 4,
    }
}

#[derive(Default)]
struct ScanCoordinator {
    active: Mutex<HashMap<Uuid, ActiveScan>>,
    authoritative_roots: Mutex<BTreeSet<Uuid>>,
}

#[derive(Debug, Clone)]
struct ActiveScan {
    root_id: Uuid,
    cancellation: ScanCancellation,
    delivery: Arc<ScanDeliveryWindow>,
}

#[derive(Debug, Default)]
struct ScanDeliveryWindow {
    in_flight: Mutex<BTreeSet<usize>>,
    acknowledged: Condvar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanDeliveryError {
    Cancelled,
    TimedOut,
    Poisoned,
    DuplicateSequence,
}

impl ScanDeliveryError {
    const fn message(self) -> &'static str {
        match self {
            Self::Cancelled => "scan delivery cancelled",
            Self::TimedOut => "scan UI acknowledgement timed out after 30 seconds",
            Self::Poisoned => "scan delivery acknowledgement lock is poisoned",
            Self::DuplicateSequence => "scan delivery received a duplicate batch sequence",
        }
    }
}

impl ScanDeliveryWindow {
    fn pending_count(&self) -> usize {
        self.in_flight.lock().map_or(0, |in_flight| in_flight.len())
    }

    fn reserve(
        &self,
        sequence: usize,
        cancellation: &ScanCancellation,
    ) -> Result<(), ScanDeliveryError> {
        let started = Instant::now();
        let mut in_flight = self
            .in_flight
            .lock()
            .map_err(|_| ScanDeliveryError::Poisoned)?;
        while in_flight.len() >= SCAN_BATCH_QUEUE_CAPACITY {
            if cancellation.is_cancelled() {
                return Err(ScanDeliveryError::Cancelled);
            }
            let remaining = Duration::from_secs(30).saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(ScanDeliveryError::TimedOut);
            }
            let (next, _) = self
                .acknowledged
                .wait_timeout(in_flight, remaining.min(Duration::from_millis(50)))
                .map_err(|_| ScanDeliveryError::Poisoned)?;
            in_flight = next;
        }
        if !in_flight.insert(sequence) {
            return Err(ScanDeliveryError::DuplicateSequence);
        }
        Ok(())
    }

    fn acknowledge(&self, sequence: usize) -> Result<bool, String> {
        let removed = self
            .in_flight
            .lock()
            .map_err(|_| ScanDeliveryError::Poisoned.message().to_owned())?
            .remove(&sequence);
        if removed {
            self.acknowledged.notify_all();
        }
        Ok(removed)
    }

    fn wait_until_empty(&self, cancellation: &ScanCancellation) -> Result<(), ScanDeliveryError> {
        let started = Instant::now();
        let mut in_flight = self
            .in_flight
            .lock()
            .map_err(|_| ScanDeliveryError::Poisoned)?;
        while !in_flight.is_empty() {
            if cancellation.is_cancelled() {
                return Err(ScanDeliveryError::Cancelled);
            }
            let remaining = Duration::from_secs(30).saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(ScanDeliveryError::TimedOut);
            }
            let (next, _) = self
                .acknowledged
                .wait_timeout(in_flight, remaining.min(Duration::from_millis(50)))
                .map_err(|_| ScanDeliveryError::Poisoned)?;
            in_flight = next;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ActiveWatch {
    root_id: Uuid,
    cancellation: ScanCancellation,
}

#[derive(Default)]
struct WatchCoordinator {
    active: Mutex<HashMap<Uuid, ActiveWatch>>,
}

#[derive(Default)]
struct ReconciliationCoordinator {
    candidates: Mutex<HashMap<Uuid, RelinkCandidate>>,
}

impl ReconciliationCoordinator {
    fn replace_report(&self, report: &ReconciliationReport) -> Result<(), String> {
        let mut candidates = self
            .candidates
            .lock()
            .map_err(|_| "reconciliation coordinator lock is poisoned".to_owned())?;
        candidates.retain(|_, candidate| candidate.root_id != report.root_id);
        candidates.extend(
            report
                .pending_moves
                .iter()
                .cloned()
                .map(|candidate| (candidate.candidate_id, candidate)),
        );
        Ok(())
    }

    fn candidate(&self, candidate_id: Uuid) -> Result<RelinkCandidate, String> {
        self.candidates
            .lock()
            .map_err(|_| "reconciliation coordinator lock is poisoned".to_owned())?
            .get(&candidate_id)
            .cloned()
            .ok_or_else(|| format!("relink candidate is missing or stale: {candidate_id}"))
    }

    fn resolve_sidecar(&self, sidecar_id: Uuid) {
        if let Ok(mut candidates) = self.candidates.lock() {
            candidates.retain(|_, candidate| candidate.sidecar_id != sidecar_id);
        }
    }
}

impl WatchCoordinator {
    fn register(
        &self,
        watch_id: Uuid,
        root_id: Uuid,
        cancellation: ScanCancellation,
    ) -> Result<(), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "watch coordinator lock is poisoned".to_owned())?;
        if active.values().any(|watch| watch.root_id == root_id) {
            return Err(format!("library root is already watched: {root_id}"));
        }
        if active.len() >= MAX_ACTIVE_WATCHES {
            return Err(format!(
                "active library watcher limit reached: {MAX_ACTIVE_WATCHES}"
            ));
        }
        active.insert(
            watch_id,
            ActiveWatch {
                root_id,
                cancellation,
            },
        );
        Ok(())
    }

    fn cancel(&self, watch_id: Uuid) -> Result<bool, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "watch coordinator lock is poisoned".to_owned())?;
        Ok(active.remove(&watch_id).is_some_and(|watch| {
            watch.cancellation.cancel();
            true
        }))
    }

    fn finish(&self, watch_id: Uuid) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&watch_id);
        }
    }

    fn active_count(&self) -> usize {
        self.active.lock().map_or(0, |active| active.len())
    }
}

impl ScanCoordinator {
    fn register(
        &self,
        scan_id: Uuid,
        root_id: Uuid,
        cancellation: ScanCancellation,
    ) -> Result<Arc<ScanDeliveryWindow>, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "scan coordinator lock is poisoned".to_owned())?;
        if active.values().any(|scan| scan.root_id == root_id) {
            return Err(format!("library root is already being scanned: {root_id}"));
        }
        if active.len() >= MAX_ACTIVE_SCANS {
            return Err(format!(
                "active library scan limit reached: {MAX_ACTIVE_SCANS}"
            ));
        }
        let mut authoritative = self
            .authoritative_roots
            .lock()
            .map_err(|_| "scan authority lock is poisoned".to_owned())?;
        let delivery = Arc::new(ScanDeliveryWindow::default());
        active.insert(
            scan_id,
            ActiveScan {
                root_id,
                cancellation,
                delivery: Arc::clone(&delivery),
            },
        );
        authoritative.remove(&root_id);
        Ok(delivery)
    }

    fn cancel(&self, scan_id: Uuid) -> Result<bool, String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "scan coordinator lock is poisoned".to_owned())?;
        Ok(active.get(&scan_id).is_some_and(|scan| {
            scan.cancellation.cancel();
            true
        }))
    }

    fn finish(&self, scan_id: Uuid) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&scan_id);
        }
    }

    fn acknowledge(&self, scan_id: Uuid, sequence: usize) -> Result<bool, String> {
        let delivery = self
            .active
            .lock()
            .map_err(|_| "scan coordinator lock is poisoned".to_owned())?
            .get(&scan_id)
            .map(|scan| Arc::clone(&scan.delivery))
            .ok_or_else(|| format!("scan is no longer active: {scan_id}"))?;
        delivery.acknowledge(sequence)
    }

    fn active_count(&self) -> usize {
        self.active.lock().map_or(0, |active| active.len())
    }

    fn pending_batch_count(&self) -> usize {
        self.active.lock().map_or(0, |active| {
            active
                .values()
                .map(|scan| scan.delivery.pending_count())
                .sum()
        })
    }

    fn mark_authoritative(&self, root_id: Uuid) -> Result<(), String> {
        self.authoritative_roots
            .lock()
            .map_err(|_| "scan authority lock is poisoned".to_owned())?
            .insert(root_id);
        Ok(())
    }

    fn pending_authoritative(&self, required: &BTreeSet<Uuid>) -> Result<usize, String> {
        let authoritative = self
            .authoritative_roots
            .lock()
            .map_err(|_| "scan authority lock is poisoned".to_owned())?;
        Ok(required.difference(&authoritative).count())
    }

    fn is_root_active(&self, root_id: Uuid) -> bool {
        self.active
            .lock()
            .is_ok_and(|active| active.values().any(|scan| scan.root_id == root_id))
    }
}

#[tauri::command]
const fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        git_commit: env!("EAGLE_GIT_COMMIT"),
        build_target: env!("EAGLE_BUILD_TARGET"),
        build_profile: env!("EAGLE_BUILD_PROFILE"),
        rustc_version: env!("EAGLE_RUSTC_VERSION"),
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_library_roots(
    state: State<'_, Mutex<LibraryRootManager>>,
) -> Result<Vec<LibraryRootStatus>, String> {
    let manager = state
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?;
    Ok(manager.roots())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_application_config(
    state: State<'_, Mutex<ApplicationConfigManager>>,
) -> Result<ApplicationConfig, String> {
    state
        .lock()
        .map_err(|_| "application configuration lock is poisoned".to_owned())
        .map(|manager| manager.config())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn update_application_config(
    input: UpdateUiPreferences,
    state: State<'_, Mutex<ApplicationConfigManager>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<ApplicationConfig, String> {
    let filter_count = input.tag_filters.len();
    let query_present = !input.query.is_empty();
    let config = state
        .lock()
        .map_err(|_| "application configuration lock is poisoned".to_owned())?
        .update_ui(input)
        .map_err(|error| error.to_string())?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "configuration",
        "ui-preferences-saved",
        [
            ("queryPresent", query_present.to_string()),
            ("tagFilterCount", filter_count.to_string()),
        ],
    );
    Ok(config)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_saved_filters(
    store: State<'_, Arc<SavedFilterStore>>,
    roots: State<'_, Mutex<LibraryRootManager>>,
) -> Result<SavedFilterCatalog, SavedFilterCommandError> {
    let statuses = roots
        .lock()
        .map_err(|_| {
            SavedFilterCommandError::simple(
                SavedFilterCommandErrorKind::Internal,
                "library root state is unavailable",
            )
        })?
        .roots();
    let (_, available) = saved_filter_root_sets(&statuses);
    store.load(&available).map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn create_saved_filter(
    expected_version: SavedFilterFileVersion,
    input: CreateSavedFilter,
    store: State<'_, Arc<SavedFilterStore>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<SavedFilterMutation, SavedFilterCommandError> {
    let mutation = store.create(&expected_version, input)?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "saved-filters",
        "filter-created",
        [("fileBytes", mutation.file_version.size.to_string())],
    );
    Ok(mutation)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn update_saved_filter(
    expected_version: SavedFilterFileVersion,
    id: Uuid,
    input: UpdateSavedFilter,
    store: State<'_, Arc<SavedFilterStore>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<SavedFilterMutation, SavedFilterCommandError> {
    let mutation = store.update(&expected_version, id, input)?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "saved-filters",
        "filter-updated",
        [("fileBytes", mutation.file_version.size.to_string())],
    );
    Ok(mutation)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn rename_saved_filter(
    expected_version: SavedFilterFileVersion,
    id: Uuid,
    name: String,
    store: State<'_, Arc<SavedFilterStore>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<SavedFilterMutation, SavedFilterCommandError> {
    let mutation = store.rename(&expected_version, id, name)?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "saved-filters",
        "filter-renamed",
        [("fileBytes", mutation.file_version.size.to_string())],
    );
    Ok(mutation)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn delete_saved_filter(
    expected_version: SavedFilterFileVersion,
    id: Uuid,
    store: State<'_, Arc<SavedFilterStore>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<SavedFilterMutation, SavedFilterCommandError> {
    let mutation = store.delete(&expected_version, id)?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "saved-filters",
        "filter-deleted",
        [("fileBytes", mutation.file_version.size.to_string())],
    );
    Ok(mutation)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn execute_saved_filter(
    id: Uuid,
    store: State<'_, Arc<SavedFilterStore>>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<SavedFilterExecution, SavedFilterCommandError> {
    let statuses = roots
        .lock()
        .map_err(|_| {
            SavedFilterCommandError::simple(
                SavedFilterCommandErrorKind::Internal,
                "library root state is unavailable",
            )
        })?
        .roots();
    let (enabled, available) = saved_filter_root_sets(&statuses);
    let saved = store.load(&available)?;
    if let Some(issue) = saved.file_issues.first() {
        return Err(SavedFilterCommandError::file_issue(issue.kind));
    }
    if saved
        .invalid_entries
        .iter()
        .any(|entry| entry.id == Some(id))
    {
        return Err(SavedFilterCommandError::simple(
            SavedFilterCommandErrorKind::InvalidEntry,
            "saved filter entry is invalid and cannot be executed",
        ));
    }
    let filter = saved
        .valid_filters
        .into_iter()
        .chain(
            saved
                .unavailable_filters
                .into_iter()
                .map(|entry| entry.filter),
        )
        .find(|filter| filter.id == id)
        .ok_or_else(|| {
            SavedFilterCommandError::simple(
                SavedFilterCommandErrorKind::NotFound,
                "saved filter is missing or stale",
            )
        })?;
    let catalog = catalog.lock().map_err(|_| {
        SavedFilterCommandError::simple(
            SavedFilterCommandErrorKind::Internal,
            "asset catalog is unavailable",
        )
    })?;
    let records = catalog.records();
    let catalog_revision = catalog.revision();
    drop(catalog);
    let execution =
        execute_saved_filter_view(&filter, &records, &enabled, &available, catalog_revision)?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "saved-filters",
        "filter-executed",
        [
            ("matchedAssets", execution.matched_assets.to_string()),
            ("missingRoots", execution.missing_root_ids.len().to_string()),
        ],
    );
    Ok(execution)
}

fn saved_filter_root_sets(statuses: &[LibraryRootStatus]) -> (BTreeSet<Uuid>, BTreeSet<Uuid>) {
    let enabled = statuses
        .iter()
        .filter(|status| status.root.enabled)
        .map(|status| status.root.id)
        .collect::<BTreeSet<_>>();
    let available = statuses
        .iter()
        .filter(|status| status.root.enabled && status.access_status == RootAccessStatus::Available)
        .map(|status| status.root.id)
        .collect();
    (enabled, available)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn runtime_recovery_status(
    state: State<'_, RuntimeState>,
    previews: State<'_, Arc<ThumbnailService>>,
) -> Result<RuntimeRecoveryStatus, ThumbnailCommandError> {
    let previews = Arc::clone(previews.inner());
    let cache_stats = tauri::async_runtime::spawn_blocking(move || previews.cache_stats())
        .await
        .map_err(|error| ThumbnailCommandError::Internal {
            message: format!("thumbnail cache status task failed: {error}"),
        })?
        .map_err(ThumbnailCommandError::from)?;
    Ok(RuntimeRecoveryStatus {
        paths: state.paths.clone(),
        cache_startup: state.cache_startup,
        cache_stats,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn runtime_resource_status(
    resources: State<'_, ResourceController>,
    scans: State<'_, Arc<ScanCoordinator>>,
    watches: State<'_, Arc<WatchCoordinator>>,
    previews: State<'_, Arc<ThumbnailService>>,
) -> Result<RuntimeResourceStatus, String> {
    Ok(RuntimeResourceStatus {
        scheduler: resources.snapshot().map_err(|error| error.to_string())?,
        active_scans: scans.active_count(),
        active_watches: watches.active_count(),
        cache: previews.cache_stats().map_err(|error| error.to_string())?,
        scan_batch_queue_capacity: SCAN_BATCH_QUEUE_CAPACITY,
        pending_scan_batches: scans.pending_batch_count(),
        max_active_scans: MAX_ACTIVE_SCANS,
        max_active_watches: MAX_ACTIVE_WATCHES,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn inspect_library_consistency(
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
    resources: State<'_, ResourceController>,
) -> Result<LibraryConsistencyReport, String> {
    let roots = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots();
    let records = catalog
        .lock()
        .map_err(|_| "asset catalog lock is poisoned".to_owned())?
        .records();
    let required_roots = roots
        .iter()
        .filter(|root| root.root.enabled && root.access_status == RootAccessStatus::Available)
        .map(|root| root.root.id)
        .collect::<BTreeSet<_>>();
    let authoritative = scans.active_count() == 0
        && scans
            .pending_authoritative(&required_roots)
            .map_err(|message| format!("cannot inspect scan authority: {message}"))?
            == 0;
    let diagnostics = Arc::clone(diagnostics.inner());
    let resources = resources.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = resources
            .acquire(WorkKind::Hash)
            .map_err(|error| error.to_string())?;
        let mut report = build_library_consistency_report(&roots, &records, authoritative);
        for root in roots
            .iter()
            .filter(|root| root.root.enabled && root.access_status == RootAccessStatus::Available)
        {
            let root_records = records
                .iter()
                .filter(|record| record.root_id == Some(root.root.id))
                .cloned()
                .collect::<Vec<_>>();
            let options = ScanOptions {
                recursive: root.root.scan.recursive,
                ignore: root.root.scan.ignore.clone(),
                ..ScanOptions::default()
            };
            match inspect_reconciliation(root.root.id, &root.root.path, &options, &root_records) {
                Ok(reconciliation) => {
                    append_reconciliation_findings(&mut report, root, &reconciliation);
                }
                Err(_) => append_reconciliation_failure(&mut report, root),
            }
        }
        record_diagnostic(
            &diagnostics,
            if report.summary.errors == 0 {
                DiagnosticLevel::Info
            } else {
                DiagnosticLevel::Warning
            },
            "support",
            "consistency-inspected",
            [
                ("assetCount", report.summary.catalog_assets.to_string()),
                ("findingCount", report.summary.findings.to_string()),
                ("errorCount", report.summary.errors.to_string()),
                ("authoritative", report.authoritative.to_string()),
            ],
        );
        Ok(report)
    })
    .await
    .map_err(|error| format!("library consistency task failed: {error}"))?
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn trace_asset_support(
    asset_id: Uuid,
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<AssetTraceReport, String> {
    let roots = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots();
    let records = catalog
        .lock()
        .map_err(|_| "asset catalog lock is poisoned".to_owned())?
        .records();
    let report = build_asset_trace(asset_id, &roots, &records);
    record_diagnostic(
        diagnostics.inner(),
        if report.match_count == 1 {
            DiagnosticLevel::Info
        } else {
            DiagnosticLevel::Warning
        },
        "support",
        "asset-traced",
        [
            ("assetId", asset_id.to_string()),
            ("matchCount", report.match_count.to_string()),
        ],
    );
    Ok(report)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn add_library_root(
    input: AddLibraryRoot,
    state: State<'_, Mutex<LibraryRootManager>>,
) -> Result<LibraryRootStatus, String> {
    state
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .add_root(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn update_library_root(
    input: UpdateLibraryRoot,
    state: State<'_, Mutex<LibraryRootManager>>,
) -> Result<LibraryRootStatus, String> {
    state
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .update_root(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn remove_library_root(
    id: Uuid,
    state: State<'_, Mutex<LibraryRootManager>>,
) -> Result<LibraryRoot, String> {
    state
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .remove_root(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_obsidian_vaults(
    state: State<'_, Mutex<VaultManager>>,
) -> Result<Vec<VaultRootStatus>, String> {
    let manager = state
        .lock()
        .map_err(|_| "Obsidian Vault manager lock is poisoned".to_owned())?;
    Ok(manager.vaults())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn add_obsidian_vault(
    input: AddVaultRoot,
    state: State<'_, Mutex<VaultManager>>,
) -> Result<VaultRootStatus, String> {
    state
        .lock()
        .map_err(|_| "Obsidian Vault manager lock is poisoned".to_owned())?
        .add_vault(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn update_obsidian_vault(
    input: UpdateVaultRoot,
    state: State<'_, Mutex<VaultManager>>,
) -> Result<VaultRootStatus, String> {
    state
        .lock()
        .map_err(|_| "Obsidian Vault manager lock is poisoned".to_owned())?
        .update_vault(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn remove_obsidian_vault(
    id: Uuid,
    state: State<'_, Mutex<VaultManager>>,
) -> Result<VaultRoot, String> {
    state
        .lock()
        .map_err(|_| "Obsidian Vault manager lock is poisoned".to_owned())?
        .remove_vault(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn resolve_obsidian_vault_references(
    input: ResolveVaultReferencesInput,
    vaults: State<'_, Mutex<VaultManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
) -> Result<ResolveVaultReferencesResult, String> {
    let mut seen = BTreeSet::new();
    let requested: Vec<_> = input
        .asset_keys
        .into_iter()
        .filter(|key| seen.insert(key.clone()))
        .collect();
    let records = {
        let catalog = catalog
            .lock()
            .map_err(|_| "asset catalog lock is poisoned".to_owned())?;
        requested
            .into_iter()
            .map(|key| {
                let path = catalog.get(&key).map(|record| record.path.clone());
                (key, path)
            })
            .collect::<Vec<_>>()
    };
    let vaults = vaults
        .lock()
        .map_err(|_| "Obsidian Vault manager lock is poisoned".to_owned())?;
    let mut resolved = Vec::new();
    let mut failures = Vec::new();
    for (asset_key, path) in records {
        let Some(path) = path else {
            failures.push(VaultReferenceFailure {
                asset_key,
                kind: VaultReferenceFailureKind::AssetNotFound,
                message: "asset is not present in the in-memory catalog".into(),
            });
            continue;
        };
        match vaults.resolve_reference(input.vault_id, &path) {
            Ok(reference) => resolved.push(ResolvedVaultReference {
                asset_key,
                reference,
            }),
            Err(error) => failures.push(VaultReferenceFailure {
                asset_key,
                kind: vault_failure_kind(&error),
                message: error.to_string(),
            }),
        }
    }
    Ok(ResolveVaultReferencesResult { resolved, failures })
}

fn vault_failure_kind(error: &VaultError) -> VaultReferenceFailureKind {
    match error {
        VaultError::NotFound(_) => VaultReferenceFailureKind::VaultNotFound,
        VaultError::Disabled(_) => VaultReferenceFailureKind::VaultDisabled,
        VaultError::InaccessibleVault { .. } => VaultReferenceFailureKind::VaultUnavailable,
        VaultError::Canonicalize { kind: "asset", .. } => {
            VaultReferenceFailureKind::AssetUnavailable
        }
        VaultError::OutsideVault { .. } => VaultReferenceFailureKind::OutsideVault,
        VaultError::UnsafeWikilink { .. } => VaultReferenceFailureKind::UnsafeWikilink,
        _ => VaultReferenceFailureKind::Internal,
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn start_library_scan(
    root_id: Uuid,
    on_event: Channel<LibraryScanEvent>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    previews: State<'_, Arc<ThumbnailService>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
    resources: State<'_, ResourceController>,
) -> Result<Uuid, String> {
    let root_statuses = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots();
    let required_roots = root_statuses
        .iter()
        .filter(|root| root.root.enabled && root.access_status == RootAccessStatus::Available)
        .map(|root| root.root.id)
        .collect::<BTreeSet<_>>();
    let root_status = root_statuses
        .into_iter()
        .find(|root| root.root.id == root_id)
        .ok_or_else(|| format!("library root was not found: {root_id}"))?;
    if !root_status.root.enabled {
        return Err(format!("library root is disabled: {root_id}"));
    }
    if root_status.access_status != RootAccessStatus::Available {
        return Err(format!(
            "library root is not available ({}): {}",
            root_status.access_status,
            root_status.root.path.display()
        ));
    }

    let scan_id = Uuid::now_v7();
    let cancellation = ScanCancellation::new();
    let delivery = scans.register(scan_id, root_id, cancellation.clone())?;
    let coordinator = Arc::clone(scans.inner());
    let catalog = Arc::clone(catalog.inner());
    let previews = Arc::clone(previews.inner());
    let diagnostics = Arc::clone(diagnostics.inner());
    let resources = resources.inner().clone();
    let root = root_status.root;
    let thread_name = format!("library-scan-{}", &scan_id.to_string()[..8]);
    let spawn_result = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_library_scan_thread(
                scan_id,
                root,
                &on_event,
                &cancellation,
                &coordinator,
                &catalog,
                &previews,
                &required_roots,
                &diagnostics,
                &resources,
                &delivery,
            );
        });
    if let Err(error) = spawn_result {
        scans.finish(scan_id);
        return Err(format!("failed to start library scan thread: {error}"));
    }
    Ok(scan_id)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn run_library_scan_thread(
    scan_id: Uuid,
    root: LibraryRoot,
    on_event: &Channel<LibraryScanEvent>,
    cancellation: &ScanCancellation,
    coordinator: &ScanCoordinator,
    catalog: &Mutex<AssetCatalog>,
    previews: &ThumbnailService,
    required_roots: &BTreeSet<Uuid>,
    diagnostics: &DiagnosticService,
    resources: &ResourceController,
    delivery: &ScanDeliveryWindow,
) {
    let previous_records = if let Ok(catalog) = catalog.lock() {
        catalog.root_records(root.id)
    } else {
        record_diagnostic(
            diagnostics,
            DiagnosticLevel::Error,
            "scanner",
            "catalog-reset-failed",
            [("scanId", scan_id.to_string())],
        );
        let _ = on_event.send(LibraryScanEvent::Failed {
            scan_id,
            message: "asset catalog lock is poisoned".into(),
            removed_keys: Vec::new(),
            restored_records: Vec::new(),
            root_access_status: unavailable_root_status(&root.path),
        });
        coordinator.finish(scan_id);
        return;
    };
    record_diagnostic(
        diagnostics,
        DiagnosticLevel::Info,
        "scanner",
        "scan-started",
        [
            ("scanId", scan_id.to_string()),
            ("rootId", root.id.to_string()),
            ("previousRecordCount", previous_records.len().to_string()),
        ],
    );
    if on_event
        .send(LibraryScanEvent::Started {
            scan_id,
            root_id: root.id,
            root: root.path.clone(),
        })
        .is_err()
    {
        cancellation.cancel();
    }
    let options = ScanOptions {
        recursive: root.scan.recursive,
        ignore_hidden: true,
        ignore: root.scan.ignore,
        ..ScanOptions::default()
    };
    let mut scanned_records = Vec::new();
    let result = std::thread::scope(|scope| {
        let (sender, receiver) = scan_pipeline_channel();
        let scan_root_id = root.id;
        let scan_root_path = &root.path;
        let worker = scope.spawn(move || {
            let result = scan_root_incremental_controlled(
                Some(scan_root_id),
                scan_root_path,
                &options,
                cancellation,
                resources,
                |batch| {
                    if sender.send(ScanPipelineMessage::Batch(batch)).is_err() {
                        cancellation.cancel();
                    }
                },
            )
            .map_err(|error| error.to_string());
            let _ = sender.send(ScanPipelineMessage::Finished(result));
        });
        let mut result = Err("scan batch producer disconnected before completion".to_owned());
        let mut delivery_error = None;
        while let Ok(message) = receiver.recv() {
            match message {
                ScanPipelineMessage::Batch(mut batch) => {
                    for record in &mut batch.assets {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        if previews
                            .enrich_media_properties(record, &root.path)
                            .is_err()
                        {
                            record.issues.push(asset_core::AssetIssue::ResourceLimited(
                                "optional image metadata could not enter the shared resource scheduler"
                                    .into(),
                            ));
                        }
                    }
                    scanned_records.extend(batch.assets.iter().cloned());
                    if let Ok(mut catalog) = catalog.lock() {
                        catalog.ingest(batch.assets.iter().cloned());
                    } else {
                        cancellation.cancel();
                    }
                    if delivery_error.is_none() && !cancellation.is_cancelled() {
                        match delivery.reserve(batch.sequence, cancellation) {
                            Ok(()) => {
                                let sequence = batch.sequence;
                                if on_event
                                    .send(LibraryScanEvent::Batch { scan_id, batch })
                                    .is_err()
                                {
                                    let _ = delivery.acknowledge(sequence);
                                    cancellation.cancel();
                                }
                            }
                            Err(ScanDeliveryError::Cancelled) => {}
                            Err(error) => {
                                cancellation.cancel();
                                delivery_error = Some(error.message().to_owned());
                            }
                        }
                    }
                }
                ScanPipelineMessage::Finished(scan_result) => {
                    result = scan_result;
                    break;
                }
            }
        }
        if worker.join().is_err() {
            Err("scan batch producer panicked".to_owned())
        } else if let Some(error) = delivery_error {
            Err(error)
        } else {
            match delivery.wait_until_empty(cancellation) {
                Ok(()) | Err(ScanDeliveryError::Cancelled) => result,
                Err(error) => Err(error.message().to_owned()),
            }
        }
    });
    let result = result.map(|mut summary| {
        if cancellation.is_cancelled() {
            summary.completion = ScanCompletion::Cancelled;
        }
        summary
    });
    let authoritative = publish_scan_result(
        scan_id,
        root.id,
        result,
        scanned_records,
        &previous_records,
        on_event,
        catalog,
        diagnostics,
        &root.path,
    );
    coordinator.finish(scan_id);
    if authoritative && coordinator.mark_authoritative(root.id).is_err() {
        record_diagnostic(
            diagnostics,
            DiagnosticLevel::Error,
            "cache",
            "maintenance-authority-failed",
            [("reason", "scan-authority-lock-poisoned".into())],
        );
        return;
    }
    if coordinator.active_count() == 0 {
        match coordinator.pending_authoritative(required_roots) {
            Ok(0) => maintain_cache_after_scans(catalog, previews, diagnostics),
            Ok(_) => {}
            Err(_) => record_diagnostic(
                diagnostics,
                DiagnosticLevel::Error,
                "cache",
                "maintenance-authority-failed",
                [("reason", "scan-authority-lock-poisoned".into())],
            ),
        }
    }
}

fn maintain_cache_after_scans(
    catalog: &Mutex<AssetCatalog>,
    previews: &ThumbnailService,
    diagnostics: &DiagnosticService,
) {
    let records = if let Ok(catalog) = catalog.lock() {
        catalog.records()
    } else {
        record_diagnostic(
            diagnostics,
            DiagnosticLevel::Error,
            "cache",
            "maintenance-catalog-failed",
            [("reason", "catalog-lock-poisoned".into())],
        );
        return;
    };
    match previews.maintain(&records) {
        Ok(report) => record_cache_maintenance(diagnostics, &report, "scan-completed"),
        Err(error) => record_diagnostic(
            diagnostics,
            DiagnosticLevel::Error,
            "cache",
            "maintenance-failed",
            [("reason", cache_error_category(&error).into())],
        ),
    }
}

const fn cache_error_category(error: &PreviewError) -> &'static str {
    match error {
        PreviewError::InvalidMaxEdge(_) => "invalid-max-edge",
        PreviewError::InvalidConcurrency(_) => "invalid-concurrency",
        PreviewError::InvalidCachePolicy(_) => "invalid-policy",
        PreviewError::UnsafeCacheRoot(_) => "unsafe-root",
        PreviewError::CacheIo { .. } => "io",
        PreviewError::CacheMetadata { .. } => "metadata",
        PreviewError::InvalidCacheKey(_) => "invalid-key",
        PreviewError::MissingCacheEntry(_) => "missing-entry",
        PreviewError::InvalidWorkerIdentity => "invalid-worker-identity",
        PreviewError::PoisonedLock(_) => "poisoned-lock",
        PreviewError::Resource(_) => "resource-control",
    }
}

#[allow(clippy::too_many_arguments)]
fn publish_scan_result(
    scan_id: Uuid,
    root_id: Uuid,
    result: Result<ScanSummary, String>,
    scanned_records: Vec<asset_core::AssetRecord>,
    previous_records: &[asset_core::AssetRecord],
    on_event: &Channel<LibraryScanEvent>,
    catalog: &Mutex<AssetCatalog>,
    diagnostics: &DiagnosticService,
    root_path: &std::path::Path,
) -> bool {
    match result {
        Ok(summary) => {
            let authoritative = summary.completion == ScanCompletion::Completed;
            let desired_records = if authoritative {
                scanned_records
            } else {
                previous_records.to_vec()
            };
            let reconciliation = if let Ok(mut catalog) = catalog.lock() {
                catalog.reconcile_root(root_id, previous_records, desired_records)
            } else {
                let _ = on_event.send(LibraryScanEvent::Failed {
                    scan_id,
                    message: "asset catalog lock is poisoned".into(),
                    removed_keys: Vec::new(),
                    restored_records: Vec::new(),
                    root_access_status: unavailable_root_status(root_path),
                });
                return false;
            };
            record_diagnostic(
                diagnostics,
                DiagnosticLevel::Info,
                "scanner",
                "scan-finished",
                [
                    ("scanId", scan_id.to_string()),
                    ("assetCount", summary.asset_count.to_string()),
                    ("problemCount", summary.problem_count.to_string()),
                ],
            );
            let _ = on_event.send(LibraryScanEvent::Finished {
                scan_id,
                summary,
                reconciliation,
            });
            authoritative
        }
        Err(error) => {
            let reconciliation = catalog.lock().ok().map(|mut catalog| {
                catalog.reconcile_root(root_id, previous_records, previous_records.to_vec())
            });
            let (removed_keys, restored_records) = reconciliation.map_or_else(
                || (Vec::new(), Vec::new()),
                |reconciliation| (reconciliation.removed_keys, reconciliation.restored_records),
            );
            record_diagnostic(
                diagnostics,
                DiagnosticLevel::Error,
                "scanner",
                "scan-failed",
                [("scanId", scan_id.to_string())],
            );
            let _ = on_event.send(LibraryScanEvent::Failed {
                scan_id,
                message: error.clone(),
                removed_keys,
                restored_records,
                root_access_status: unavailable_root_status(root_path),
            });
            false
        }
    }
}

fn unavailable_root_status(root: &std::path::Path) -> Option<RootAccessStatus> {
    let (status, _) = inspect_root_access(root);
    (status != RootAccessStatus::Available).then_some(status)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn cancel_library_scan(
    scan_id: Uuid,
    scans: State<'_, Arc<ScanCoordinator>>,
) -> Result<bool, String> {
    scans.cancel(scan_id)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn acknowledge_library_scan_batch(
    scan_id: Uuid,
    sequence: usize,
    scans: State<'_, Arc<ScanCoordinator>>,
) -> Result<bool, String> {
    scans.acknowledge(scan_id, sequence)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn inspect_library_reconciliation(
    root_id: Uuid,
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    reconciliation: State<'_, Arc<ReconciliationCoordinator>>,
    resources: State<'_, ResourceController>,
) -> Result<ReconciliationReport, String> {
    let _permit = resources
        .acquire(WorkKind::Hash)
        .map_err(|error| error.to_string())?;
    let root_status = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots()
        .into_iter()
        .find(|root| root.root.id == root_id)
        .ok_or_else(|| format!("library root was not found: {root_id}"))?;
    if !root_status.root.enabled || root_status.access_status != RootAccessStatus::Available {
        return Err(format!("library root is not available: {root_id}"));
    }
    let assets = catalog
        .lock()
        .map_err(|_| "asset catalog lock is poisoned".to_owned())?
        .root_records(root_id);
    let options = ScanOptions {
        recursive: root_status.root.scan.recursive,
        ignore_hidden: true,
        ignore: root_status.root.scan.ignore.clone(),
        ..ScanOptions::default()
    };
    let report = inspect_reconciliation(root_id, &root_status.root.path, &options, &assets)
        .map_err(|error| error.to_string())?;
    reconciliation.replace_report(&report)?;
    Ok(report)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn confirm_library_relink(
    candidate_id: Uuid,
    roots: State<'_, Mutex<LibraryRootManager>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    reconciliation: State<'_, Arc<ReconciliationCoordinator>>,
    resources: State<'_, ResourceController>,
) -> Result<RelinkReceipt, String> {
    let _permit = resources
        .acquire(WorkKind::Hash)
        .map_err(|error| error.to_string())?;
    let candidate = reconciliation.candidate(candidate_id)?;
    if scans.is_root_active(candidate.root_id) {
        return Err(format!(
            "library root is currently being scanned: {}",
            candidate.root_id
        ));
    }
    let root_status = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots()
        .into_iter()
        .find(|root| root.root.id == candidate.root_id)
        .ok_or_else(|| format!("library root was not found: {}", candidate.root_id))?;
    if !root_status.root.enabled || root_status.access_status != RootAccessStatus::Available {
        return Err(format!(
            "library root is not available: {}",
            candidate.root_id
        ));
    }
    let receipt =
        apply_relink(&root_status.root.path, &candidate).map_err(|error| error.to_string())?;
    reconciliation.resolve_sidecar(candidate.sidecar_id);
    Ok(receipt)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn start_library_watch(
    root_id: Uuid,
    on_event: Channel<LibraryWatchEvent>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    watches: State<'_, Arc<WatchCoordinator>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<Uuid, String> {
    let root_status = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots()
        .into_iter()
        .find(|root| root.root.id == root_id)
        .ok_or_else(|| format!("library root was not found: {root_id}"))?;
    if !root_status.root.enabled {
        return Err(format!("library root is disabled: {root_id}"));
    }
    if root_status.access_status != RootAccessStatus::Available {
        return Err(format!(
            "library root is not available ({}): {}",
            root_status.access_status,
            root_status.root.path.display()
        ));
    }

    let watch_id = Uuid::now_v7();
    let cancellation = ScanCancellation::new();
    watches.register(watch_id, root_id, cancellation.clone())?;
    let coordinator = Arc::clone(watches.inner());
    let diagnostics = Arc::clone(diagnostics.inner());
    let root = root_status.root;
    let thread_name = format!("library-watch-{}", &watch_id.to_string()[..8]);
    let spawn_result = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            run_library_watch_thread(
                watch_id,
                &root,
                &on_event,
                &cancellation,
                &coordinator,
                &diagnostics,
            );
        });
    if let Err(error) = spawn_result {
        watches.finish(watch_id);
        return Err(format!("failed to start library watch thread: {error}"));
    }
    Ok(watch_id)
}

fn run_library_watch_thread(
    watch_id: Uuid,
    root: &LibraryRoot,
    on_event: &Channel<LibraryWatchEvent>,
    cancellation: &ScanCancellation,
    coordinator: &WatchCoordinator,
    diagnostics: &DiagnosticService,
) {
    let session = match WatchSession::start(&root.path) {
        Ok(session) => session,
        Err(error) => {
            record_diagnostic(
                diagnostics,
                DiagnosticLevel::Error,
                "watcher",
                "watch-start-failed",
                [("rootId", root.id.to_string())],
            );
            let _ = on_event.send(LibraryWatchEvent::Failed {
                watch_id,
                root_id: root.id,
                message: error.to_string(),
                root_access_status: unavailable_root_status(&root.path),
            });
            coordinator.finish(watch_id);
            return;
        }
    };
    record_diagnostic(
        diagnostics,
        DiagnosticLevel::Info,
        "watcher",
        "watch-started",
        [
            ("watchId", watch_id.to_string()),
            ("rootId", root.id.to_string()),
        ],
    );
    if on_event
        .send(LibraryWatchEvent::Started {
            watch_id,
            root_id: root.id,
        })
        .is_err()
    {
        cancellation.cancel();
    }

    while !cancellation.is_cancelled() {
        match session.next_batch_timeout(Duration::from_millis(250)) {
            Ok(Some(batch)) if !batch.changes.is_empty() => {
                let terminal_loss = batch
                    .changes
                    .iter()
                    .any(|change| change.reason == Some(FsRescanReason::ChannelDisconnected));
                if !publish_watch_batch(watch_id, root.id, batch, on_event, diagnostics) {
                    cancellation.cancel();
                }
                if terminal_loss {
                    let _ = on_event.send(LibraryWatchEvent::Failed {
                        watch_id,
                        root_id: root.id,
                        message: "file watcher channel disconnected; consistency scan required"
                            .into(),
                        root_access_status: unavailable_root_status(&root.path),
                    });
                    break;
                }
            }
            Ok(_) => {}
            Err(error) => {
                record_diagnostic(
                    diagnostics,
                    DiagnosticLevel::Error,
                    "watcher",
                    "watch-failed",
                    [("rootId", root.id.to_string())],
                );
                let _ = on_event.send(LibraryWatchEvent::Failed {
                    watch_id,
                    root_id: root.id,
                    message: error.to_string(),
                    root_access_status: unavailable_root_status(&root.path),
                });
                break;
            }
        }
    }
    record_diagnostic(
        diagnostics,
        DiagnosticLevel::Info,
        "watcher",
        "watch-stopped",
        [("rootId", root.id.to_string())],
    );
    let _ = on_event.send(LibraryWatchEvent::Stopped {
        watch_id,
        root_id: root.id,
    });
    coordinator.finish(watch_id);
}

fn publish_watch_batch(
    watch_id: Uuid,
    root_id: Uuid,
    batch: FsChangeBatch,
    on_event: &Channel<LibraryWatchEvent>,
    diagnostics: &DiagnosticService,
) -> bool {
    let requires_rescan = batch.requires_rescan();
    record_diagnostic(
        diagnostics,
        if requires_rescan {
            DiagnosticLevel::Warning
        } else {
            DiagnosticLevel::Info
        },
        "watcher",
        if requires_rescan {
            "bounded-rescan-requested"
        } else {
            "change-batch"
        },
        [
            ("rootId", root_id.to_string()),
            ("rawEventCount", batch.raw_event_count.to_string()),
            ("changeCount", batch.changes.len().to_string()),
        ],
    );
    on_event
        .send(LibraryWatchEvent::Changes {
            watch_id,
            root_id,
            batch,
        })
        .is_ok()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn stop_library_watch(
    watch_id: Uuid,
    watches: State<'_, Arc<WatchCoordinator>>,
) -> Result<bool, String> {
    watches.cancel(watch_id)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn edit_asset_metadata(
    input: BatchMetadataEdit,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    transactions: State<'_, Arc<MetadataTransactionStore>>,
    conflicts: State<'_, Arc<MetadataConflictStore>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
    resources: State<'_, ResourceController>,
) -> Result<BatchMetadataEditCommandResult, String> {
    let _permit = resources
        .acquire(WorkKind::Hash)
        .map_err(|error| error.to_string())?;
    let targets = {
        let catalog = catalog
            .lock()
            .map_err(|_| "asset catalog lock is poisoned".to_owned())?;
        transaction_targets(&catalog, &input)
    };
    let mut result = if let Some(targets) = targets {
        let execution = transactions
            .execute(&targets, &input.patch)
            .map_err(|error| error.to_string())?;
        let mut catalog = catalog
            .lock()
            .map_err(|_| "asset catalog lock is poisoned".to_owned())?;
        let updated = execution
            .committed
            .into_iter()
            .filter_map(|committed| {
                catalog.apply_committed_sidecar(
                    &committed.key,
                    committed.sidecar_path,
                    committed.sidecar,
                    committed.version,
                )
            })
            .collect();
        BatchMetadataEditCommandResult {
            updated,
            failures: execution
                .failures
                .into_iter()
                .map(|failure| MetadataEditFailure {
                    key: failure.key,
                    kind: match failure.kind {
                        TransactionFailureKind::Conflict => EditFailureKind::Conflict,
                        TransactionFailureKind::InvalidInput => EditFailureKind::InvalidInput,
                        TransactionFailureKind::WriteFailed => EditFailureKind::WriteFailed,
                    },
                    message: failure.message,
                })
                .collect(),
            transaction: Some(execution.summary),
            conflicts: Vec::new(),
        }
    } else if input.targets.len() >= 2 {
        let catalog = catalog
            .lock()
            .map_err(|_| "asset catalog lock is poisoned".to_owned())?;
        BatchMetadataEditCommandResult {
            updated: Vec::new(),
            failures: failed_batch_transaction_preflight(&catalog, &input),
            transaction: None,
            conflicts: Vec::new(),
        }
    } else {
        let BatchMetadataEditResult { updated, failures } = catalog
            .lock()
            .map_err(|_| "asset catalog lock is poisoned".to_owned())?
            .edit_metadata(&input)
            .map_err(|error| error.to_string())?;
        BatchMetadataEditCommandResult {
            updated,
            failures,
            transaction: None,
            conflicts: Vec::new(),
        }
    };
    conflicts.invalidate_keys(result.updated.iter().map(|record| record.key.as_str()))?;
    result.conflicts =
        capture_metadata_conflicts(&result.failures, &catalog, conflicts.inner(), &input.patch)?;
    record_diagnostic(
        diagnostics.inner(),
        if result.failures.is_empty() {
            DiagnosticLevel::Info
        } else {
            DiagnosticLevel::Warning
        },
        "metadata",
        "batch-edit-finished",
        [
            ("updatedCount", result.updated.len().to_string()),
            ("failureCount", result.failures.len().to_string()),
            ("conflictCount", result.conflicts.len().to_string()),
        ],
    );
    Ok(result)
}

fn capture_metadata_conflicts(
    failures: &[MetadataEditFailure],
    catalog: &Mutex<AssetCatalog>,
    conflicts: &MetadataConflictStore,
    patch: &metadata::MetadataPatch,
) -> Result<Vec<MetadataConflictView>, String> {
    let catalog = catalog
        .lock()
        .map_err(|_| "asset catalog lock is poisoned".to_owned())?;
    Ok(failures
        .iter()
        .filter(|failure| failure.kind == EditFailureKind::Conflict)
        .filter_map(|failure| catalog.get(&failure.key))
        .filter_map(|record| conflicts.capture(record, patch).ok())
        .collect())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn resolve_metadata_conflict(
    input: ResolveMetadataConflictInput,
    conflicts: State<'_, Arc<MetadataConflictStore>>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
    resources: State<'_, ResourceController>,
) -> Result<asset_core::AssetRecord, String> {
    let _permit = resources
        .acquire(WorkKind::Hash)
        .map_err(|error| error.to_string())?;
    let pending = conflicts.get(input.conflict_id)?;
    let root = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots()
        .into_iter()
        .find(|root| root.root.id == pending.root_id)
        .ok_or_else(|| format!("conflict library root was not found: {}", pending.root_id))?;
    if !root.root.enabled || root.access_status != RootAccessStatus::Available {
        return Err(format!(
            "conflict library root is not available: {}",
            pending.root_id
        ));
    }
    if scans.is_root_active(pending.root_id) {
        return Err(format!(
            "library root is currently being scanned: {}",
            pending.root_id
        ));
    }
    ensure_transaction_item_inside_root(
        &TransactionScopeItem {
            root_id: pending.root_id,
            asset_path: pending.asset_path.clone(),
            sidecar_path: pending.sidecar_path.clone(),
        },
        &root.root,
    )?;
    let actual_version = metadata::inspect_sidecar_version(&pending.sidecar_path)
        .map_err(|error| error.to_string())?;
    if actual_version != pending.current_version {
        return Err("Sidecar 在冲突界面打开后再次发生变化；请重新扫描后重试".into());
    }
    let mut resolved = metadata::resolve_metadata_conflict(
        &pending.current_sidecar,
        &pending.patch,
        &pending.analysis,
        &input.resolution,
    )
    .map_err(|error| error.to_string())?;
    resolved.fingerprint =
        Some(metadata::fingerprint_asset(&pending.asset_path).map_err(|error| error.to_string())?);
    let version = if resolved == pending.current_sidecar {
        pending.current_version
    } else {
        resolved.touch();
        let receipt = metadata::write_sidecar_atomic(
            &pending.sidecar_path,
            &resolved,
            &metadata::ExpectedVersion::Snapshot(pending.current_version),
        )
        .map_err(|error| error.to_string())?;
        metadata::SidecarFileVersion {
            digest: receipt.digest,
            size: receipt.size,
            modified_unix_ms: receipt.modified_unix_ms,
        }
    };
    let updated = catalog
        .lock()
        .map_err(|_| "asset catalog lock is poisoned".to_owned())?
        .apply_committed_sidecar(&pending.key, pending.sidecar_path, resolved, version)
        .ok_or_else(|| format!("asset was not found: {}", pending.key))?;
    conflicts.remove(input.conflict_id)?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "metadata",
        "conflict-resolved",
        [("conflictId", input.conflict_id.to_string())],
    );
    Ok(updated)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn dismiss_metadata_conflict(
    conflict_id: Uuid,
    conflicts: State<'_, Arc<MetadataConflictStore>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<(), String> {
    conflicts.remove(conflict_id)?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "metadata",
        "conflict-dismissed",
        [("conflictId", conflict_id.to_string())],
    );
    Ok(())
}

fn failed_batch_transaction_preflight(
    catalog: &AssetCatalog,
    input: &BatchMetadataEdit,
) -> Vec<MetadataEditFailure> {
    input
        .targets
        .iter()
        .map(|target| match catalog.get(&target.key) {
            None => MetadataEditFailure {
                key: target.key.clone(),
                kind: EditFailureKind::NotFound,
                message: format!("asset was not found: {}", target.key),
            },
            Some(record) if record.root_id.is_none() => MetadataEditFailure {
                key: target.key.clone(),
                kind: EditFailureKind::WriteFailed,
                message: "批量事务未启动：素材缺少授权根信息".into(),
            },
            Some(_) => MetadataEditFailure {
                key: target.key.clone(),
                kind: EditFailureKind::WriteFailed,
                message: "批量事务未启动：至少一个目标已经不可用".into(),
            },
        })
        .collect()
}

fn transaction_targets(
    catalog: &AssetCatalog,
    input: &BatchMetadataEdit,
) -> Option<Vec<TransactionTarget>> {
    if input.targets.len() < 2 {
        return None;
    }
    input
        .targets
        .iter()
        .map(|target| {
            let record = catalog.get(&target.key)?;
            TransactionTarget::from_record(
                record,
                target.expected_sidecar_digest.clone(),
                target.expected_sidecar_size,
                target.expected_sidecar_modified_unix_ms,
            )
        })
        .collect()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn list_metadata_transactions(
    transactions: State<'_, Arc<MetadataTransactionStore>>,
) -> Result<Vec<TransactionSummary>, String> {
    transactions.list().map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn continue_metadata_transaction(
    id: Uuid,
    transactions: State<'_, Arc<MetadataTransactionStore>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    resources: State<'_, ResourceController>,
) -> Result<TransactionRecoveryResult, String> {
    let _permit = resources
        .acquire(WorkKind::Hash)
        .map_err(|error| error.to_string())?;
    ensure_transaction_scope_available(id, transactions.inner(), scans.inner(), &roots)?;
    transactions
        .continue_transaction(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn restore_metadata_transaction(
    id: Uuid,
    transactions: State<'_, Arc<MetadataTransactionStore>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    resources: State<'_, ResourceController>,
) -> Result<TransactionRecoveryResult, String> {
    let _permit = resources
        .acquire(WorkKind::Hash)
        .map_err(|error| error.to_string())?;
    ensure_transaction_scope_available(id, transactions.inner(), scans.inner(), &roots)?;
    transactions
        .restore_transaction(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn dismiss_metadata_transaction(
    id: Uuid,
    transactions: State<'_, Arc<MetadataTransactionStore>>,
) -> Result<(), String> {
    transactions.dismiss(id).map_err(|error| error.to_string())
}

fn ensure_transaction_scope_available(
    id: Uuid,
    transactions: &MetadataTransactionStore,
    scans: &ScanCoordinator,
    roots: &Mutex<LibraryRootManager>,
) -> Result<(), String> {
    let scope = transactions.scope(id).map_err(|error| error.to_string())?;
    let configured_roots = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots();
    for item in scope {
        let root = configured_roots
            .iter()
            .find(|root| root.root.id == item.root_id)
            .ok_or_else(|| format!("transaction library root was not found: {}", item.root_id))?;
        if !root.root.enabled {
            return Err(format!(
                "transaction library root is disabled: {}",
                item.root_id
            ));
        }
        if root.access_status != RootAccessStatus::Available {
            return Err(format!(
                "transaction library root is not available ({}): {}",
                root.access_status,
                root.root.path.display()
            ));
        }
        if scans.is_root_active(item.root_id) {
            return Err(format!(
                "library root is currently being scanned: {}",
                item.root_id
            ));
        }
        ensure_transaction_item_inside_root(&item, &root.root)?;
    }
    Ok(())
}

fn ensure_transaction_item_inside_root(
    item: &TransactionScopeItem,
    root: &LibraryRoot,
) -> Result<(), String> {
    if item.sidecar_path != metadata::sidecar_path_for(&item.asset_path) {
        return Err(format!(
            "transaction Sidecar path does not match asset path: {}",
            item.asset_path.display()
        ));
    }
    let parent = item.asset_path.parent().ok_or_else(|| {
        format!(
            "transaction asset has no parent: {}",
            item.asset_path.display()
        )
    })?;
    let canonical_parent = parent.canonicalize().map_err(|error| {
        format!(
            "transaction asset parent is unavailable ({}): {error}",
            parent.display()
        )
    })?;
    if !canonical_parent.starts_with(&root.path) {
        return Err(format!(
            "transaction asset is outside its library root: {}",
            item.asset_path.display()
        ));
    }
    if item.asset_path.is_file() {
        let canonical_asset = item.asset_path.canonicalize().map_err(|error| {
            format!(
                "transaction asset is unavailable ({}): {error}",
                item.asset_path.display()
            )
        })?;
        if !canonical_asset.starts_with(&root.path) {
            return Err(format!(
                "transaction asset resolves outside its library root: {}",
                item.asset_path.display()
            ));
        }
    }
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn query_assets(
    input: QueryAssetsInput,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
) -> Result<QueryAssetsResult, QueryAssetsError> {
    catalog
        .lock()
        .map_err(|_| QueryAssetsError::Internal {
            message: "asset catalog lock is poisoned".into(),
        })?
        .query_assets(&input)
        .map_err(|error| QueryAssetsError::Parse { error })
}

fn validate_selection_roots(
    statuses: &[LibraryRootStatus],
    requested: &BTreeSet<Uuid>,
) -> Result<(), SelectionCommandError> {
    for root_id in requested {
        let Some(status) = statuses.iter().find(|status| status.root.id == *root_id) else {
            return Err(SelectionCommandError::root(
                SelectionCommandErrorKind::AuthorizationLost,
                *root_id,
            ));
        };
        if !status.root.enabled {
            return Err(SelectionCommandError::root(
                SelectionCommandErrorKind::RootDisabled,
                *root_id,
            ));
        }
        if status.access_status != RootAccessStatus::Available {
            return Err(SelectionCommandError::root(
                SelectionCommandErrorKind::RootOffline,
                *root_id,
            ));
        }
    }
    Ok(())
}

fn selection_root_statuses(
    roots: &Mutex<LibraryRootManager>,
) -> Result<Vec<LibraryRootStatus>, SelectionCommandError> {
    roots
        .lock()
        .map_err(|_| {
            SelectionCommandError::simple(
                SelectionCommandErrorKind::Internal,
                "library root state is unavailable",
            )
        })
        .map(|manager| manager.roots())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn create_query_selection_snapshot(
    input: QuerySelectionInput,
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    selections: State<'_, Arc<SelectionSessionStore>>,
) -> Result<SelectionSnapshotSummary, SelectionCommandError> {
    let statuses = selection_root_statuses(roots.inner())?;
    validate_selection_roots(&statuses, &input.scope_root_ids)?;
    let catalog = catalog.lock().map_err(|_| {
        SelectionCommandError::simple(
            SelectionCommandErrorKind::Internal,
            "asset catalog state is unavailable",
        )
    })?;
    selections
        .create_query_snapshot(&catalog, &input)
        .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn create_range_selection_snapshot(
    input: RangeSelectionInput,
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    selections: State<'_, Arc<SelectionSessionStore>>,
) -> Result<SelectionSnapshotSummary, SelectionCommandError> {
    let statuses = selection_root_statuses(roots.inner())?;
    validate_selection_roots(&statuses, &input.query.scope_root_ids)?;
    let catalog = catalog.lock().map_err(|_| {
        SelectionCommandError::simple(
            SelectionCommandErrorKind::Internal,
            "asset catalog state is unavailable",
        )
    })?;
    selections
        .create_range_snapshot(&catalog, &input)
        .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn create_explicit_selection_snapshot(
    input: ExplicitSelectionInput,
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    selections: State<'_, Arc<SelectionSessionStore>>,
) -> Result<SelectionSnapshotSummary, SelectionCommandError> {
    let statuses = selection_root_statuses(roots.inner())?;
    let catalog = catalog.lock().map_err(|_| {
        SelectionCommandError::simple(
            SelectionCommandErrorKind::Internal,
            "asset catalog state is unavailable",
        )
    })?;
    let mut requested = BTreeSet::new();
    for key in &input.keys {
        let record = catalog.get(key).ok_or_else(|| {
            SelectionCommandError::simple(
                SelectionCommandErrorKind::AssetMissing,
                "selection contains an asset that is no longer in the catalog",
            )
        })?;
        let root_id = record.root_id.ok_or_else(|| {
            SelectionCommandError::simple(
                SelectionCommandErrorKind::AuthorizationLost,
                "selection asset is not associated with an authorized library root",
            )
        })?;
        requested.insert(root_id);
    }
    validate_selection_roots(&statuses, &requested)?;
    selections
        .create_explicit_snapshot(&catalog, &input)
        .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn release_selection_snapshot(
    snapshot_id: Uuid,
    selections: State<'_, Arc<SelectionSessionStore>>,
) -> Result<bool, SelectionCommandError> {
    selections.release(snapshot_id).map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn selection_session_stats(
    selections: State<'_, Arc<SelectionSessionStore>>,
) -> Result<SelectionSessionStats, SelectionCommandError> {
    selections.stats().map_err(Into::into)
}

fn batch_root_authorizations(statuses: &[LibraryRootStatus]) -> Vec<BatchRootAuthorization> {
    statuses
        .iter()
        .map(|status| BatchRootAuthorization {
            id: status.root.id,
            path: status.root.path.clone(),
            state: if !status.root.enabled {
                RootRuntimeState::Disabled
            } else if status.access_status == RootAccessStatus::Available {
                RootRuntimeState::Available
            } else {
                RootRuntimeState::Offline
            },
        })
        .collect()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn prepare_metadata_batch(
    input: MetadataPreflightInput,
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    selections: State<'_, Arc<SelectionSessionStore>>,
    preflights: State<'_, Arc<BatchPreflightStore>>,
) -> Result<BatchPreflightSummary, BatchCommandError> {
    let statuses = roots
        .lock()
        .map_err(|_| {
            BatchCommandError::new(
                BatchCommandErrorKind::Internal,
                "library root state is unavailable",
            )
        })?
        .roots();
    let authorizations = batch_root_authorizations(&statuses);
    let catalog = Arc::clone(catalog.inner());
    let selections = Arc::clone(selections.inner());
    let preflights = Arc::clone(preflights.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let catalog = catalog.lock().map_err(|_| {
            BatchCommandError::new(
                BatchCommandErrorKind::Internal,
                "asset catalog state is unavailable",
            )
        })?;
        preflights
            .prepare_metadata(&selections, &catalog, &authorizations, &input)
            .map_err(Into::into)
    })
    .await
    .map_err(|_| {
        BatchCommandError::new(
            BatchCommandErrorKind::Internal,
            "batch preflight task did not complete",
        )
    })?
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn release_batch_preflight(
    operation_id: Uuid,
    preflights: State<'_, Arc<BatchPreflightStore>>,
) -> Result<bool, BatchCommandError> {
    preflights.release(operation_id).map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn request_thumbnail(
    input: ThumbnailRequest,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    previews: State<'_, Arc<ThumbnailService>>,
) -> Result<ThumbnailOutcome, ThumbnailCommandError> {
    let record = catalog
        .lock()
        .map_err(|_| ThumbnailCommandError::Internal {
            message: "asset catalog lock is poisoned".into(),
        })?
        .get(&input.asset_key)
        .cloned()
        .ok_or_else(|| ThumbnailCommandError::AssetNotFound {
            asset_key: input.asset_key.clone(),
        })?;
    let authorized_root = record.root_id.and_then(|root_id| {
        roots.lock().ok()?.roots().into_iter().find_map(|status| {
            (status.root.id == root_id
                && status.root.enabled
                && status.access_status == RootAccessStatus::Available)
                .then_some(status.root.path)
        })
    });
    let previews = Arc::clone(previews.inner());
    tauri::async_runtime::spawn_blocking(move || {
        authorized_root.as_ref().map_or_else(
            || previews.request(&record, input.max_edge),
            |root| previews.request_with_authorized_root(&record, input.max_edge, root),
        )
    })
    .await
    .map_err(|error| ThumbnailCommandError::Internal {
        message: format!("thumbnail task failed: {error}"),
    })?
    .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn read_thumbnail(
    cache_key: String,
    previews: State<'_, Arc<ThumbnailService>>,
) -> Result<Response, ThumbnailCommandError> {
    let previews = Arc::clone(previews.inner());
    let bytes = tauri::async_runtime::spawn_blocking(move || previews.read(&cache_key))
        .await
        .map_err(|error| ThumbnailCommandError::Internal {
            message: format!("thumbnail read task failed: {error}"),
        })?
        .map_err(ThumbnailCommandError::from)?;
    Ok(Response::new(bytes))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn clear_thumbnail_cache(
    previews: State<'_, Arc<ThumbnailService>>,
) -> Result<CacheClearReport, ThumbnailCommandError> {
    let previews = Arc::clone(previews.inner());
    tauri::async_runtime::spawn_blocking(move || previews.clear())
        .await
        .map_err(|error| ThumbnailCommandError::Internal {
            message: format!("thumbnail cache clear task failed: {error}"),
        })?
        .map_err(Into::into)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn maintain_thumbnail_cache(
    previews: State<'_, Arc<ThumbnailService>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<CacheMaintenanceReport, ThumbnailCommandError> {
    let active_scans = scans.active_count();
    if active_scans > 0 {
        return Err(ThumbnailCommandError::RecoveryBusy {
            active_scans,
            message: format!(
                "cannot reconcile thumbnail cache while {active_scans} library scans are active"
            ),
        });
    }
    let required_roots = roots
        .lock()
        .map_err(|_| ThumbnailCommandError::Internal {
            message: "library root manager lock is poisoned".into(),
        })?
        .roots()
        .into_iter()
        .filter(|root| root.root.enabled && root.access_status == RootAccessStatus::Available)
        .map(|root| root.root.id)
        .collect::<BTreeSet<_>>();
    let pending_roots = scans
        .pending_authoritative(&required_roots)
        .map_err(|message| ThumbnailCommandError::Internal { message })?;
    if pending_roots > 0 {
        return Err(ThumbnailCommandError::RecoveryIncomplete {
            pending_roots,
            message: format!(
                "cannot reconcile thumbnail cache before {pending_roots} library roots complete a full scan"
            ),
        });
    }
    let records = catalog
        .lock()
        .map_err(|_| ThumbnailCommandError::Internal {
            message: "asset catalog lock is poisoned".into(),
        })?
        .records();
    let previews = Arc::clone(previews.inner());
    let report = tauri::async_runtime::spawn_blocking(move || previews.maintain(&records))
        .await
        .map_err(|error| ThumbnailCommandError::Internal {
            message: format!("thumbnail cache maintenance task failed: {error}"),
        })?
        .map_err(ThumbnailCommandError::from)?;
    record_cache_maintenance(diagnostics.inner(), &report, "manual");
    Ok(report)
}

fn record_cache_maintenance(
    diagnostics: &DiagnosticService,
    report: &CacheMaintenanceReport,
    trigger: &str,
) {
    record_diagnostic(
        diagnostics,
        DiagnosticLevel::Info,
        "cache",
        "maintenance-completed",
        [
            ("trigger", trigger.to_owned()),
            ("removedEntries", report.removed_entries.to_string()),
            ("removedBytes", report.removed_bytes.to_string()),
            ("remainingEntries", report.stats.entry_count.to_string()),
        ],
    );
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn reset_derived_state(
    previews: State<'_, Arc<ThumbnailService>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<DerivedStateResetReport, ThumbnailCommandError> {
    let active_scans = scans.active_count();
    if active_scans > 0 {
        return Err(ThumbnailCommandError::RecoveryBusy {
            active_scans,
            message: format!(
                "cannot rebuild derived state while {active_scans} library scans are active"
            ),
        });
    }
    let previews = Arc::clone(previews.inner());
    let cache = tauri::async_runtime::spawn_blocking(move || previews.clear())
        .await
        .map_err(|error| ThumbnailCommandError::Internal {
            message: format!("derived state reset task failed: {error}"),
        })?
        .map_err(ThumbnailCommandError::from)?;
    let catalog_assets_removed = {
        let mut catalog = catalog
            .lock()
            .map_err(|_| ThumbnailCommandError::Internal {
                message: "asset catalog lock is poisoned".into(),
            })?;
        let count = catalog.len();
        catalog.clear();
        count
    };
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "recovery",
        "derived-state-reset",
        [
            ("cacheFilesRemoved", cache.removed_files.to_string()),
            ("catalogAssetsRemoved", catalog_assets_removed.to_string()),
        ],
    );
    Ok(DerivedStateResetReport {
        cache,
        catalog_assets_removed,
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
async fn export_diagnostics(
    roots: State<'_, Mutex<LibraryRootManager>>,
    vaults: State<'_, Mutex<VaultManager>>,
    config: State<'_, Mutex<ApplicationConfigManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    watches: State<'_, Arc<WatchCoordinator>>,
    previews: State<'_, Arc<ThumbnailService>>,
    resources: State<'_, ResourceController>,
    runtime: State<'_, RuntimeState>,
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<DiagnosticExportReport, String> {
    let root_statuses = roots
        .lock()
        .map_err(|_| "library root manager lock is poisoned".to_owned())?
        .roots();
    let vault_statuses = vaults
        .lock()
        .map_err(|_| "Obsidian Vault manager lock is poisoned".to_owned())?
        .vaults();
    let application_config = config
        .lock()
        .map_err(|_| "application configuration lock is poisoned".to_owned())?
        .config();
    let asset_count = catalog
        .lock()
        .map_err(|_| "asset catalog lock is poisoned".to_owned())?
        .len();
    let active_scan_count = scans.active_count();
    let active_watch_count = watches.active_count();
    let resource_snapshot = resources.snapshot().map_err(|error| error.to_string())?;
    let previews_for_stats = Arc::clone(previews.inner());
    let cache_stats =
        tauri::async_runtime::spawn_blocking(move || previews_for_stats.cache_stats())
            .await
            .map_err(|error| format!("cache stats task failed: {error}"))?
            .map_err(|error| error.to_string())?;
    let snapshot = DiagnosticSnapshot {
        build: diagnostic_build(),
        runtime: DiagnosticRuntime {
            operating_system: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
        },
        configuration: DiagnosticConfigurationSummary {
            application_schema: APPLICATION_CONFIG_SCHEMA_VERSION,
            library_root_count: root_statuses.len(),
            enabled_library_root_count: root_statuses
                .iter()
                .filter(|root| root.root.enabled)
                .count(),
            obsidian_vault_count: vault_statuses.len(),
            enabled_obsidian_vault_count: vault_statuses
                .iter()
                .filter(|vault| vault.vault.enabled)
                .count(),
            saved_query_present: !application_config.ui.query.is_empty(),
            saved_tag_filter_count: application_config.ui.tag_filters.len(),
        },
        cache: DiagnosticCacheSummary {
            layout_version: cache_stats.layout_version,
            startup_disposition: runtime.cache_startup.disposition.to_string(),
            file_count: cache_stats.file_count,
            byte_count: cache_stats.byte_count,
        },
        catalog: DiagnosticCatalogSummary {
            asset_count,
            active_scan_count,
        },
        performance: DiagnosticPerformanceSummary {
            active_scans: active_scan_count,
            active_watches: active_watch_count,
            scheduler_active: resource_snapshot.active_total,
            scheduler_waiting: resource_snapshot.waiting_total,
            scheduler_peak_active: resource_snapshot.peak_active_total,
            scheduler_peak_waiting: resource_snapshot.peak_waiting_total,
            cache_entries: cache_stats.entry_count,
            cache_bytes: cache_stats.byte_count,
        },
        library_roots: root_statuses
            .iter()
            .enumerate()
            .map(|(index, root)| DiagnosticAccessSummary {
                name: format!("library-root-{}", index + 1),
                enabled: root.root.enabled,
                access_status: root.access_status.to_string(),
                path_fingerprint: path_fingerprint(&root.root.path),
            })
            .collect(),
        obsidian_vaults: vault_statuses
            .iter()
            .enumerate()
            .map(|(index, vault)| DiagnosticAccessSummary {
                name: format!("obsidian-vault-{}", index + 1),
                enabled: vault.vault.enabled,
                access_status: vault.access_status.to_string(),
                path_fingerprint: path_fingerprint(&vault.vault.path),
            })
            .collect(),
        ..DiagnosticSnapshot::default()
    };
    let service = Arc::clone(diagnostics.inner());
    let report = tauri::async_runtime::spawn_blocking(move || service.export(snapshot))
        .await
        .map_err(|error| format!("diagnostic export task failed: {error}"))?
        .map_err(|error| error.to_string())?;
    record_diagnostic(
        diagnostics.inner(),
        DiagnosticLevel::Info,
        "diagnostics",
        "export-created",
        [("eventCount", report.event_count.to_string())],
    );
    Ok(report)
}

fn diagnostic_build() -> DiagnosticBuild {
    let build = build_info();
    DiagnosticBuild {
        version: build.version.into(),
        git_commit: build.git_commit.into(),
        target: build.build_target.into(),
        profile: build.build_profile.into(),
        rustc: build.rustc_version.into(),
    }
}

fn path_fingerprint(path: &std::path::Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    format!("{digest:x}")[..16].to_owned()
}

fn record_diagnostic<const N: usize>(
    service: &DiagnosticService,
    level: DiagnosticLevel,
    category: &str,
    code: &str,
    details: [(&str, String); N],
) {
    let details = details
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    let _ = service.record(level, category, code, details);
}

/// Starts the desktop shell and blocks until its final window closes.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application event loop.
#[allow(clippy::too_many_lines)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(focused) = event
                && let Some(resources) = window.app_handle().try_state::<ResourceController>()
            {
                let mode = if *focused {
                    ResourceMode::Foreground
                } else {
                    ResourceMode::Background
                };
                let _ = resources.set_mode(mode);
            }
        })
        .setup(|app| {
            let config_directory = app.path().app_config_dir()?;
            let cache_directory = app.path().app_cache_dir()?;
            let log_directory = app.path().app_log_dir()?;
            let roots = LibraryRootManager::open(config_directory.join("library-roots.yml"))?;
            app.manage(Mutex::new(roots));
            let vaults = VaultManager::open(config_directory.join("obsidian-vaults.yml"))?;
            app.manage(Mutex::new(vaults));
            let application_config =
                ApplicationConfigManager::open(config_directory.join("application.yml"))?;
            app.manage(Mutex::new(application_config));
            app.manage(Arc::new(SavedFilterStore::new(
                config_directory.join("saved-filters.yml"),
            )));
            app.manage(Arc::new(ScanCoordinator::default()));
            app.manage(Arc::new(WatchCoordinator::default()));
            app.manage(Arc::new(ReconciliationCoordinator::default()));
            app.manage(Arc::new(Mutex::new(AssetCatalog::default())));
            app.manage(Arc::new(SelectionSessionStore::default()));
            app.manage(Arc::new(BatchPreflightStore::default()));
            app.manage(Arc::new(MetadataTransactionStore::open(
                config_directory.join("metadata-transactions-v1"),
            )?));
            app.manage(Arc::new(MetadataConflictStore::default()));
            let diagnostics = Arc::new(DiagnosticService::new(log_directory.join("diagnostics")));
            let resources = ResourceController::with_defaults();
            let worker_bundle = app.path().resource_dir()?.join("format-workers/libheif");
            let worker_manifest = worker_bundle.join(WORKER_BUNDLE_MANIFEST);
            let worker = match std::fs::symlink_metadata(&worker_manifest) {
                Ok(_) => match open_libheif_worker_bundle(&worker_bundle) {
                    Ok(worker) => {
                        record_diagnostic(
                            &diagnostics,
                            DiagnosticLevel::Info,
                            "preview",
                            "libheif-worker-ready",
                            [("providerVersion", worker.provider_version().to_owned())],
                        );
                        Some(worker)
                    }
                    Err(error) => {
                        record_diagnostic(
                            &diagnostics,
                            DiagnosticLevel::Warning,
                            "preview",
                            "libheif-worker-rejected",
                            [("reason", error.to_string())],
                        );
                        None
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    record_diagnostic(
                        &diagnostics,
                        DiagnosticLevel::Warning,
                        "preview",
                        "libheif-worker-manifest-unreadable",
                        [("reason", error.to_string())],
                    );
                    None
                }
            };
            let mut previews =
                ThumbnailService::open_with_resources(&cache_directory, resources.clone())?;
            if let Some(worker) = worker {
                previews = previews.with_libheif_worker(worker)?;
            }
            let previews = Arc::new(previews);
            let cache_startup = previews.startup_report();
            app.manage(previews);
            app.manage(resources);
            record_diagnostic(
                &diagnostics,
                DiagnosticLevel::Info,
                "runtime",
                "application-started",
                [
                    ("cacheDisposition", cache_startup.disposition.to_string()),
                    ("cacheFilesRemoved", cache_startup.removed_files.to_string()),
                ],
            );
            app.manage(diagnostics);
            app.manage(RuntimeState {
                paths: ApplicationPaths {
                    config: config_directory,
                    cache: cache_directory,
                    log: log_directory,
                },
                cache_startup,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            build_info,
            get_application_config,
            update_application_config,
            list_saved_filters,
            create_saved_filter,
            update_saved_filter,
            rename_saved_filter,
            delete_saved_filter,
            execute_saved_filter,
            runtime_recovery_status,
            runtime_resource_status,
            inspect_library_consistency,
            trace_asset_support,
            list_library_roots,
            add_library_root,
            update_library_root,
            remove_library_root,
            list_obsidian_vaults,
            add_obsidian_vault,
            update_obsidian_vault,
            remove_obsidian_vault,
            resolve_obsidian_vault_references,
            start_library_scan,
            cancel_library_scan,
            acknowledge_library_scan_batch,
            inspect_library_reconciliation,
            confirm_library_relink,
            start_library_watch,
            stop_library_watch,
            edit_asset_metadata,
            resolve_metadata_conflict,
            dismiss_metadata_conflict,
            list_metadata_transactions,
            continue_metadata_transaction,
            restore_metadata_transaction,
            dismiss_metadata_transaction,
            query_assets,
            create_query_selection_snapshot,
            create_range_selection_snapshot,
            create_explicit_selection_snapshot,
            release_selection_snapshot,
            selection_session_stats,
            prepare_metadata_batch,
            release_batch_preflight,
            request_thumbnail,
            read_thumbnail,
            clear_thumbnail_cache,
            maintain_thumbnail_cache,
            reset_derived_state,
            export_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Material Eagle desktop application");
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::mpsc::{self, RecvTimeoutError, TrySendError};
    use std::time::Duration;

    use asset_catalog::{AssetCatalog, AssetEditTarget, BatchMetadataEdit, EditFailureKind};
    use asset_core::{AssetRecord, SidecarState};
    use asset_filesystem::{
        FsChange, FsChangeBatch, FsChangeKind, FsRescanReason, LibraryRoot, LibraryRootStatus,
        RootAccessStatus, RootScanSettings, ScanBatch,
    };
    use asset_preview::{CacheClearReport, CacheMaintenanceReport, CacheStats};
    use asset_transactions::{TransactionScopeItem, TransactionState, TransactionSummary};
    use metadata::{MetadataPatch, sidecar_path_for};
    use tempfile::tempdir;
    use uuid::Uuid;

    use asset_index::{QueryParseError, QueryParseErrorKind};

    use super::{
        ApplicationPaths, BatchMetadataEditCommandResult, DerivedStateResetReport,
        LibraryScanEvent, LibraryWatchEvent, MAX_ACTIVE_SCANS, MAX_ACTIVE_WATCHES,
        QueryAssetsError, SCAN_BATCH_QUEUE_CAPACITY, SavedFilterCommandError,
        SavedFilterCommandErrorKind, SavedFilterFileVersion, ScanCancellation, ScanCoordinator,
        ScanDeliveryWindow, ScanPipelineMessage, SelectionCommandError, SelectionCommandErrorKind,
        ThumbnailCommandError, VaultReferenceFailureKind, WatchCoordinator, build_info,
        ensure_transaction_item_inside_root, failed_batch_transaction_preflight, path_fingerprint,
        saved_filter_root_sets, scan_pipeline_channel, transaction_targets, vault_failure_kind,
    };

    #[test]
    fn build_information_is_traceable() {
        let info = build_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.git_commit.is_empty());
        assert!(!info.build_target.is_empty());
        assert!(!info.rustc_version.is_empty());
    }

    #[test]
    fn scan_pipeline_applies_a_fixed_batch_bound() {
        let (sender, _receiver) = scan_pipeline_channel();
        for sequence in 0..SCAN_BATCH_QUEUE_CAPACITY {
            sender
                .try_send(ScanPipelineMessage::Batch(ScanBatch {
                    sequence,
                    assets: Vec::new(),
                    problems: Vec::new(),
                    visited_files: 0,
                }))
                .expect("queue slot");
        }
        assert!(matches!(
            sender.try_send(ScanPipelineMessage::Batch(ScanBatch {
                sequence: SCAN_BATCH_QUEUE_CAPACITY,
                assets: Vec::new(),
                problems: Vec::new(),
                visited_files: 0,
            })),
            Err(TrySendError::Full(_))
        ));
    }

    #[test]
    fn scan_delivery_waits_for_frontend_acknowledgement() {
        let delivery = Arc::new(ScanDeliveryWindow::default());
        let cancellation = ScanCancellation::new();
        for sequence in 0..SCAN_BATCH_QUEUE_CAPACITY {
            delivery
                .reserve(sequence, &cancellation)
                .expect("delivery slot");
        }
        let pending_delivery = Arc::clone(&delivery);
        let pending_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            sender
                .send(pending_delivery.reserve(SCAN_BATCH_QUEUE_CAPACITY, &pending_cancellation))
                .expect("send result");
        });
        assert_eq!(
            receiver.recv_timeout(Duration::from_millis(20)),
            Err(RecvTimeoutError::Timeout)
        );
        assert_eq!(delivery.acknowledge(0), Ok(true));
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("unblocked delivery")
                .is_ok()
        );
        worker.join().expect("join delivery worker");
        assert_eq!(delivery.pending_count(), SCAN_BATCH_QUEUE_CAPACITY);
    }

    #[test]
    fn scan_coordinator_cancels_and_releases_registered_scans() {
        let coordinator = ScanCoordinator::default();
        let scan_id = Uuid::now_v7();
        let root_id = Uuid::now_v7();
        let cancellation = ScanCancellation::new();
        let delivery = coordinator
            .register(scan_id, root_id, cancellation.clone())
            .expect("register scan");
        delivery.reserve(7, &cancellation).expect("reserve batch");
        assert_eq!(coordinator.acknowledge(scan_id, 7), Ok(true));
        assert_eq!(coordinator.acknowledge(scan_id, 7), Ok(false));
        assert!(coordinator.is_root_active(root_id));
        let required = BTreeSet::from([root_id]);
        assert_eq!(coordinator.pending_authoritative(&required), Ok(1));

        assert!(
            coordinator
                .register(Uuid::now_v7(), root_id, ScanCancellation::new())
                .expect_err("duplicate root scan")
                .contains("already being scanned")
        );

        assert!(coordinator.cancel(scan_id).expect("cancel scan"));
        assert!(cancellation.is_cancelled());
        coordinator.finish(scan_id);
        assert!(!coordinator.is_root_active(root_id));
        assert!(!coordinator.cancel(scan_id).expect("cancel finished scan"));
        coordinator
            .mark_authoritative(root_id)
            .expect("mark complete scan");
        assert_eq!(coordinator.pending_authoritative(&required), Ok(0));
    }

    #[test]
    fn coordinators_bound_active_scan_and_watcher_threads() {
        let scans = ScanCoordinator::default();
        let watches = WatchCoordinator::default();
        for _ in 0..MAX_ACTIVE_SCANS {
            scans
                .register(Uuid::now_v7(), Uuid::now_v7(), ScanCancellation::new())
                .expect("scan capacity");
        }
        assert!(
            scans
                .register(Uuid::now_v7(), Uuid::now_v7(), ScanCancellation::new())
                .expect_err("scan limit")
                .contains("limit")
        );
        for _ in 0..MAX_ACTIVE_WATCHES {
            watches
                .register(Uuid::now_v7(), Uuid::now_v7(), ScanCancellation::new())
                .expect("watch capacity");
        }
        assert!(
            watches
                .register(Uuid::now_v7(), Uuid::now_v7(), ScanCancellation::new())
                .expect_err("watch limit")
                .contains("limit")
        );
    }

    #[test]
    fn scan_events_use_the_frontend_wire_shape() {
        let scan_id = Uuid::now_v7();
        let event = serde_json::to_value(LibraryScanEvent::Failed {
            scan_id,
            message: "invalid root".into(),
            removed_keys: vec!["/library/stale.png".into()],
            restored_records: Vec::new(),
            root_access_status: Some(RootAccessStatus::Missing),
        })
        .expect("serialize scan event");

        assert_eq!(event["event"], "failed");
        assert_eq!(event["data"]["scanId"], scan_id.to_string());
        assert_eq!(event["data"]["message"], "invalid root");
        assert_eq!(event["data"]["removedKeys"][0], "/library/stale.png");
        assert_eq!(event["data"]["rootAccessStatus"], "missing");
    }

    #[test]
    fn watch_coordinator_prevents_duplicate_root_watchers() {
        let coordinator = WatchCoordinator::default();
        let root_id = Uuid::now_v7();
        let first_id = Uuid::now_v7();
        let second_id = Uuid::now_v7();
        let cancellation = ScanCancellation::new();
        coordinator
            .register(first_id, root_id, cancellation.clone())
            .expect("register watch");

        assert!(
            coordinator
                .register(second_id, root_id, ScanCancellation::new())
                .expect_err("duplicate root")
                .contains("already watched")
        );
        assert_eq!(coordinator.active_count(), 1);
        assert!(coordinator.cancel(first_id).expect("cancel watch"));
        assert!(cancellation.is_cancelled());
        assert_eq!(coordinator.active_count(), 0);
        coordinator.finish(first_id);
        assert_eq!(coordinator.active_count(), 0);
    }

    #[test]
    fn watch_events_use_a_bounded_frontend_wire_shape() {
        let watch_id = Uuid::now_v7();
        let root_id = Uuid::now_v7();
        let value = serde_json::to_value(LibraryWatchEvent::Changes {
            watch_id,
            root_id,
            batch: FsChangeBatch {
                root: "/library".into(),
                changes: vec![FsChange {
                    kind: FsChangeKind::RescanRequired,
                    paths: vec!["/library".into()],
                    reason: Some(FsRescanReason::BatchOverflow),
                }],
                raw_event_count: 3,
            },
        })
        .expect("serialize watch event");

        assert_eq!(value["event"], "changes");
        assert_eq!(value["data"]["watchId"], watch_id.to_string());
        assert_eq!(value["data"]["rootId"], root_id.to_string());
        assert_eq!(value["data"]["batch"]["rawEventCount"], 3);
        assert_eq!(
            value["data"]["batch"]["changes"][0]["kind"],
            "rescan-required"
        );
        assert_eq!(
            value["data"]["batch"]["changes"][0]["reason"],
            "batch-overflow"
        );
    }

    #[test]
    fn query_errors_use_a_structured_frontend_wire_shape() {
        let value = serde_json::to_value(QueryAssetsError::Parse {
            error: QueryParseError {
                kind: QueryParseErrorKind::UnknownFilter,
                offset: 4,
                token: Some("kind:image".into()),
                message: "unknown filter".into(),
            },
        })
        .expect("serialize query error");

        assert_eq!(value["kind"], "parse");
        assert_eq!(value["error"]["kind"], "unknown-filter");
        assert_eq!(value["error"]["offset"], 4);
        assert_eq!(value["error"]["token"], "kind:image");
    }

    #[test]
    fn selection_errors_use_stable_redacted_wire_shapes() {
        let changed =
            SelectionCommandError::from(asset_selection::SelectionError::CatalogChanged {
                actual_revision: 41,
            });
        let value = serde_json::to_value(changed).expect("serialize selection error");
        assert_eq!(value["kind"], "catalog-changed");
        assert_eq!(value["actualRevision"], 41);
        assert!(value.get("rootId").is_none());

        let root_id = Uuid::now_v7();
        let unavailable =
            SelectionCommandError::root(SelectionCommandErrorKind::RootOffline, root_id);
        let value = serde_json::to_value(unavailable).expect("serialize root error");
        assert_eq!(value["kind"], "root-offline");
        assert_eq!(value["rootId"], root_id.to_string());
        assert!(!value.to_string().contains('/'));
    }

    #[test]
    fn saved_filter_errors_are_structured_and_do_not_expose_paths() {
        let actual_version = SavedFilterFileVersion {
            exists: true,
            size: 128,
            modified_unix_ms: Some(1_700_000_000_000),
            sha256: Some("a".repeat(64)),
        };
        let value = serde_json::to_value(SavedFilterCommandError {
            kind: SavedFilterCommandErrorKind::ExternalChange,
            message: "reload before saving".into(),
            actual_version: Some(actual_version),
            query_kind: None,
            query_offset: None,
        })
        .expect("serialize saved filter error");

        assert_eq!(value["kind"], "external-change");
        assert_eq!(value["actualVersion"]["size"], 128);
        assert!(value.get("queryKind").is_none());
        assert!(value.get("queryOffset").is_none());
        assert!(!value.to_string().contains("path"));
    }

    #[test]
    fn saved_filter_root_sets_keep_disabled_and_unavailable_roots_out_of_execution() {
        let available_id = Uuid::now_v7();
        let unavailable_id = Uuid::now_v7();
        let disabled_id = Uuid::now_v7();
        let statuses = [
            root_status(available_id, true, RootAccessStatus::Available),
            root_status(unavailable_id, true, RootAccessStatus::Missing),
            root_status(disabled_id, false, RootAccessStatus::Available),
        ];

        let (enabled, available) = saved_filter_root_sets(&statuses);

        assert_eq!(enabled, BTreeSet::from([available_id, unavailable_id]));
        assert_eq!(available, BTreeSet::from([available_id]));
    }

    fn root_status(id: Uuid, enabled: bool, access_status: RootAccessStatus) -> LibraryRootStatus {
        LibraryRootStatus {
            root: LibraryRoot {
                id,
                path: format!("/library/{id}").into(),
                name: id.to_string(),
                enabled,
                scan: RootScanSettings::default(),
                extra: BTreeMap::new(),
            },
            access_status,
            access_message: None,
        }
    }

    #[test]
    fn thumbnail_errors_use_a_structured_frontend_wire_shape() {
        let value = serde_json::to_value(ThumbnailCommandError::AssetNotFound {
            asset_key: "/assets/missing.png".into(),
        })
        .expect("serialize thumbnail error");

        assert_eq!(value["kind"], "asset-not-found");
        assert_eq!(value["assetKey"], "/assets/missing.png");

        let busy = serde_json::to_value(ThumbnailCommandError::RecoveryBusy {
            active_scans: 2,
            message: "two scans are active".into(),
        })
        .expect("serialize recovery busy error");
        assert_eq!(busy["kind"], "recovery-busy");
        assert_eq!(busy["activeScans"], 2);
        assert_eq!(busy["message"], "two scans are active");

        let incomplete = serde_json::to_value(ThumbnailCommandError::RecoveryIncomplete {
            pending_roots: 1,
            message: "one root has not completed".into(),
        })
        .expect("serialize recovery incomplete error");
        assert_eq!(incomplete["kind"], "recovery-incomplete");
        assert_eq!(incomplete["pendingRoots"], 1);
    }

    #[test]
    fn recovery_reports_use_the_frontend_wire_shape() {
        let value = serde_json::to_value(DerivedStateResetReport {
            cache: CacheClearReport {
                removed_files: 4,
                removed_bytes: 8192,
            },
            catalog_assets_removed: 12,
        })
        .expect("serialize recovery report");
        let paths = serde_json::to_value(ApplicationPaths {
            config: "/app/config".into(),
            cache: "/app/cache".into(),
            log: "/app/log".into(),
        })
        .expect("serialize application paths");
        let maintenance = serde_json::to_value(CacheMaintenanceReport {
            removed_entries: 3,
            orphan_entries: 1,
            stats: CacheStats {
                layout_version: 2,
                entry_count: 12,
                max_entries: 20_000,
                decoder_version: "test-decoder-v2",
                ..CacheStats::default()
            },
            ..CacheMaintenanceReport::default()
        })
        .expect("serialize maintenance report");

        assert_eq!(value["cache"]["removedFiles"], 4);
        assert_eq!(value["cache"]["removedBytes"], 8192);
        assert_eq!(value["catalogAssetsRemoved"], 12);
        assert_eq!(paths["configDirectory"], "/app/config");
        assert_eq!(paths["cacheDirectory"], "/app/cache");
        assert_eq!(paths["logDirectory"], "/app/log");
        assert_eq!(maintenance["removedEntries"], 3);
        assert_eq!(maintenance["orphanEntries"], 1);
        assert_eq!(maintenance["stats"]["entryCount"], 12);
        assert_eq!(maintenance["stats"]["maxEntries"], 20_000);
        assert_eq!(maintenance["stats"]["decoderVersion"], "test-decoder-v2");
    }

    #[test]
    fn batch_transaction_summary_uses_the_frontend_wire_shape() {
        let transaction_id = Uuid::now_v7();
        let root_id = Uuid::now_v7();
        let value = serde_json::to_value(BatchMetadataEditCommandResult {
            updated: Vec::new(),
            failures: Vec::new(),
            transaction: Some(TransactionSummary {
                id: transaction_id,
                state: TransactionState::Active,
                created_at: "2026-08-19T08:00:00.000Z".into(),
                updated_at: "2026-08-19T08:01:00.000Z".into(),
                item_count: 1_000,
                applied_count: 317,
                failed_count: 0,
                conflict_count: 0,
                restored_count: 0,
                root_ids: vec![root_id],
            }),
            conflicts: Vec::new(),
        })
        .expect("serialize batch transaction result");

        assert_eq!(value["transaction"]["id"], transaction_id.to_string());
        assert_eq!(value["transaction"]["state"], "active");
        assert_eq!(value["transaction"]["itemCount"], 1_000);
        assert_eq!(value["transaction"]["appliedCount"], 317);
        assert_eq!(value["transaction"]["rootIds"][0], root_id.to_string());
    }

    #[test]
    fn stale_batch_targets_still_enter_the_transaction_plan() {
        let root_id = Uuid::now_v7();
        let mut catalog = AssetCatalog::default();
        let records = ["first", "second"].map(|key| {
            let mut record = AssetRecord::untagged(
                key.into(),
                format!("/library/{key}.png").into(),
                "image/png".into(),
                1,
                0,
            );
            record.root_id = Some(root_id);
            record.sidecar_state = Some(SidecarState {
                schema: 1,
                digest: format!("current-{key}"),
                size: 128,
                modified_unix_ms: 1_234,
                updated_at: "2026-08-19T08:00:00.000Z".into(),
            });
            record
        });
        catalog.ingest(records);
        let input = BatchMetadataEdit {
            targets: vec![
                AssetEditTarget {
                    key: "first".into(),
                    expected_sidecar_digest: Some("stale".into()),
                    expected_sidecar_size: Some(128),
                    expected_sidecar_modified_unix_ms: Some(1_234),
                },
                AssetEditTarget {
                    key: "second".into(),
                    expected_sidecar_digest: Some("current-second".into()),
                    expected_sidecar_size: Some(128),
                    expected_sidecar_modified_unix_ms: Some(1_234),
                },
            ],
            patch: MetadataPatch {
                add_tags: ["batch/test".into()].into(),
                ..MetadataPatch::default()
            },
        };

        let targets = transaction_targets(&catalog, &input).expect("transaction targets");

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].expected_sidecar_digest.as_deref(), Some("stale"));

        let unavailable = BatchMetadataEdit {
            targets: vec![
                input.targets[0].clone(),
                AssetEditTarget {
                    key: "missing".into(),
                    expected_sidecar_digest: None,
                    expected_sidecar_size: None,
                    expected_sidecar_modified_unix_ms: None,
                },
            ],
            patch: input.patch,
        };
        assert!(transaction_targets(&catalog, &unavailable).is_none());
        let failures = failed_batch_transaction_preflight(&catalog, &unavailable);
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].kind, EditFailureKind::WriteFailed);
        assert_eq!(failures[1].kind, EditFailureKind::NotFound);
    }

    #[test]
    fn transaction_scope_rejects_forged_or_outside_paths() {
        let directory = tempdir().expect("tempdir");
        let root_path = directory.path().join("library");
        let outside_path = directory.path().join("outside.png");
        fs::create_dir(&root_path).expect("root");
        fs::write(&outside_path, b"outside").expect("outside asset");
        let asset_path = root_path.join("inside.png");
        fs::write(&asset_path, b"inside").expect("inside asset");
        let root = LibraryRoot {
            id: Uuid::now_v7(),
            path: root_path.canonicalize().expect("canonical root"),
            name: "Library".into(),
            enabled: true,
            scan: RootScanSettings::default(),
            extra: BTreeMap::new(),
        };
        let valid = TransactionScopeItem {
            root_id: root.id,
            asset_path: asset_path.clone(),
            sidecar_path: sidecar_path_for(&asset_path),
        };
        assert!(ensure_transaction_item_inside_root(&valid, &root).is_ok());

        let forged_sidecar = TransactionScopeItem {
            sidecar_path: directory.path().join("forged.asset.yml"),
            ..valid.clone()
        };
        assert!(ensure_transaction_item_inside_root(&forged_sidecar, &root).is_err());
        let outside = TransactionScopeItem {
            root_id: root.id,
            asset_path: outside_path.clone(),
            sidecar_path: sidecar_path_for(&outside_path),
        };
        assert!(ensure_transaction_item_inside_root(&outside, &root).is_err());
    }

    #[test]
    fn diagnostic_path_fingerprints_are_stable_and_redacted() {
        let path = Path::new("/private/library/client-a/secret.png");
        let fingerprint = path_fingerprint(path);

        assert_eq!(fingerprint, path_fingerprint(path));
        assert_eq!(fingerprint.len(), 16);
        assert!(!fingerprint.contains("secret"));
        assert_ne!(fingerprint, path_fingerprint(Path::new("/another/path")));
    }

    #[test]
    fn vault_reference_errors_have_stable_user_facing_kinds() {
        let outside = asset_link_resolver::VaultError::OutsideVault {
            vault: "/vault".into(),
            asset: "/other/image.png".into(),
        };
        let missing_asset = asset_link_resolver::VaultError::Canonicalize {
            kind: "asset",
            path: "/missing.png".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };

        assert_eq!(
            vault_failure_kind(&outside),
            VaultReferenceFailureKind::OutsideVault
        );
        assert_eq!(
            vault_failure_kind(&missing_asset),
            VaultReferenceFailureKind::AssetUnavailable
        );
    }
}
