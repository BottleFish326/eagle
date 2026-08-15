mod events;
mod library;
mod scanner;

pub use events::{
    FsChange, FsChangeBatch, FsChangeKind, FsRescanReason, WatchOptions, WatchSession,
    coalesce_changes, normalize_event,
};
pub use library::{
    AddLibraryRoot, LibraryConfig, LibraryRoot, LibraryRootError, LibraryRootManager,
    LibraryRootStatus, RootAccessStatus, RootOverlapKind, RootScanSettings, UpdateLibraryRoot,
    inspect_root_access,
};
pub use scanner::{
    FilesystemError, ScanBatch, ScanCancellation, ScanCompletion, ScanOptions, ScanProblem,
    ScanReport, ScanSummary, scan_root, scan_root_incremental,
};
