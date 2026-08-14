use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use asset_core::{AssetIssue, AssetKind, AssetRecord};
use metadata::{read_sidecar, sidecar_path_for};
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

mod library;

pub use library::{
    AddLibraryRoot, LibraryConfig, LibraryRoot, LibraryRootError, LibraryRootManager,
    LibraryRootStatus, RootAccessStatus, RootOverlapKind, RootScanSettings, UpdateLibraryRoot,
    inspect_root_access,
};

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub recursive: bool,
    pub ignore_hidden: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            ignore_hidden: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProblem {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub root: PathBuf,
    pub assets: Vec<AssetRecord>,
    pub problems: Vec<ScanProblem>,
    pub visited_files: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Error)]
pub enum FilesystemError {
    #[error("scan root does not exist or is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("cannot canonicalize scan root {path}: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("file watcher error: {0}")]
    Watch(#[from] notify::Error),
}

/// Scans one authorized root without following symbolic links.
///
/// # Errors
///
/// Returns [`FilesystemError`] when the root is invalid or cannot be canonicalized.
pub fn scan_root(root: &Path, options: &ScanOptions) -> Result<ScanReport, FilesystemError> {
    if !root.is_dir() {
        return Err(FilesystemError::InvalidRoot(root.to_path_buf()));
    }
    let root = root
        .canonicalize()
        .map_err(|source| FilesystemError::Canonicalize {
            path: root.to_path_buf(),
            source,
        })?;
    let started = Instant::now();
    let mut problems = Vec::new();
    let mut paths = Vec::new();

    let mut walker = WalkDir::new(&root).follow_links(false);
    if !options.recursive {
        walker = walker.max_depth(1);
    }

    for entry in walker.into_iter().filter_entry(|entry| {
        !options.ignore_hidden || entry.depth() == 0 || !is_hidden(entry.file_name())
    }) {
        match entry {
            Ok(entry) if entry.file_type().is_file() => {
                let path = entry.into_path();
                if !is_metadata_file(&path) {
                    paths.push(path);
                }
            }
            Ok(_) => {}
            Err(error) => problems.push(ScanProblem {
                path: error.path().map_or_else(|| root.clone(), Path::to_path_buf),
                message: error.to_string(),
            }),
        }
    }

    let visited_files = paths.len();
    let parsed = paths
        .par_iter()
        .map(|path| parse_asset(path))
        .collect::<Vec<_>>();
    let mut assets = Vec::new();
    for result in parsed {
        match result {
            Ok(Some(asset)) => assets.push(asset),
            Ok(None) => {}
            Err(problem) => problems.push(problem),
        }
    }
    assets.sort_by(|left, right| left.key.cmp(&right.key));

    Ok(ScanReport {
        root,
        assets,
        problems,
        visited_files,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

fn parse_asset(path: &Path) -> Result<Option<AssetRecord>, ScanProblem> {
    let file_metadata = fs::metadata(path).map_err(|error| ScanProblem {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mime = detect_mime(path);
    if AssetKind::from_mime(&mime) == AssetKind::Other {
        return Ok(None);
    }
    let canonical = path.canonicalize().map_err(|error| ScanProblem {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let key = canonical.to_string_lossy().into_owned();
    let modified_unix_ms = file_metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        });
    let mut asset =
        AssetRecord::untagged(key, canonical, mime, file_metadata.len(), modified_unix_ms);
    let sidecar_path = sidecar_path_for(path);
    if sidecar_path.is_file() {
        asset.sidecar_path = Some(sidecar_path.clone());
        match read_sidecar(&sidecar_path) {
            Ok((sidecar, _)) => {
                asset.id = Some(sidecar.id);
                asset.tags = sidecar.tags;
                asset.rating = sidecar.rating;
                asset.favorite = sidecar.favorite;
                asset.note = sidecar.note;
                asset.aliases = sidecar.aliases;
            }
            Err(error) => asset
                .issues
                .push(AssetIssue::InvalidSidecar(error.to_string())),
        }
    }
    Ok(Some(asset))
}

fn detect_mime(path: &Path) -> String {
    if let Ok(Some(kind)) = infer::get_from_path(path) {
        return kind.mime_type().to_owned();
    }
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("avif") => "image/avif",
        Some("heic" | "heif") => "image/heic",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn is_metadata_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    name.ends_with(".asset.yml") || name == ".asset-library.yml"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FsChangeKind {
    Create,
    Modify,
    Move,
    Delete,
    RescanRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsChange {
    pub kind: FsChangeKind,
    pub paths: Vec<PathBuf>,
}

pub struct WatchSession {
    _watcher: RecommendedWatcher,
    receiver: Receiver<notify::Result<Event>>,
}

impl WatchSession {
    /// Starts a recursive operating-system watcher for an authorized root.
    ///
    /// # Errors
    ///
    /// Returns [`FilesystemError`] when the root is invalid or the platform watcher fails.
    pub fn start(root: &Path) -> Result<Self, FilesystemError> {
        if !root.is_dir() {
            return Err(FilesystemError::InvalidRoot(root.to_path_buf()));
        }
        let (sender, receiver) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |result| {
                let _ = sender.send(result);
            },
            Config::default(),
        )?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            receiver,
        })
    }

    /// Waits for one normalized change until the supplied timeout expires.
    ///
    /// # Errors
    ///
    /// Returns [`FilesystemError`] when the platform watcher reports an error.
    pub fn next_timeout(&self, timeout: Duration) -> Result<Option<FsChange>, FilesystemError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(Ok(event)) => Ok(Some(normalize_event(&event))),
            Ok(Err(error)) => Err(FilesystemError::Watch(error)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Ok(Some(FsChange {
                kind: FsChangeKind::RescanRequired,
                paths: Vec::new(),
            })),
        }
    }
}

#[must_use]
pub fn normalize_event(event: &Event) -> FsChange {
    let kind = match event.kind {
        EventKind::Create(_) => FsChangeKind::Create,
        EventKind::Remove(_) => FsChangeKind::Delete,
        EventKind::Modify(ModifyKind::Name(
            RenameMode::Any | RenameMode::Both | RenameMode::From | RenameMode::To,
        )) => FsChangeKind::Move,
        EventKind::Modify(_) | EventKind::Access(_) => FsChangeKind::Modify,
        EventKind::Other | EventKind::Any => FsChangeKind::RescanRequired,
    };
    FsChange {
        kind,
        paths: event.paths.clone(),
    }
}

#[allow(dead_code)]
fn system_time_to_unix_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |duration| {
        i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use metadata::{AssetSidecar, ExpectedVersion, sidecar_path_for, write_sidecar_atomic};
    use tempfile::tempdir;

    use super::{ScanOptions, scan_root};

    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5,
        0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn scans_assets_and_merges_sidecar_metadata() {
        let directory = tempdir().expect("tempdir");
        let image = directory.path().join("logo.png");
        fs::write(&image, PNG).expect("write png");
        fs::write(directory.path().join("ignored.txt"), "not an asset").expect("write text");
        let mut sidecar = AssetSidecar::new();
        sidecar.tags.insert("ui/icon".into());
        write_sidecar_atomic(
            &sidecar_path_for(&image),
            &sidecar,
            &ExpectedVersion::Missing,
        )
        .expect("write sidecar");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan");
        assert_eq!(report.assets.len(), 1);
        assert_eq!(report.assets[0].id, Some(sidecar.id));
        assert!(report.assets[0].tags.contains("ui/icon"));
        assert!(report.problems.is_empty());
    }

    #[test]
    fn isolates_a_broken_sidecar() {
        let directory = tempdir().expect("tempdir");
        let image = directory.path().join("logo.png");
        fs::write(&image, PNG).expect("write png");
        fs::write(sidecar_path_for(&image), "not: [valid").expect("write broken sidecar");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan");
        assert_eq!(report.assets.len(), 1);
        assert_eq!(report.assets[0].issues.len(), 1);
    }
}
