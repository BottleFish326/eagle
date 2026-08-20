use std::collections::{BTreeMap, BTreeSet};
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, FileTimes, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{RwLock, RwLockReadGuard};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::{
    CacheClearReport, CacheMaintenanceReport, CachePolicy, CacheStartupDisposition,
    CacheStartupReport, CacheStats, PreviewError, PreviewProviderIdentity,
    THUMBNAIL_CACHE_LAYOUT_VERSION, THUMBNAIL_DECODER_VERSION, is_current_preview_provider,
};

const CACHE_DIRECTORY: &str = "thumbnails-v1";
const CACHE_MARKER: &str = ".material-eagle-thumbnail-cache";
const CACHE_MARKER_CONTENT: &str = "material-eagle-thumbnail-cache-v3\n";
const TOMBSTONE_PREFIX: &str = ".material-eagle-thumbnail-cache-gc-";
const TOMBSTONE_MARKER: &str = ".material-eagle-thumbnail-cache-tombstone";
const TOMBSTONE_MARKER_CONTENT: &str = "material-eagle-thumbnail-cache-tombstone-v1\n";
const ENTRY_SCHEMA_VERSION: u32 = 2;

#[derive(Debug)]
pub(crate) struct ThumbnailCache {
    base: PathBuf,
    root: PathBuf,
    gate: RwLock<()>,
    policy: CachePolicy,
    stores_since_maintenance: AtomicU64,
    estimated_entries: AtomicU64,
    estimated_bytes: AtomicU64,
}

