use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_config::{
    APPLICATION_CONFIG_SCHEMA_VERSION, ApplicationConfig, ApplicationConfigManager,
    DiagnosticAccessSummary, DiagnosticBuild, DiagnosticCacheSummary, DiagnosticCatalogSummary,
    DiagnosticConfigurationSummary, DiagnosticExportReport, DiagnosticLevel, DiagnosticRuntime,
    DiagnosticService, DiagnosticSnapshot, UpdateUiPreferences,
};
use asset_catalog::{
    AssetCatalog, BatchMetadataEdit, BatchMetadataEditResult, CatalogRootReconciliation,
    EditFailureKind, MetadataEditFailure, QueryAssetsInput, QueryAssetsResult,
};
use asset_filesystem::{
    AddLibraryRoot, FilesystemError, FsChangeBatch, FsRescanReason, LibraryRoot,
    LibraryRootManager, LibraryRootStatus, ReconciliationReport, RelinkCandidate, RelinkReceipt,
    RootAccessStatus, ScanBatch, ScanCancellation, ScanCompletion, ScanOptions, ScanSummary,
    UpdateLibraryRoot, WatchSession, apply_relink, inspect_reconciliation, scan_root_incremental,
};
use asset_index::QueryParseError;
use asset_link_resolver::{
    AddVaultRoot, UpdateVaultRoot, VaultError, VaultManager, VaultReference, VaultRoot,
    VaultRootStatus,
};
use asset_preview::{
    CacheClearReport, CacheStartupReport, PreviewError, ThumbnailOutcome, ThumbnailRequest,
    ThumbnailService,
};
use asset_transactions::{
    MetadataTransactionStore, TransactionFailureKind, TransactionRecoveryResult,
    TransactionScopeItem, TransactionSummary, TransactionTarget,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Manager, State, ipc::Channel, ipc::Response};
use uuid::Uuid;

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
            | PreviewError::MissingCacheEntry(_) => Self::Cache {
                message: error.to_string(),
            },
            PreviewError::InvalidConcurrency(_) | PreviewError::PoisonedLock(_) => Self::Internal {
                message: error.to_string(),
            },
        }
    }
}

#[derive(Default)]
struct ScanCoordinator {
    active: Mutex<HashMap<Uuid, ActiveScan>>,
}

#[derive(Debug, Clone)]
struct ActiveScan {
    root_id: Uuid,
    cancellation: ScanCancellation,
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

    #[cfg(test)]
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
    ) -> Result<(), String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "scan coordinator lock is poisoned".to_owned())?;
        if active.values().any(|scan| scan.root_id == root_id) {
            return Err(format!("library root is already being scanned: {root_id}"));
        }
        active.insert(
            scan_id,
            ActiveScan {
                root_id,
                cancellation,
            },
        );
        Ok(())
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

    fn active_count(&self) -> usize {
        self.active.lock().map_or(0, |active| active.len())
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
fn runtime_recovery_status(state: State<'_, RuntimeState>) -> RuntimeRecoveryStatus {
    RuntimeRecoveryStatus {
        paths: state.paths.clone(),
        cache_startup: state.cache_startup,
    }
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
#[allow(clippy::needless_pass_by_value)]
fn start_library_scan(
    root_id: Uuid,
    on_event: Channel<LibraryScanEvent>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
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

    let scan_id = Uuid::now_v7();
    let cancellation = ScanCancellation::new();
    scans.register(scan_id, root_id, cancellation.clone())?;
    let coordinator = Arc::clone(scans.inner());
    let catalog = Arc::clone(catalog.inner());
    let diagnostics = Arc::clone(diagnostics.inner());
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
                &diagnostics,
            );
        });
    if let Err(error) = spawn_result {
        scans.finish(scan_id);
        return Err(format!("failed to start library scan thread: {error}"));
    }
    Ok(scan_id)
}

fn run_library_scan_thread(
    scan_id: Uuid,
    root: LibraryRoot,
    on_event: &Channel<LibraryScanEvent>,
    cancellation: &ScanCancellation,
    coordinator: &ScanCoordinator,
    catalog: &Mutex<AssetCatalog>,
    diagnostics: &DiagnosticService,
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
    let result =
        scan_root_incremental(Some(root.id), &root.path, &options, cancellation, |batch| {
            scanned_records.extend(batch.assets.iter().cloned());
            if let Ok(mut catalog) = catalog.lock() {
                catalog.ingest(batch.assets.iter().cloned());
            } else {
                cancellation.cancel();
            }
            if on_event
                .send(LibraryScanEvent::Batch { scan_id, batch })
                .is_err()
            {
                cancellation.cancel();
            }
        });
    publish_scan_result(
        scan_id,
        root.id,
        result,
        scanned_records,
        &previous_records,
        on_event,
        catalog,
        diagnostics,
    );
    coordinator.finish(scan_id);
}

