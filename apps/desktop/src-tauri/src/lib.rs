use std::sync::Mutex;

use asset_filesystem::{
    AddLibraryRoot, LibraryRoot, LibraryRootManager, LibraryRootStatus, UpdateLibraryRoot,
};
use serde::Serialize;
use tauri::{Manager, State};
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            build_info,
            list_library_roots,
            add_library_root,
            update_library_root,
            remove_library_root
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Material Eagle desktop application");
}

#[cfg(test)]
mod tests {
    use super::build_info;

    #[test]
    fn build_information_is_traceable() {
        let info = build_info();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.git_commit.is_empty());
        assert!(!info.build_target.is_empty());
        assert!(!info.rustc_version.is_empty());
    }
}
