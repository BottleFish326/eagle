use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use asset_catalog::{
    AssetCatalog, BatchMetadataEdit, BatchMetadataEditResult, QueryAssetsInput, QueryAssetsResult,
};
use asset_filesystem::{
    AddLibraryRoot, LibraryRoot, LibraryRootManager, LibraryRootStatus, RootAccessStatus,
    ScanBatch, ScanCancellation, ScanOptions, ScanSummary, UpdateLibraryRoot,
    scan_root_incremental,
};
use asset_index::QueryParseError;
use asset_preview::{
    CacheClearReport, PreviewError, ThumbnailOutcome, ThumbnailRequest, ThumbnailService,
};
use serde::Serialize;
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
    },
    Failed {
        scan_id: Uuid,
        message: String,
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
    active: Mutex<HashMap<Uuid, ScanCancellation>>,
}

impl ScanCoordinator {
    fn register(&self, scan_id: Uuid, cancellation: ScanCancellation) -> Result<(), String> {
        self.active
            .lock()
            .map_err(|_| "scan coordinator lock is poisoned".to_owned())?
            .insert(scan_id, cancellation);
        Ok(())
    }

    fn cancel(&self, scan_id: Uuid) -> Result<bool, String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "scan coordinator lock is poisoned".to_owned())?;
        Ok(active.get(&scan_id).is_some_and(|token| {
            token.cancel();
            true
        }))
    }

    fn finish(&self, scan_id: Uuid) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&scan_id);
        }
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
fn start_library_scan(
    root_id: Uuid,
    on_event: Channel<LibraryScanEvent>,
    roots: State<'_, Mutex<LibraryRootManager>>,
    scans: State<'_, Arc<ScanCoordinator>>,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
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
    scans.register(scan_id, cancellation.clone())?;
    let coordinator = Arc::clone(scans.inner());
    let catalog = Arc::clone(catalog.inner());
    let root = root_status.root;
    let thread_name = format!("library-scan-{}", &scan_id.to_string()[..8]);
    let spawn_result = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
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
            let result = scan_root_incremental(
                Some(root.id),
                &root.path,
                &options,
                &cancellation,
                |batch| {
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
                },
            );
            match result {
                Ok(summary) => {
                    let _ = on_event.send(LibraryScanEvent::Finished { scan_id, summary });
                }
                Err(error) => {
                    let _ = on_event.send(LibraryScanEvent::Failed {
                        scan_id,
                        message: error.to_string(),
                    });
                }
            }
            coordinator.finish(scan_id);
        });
    if let Err(error) = spawn_result {
        scans.finish(scan_id);
        return Err(format!("failed to start library scan thread: {error}"));
    }
    Ok(scan_id)
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
fn edit_asset_metadata(
    input: BatchMetadataEdit,
    catalog: State<'_, Arc<Mutex<AssetCatalog>>>,
) -> Result<BatchMetadataEditResult, String> {
    catalog
        .lock()
        .map_err(|_| "asset catalog lock is poisoned".to_owned())?
        .edit_metadata(&input)
        .map_err(|error| error.to_string())
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

/// Starts the desktop shell and blocks until its final window closes.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application event loop.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("library-roots.yml");
            let roots = LibraryRootManager::open(config_path)?;
            app.manage(Mutex::new(roots));
            app.manage(Arc::new(ScanCoordinator::default()));
            app.manage(Arc::new(Mutex::new(AssetCatalog::default())));
            let cache_directory = app.path().app_cache_dir()?;
            app.manage(Arc::new(ThumbnailService::open(&cache_directory, 4)?));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            build_info,
            list_library_roots,
            add_library_root,
            update_library_root,
            remove_library_root,
            start_library_scan,
            cancel_library_scan,
            edit_asset_metadata,
            query_assets,
            request_thumbnail,
            read_thumbnail,
            clear_thumbnail_cache
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Material Eagle desktop application");
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use asset_index::{QueryParseError, QueryParseErrorKind};

    use super::{
        LibraryScanEvent, QueryAssetsError, ScanCancellation, ScanCoordinator,
        ThumbnailCommandError, build_info,
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
        let cancellation = ScanCancellation::new();
        coordinator
            .register(scan_id, cancellation.clone())
            .expect("register scan");

        assert!(coordinator.cancel(scan_id).expect("cancel scan"));
        assert!(cancellation.is_cancelled());
        coordinator.finish(scan_id);
        assert!(!coordinator.cancel(scan_id).expect("cancel finished scan"));
    }

    #[test]
    fn scan_events_use_the_frontend_wire_shape() {
        let scan_id = Uuid::now_v7();
        let event = serde_json::to_value(LibraryScanEvent::Failed {
            scan_id,
            message: "invalid root".into(),
        })
        .expect("serialize scan event");

        assert_eq!(event["event"], "failed");
        assert_eq!(event["data"]["scanId"], scan_id.to_string());
        assert_eq!(event["data"]["message"], "invalid root");
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
    }
}
