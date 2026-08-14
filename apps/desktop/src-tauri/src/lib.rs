use serde::Serialize;

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

/// Starts the desktop shell and blocks until its final window closes.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application event loop.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![build_info])
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