impl ThumbnailCache {
    pub(crate) fn open(
        base: &Path,
        policy: CachePolicy,
    ) -> Result<(Self, CacheStartupReport), PreviewError> {
        fs::create_dir_all(base).map_err(|source| PreviewError::CacheIo {
            path: base.to_path_buf(),
            source,
        })?;
        let base = base
            .canonicalize()
            .map_err(|source| PreviewError::CacheIo {
                path: base.to_path_buf(),
                source,
            })?;
        let reaped = reap_tombstones(&base)?;
        let root = base.join(CACHE_DIRECTORY);
        let existed = match root.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(PreviewError::UnsafeCacheRoot(root));
            }
            Ok(_) => true,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => false,
            Err(source) => return Err(PreviewError::CacheIo { path: root, source }),
        };
        fs::create_dir_all(&root).map_err(|source| PreviewError::CacheIo {
            path: root.clone(),
            source,
        })?;
        verify_root(&base, &root)?;
        let mut startup = prepare_cache_root(&base, &root, existed)?;
        let cache = Self {
            base,
            root,
            gate: RwLock::new(()),
            policy,
            stores_since_maintenance: AtomicU64::new(0),
            estimated_entries: AtomicU64::new(0),
            estimated_bytes: AtomicU64::new(0),
        };
        let maintenance = cache.maintain(None)?;
        let lifecycle_files = reaped
            .removed_files
            .saturating_add(maintenance.removed_files);
        startup.removed_files = startup.removed_files.saturating_add(lifecycle_files);
        startup.removed_bytes = startup
            .removed_bytes
            .saturating_add(reaped.removed_bytes)
            .saturating_add(maintenance.removed_bytes);
        if matches!(
            startup.disposition,
            CacheStartupDisposition::Created | CacheStartupDisposition::Reused
        ) && lifecycle_files > 0
        {
            startup.disposition = CacheStartupDisposition::Maintained;
        }
        Ok((cache, startup))
    }

    pub(crate) fn read_guard(&self) -> Result<RwLockReadGuard<'_, ()>, PreviewError> {
        self.gate
            .read()
            .map_err(|_| PreviewError::PoisonedLock("thumbnail cache"))
    }

    pub(crate) fn lookup(
        &self,
        key: &str,
        source_token: &str,
        max_edge: u32,
        provider: PreviewProviderIdentity,
    ) -> Result<Option<CacheEntry>, PreviewError> {
        let path = self.path_for(key)?;
        let descriptor_path = metadata_path_for(&path);
        let Some(metadata) = entry_metadata(&path)? else {
            remove_regular_file_if_present(&descriptor_path)?;
            return Ok(None);
        };
        ensure_regular_entry(&path, &metadata)?;
        let descriptor = read_descriptor(&descriptor_path)?;
        if descriptor
            .as_ref()
            .is_none_or(|descriptor| !descriptor.matches(key, source_token, max_edge, provider))
        {
            remove_regular_file_if_present(&path)?;
            remove_regular_file_if_present(&descriptor_path)?;
            return Ok(None);
        }
        if let Ok((width, height)) = image::image_dimensions(&path) {
            touch_entry(&path)?;
            Ok(Some(CacheEntry { width, height }))
        } else {
            remove_regular_file_if_present(&path)?;
            remove_regular_file_if_present(&descriptor_path)?;
            Ok(None)
        }
    }

    pub(crate) fn store(
        &self,
        key: &str,
        source_token: &str,
        max_edge: u32,
        provider: PreviewProviderIdentity,
        bytes: &[u8],
    ) -> Result<PathBuf, PreviewError> {
        validate_key(key)?;
        validate_key(source_token)?;
        let path = self.path_for(key)?;
        let parent = path
            .parent()
            .ok_or_else(|| PreviewError::UnsafeCacheRoot(path.clone()))?;
        fs::create_dir_all(parent).map_err(|source| PreviewError::CacheIo {
            path: parent.to_path_buf(),
            source,
        })?;
        verify_shard(&self.root, parent)?;
        atomic_write(parent, &path, bytes)?;
        let descriptor = CacheEntryDescriptor {
            schema: ENTRY_SCHEMA_VERSION,
            cache_key: key.to_owned(),
            source_token: source_token.to_owned(),
            provider_id: provider.id.to_owned(),
            provider_version: provider.version.to_owned(),
            max_edge,
        };
        let descriptor_path = metadata_path_for(&path);
        let metadata =
            serde_json::to_vec(&descriptor).map_err(|error| PreviewError::CacheMetadata {
                path: descriptor_path.clone(),
                message: error.to_string(),
            })?;
        if let Err(error) = atomic_write(parent, &descriptor_path, &metadata) {
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        self.estimated_entries.fetch_add(1, Ordering::Relaxed);
        self.estimated_bytes.fetch_add(
            u64::try_from(bytes.len().saturating_add(metadata.len())).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        Ok(path)
    }

    pub(crate) fn read(&self, key: &str) -> Result<Vec<u8>, PreviewError> {
        let _guard = self.read_guard()?;
        let path = self.path_for(key)?;
        let Some(metadata) = entry_metadata(&path)? else {
            return Err(PreviewError::MissingCacheEntry(key.to_owned()));
        };
        ensure_regular_entry(&path, &metadata)?;
        let descriptor = read_descriptor(&metadata_path_for(&path))?;
        if descriptor.as_ref().is_none_or(|descriptor| {
            descriptor.schema != ENTRY_SCHEMA_VERSION
                || descriptor.cache_key != key
                || !is_current_preview_provider(
                    &descriptor.provider_id,
                    &descriptor.provider_version,
                )
        }) {
            return Err(PreviewError::MissingCacheEntry(key.to_owned()));
        }
        fs::read(&path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                PreviewError::MissingCacheEntry(key.to_owned())
            } else {
                PreviewError::CacheIo { path, source }
            }
        })
    }

    pub(crate) fn clear(&self) -> Result<CacheClearReport, PreviewError> {
        let _guard = self
            .gate
            .write()
            .map_err(|_| PreviewError::PoisonedLock("thumbnail cache"))?;
        verify_root(&self.base, &self.root)?;
        verify_marker(&self.root)?;
        let report = measure_cache(&self.root)?;
        rotate_cache_root(&self.base, &self.root)?;
        self.estimated_entries.store(0, Ordering::Relaxed);
        self.estimated_bytes.store(0, Ordering::Relaxed);
        Ok(report)
    }

    pub(crate) fn stats(&self) -> Result<CacheStats, PreviewError> {
        let _guard = self.read_guard()?;
        self.stats_locked()
    }

    pub(crate) fn maintain(
        &self,
        active_sources: Option<&BTreeSet<String>>,
    ) -> Result<CacheMaintenanceReport, PreviewError> {
        let _guard = self
            .gate
            .write()
            .map_err(|_| PreviewError::PoisonedLock("thumbnail cache"))?;
        self.stores_since_maintenance.store(0, Ordering::Relaxed);
        verify_root(&self.base, &self.root)?;
        verify_marker(&self.root)?;
        let mut report = CacheMaintenanceReport::default();
        let scan = scan_entries(&self.root)?;

        for garbage in scan.garbage {
            report.removed_entries = report.removed_entries.saturating_add(1);
            report.orphan_entries = report.orphan_entries.saturating_add(1);
            for path in garbage {
                add_removed(&mut report, remove_owned_path(&path)?);
            }
        }

        let now = SystemTime::now();
        let mut retained = Vec::new();
        for entry in scan.entries {
            let reason = if !is_current_preview_provider(
                &entry.descriptor.provider_id,
                &entry.descriptor.provider_version,
            ) {
                Some(RemovalReason::Incompatible)
            } else if active_sources
                .is_some_and(|sources| !sources.contains(&entry.descriptor.source_token))
            {
                Some(RemovalReason::Orphan)
            } else if now
                .duration_since(entry.last_used)
                .is_ok_and(|age| age > self.policy.max_age)
            {
                Some(RemovalReason::Expired)
            } else {
                None
            };
            if let Some(reason) = reason {
                let removed = remove_entry(&entry)?;
                record_reason(&mut report, reason);
                add_removed(&mut report, removed);
            } else {
                retained.push(entry);
            }
        }

        retained.sort_by(|left, right| {
            left.last_used
                .cmp(&right.last_used)
                .then_with(|| left.key.cmp(&right.key))
        });
        let mut retained_entries = u64::try_from(retained.len()).unwrap_or(u64::MAX);
        let mut retained_bytes = retained
            .iter()
            .fold(0_u64, |total, entry| total.saturating_add(entry.bytes));
        for entry in retained {
            if retained_entries <= self.policy.max_entries
                && retained_bytes <= self.policy.max_bytes
            {
                break;
            }
            let removed = remove_entry(&entry)?;
            retained_entries = retained_entries.saturating_sub(1);
            retained_bytes = retained_bytes.saturating_sub(entry.bytes);
            record_reason(&mut report, RemovalReason::Capacity);
            add_removed(&mut report, removed);
        }
        remove_empty_shards(&self.root)?;
        report.stats = self.stats_locked()?;
        self.estimated_entries
            .store(report.stats.entry_count, Ordering::Relaxed);
        self.estimated_bytes
            .store(report.stats.byte_count, Ordering::Relaxed);
        Ok(report)
    }

    pub(crate) fn maintain_if_due(&self) -> Result<(), PreviewError> {
        let interval = (self.policy.max_entries / 10).clamp(1, 64);
        let stores = self
            .stores_since_maintenance
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let capacity_due = self.estimated_entries.load(Ordering::Relaxed) > self.policy.max_entries
            || self.estimated_bytes.load(Ordering::Relaxed) > self.policy.max_bytes;
        if stores >= interval || capacity_due {
            self.maintain(None)?;
        }
        Ok(())
    }

    fn stats_locked(&self) -> Result<CacheStats, PreviewError> {
        verify_root(&self.base, &self.root)?;
        verify_marker(&self.root)?;
        let measured = measure_cache(&self.root)?;
        Ok(CacheStats {
            layout_version: THUMBNAIL_CACHE_LAYOUT_VERSION,
            file_count: measured.removed_files,
            entry_count: count_png_entries(&self.root)?,
            byte_count: measured.removed_bytes,
            max_entries: self.policy.max_entries,
            max_bytes: self.policy.max_bytes,
            retention_days: self.policy.max_age.as_secs() / (24 * 60 * 60),
            decoder_version: THUMBNAIL_DECODER_VERSION,
        })
    }

    fn path_for(&self, key: &str) -> Result<PathBuf, PreviewError> {
        validate_key(key)?;
        let shard = self.root.join(&key[..2]);
        if entry_metadata(&shard)?
            .is_some_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
        {
            return Err(PreviewError::UnsafeCacheRoot(shard));
        }
        Ok(shard.join(format!("{key}.png")))
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

#[derive(Debug)]
pub(crate) struct CacheEntry {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheEntryDescriptor {
    schema: u32,
    cache_key: String,
    source_token: String,
    provider_id: String,
    provider_version: String,
    max_edge: u32,
}

impl CacheEntryDescriptor {
    fn matches(
        &self,
        key: &str,
        source_token: &str,
        max_edge: u32,
        provider: PreviewProviderIdentity,
    ) -> bool {
        self.schema == ENTRY_SCHEMA_VERSION
            && self.cache_key == key
            && self.source_token == source_token
            && self.provider_id == provider.id
            && self.provider_version == provider.version
            && self.max_edge == max_edge
    }
}

#[derive(Debug, Default)]
struct PartialEntry {
    png: Option<(PathBuf, fs::Metadata)>,
    descriptor: Option<(PathBuf, fs::Metadata)>,
}

#[derive(Debug)]
struct ManagedEntry {
    key: String,
    png: PathBuf,
    descriptor_path: PathBuf,
    descriptor: CacheEntryDescriptor,
    bytes: u64,
    last_used: SystemTime,
}

#[derive(Debug, Default)]
struct CacheScan {
    entries: Vec<ManagedEntry>,
    garbage: Vec<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Copy)]
enum RemovalReason {
    Incompatible,
    Orphan,
    Expired,
    Capacity,
}

fn validate_key(key: &str) -> Result<(), PreviewError> {
    if key.len() == 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(PreviewError::InvalidCacheKey(key.to_owned()))
    }
}