#[allow(clippy::too_many_arguments)]
fn publish_scan_result(
    scan_id: Uuid,
    root_id: Uuid,
    result: Result<ScanSummary, FilesystemError>,
    scanned_records: Vec<asset_core::AssetRecord>,
    previous_records: &[asset_core::AssetRecord],
    on_event: &Channel<LibraryScanEvent>,
    catalog: &Mutex<AssetCatalog>,
    diagnostics: &DiagnosticService,
) {
    match result {
        Ok(summary) => {
            let desired_records = if summary.completion == ScanCompletion::Completed {
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
                });
                return;
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
                message: error.to_string(),
                removed_keys,
                restored_records,
            });
        }
    }
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
fn inspect_library_reconciliation(
    root_id: Uuid,
    roots: State<'_, Mutex<LibraryRootManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    reconciliation: State<'_, Arc<ReconciliationCoordinator>>,
) -> Result<ReconciliationReport, String> {
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
) -> Result<RelinkReceipt, String> {
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
    diagnostics: State<'_, Arc<DiagnosticService>>,
) -> Result<BatchMetadataEditCommandResult, String> {
    let targets = {
        let catalog = catalog
            .lock()
            .map_err(|_| "asset catalog lock is poisoned".to_owned())?;
        transaction_targets(&catalog, &input)
    };
    let result = if let Some(targets) = targets {
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
                    committed.digest,
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
        }
    } else if input.targets.len() >= 2 {
        let catalog = catalog
            .lock()
            .map_err(|_| "asset catalog lock is poisoned".to_owned())?;
        BatchMetadataEditCommandResult {
            updated: Vec::new(),
            failures: failed_batch_transaction_preflight(&catalog, &input),
            transaction: None,
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
        }
    };
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
        ],
    );
    Ok(result)
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
            TransactionTarget::from_record(record, target.expected_sidecar_digest.clone())
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
) -> Result<TransactionRecoveryResult, String> {
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
) -> Result<TransactionRecoveryResult, String> {
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

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
async fn request_thumbnail(
    input: ThumbnailRequest,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
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
    let previews = Arc::clone(previews.inner());
    tauri::async_runtime::spawn_blocking(move || previews.request(&record, input.max_edge))
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
async fn export_diagnostics(
    roots: State<'_, Mutex<LibraryRootManager>>,
    vaults: State<'_, Mutex<VaultManager>>,
    config: State<'_, Mutex<ApplicationConfigManager>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    previews: State<'_, Arc<ThumbnailService>>,
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
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
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
            app.manage(Arc::new(ScanCoordinator::default()));
            app.manage(Arc::new(WatchCoordinator::default()));
            app.manage(Arc::new(ReconciliationCoordinator::default()));
            app.manage(Arc::new(Mutex::new(AssetCatalog::default())));
            app.manage(Arc::new(MetadataTransactionStore::open(
                config_directory.join("metadata-transactions-v1"),
            )?));
            let previews = Arc::new(ThumbnailService::open(&cache_directory, 4)?);
            let cache_startup = previews.startup_report();
            app.manage(previews);
            let diagnostics = Arc::new(DiagnosticService::new(log_directory.join("diagnostics")));
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
            runtime_recovery_status,
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
            inspect_library_reconciliation,
            confirm_library_relink,
            start_library_watch,
            stop_library_watch,
            edit_asset_metadata,
            list_metadata_transactions,
            continue_metadata_transaction,
            restore_metadata_transaction,
            dismiss_metadata_transaction,
            query_assets,
            request_thumbnail,
            read_thumbnail,
            clear_thumbnail_cache,
            reset_derived_state,
            export_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Material Eagle desktop application");
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use asset_catalog::{AssetCatalog, AssetEditTarget, BatchMetadataEdit, EditFailureKind};
    use asset_core::{AssetRecord, SidecarState};
    use asset_filesystem::{
        FsChange, FsChangeBatch, FsChangeKind, FsRescanReason, LibraryRoot, RootScanSettings,
    };
    use asset_preview::CacheClearReport;
    use asset_transactions::{TransactionScopeItem, TransactionState, TransactionSummary};
    use metadata::{MetadataPatch, sidecar_path_for};
    use tempfile::tempdir;
    use uuid::Uuid;

    use asset_index::{QueryParseError, QueryParseErrorKind};

    use super::{
        ApplicationPaths, BatchMetadataEditCommandResult, DerivedStateResetReport,
        LibraryScanEvent, LibraryWatchEvent, QueryAssetsError, ScanCancellation, ScanCoordinator,
        ThumbnailCommandError, VaultReferenceFailureKind, WatchCoordinator, build_info,
        ensure_transaction_item_inside_root, failed_batch_transaction_preflight, path_fingerprint,
        transaction_targets, vault_failure_kind,
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
    fn scan_coordinator_cancels_and_releases_registered_scans() {
        let coordinator = ScanCoordinator::default();
        let scan_id = Uuid::now_v7();
        let root_id = Uuid::now_v7();
        let cancellation = ScanCancellation::new();
        coordinator
            .register(scan_id, root_id, cancellation.clone())
            .expect("register scan");
        assert!(coordinator.is_root_active(root_id));

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
    }

    #[test]
    fn scan_events_use_the_frontend_wire_shape() {
        let scan_id = Uuid::now_v7();
        let event = serde_json::to_value(LibraryScanEvent::Failed {
            scan_id,
            message: "invalid root".into(),
            removed_keys: vec!["/library/stale.png".into()],
            restored_records: Vec::new(),
        })
        .expect("serialize scan event");

        assert_eq!(event["event"], "failed");
        assert_eq!(event["data"]["scanId"], scan_id.to_string());
        assert_eq!(event["data"]["message"], "invalid root");
        assert_eq!(event["data"]["removedKeys"][0], "/library/stale.png");
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

        assert_eq!(value["cache"]["removedFiles"], 4);
        assert_eq!(value["cache"]["removedBytes"], 8192);
        assert_eq!(value["catalogAssetsRemoved"], 12);
        assert_eq!(paths["configDirectory"], "/app/config");
        assert_eq!(paths["cacheDirectory"], "/app/cache");
        assert_eq!(paths["logDirectory"], "/app/log");
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
                },
                AssetEditTarget {
                    key: "second".into(),
                    expected_sidecar_digest: Some("current-second".into()),
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
