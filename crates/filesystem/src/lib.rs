use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

mod library;
mod scanner;

pub use library::{
    AddLibraryRoot, LibraryConfig, LibraryRoot, LibraryRootError, LibraryRootManager,
    LibraryRootStatus, RootAccessStatus, RootOverlapKind, RootScanSettings, UpdateLibraryRoot,
    inspect_root_access,
};
pub use scanner::{
    FilesystemError, ScanBatch, ScanCancellation, ScanCompletion, ScanOptions, ScanProblem,
    ScanReport, ScanSummary, scan_root, scan_root_incremental,
};

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