fn is_valid_key(value: &str) -> bool {
    validate_key(value).is_ok()
}

fn metadata_path_for(png: &Path) -> PathBuf {
    png.with_extension("json")
}

fn entry_metadata(path: &Path) -> Result<Option<fs::Metadata>, PreviewError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PreviewError::CacheIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn ensure_regular_entry(path: &Path, metadata: &fs::Metadata) -> Result<(), PreviewError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        Err(PreviewError::UnsafeCacheRoot(path.to_path_buf()))
    } else {
        Ok(())
    }
}

fn read_descriptor(path: &Path) -> Result<Option<CacheEntryDescriptor>, PreviewError> {
    let Some(metadata) = entry_metadata(path)? else {
        return Ok(None);
    };
    ensure_regular_entry(path, &metadata)?;
    let bytes = fs::read(path).map_err(|source| PreviewError::CacheIo {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(serde_json::from_slice(&bytes).ok())
}

fn verify_shard(root: &Path, shard: &Path) -> Result<(), PreviewError> {
    let metadata = shard
        .symlink_metadata()
        .map_err(|source| PreviewError::CacheIo {
            path: shard.to_path_buf(),
            source,
        })?;
    let canonical = shard
        .canonicalize()
        .map_err(|source| PreviewError::CacheIo {
            path: shard.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || canonical.parent() != Some(root) {
        return Err(PreviewError::UnsafeCacheRoot(shard.to_path_buf()));
    }
    Ok(())
}

fn verify_root(base: &Path, root: &Path) -> Result<(), PreviewError> {
    let metadata = root
        .symlink_metadata()
        .map_err(|source| PreviewError::CacheIo {
            path: root.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PreviewError::UnsafeCacheRoot(root.to_path_buf()));
    }
    let canonical = root
        .canonicalize()
        .map_err(|source| PreviewError::CacheIo {
            path: root.to_path_buf(),
            source,
        })?;
    if canonical.parent() != Some(base)
        || canonical
            .file_name()
            .is_none_or(|name| name != CACHE_DIRECTORY)
    {
        return Err(PreviewError::UnsafeCacheRoot(root.to_path_buf()));
    }
    Ok(())
}

fn initialize_marker(root: &Path) -> Result<(), PreviewError> {
    let marker = root.join(CACHE_MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(PreviewError::UnsafeCacheRoot(marker));
        }
        Ok(_) => return verify_marker(root),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(PreviewError::CacheIo {
                path: marker,
                source,
            });
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .map_err(|source| PreviewError::CacheIo {
            path: marker.clone(),
            source,
        })?;
    file.write_all(CACHE_MARKER_CONTENT.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|source| PreviewError::CacheIo {
            path: marker,
            source,
        })?;
    sync_directory(root)
}

fn prepare_cache_root(
    base: &Path,
    root: &Path,
    existed: bool,
) -> Result<CacheStartupReport, PreviewError> {
    if !existed {
        initialize_marker(root)?;
        return Ok(CacheStartupReport {
            disposition: CacheStartupDisposition::Created,
            removed_files: 0,
            removed_bytes: 0,
        });
    }
    let marker = root.join(CACHE_MARKER);
    let marker_metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(PreviewError::UnsafeCacheRoot(marker));
        }
        Ok(metadata) => Some(metadata),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(PreviewError::CacheIo {
                path: marker,
                source,
            });
        }
    };
    if marker_metadata.is_none() {
        let report = measure_cache(root)?;
        let disposition = if report.removed_files == 0 {
            CacheStartupDisposition::Created
        } else {
            CacheStartupDisposition::RebuiltMissingMarker
        };
        rotate_cache_root(base, root)?;
        return Ok(CacheStartupReport {
            disposition,
            removed_files: report.removed_files,
            removed_bytes: report.removed_bytes,
        });
    }
    let contents = fs::read_to_string(&marker).map_err(|source| PreviewError::CacheIo {
        path: marker,
        source,
    })?;
    if contents == CACHE_MARKER_CONTENT {
        return Ok(CacheStartupReport {
            disposition: CacheStartupDisposition::Reused,
            removed_files: 0,
            removed_bytes: 0,
        });
    }
    let report = measure_cache(root)?;
    rotate_cache_root(base, root)?;
    Ok(CacheStartupReport {
        disposition: CacheStartupDisposition::RebuiltIncompatible,
        removed_files: report.removed_files,
        removed_bytes: report.removed_bytes,
    })
}

fn rotate_cache_root(base: &Path, root: &Path) -> Result<(), PreviewError> {
    verify_root(base, root)?;
    let tombstone = base.join(format!("{TOMBSTONE_PREFIX}{}", Uuid::now_v7()));
    initialize_tombstone_marker(root)?;
    fs::rename(root, &tombstone).map_err(|source| PreviewError::CacheIo {
        path: root.to_path_buf(),
        source,
    })?;
    sync_directory(base)?;
    inject_fault("after-cache-rename");
    fs::create_dir(root).map_err(|source| PreviewError::CacheIo {
        path: root.to_path_buf(),
        source,
    })?;
    verify_root(base, root)?;
    initialize_marker(root)?;
    sync_directory(base)?;
    inject_fault("after-cache-recreate");
    remove_owned_tree(&tombstone)?;
    sync_directory(base)
}

fn reap_tombstones(base: &Path) -> Result<CacheClearReport, PreviewError> {
    let mut report = CacheClearReport::default();
    for entry in fs::read_dir(base).map_err(|source| PreviewError::CacheIo {
        path: base.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| PreviewError::CacheIo {
            path: base.to_path_buf(),
            source,
        })?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = file_name.strip_prefix(TOMBSTONE_PREFIX) else {
            continue;
        };
        if Uuid::parse_str(id).is_err() {
            continue;
        }
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|source| PreviewError::CacheIo {
                path: entry.path(),
                source,
            })?;
        if metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && verify_tombstone_marker(&entry.path()).is_ok()
        {
            let removed = measure_cache(&entry.path())?;
            report.removed_files = report.removed_files.saturating_add(removed.removed_files);
            report.removed_bytes = report.removed_bytes.saturating_add(removed.removed_bytes);
            remove_owned_tree(&entry.path())?;
        }
    }
    sync_directory(base)?;
    Ok(report)
}

