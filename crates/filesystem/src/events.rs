use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode, ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

use crate::FilesystemError;

const DEFAULT_SETTLE_TIME: Duration = Duration::from_millis(120);
const DEFAULT_MAX_BATCH_LATENCY: Duration = Duration::from_millis(750);
const DEFAULT_MAX_BATCH_EVENTS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FsChangeKind {
    Create,
    Modify,
    Move,
    Delete,
    RescanRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FsRescanReason {
    AmbiguousRename,
    BatchOverflow,
    ChannelDisconnected,
    OutOfScope,
    UnknownEvent,
    WatcherError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChange {
    pub kind: FsChangeKind,
    pub paths: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<FsRescanReason>,
}

impl FsChange {
    fn rescan(root: &Path, reason: FsRescanReason) -> Self {
        Self {
            kind: FsChangeKind::RescanRequired,
            paths: vec![root.to_path_buf()],
            reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChangeBatch {
    pub root: PathBuf,
    pub changes: Vec<FsChange>,
    pub raw_event_count: usize,
}

impl FsChangeBatch {
    #[must_use]
    pub fn requires_rescan(&self) -> bool {
        self.changes
            .iter()
            .any(|change| change.kind == FsChangeKind::RescanRequired)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchOptions {
    pub settle_time: Duration,
    pub max_batch_latency: Duration,
    pub max_batch_events: usize,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            settle_time: DEFAULT_SETTLE_TIME,
            max_batch_latency: DEFAULT_MAX_BATCH_LATENCY,
            max_batch_events: DEFAULT_MAX_BATCH_EVENTS,
        }
    }
}

pub struct WatchSession {
    root: PathBuf,
    _watcher: RecommendedWatcher,
    receiver: Receiver<notify::Result<Event>>,
    options: WatchOptions,
}

impl WatchSession {
    /// Starts a recursive operating-system watcher for an authorized root.
    ///
    /// # Errors
    ///
    /// Returns [`FilesystemError`] when the root is invalid, the batch bound is zero,
    /// or the platform watcher fails to start.
    pub fn start(root: &Path) -> Result<Self, FilesystemError> {
        Self::start_with_options(root, WatchOptions::default())
    }

    /// Starts a watcher with explicit batching options.
    ///
    /// # Errors
    ///
    /// Returns [`FilesystemError`] when the root is invalid, the batch bound is zero,
    /// or the platform watcher fails to start.
    pub fn start_with_options(root: &Path, options: WatchOptions) -> Result<Self, FilesystemError> {
        if options.max_batch_events == 0 {
            return Err(FilesystemError::InvalidWatchBatchSize);
        }
        if !root.is_dir() {
            return Err(FilesystemError::InvalidRoot(root.to_path_buf()));
        }
        let root = root
            .canonicalize()
            .map_err(|source| FilesystemError::Canonicalize {
                path: root.to_path_buf(),
                source,
            })?;
        let (sender, receiver) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |result| {
                let _ = sender.send(result);
            },
            Config::default(),
        )?;
        watcher.watch(&root, RecursiveMode::Recursive)?;
        Ok(Self {
            root,
            _watcher: watcher,
            receiver,
            options,
        })
    }

    /// Waits for one bounded, settled and coalesced change batch.
    ///
    /// Runtime watcher errors, channel loss and batch overflow are returned as a
    /// root-scoped `rescan-required` change instead of aborting the application.
    ///
    /// # Errors
    ///
    /// This method reserves a result for compatibility with watcher startup errors;
    /// runtime loss is represented in the returned batch.
    pub fn next_batch_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Option<FsChangeBatch>, FilesystemError> {
        let first = match self.receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => return Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                return Ok(Some(rescan_batch(
                    &self.root,
                    FsRescanReason::ChannelDisconnected,
                    0,
                )));
            }
        };
        let mut raw_event_count = 1;
        let mut changes = Vec::new();
        collect_result(&self.root, first, &mut changes);
        let deadline = Instant::now() + self.options.max_batch_latency;

        while raw_event_count < self.options.max_batch_events {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self
                .receiver
                .recv_timeout(self.options.settle_time.min(remaining))
            {
                Ok(result) => {
                    raw_event_count += 1;
                    collect_result(&self.root, result, &mut changes);
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    return Ok(Some(rescan_batch(
                        &self.root,
                        FsRescanReason::ChannelDisconnected,
                        raw_event_count,
                    )));
                }
            }
        }

        if raw_event_count == self.options.max_batch_events {
            return Ok(Some(rescan_batch(
                &self.root,
                FsRescanReason::BatchOverflow,
                raw_event_count,
            )));
        }
        let batch = coalesce_changes(&self.root, changes, raw_event_count);
        Ok((!batch.changes.is_empty()).then_some(batch))
    }
}

fn collect_result(root: &Path, result: notify::Result<Event>, changes: &mut Vec<FsChange>) {
    match result {
        Ok(event) => {
            if let Some(change) = normalize_event(&event) {
                changes.push(change);
            }
        }
        Err(_) => changes.push(FsChange::rescan(root, FsRescanReason::WatcherError)),
    }
}

/// Converts one platform event into the shared event vocabulary.
/// Access-only events are ignored. A rename without both endpoints is unsafe for
/// incremental application and therefore requests a bounded consistency scan.
#[must_use]
pub fn normalize_event(event: &Event) -> Option<FsChange> {
    let (kind, reason) = match event.kind {
        EventKind::Create(_) => (FsChangeKind::Create, None),
        EventKind::Remove(_) => (FsChangeKind::Delete, None),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
            (FsChangeKind::Move, None)
        }
        EventKind::Modify(ModifyKind::Name(_)) => (
            FsChangeKind::RescanRequired,
            Some(FsRescanReason::AmbiguousRename),
        ),
        EventKind::Modify(_) => (FsChangeKind::Modify, None),
        EventKind::Access(AccessKind::Close(AccessMode::Write | AccessMode::Any)) => {
            (FsChangeKind::Modify, None)
        }
        EventKind::Access(_) => return None,
        EventKind::Other | EventKind::Any => (
            FsChangeKind::RescanRequired,
            Some(FsRescanReason::UnknownEvent),
        ),
    };
    Some(FsChange {
        kind,
        paths: event.paths.clone(),
        reason,
    })
}

/// Deduplicates a raw batch, folds common atomic-save temporary file sequences,
/// and rejects paths outside the authorized root. The output is deterministic.
#[must_use]
pub fn coalesce_changes(
    root: &Path,
    changes: impl IntoIterator<Item = FsChange>,
    raw_event_count: usize,
) -> FsChangeBatch {
    let changes = changes.into_iter().collect::<Vec<_>>();
    if let Some(rescan) = changes
        .iter()
        .find(|change| change.kind == FsChangeKind::RescanRequired)
    {
        return rescan_batch(
            root,
            rescan.reason.unwrap_or(FsRescanReason::UnknownEvent),
            raw_event_count,
        );
    }

    let mut point_changes = BTreeMap::<PathBuf, FsChangeKind>::new();
    let mut moves = BTreeSet::<(PathBuf, PathBuf)>::new();
    for change in changes {
        if change.paths.is_empty() {
            return rescan_batch(root, FsRescanReason::UnknownEvent, raw_event_count);
        }
        if change.paths.iter().any(|path| !path_is_in_root(root, path)) {
            return rescan_batch(root, FsRescanReason::OutOfScope, raw_event_count);
        }
        if change.kind == FsChangeKind::Move {
            if change.paths.len() != 2 {
                return rescan_batch(root, FsRescanReason::AmbiguousRename, raw_event_count);
            }
            let from = change.paths[0].clone();
            let to = change.paths[1].clone();
            let from_temporary = is_temporary_path(&from);
            let to_temporary = is_temporary_path(&to);
            match (from_temporary, to_temporary) {
                (true, true) => {}
                (true, false) => apply_point_change(&mut point_changes, to, FsChangeKind::Modify),
                (false, true) => apply_point_change(&mut point_changes, from, FsChangeKind::Delete),
                (false, false) => {
                    point_changes.remove(&from);
                    point_changes.remove(&to);
                    moves.insert((from, to));
                }
            }
            continue;
        }
        for path in change.paths {
            if is_temporary_path(&path) {
                continue;
            }
            apply_point_change(&mut point_changes, path, change.kind);
        }
    }

    let mut coalesced = moves
        .into_iter()
        .map(|(from, to)| FsChange {
            kind: FsChangeKind::Move,
            paths: vec![from, to],
            reason: None,
        })
        .collect::<Vec<_>>();
    coalesced.extend(point_changes.into_iter().map(|(path, kind)| FsChange {
        kind,
        paths: vec![path],
        reason: None,
    }));
    FsChangeBatch {
        root: root.to_path_buf(),
        changes: coalesced,
        raw_event_count,
    }
}

fn apply_point_change(
    changes: &mut BTreeMap<PathBuf, FsChangeKind>,
    path: PathBuf,
    next: FsChangeKind,
) {
    let previous = changes.get(&path).copied();
    let folded = match (previous, next) {
        (None, kind) => Some(kind),
        (Some(FsChangeKind::Create), FsChangeKind::Delete) => None,
        (Some(FsChangeKind::Create), _) | (Some(FsChangeKind::Modify), FsChangeKind::Create) => {
            Some(FsChangeKind::Create)
        }
        (Some(FsChangeKind::Modify), FsChangeKind::Delete) => Some(FsChangeKind::Delete),
        (Some(FsChangeKind::Modify), _) => Some(FsChangeKind::Modify),
        (Some(FsChangeKind::Delete), FsChangeKind::Create | FsChangeKind::Modify) => {
            Some(FsChangeKind::Modify)
        }
        (Some(FsChangeKind::Delete), _) => Some(FsChangeKind::Delete),
        (Some(FsChangeKind::Move | FsChangeKind::RescanRequired), _) => Some(next),
    };
    if let Some(kind) = folded {
        changes.insert(path, kind);
    } else {
        changes.remove(&path);
    }
}

fn path_is_in_root(root: &Path, path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| component == Component::ParentDir)
        && path.starts_with(root)
}

