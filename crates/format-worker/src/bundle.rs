use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    LIBHEIF_PROVIDER_ID, LIBHEIF_PROVIDER_VERSION, WorkerClient, WorkerRunError, WorkerSpec,
};

pub const WORKER_BUNDLE_SCHEMA: u32 = 1;
pub const WORKER_BUNDLE_MANIFEST: &str = "manifest.json";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
const BUNDLED_WORKER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerBundleManifest {
    pub schema: u32,
    pub platform: String,
    pub architecture: String,
    pub provider_id: String,
    pub provider_version: String,
    pub executable: String,
    pub sha256: String,
}

#[derive(Debug, Error)]
pub enum WorkerBundleError {
    #[error("worker bundle path is invalid")]
    InvalidPath,
    #[error("worker bundle I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("worker bundle manifest is invalid: {0}")]
    InvalidManifest(&'static str),
    #[error("worker bundle manifest JSON is invalid")]
    InvalidJson(#[source] serde_json::Error),
    #[error("worker bundle client could not be opened: {0}")]
    Client(#[from] WorkerRunError),
}

/// Opens one platform-specific worker bundle after validating its manifest and binary digest.
///
/// # Errors
///
/// Rejects symbolic links, oversized or mismatched manifests, non-leaf executable names, and
/// worker binaries whose SHA-256 does not match the declared digest.
pub fn open_libheif_worker_bundle(directory: &Path) -> Result<WorkerClient, WorkerBundleError> {
    if !directory.is_absolute() || fs::symlink_metadata(directory)?.file_type().is_symlink() {
        return Err(WorkerBundleError::InvalidPath);
    }
    let directory = fs::canonicalize(directory)?;
    if !directory.is_dir() {
        return Err(WorkerBundleError::InvalidPath);
    }
    let manifest_path = directory.join(WORKER_BUNDLE_MANIFEST);
    if fs::symlink_metadata(&manifest_path)?
        .file_type()
        .is_symlink()
    {
        return Err(WorkerBundleError::InvalidPath);
    }
    let metadata = fs::metadata(&manifest_path)?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(WorkerBundleError::InvalidManifest("manifest size"));
    }
    let manifest: WorkerBundleManifest = serde_json::from_slice(&fs::read(&manifest_path)?)
        .map_err(WorkerBundleError::InvalidJson)?;
    validate_manifest(&manifest)?;
    let executable = directory.join(&manifest.executable);
    let mut spec = WorkerSpec::new(
        executable,
        directory,
        manifest.sha256,
        manifest.provider_id,
        manifest.provider_version,
    );
    spec.timeout = BUNDLED_WORKER_TIMEOUT;
    let client = WorkerClient::open(spec)?;
    Ok(client)
}

fn validate_manifest(manifest: &WorkerBundleManifest) -> Result<(), WorkerBundleError> {
    if manifest.schema != WORKER_BUNDLE_SCHEMA {
        return Err(WorkerBundleError::InvalidManifest("schema"));
    }
    if manifest.platform != std::env::consts::OS {
        return Err(WorkerBundleError::InvalidManifest("platform"));
    }
    if manifest.architecture != std::env::consts::ARCH {
        return Err(WorkerBundleError::InvalidManifest("architecture"));
    }
    if manifest.provider_id != LIBHEIF_PROVIDER_ID
        || manifest.provider_version != LIBHEIF_PROVIDER_VERSION
    {
        return Err(WorkerBundleError::InvalidManifest("provider identity"));
    }
    let executable = PathBuf::from(&manifest.executable);
    if manifest.executable.is_empty()
        || executable.is_absolute()
        || executable.components().count() != 1
        || !matches!(executable.components().next(), Some(Component::Normal(_)))
    {
        return Err(WorkerBundleError::InvalidManifest("executable name"));
    }
    if manifest.sha256.len() != 64
        || !manifest
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(WorkerBundleError::InvalidManifest("SHA-256"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::digest_file_sha256;

    #[test]
    fn opens_an_exact_platform_bundle_and_rejects_manifest_drift() {
        let temporary = tempdir().expect("tempdir");
        let bundle = temporary.path().join("bundle");
        fs::create_dir(&bundle).expect("bundle directory");
        let executable = bundle.join("worker");
        fs::write(&executable, b"fixed worker fixture").expect("worker fixture");
        let manifest = WorkerBundleManifest {
            schema: WORKER_BUNDLE_SCHEMA,
            platform: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
            provider_id: LIBHEIF_PROVIDER_ID.into(),
            provider_version: LIBHEIF_PROVIDER_VERSION.into(),
            executable: "worker".into(),
            sha256: digest_file_sha256(&executable).expect("worker digest"),
        };
        fs::write(
            bundle.join(WORKER_BUNDLE_MANIFEST),
            serde_json::to_vec(&manifest).expect("manifest JSON"),
        )
        .expect("manifest");

        let client = open_libheif_worker_bundle(&bundle).expect("open worker bundle");
        assert_eq!(client.provider_id(), LIBHEIF_PROVIDER_ID);
        assert_eq!(client.provider_version(), LIBHEIF_PROVIDER_VERSION);

        let mut wrong_platform = manifest;
        wrong_platform.platform = "different-platform".into();
        fs::write(
            bundle.join(WORKER_BUNDLE_MANIFEST),
            serde_json::to_vec(&wrong_platform).expect("manifest JSON"),
        )
        .expect("manifest");
        assert!(matches!(
            open_libheif_worker_bundle(&bundle),
            Err(WorkerBundleError::InvalidManifest("platform"))
        ));
    }

    #[test]
    fn rejects_traversal_unknown_fields_and_symlinked_manifests() {
        let temporary = tempdir().expect("tempdir");
        let bundle = temporary.path().join("bundle");
        fs::create_dir(&bundle).expect("bundle directory");
        let manifest_path = bundle.join(WORKER_BUNDLE_MANIFEST);
        let invalid = serde_json::json!({
            "schema": WORKER_BUNDLE_SCHEMA,
            "platform": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "providerId": LIBHEIF_PROVIDER_ID,
            "providerVersion": LIBHEIF_PROVIDER_VERSION,
            "executable": "../worker",
            "sha256": "0".repeat(64),
            "unexpected": true
        });
        fs::write(
            &manifest_path,
            serde_json::to_vec(&invalid).expect("manifest JSON"),
        )
        .expect("manifest");
        assert!(matches!(
            open_libheif_worker_bundle(&bundle),
            Err(WorkerBundleError::InvalidJson(_))
        ));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let target = temporary.path().join("manifest-target.json");
            fs::write(&target, b"{}").expect("manifest target");
            fs::remove_file(&manifest_path).expect("remove manifest");
            symlink(&target, &manifest_path).expect("manifest symlink");
            assert!(matches!(
                open_libheif_worker_bundle(&bundle),
                Err(WorkerBundleError::InvalidPath)
            ));
        }
    }
}