fn initialize_tombstone_marker(root: &Path) -> Result<(), PreviewError> {
    let path = root.join(TOMBSTONE_MARKER);
    if entry_metadata(&path)?.is_some() {
        return verify_tombstone_marker(root);
    }
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| PreviewError::CacheIo {
            path: path.clone(),
            source,
        })?;
    marker
        .write_all(TOMBSTONE_MARKER_CONTENT.as_bytes())
        .and_then(|()| marker.sync_all())
        .map_err(|source| PreviewError::CacheIo { path, source })?;
    sync_directory(root)
}

fn verify_tombstone_marker(root: &Path) -> Result<(), PreviewError> {
    let path = root.join(TOMBSTONE_MARKER);
    let metadata = fs::symlink_metadata(&path).map_err(|source| PreviewError::CacheIo {
        path: path.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PreviewError::UnsafeCacheRoot(path));
    }
    let content = fs::read_to_string(&path).map_err(|source| PreviewError::CacheIo {
        path: path.clone(),
        source,
    })?;
    if content == TOMBSTONE_MARKER_CONTENT {
        Ok(())
    } else {
        Err(PreviewError::UnsafeCacheRoot(path))
    }
}

fn verify_marker(root: &Path) -> Result<(), PreviewError> {
    let marker = root.join(CACHE_MARKER);
    let metadata = fs::symlink_metadata(&marker).map_err(|source| PreviewError::CacheIo {
        path: marker.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PreviewError::UnsafeCacheRoot(marker));
    }
    let contents = fs::read_to_string(&marker).map_err(|source| PreviewError::CacheIo {
        path: marker,
        source,
    })?;
    if contents == CACHE_MARKER_CONTENT {
        Ok(())
    } else {
        Err(PreviewError::UnsafeCacheRoot(root.to_path_buf()))
    }
}