fn is_temporary_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    name.starts_with(".tmp")
        || name.starts_with("~$")
        || name.ends_with('~')
        || [
            ".tmp",
            ".temp",
            ".part",
            ".crdownload",
            ".download",
            ".swp",
            ".swx",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

fn rescan_batch(root: &Path, reason: FsRescanReason, raw_event_count: usize) -> FsChangeBatch {
    FsChangeBatch {
        root: root.to_path_buf(),
        changes: vec![FsChange::rescan(root, reason)],
        raw_event_count,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode,
    };
    use notify::{Event, EventKind};
    use tempfile::tempdir;

    use super::{
        FsChange, FsChangeKind, FsRescanReason, WatchOptions, WatchSession, coalesce_changes,
        normalize_event,
    };

    #[test]
    fn normalizes_platform_events_and_ignores_reads() {
        let create = Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/library/new.png"));
        let modify = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(PathBuf::from("/library/new.png"));
        let remove = Event::new(EventKind::Remove(RemoveKind::File))
            .add_path(PathBuf::from("/library/new.png"));
        let access = Event::new(EventKind::Access(AccessKind::Read))
            .add_path(PathBuf::from("/library/new.png"));
        let close_write = Event::new(EventKind::Access(AccessKind::Close(AccessMode::Write)))
            .add_path(PathBuf::from("/library/new.png"));

        assert_eq!(
            normalize_event(&create).expect("create").kind,
            FsChangeKind::Create
        );
        assert_eq!(
            normalize_event(&modify).expect("modify").kind,
            FsChangeKind::Modify
        );
        assert_eq!(
            normalize_event(&remove).expect("remove").kind,
            FsChangeKind::Delete
        );
        assert!(normalize_event(&access).is_none());
        assert_eq!(
            normalize_event(&close_write).expect("close write").kind,
            FsChangeKind::Modify
        );
    }

    #[test]
    fn default_watch_batch_is_bounded_by_time_and_count() {
        let options = WatchOptions::default();

        assert_eq!(options.settle_time, Duration::from_millis(120));
        assert_eq!(options.max_batch_latency, Duration::from_millis(750));
        assert_eq!(options.max_batch_events, 4_096);
    }

    #[test]
    fn requires_rescan_for_a_rename_without_both_endpoints() {
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
            .add_path(PathBuf::from("/library/old.png"));
        let change = normalize_event(&event).expect("ambiguous rename");

        assert_eq!(change.kind, FsChangeKind::RescanRequired);
        assert_eq!(change.reason, Some(FsRescanReason::AmbiguousRename));
    }

    #[test]
    fn folds_duplicate_and_atomic_save_events_deterministically() {
        let root = PathBuf::from("/library");
        let target = root.join("asset.png.asset.yml");
        let temporary = root.join(".tmpABCD");
        let batch = coalesce_changes(
            &root,
            [
                change(FsChangeKind::Create, temporary.clone()),
                change(FsChangeKind::Modify, temporary.clone()),
                move_change(temporary, target.clone()),
                change(FsChangeKind::Modify, target.clone()),
                change(FsChangeKind::Modify, target.clone()),
            ],
            5,
        );

        assert_eq!(batch.raw_event_count, 5);
        assert_eq!(batch.changes, [change(FsChangeKind::Modify, target)]);
    }

    #[test]
    fn cancels_ephemeral_creates_and_treats_replacement_as_modify() {
        let root = PathBuf::from("/library");
        let transient = root.join("transient.png");
        let replaced = root.join("replaced.png");
        let batch = coalesce_changes(
            &root,
            [
                change(FsChangeKind::Create, transient.clone()),
                change(FsChangeKind::Delete, transient),
                change(FsChangeKind::Delete, replaced.clone()),
                change(FsChangeKind::Create, replaced.clone()),
            ],
            4,
        );

        assert_eq!(batch.changes, [change(FsChangeKind::Modify, replaced)]);
    }

    #[test]
    fn preserves_an_unambiguous_move_once() {
        let root = PathBuf::from("/library");
        let old = root.join("old.png");
        let new = root.join("new.png");
        let batch = coalesce_changes(
            &root,
            [
                move_change(old.clone(), new.clone()),
                move_change(old.clone(), new.clone()),
            ],
            2,
        );

        assert_eq!(batch.changes, [move_change(old, new)]);
    }

    #[test]
    fn rejects_paths_outside_the_authorized_root() {
        let root = PathBuf::from("/library");
        let batch = coalesce_changes(
            &root,
            [change(
                FsChangeKind::Create,
                PathBuf::from("/private/escape.png"),
            )],
            1,
        );

        assert!(batch.requires_rescan());
        assert_eq!(
            batch.changes[0].paths.as_slice(),
            std::slice::from_ref(&root)
        );
        assert_eq!(batch.changes[0].reason, Some(FsRescanReason::OutOfScope));

        let traversal = coalesce_changes(
            &root,
            [change(
                FsChangeKind::Modify,
                PathBuf::from("/library/../private/escape.png"),
            )],
            1,
        );
        assert_eq!(
            traversal.changes[0].reason,
            Some(FsRescanReason::OutOfScope)
        );
    }

    #[test]
    fn live_watcher_overflow_requests_only_its_canonical_root() {
        let directory = tempdir().expect("tempdir");
        let session = WatchSession::start_with_options(
            directory.path(),
            WatchOptions {
                settle_time: Duration::from_millis(20),
                max_batch_events: 1,
                ..WatchOptions::default()
            },
        )
        .expect("start watcher");
        fs::write(directory.path().join("new.png"), b"image").expect("write watched file");

        let batch = session
            .next_batch_timeout(Duration::from_secs(5))
            .expect("watch batch")
            .expect("filesystem event");

        assert_eq!(batch.root, directory.path().canonicalize().expect("root"));
        assert_eq!(batch.raw_event_count, 1);
        assert_eq!(batch.changes[0].reason, Some(FsRescanReason::BatchOverflow));
        assert_eq!(
            batch.changes[0].paths.as_slice(),
            std::slice::from_ref(&batch.root)
        );
    }

    fn change(kind: FsChangeKind, path: PathBuf) -> FsChange {
        FsChange {
            kind,
            paths: vec![path],
            reason: None,
        }
    }

    fn move_change(from: PathBuf, to: PathBuf) -> FsChange {
        FsChange {
            kind: FsChangeKind::Move,
            paths: vec![from, to],
            reason: None,
        }
    }
}
