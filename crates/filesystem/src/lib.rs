mod events;
mod formats;
mod isobmff;
mod library;
mod platform;
mod reconciliation;
mod scanner;

pub use events::{
    FsChange, FsChangeBatch, FsChangeKind, FsRescanReason, WatchOptions, WatchSession,
    coalesce_changes, normalize_event,
};
pub use formats::{
    FORMAT_REGISTRY, FormatDescriptor, FormatRecognition, MAX_SIGNATURE_BYTES, RecognitionSource,
    descriptor_for_extension, recognize_format,
};
pub use library::{
    AddLibraryRoot, LibraryConfig, LibraryRoot, LibraryRootError, LibraryRootManager,
    LibraryRootStatus, RootAccessStatus, RootOverlapKind, RootScanSettings, UpdateLibraryRoot,
    inspect_root_access,
};
pub use platform::{
    PathCompatibilityIssue, PathCompatibilityIssueKind, PathRelation, PlatformFamily,
    inspect_relative_path_compatibility, path_relation_for_platform, platform_path_key,
};
pub use reconciliation::{
    MissingAsset, OrphanSidecar, OrphanSidecarState, ReconciliationError, ReconciliationReport,
    RelinkCandidate, RelinkReceipt, SyncConflictCopy, SyncConflictSource, apply_relink,
    inspect_reconciliation,
};
pub use scanner::{
    FilesystemError, ScanBatch, ScanCancellation, ScanCompletion, ScanOptions, ScanProblem,
    ScanReport, ScanSummary, scan_root, scan_root_incremental, scan_root_incremental_controlled,
};