fn atomic_write(parent: &Path, path: &Path, bytes: &[u8]) -> Result<(), PreviewError> {
    let mut temporary = NamedTempFile::new_in(parent).map_err(|source| PreviewError::CacheIo {
        path: parent.to_path_buf(),
        source,
    })?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| PreviewError::CacheIo {
            path: temporary.path().to_path_buf(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| PreviewError::CacheIo {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    sync_directory(parent)
}

fn touch_entry(path: &Path) -> Result<(), PreviewError> {
    let file =
        OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|source| PreviewError::CacheIo {
                path: path.to_path_buf(),
                source,
            })?;
    file.set_times(FileTimes::new().set_modified(SystemTime::now()))
        .map_err(|source| PreviewError::CacheIo {
            path: path.to_path_buf(),
            source,
        })
}

fn scan_entries(root: &Path) -> Result<CacheScan, PreviewError> {
    let mut scan = CacheScan::default();
    let mut partial = BTreeMap::<String, PartialEntry>::new();
    for root_entry in fs::read_dir(root).map_err(|source| PreviewError::CacheIo {
        path: root.to_path_buf(),
        source,
    })? {
        let root_entry = root_entry.map_err(|source| PreviewError::CacheIo {
            path: root.to_path_buf(),
            source,
        })?;
        if root_entry.file_name() == CACHE_MARKER {
            continue;
        }
        let path = root_entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| PreviewError::CacheIo {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(PreviewError::UnsafeCacheRoot(path));
        }
        if !metadata.is_dir() || !is_shard_name(&root_entry.file_name().to_string_lossy()) {
            scan.garbage.push(vec![path]);
            continue;
        }
        verify_shard(root, &path)?;
        scan_shard(&path, &mut partial, &mut scan.garbage)?;
    }

    for (key, entry) in partial {
        let (png, png_metadata, descriptor_path, descriptor_metadata) =
            match (entry.png, entry.descriptor) {
                (Some((png, png_metadata)), Some((descriptor_path, descriptor_metadata))) => {
                    (png, png_metadata, descriptor_path, descriptor_metadata)
                }
                (png, descriptor) => {
                    let mut paths = Vec::new();
                    if let Some((path, _)) = png {
                        paths.push(path);
                    }
                    if let Some((path, _)) = descriptor {
                        paths.push(path);
                    }
                    if !paths.is_empty() {
                        scan.garbage.push(paths);
                    }
                    continue;
                }
            };
        let Some(descriptor) = read_descriptor(&descriptor_path)? else {
            scan.garbage.push(vec![png, descriptor_path]);
            continue;
        };
        if descriptor.schema != ENTRY_SCHEMA_VERSION
            || descriptor.cache_key != key
            || !is_valid_key(&descriptor.source_token)
        {
            scan.garbage.push(vec![png, descriptor_path]);
            continue;
        }
        scan.entries.push(ManagedEntry {
            key,
            png,
            descriptor_path,
            descriptor,
            bytes: png_metadata.len().saturating_add(descriptor_metadata.len()),
            last_used: png_metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }
    Ok(scan)
}

fn scan_shard(
    shard: &Path,
    partial: &mut BTreeMap<String, PartialEntry>,
    garbage: &mut Vec<Vec<PathBuf>>,
) -> Result<(), PreviewError> {
    for entry in fs::read_dir(shard).map_err(|source| PreviewError::CacheIo {
        path: shard.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| PreviewError::CacheIo {
            path: shard.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| PreviewError::CacheIo {
            path: path.clone(),
            source,
        })?;
        ensure_regular_entry(&path, &metadata)?;
        let Some((key, kind)) = entry_key_and_kind(&path) else {
            garbage.push(vec![path]);
            continue;
        };
        let slot = partial.entry(key).or_default();
        match kind {
            EntryKind::Png if slot.png.is_none() => slot.png = Some((path, metadata)),
            EntryKind::Descriptor if slot.descriptor.is_none() => {
                slot.descriptor = Some((path, metadata));
            }
            EntryKind::Png | EntryKind::Descriptor => garbage.push(vec![path]),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum EntryKind {
    Png,
    Descriptor,
}

fn entry_key_and_kind(path: &Path) -> Option<(String, EntryKind)> {
    let kind = match path.extension()?.to_str()? {
        "png" => EntryKind::Png,
        "json" => EntryKind::Descriptor,
        _ => return None,
    };
    let key = path.file_stem()?.to_str()?;
    is_valid_key(key).then(|| (key.to_owned(), kind))
}

fn is_shard_name(value: &str) -> bool {
    value.len() == 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn remove_entry(entry: &ManagedEntry) -> Result<CacheClearReport, PreviewError> {
    let mut report = CacheClearReport::default();
    add_path_to_clear_report(&mut report, &entry.png)?;
    remove_regular_file_if_present(&entry.png)?;
    add_path_to_clear_report(&mut report, &entry.descriptor_path)?;
    remove_regular_file_if_present(&entry.descriptor_path)?;
    Ok(report)
}

fn remove_owned_path(path: &Path) -> Result<CacheClearReport, PreviewError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| PreviewError::CacheIo {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(PreviewError::UnsafeCacheRoot(path.to_path_buf()));
    }
    if metadata.is_dir() {
        let report = measure_cache(path)?;
        remove_owned_tree(path)?;
        Ok(report)
    } else if metadata.is_file() {
        let report = CacheClearReport {
            removed_files: 1,
            removed_bytes: metadata.len(),
        };
        fs::remove_file(path).map_err(|source| PreviewError::CacheIo {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(report)
    } else {
        Err(PreviewError::UnsafeCacheRoot(path.to_path_buf()))
    }
}

fn remove_owned_tree(path: &Path) -> Result<(), PreviewError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| PreviewError::CacheIo {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PreviewError::UnsafeCacheRoot(path.to_path_buf()));
    }
    fs::remove_dir_all(path).map_err(|source| PreviewError::CacheIo {
        path: path.to_path_buf(),
        source,
    })
}

fn remove_regular_file_if_present(path: &Path) -> Result<(), PreviewError> {
    let Some(metadata) = entry_metadata(path)? else {
        return Ok(());
    };
    ensure_regular_entry(path, &metadata)?;
    fs::remove_file(path).map_err(|source| PreviewError::CacheIo {
        path: path.to_path_buf(),
        source,
    })
}

fn add_path_to_clear_report(
    report: &mut CacheClearReport,
    path: &Path,
) -> Result<(), PreviewError> {
    if let Some(metadata) = entry_metadata(path)? {
        ensure_regular_entry(path, &metadata)?;
        report.removed_files = report.removed_files.saturating_add(1);
        report.removed_bytes = report.removed_bytes.saturating_add(metadata.len());
    }
    Ok(())
}

fn add_removed(report: &mut CacheMaintenanceReport, removed: CacheClearReport) {
    report.removed_files = report.removed_files.saturating_add(removed.removed_files);
    report.removed_bytes = report.removed_bytes.saturating_add(removed.removed_bytes);
}

fn record_reason(report: &mut CacheMaintenanceReport, reason: RemovalReason) {
    report.removed_entries = report.removed_entries.saturating_add(1);
    match reason {
        RemovalReason::Incompatible => {
            report.incompatible_entries = report.incompatible_entries.saturating_add(1);
        }
        RemovalReason::Orphan => {
            report.orphan_entries = report.orphan_entries.saturating_add(1);
        }
        RemovalReason::Expired => {
            report.expired_entries = report.expired_entries.saturating_add(1);
        }
        RemovalReason::Capacity => {
            report.capacity_entries = report.capacity_entries.saturating_add(1);
        }
    }
}

fn remove_empty_shards(root: &Path) -> Result<(), PreviewError> {
    for entry in fs::read_dir(root).map_err(|source| PreviewError::CacheIo {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| PreviewError::CacheIo {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| PreviewError::CacheIo {
            path: path.clone(),
            source,
        })?;
        if metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && is_shard_name(&entry.file_name().to_string_lossy())
            && fs::read_dir(&path)
                .map_err(|source| PreviewError::CacheIo {
                    path: path.clone(),
                    source,
                })?
                .next()
                .is_none()
        {
            fs::remove_dir(&path).map_err(|source| PreviewError::CacheIo { path, source })?;
        }
    }
    Ok(())
}

fn count_png_entries(root: &Path) -> Result<u64, PreviewError> {
    let mut count = 0_u64;
    for entry in fs::read_dir(root).map_err(|source| PreviewError::CacheIo {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| PreviewError::CacheIo {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| PreviewError::CacheIo {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(PreviewError::UnsafeCacheRoot(path));
        }
        if !metadata.is_dir() || !is_shard_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        for child in fs::read_dir(&path).map_err(|source| PreviewError::CacheIo {
            path: path.clone(),
            source,
        })? {
            let child = child.map_err(|source| PreviewError::CacheIo {
                path: path.clone(),
                source,
            })?;
            if entry_key_and_kind(&child.path())
                .is_some_and(|(_, kind)| matches!(kind, EntryKind::Png))
            {
                count = count.saturating_add(1);
            }
        }
    }
    Ok(count)
}

fn measure_cache(root: &Path) -> Result<CacheClearReport, PreviewError> {
    let mut report = CacheClearReport::default();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|source| PreviewError::CacheIo {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| PreviewError::CacheIo {
                path: directory.clone(),
                source,
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| PreviewError::CacheIo {
                path: path.clone(),
                source,
            })?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                pending.push(path);
            } else if entry.file_name() != CACHE_MARKER && entry.file_name() != TOMBSTONE_MARKER {
                report.removed_files = report.removed_files.saturating_add(1);
                report.removed_bytes = report.removed_bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(report)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), PreviewError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| PreviewError::CacheIo {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
const fn sync_directory(_path: &Path) -> Result<(), PreviewError> {
    Ok(())
}

#[cfg(feature = "fault-injection")]
fn inject_fault(point: &str) {
    if std::env::var("EAGLE_CACHE_FAULT_POINT").as_deref() == Ok(point) {
        std::process::abort();
    }
}

#[cfg(not(feature = "fault-injection"))]
const fn inject_fault(_point: &str) {}
