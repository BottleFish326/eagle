use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use asset_core::{
    AssetDimensions, AssetIssue, AssetKind, AssetRecord, MediaProperties, NativeImageMetadata,
};
use asset_svg::{SVG_PROVIDER_ID, SVG_PROVIDER_VERSION};
use format_worker::{
    HeifProperties, LIBHEIF_PROVIDER_ID, LIBHEIF_PROVIDER_VERSION, WorkerClient, WorkerErrorCode,
    WorkerRunError,
};
use resource_control::{ResourceController, ResourceError, ResourceLimits, WorkKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use cache::ThumbnailCache;
use decoder::decode_thumbnail;

mod cache;
mod decoder;

pub const BUILTIN_RASTER_PROVIDER_ID: &str = "builtin-raster";
pub const THUMBNAIL_DECODER_VERSION: &str = "image-0.25.9-triangle-png-v1";
pub const THUMBNAIL_CACHE_LAYOUT_VERSION: u32 = 3;
pub const MIN_THUMBNAIL_EDGE: u32 = 16;
pub const MAX_THUMBNAIL_EDGE: u32 = 2_048;
pub const DEFAULT_CACHE_MAX_BYTES: u64 = 1_073_741_824;
pub const DEFAULT_CACHE_MAX_ENTRIES: u64 = 20_000;
pub const DEFAULT_CACHE_RETENTION_DAYS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    pub max_bytes: u64,
    pub max_entries: u64,
    pub max_age: Duration,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_CACHE_MAX_BYTES,
            max_entries: DEFAULT_CACHE_MAX_ENTRIES,
            max_age: Duration::from_secs(DEFAULT_CACHE_RETENTION_DAYS * 24 * 60 * 60),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailRequest {
    pub asset_key: String,
    pub max_edge: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum ThumbnailOutcome {
    Ready {
        thumbnail: ThumbnailReady,
    },
    Placeholder {
        asset_key: String,
        reason: ThumbnailPlaceholderReason,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailReady {
    pub asset_key: String,
    pub cache_key: String,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub source_size: u64,
    pub source_modified_unix_ms: i64,
    pub cache_hit: bool,
    pub provider_id: String,
    pub provider_version: String,
    pub decoder_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbnailPlaceholderReason {
    MissingAsset,
    CodecUnavailable,
    PreviewUnavailable,
    UnsupportedFormat,
    Unreadable,
    InvalidContent,
    DecodeFailed,
    ResourceLimited,
    TimedOut,
    SourceChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewProviderIdentity {
    pub id: &'static str,
    pub version: &'static str,
}

const BUILTIN_RASTER_PROVIDER: PreviewProviderIdentity = PreviewProviderIdentity {
    id: BUILTIN_RASTER_PROVIDER_ID,
    version: THUMBNAIL_DECODER_VERSION,
};
const SAFE_SVG_PROVIDER: PreviewProviderIdentity = PreviewProviderIdentity {
    id: SVG_PROVIDER_ID,
    version: SVG_PROVIDER_VERSION,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheClearReport {
    pub removed_files: u64,
    pub removed_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheStartupDisposition {
    Created,
    Reused,
    Maintained,
    RebuiltMissingMarker,
    RebuiltIncompatible,
}

impl std::fmt::Display for CacheStartupDisposition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Created => "created",
            Self::Reused => "reused",
            Self::Maintained => "maintained",
            Self::RebuiltMissingMarker => "rebuilt-missing-marker",
            Self::RebuiltIncompatible => "rebuilt-incompatible",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStartupReport {
    pub disposition: CacheStartupDisposition,
    pub removed_files: u64,
    pub removed_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub layout_version: u32,
    pub file_count: u64,
    pub entry_count: u64,
    pub byte_count: u64,
    pub max_entries: u64,
    pub max_bytes: u64,
    pub retention_days: u64,
    pub decoder_version: &'static str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheMaintenanceReport {
    pub removed_entries: u64,
    pub removed_files: u64,
    pub removed_bytes: u64,
    pub incompatible_entries: u64,
    pub orphan_entries: u64,
    pub expired_entries: u64,
    pub capacity_entries: u64,
    pub stats: CacheStats,
}

#[derive(Debug, Error)]
pub enum PreviewError {
    #[error(
        "thumbnail edge must be between {MIN_THUMBNAIL_EDGE} and {MAX_THUMBNAIL_EDGE}, got {0}"
    )]
    InvalidMaxEdge(u32),
    #[error("thumbnail concurrency must be between 1 and 32, got {0}")]
    InvalidConcurrency(usize),
    #[error("invalid thumbnail cache policy: {0}")]
    InvalidCachePolicy(String),
    #[error("unsafe thumbnail cache root: {0}")]
    UnsafeCacheRoot(PathBuf),
    #[error("thumbnail cache I/O error at {path}: {source}")]
    CacheIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("thumbnail cache metadata error at {path}: {message}")]
    CacheMetadata { path: PathBuf, message: String },
    #[error("invalid thumbnail cache key: {0}")]
    InvalidCacheKey(String),
    #[error("thumbnail cache entry does not exist: {0}")]
    MissingCacheEntry(String),
    #[error("shared state lock is poisoned: {0}")]
    PoisonedLock(&'static str),
    #[error("format worker identity is not the pinned libheif provider")]
    InvalidWorkerIdentity,
    #[error("thumbnail resource control error: {0}")]
    Resource(#[from] ResourceError),
}

#[derive(Debug)]
pub struct ThumbnailService {
    cache: ThumbnailCache,
    resources: ResourceController,
    startup: CacheStartupReport,
    libheif_worker: Option<WorkerClient>,
}

impl ThumbnailService {
    /// Opens the derived thumbnail cache below an application-owned cache directory.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] when the concurrency bound is invalid or the cache
    /// root cannot be created and verified safely.
    pub fn open(base_cache_directory: &Path, max_concurrent: usize) -> Result<Self, PreviewError> {
        if !(1..=32).contains(&max_concurrent) {
            return Err(PreviewError::InvalidConcurrency(max_concurrent));
        }
        Self::open_with_policy(base_cache_directory, max_concurrent, CachePolicy::default())
    }

    /// Opens the cache with explicit lifecycle bounds.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] when the policy is empty or the cache is unsafe.
    pub fn open_with_policy(
        base_cache_directory: &Path,
        max_concurrent: usize,
        policy: CachePolicy,
    ) -> Result<Self, PreviewError> {
        if !(1..=32).contains(&max_concurrent) {
            return Err(PreviewError::InvalidConcurrency(max_concurrent));
        }
        if policy.max_bytes == 0 || policy.max_entries == 0 || policy.max_age.is_zero() {
            return Err(PreviewError::InvalidCachePolicy(
                "max bytes, max entries, and max age must all be positive".into(),
            ));
        }
        let resources =
            ResourceController::new(ResourceLimits::for_decode_capacity(max_concurrent))?;
        Self::open_with_policy_and_resources(base_cache_directory, policy, resources)
    }

    /// Opens the cache on a process-wide resource controller shared with scan and hash work.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] when the cache cannot be opened safely.
    pub fn open_with_resources(
        base_cache_directory: &Path,
        resources: ResourceController,
    ) -> Result<Self, PreviewError> {
        Self::open_with_policy_and_resources(
            base_cache_directory,
            CachePolicy::default(),
            resources,
        )
    }

    /// Opens the cache with explicit lifecycle bounds and a shared work scheduler.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] when the policy is empty or the cache is unsafe.
    pub fn open_with_policy_and_resources(
        base_cache_directory: &Path,
        policy: CachePolicy,
        resources: ResourceController,
    ) -> Result<Self, PreviewError> {
        if policy.max_bytes == 0 || policy.max_entries == 0 || policy.max_age.is_zero() {
            return Err(PreviewError::InvalidCachePolicy(
                "max bytes, max entries, and max age must all be positive".into(),
            ));
        }
        let (cache, startup) = ThumbnailCache::open(base_cache_directory, policy)?;
        Ok(Self {
            cache,
            resources,
            startup,
            libheif_worker: None,
        })
    }

    /// Enables the pinned libheif worker for requests that also provide an authorized root.
    ///
    /// # Errors
    ///
    /// Rejects any worker whose provider identity does not match the build-pinned backend.
    pub fn with_libheif_worker(mut self, worker: WorkerClient) -> Result<Self, PreviewError> {
        if worker.provider_id() != LIBHEIF_PROVIDER_ID
            || worker.provider_version() != LIBHEIF_PROVIDER_VERSION
        {
            return Err(PreviewError::InvalidWorkerIdentity);
        }
        self.libheif_worker = Some(worker);
        Ok(self)
    }

    /// Lazily returns or generates a thumbnail for one scanned asset.
    ///
    /// Source decode problems return [`ThumbnailOutcome::Placeholder`]. Cache or
    /// request failures return an error.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] for invalid sizing, unsafe cache state, or cache I/O.
    pub fn request(
        &self,
        record: &AssetRecord,
        max_edge: u32,
    ) -> Result<ThumbnailOutcome, PreviewError> {
        self.request_internal(record, max_edge, None)
    }

    /// Generates a thumbnail with one library root explicitly authorizing worker access.
    ///
    /// The root is used only by the isolated optional-format worker. Built-in providers retain
    /// their existing behavior. Source containment is canonicalized again by the worker client.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] for invalid sizing, unsafe cache state, or cache I/O.
    pub fn request_with_authorized_root(
        &self,
        record: &AssetRecord,
        max_edge: u32,
        authorized_root: &Path,
    ) -> Result<ThumbnailOutcome, PreviewError> {
        self.request_internal(record, max_edge, Some(authorized_root))
    }

    /// Adds file-derived AVIF/HEIC properties to one in-memory scan record.
    ///
    /// No property is written to a Sidecar or any other authoritative store. Missing codecs are a
    /// neutral capability downgrade; malformed or resource-limited sources receive a stable issue.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] only when the shared resource scheduler cannot admit the request.
    pub fn enrich_media_properties(
        &self,
        record: &mut AssetRecord,
        authorized_root: &Path,
    ) -> Result<bool, PreviewError> {
        if !matches!(
            record.mime.as_str(),
            "image/avif" | "image/heic" | "image/heif"
        ) {
            return Ok(false);
        }
        let Some(worker) = self.libheif_worker.as_ref() else {
            return Ok(false);
        };
        let _permit = self.resources.acquire(WorkKind::Decode)?;
        let result = worker
            .metadata_request(Uuid::now_v7(), &record.path, authorized_root)
            .and_then(|request| worker.execute(&request, authorized_root));
        match result {
            Ok(success) => {
                apply_heif_properties(record, success.properties);
                Ok(true)
            }
            Err(error) => {
                if let Some(issue) = worker_asset_issue(&error) {
                    record.issues.push(issue);
                }
                Ok(false)
            }
        }
    }

    fn request_internal(
        &self,
        record: &AssetRecord,
        max_edge: u32,
        authorized_root: Option<&Path>,
    ) -> Result<ThumbnailOutcome, PreviewError> {
        if !(MIN_THUMBNAIL_EDGE..=MAX_THUMBNAIL_EDGE).contains(&max_edge) {
            return Err(PreviewError::InvalidMaxEdge(max_edge));
        }
        let cache_guard = self.cache.read_guard()?;
        let Some(mut version) = read_source_version(&record.path) else {
            return Ok(placeholder(
                record,
                ThumbnailPlaceholderReason::MissingAsset,
                "asset is missing or cannot be read".into(),
            ));
        };
        let worker = self.libheif_worker.as_ref().zip(authorized_root);
        let provider = match preview_provider(record, worker.is_some()) {
            Ok(provider) => provider,
            Err((reason, message)) => return Ok(placeholder(record, reason, message.into())),
        };
        let mut key = thumbnail_key(record, &version, max_edge, provider);
        let mut source_identity = source_token(record, &version);
        if let Some(entry) = self
            .cache
            .lookup(&key, &source_identity, max_edge, provider)?
        {
            return Ok(ready(
                record,
                key,
                entry.width,
                entry.height,
                &version,
                true,
                provider,
            ));
        }

        let _permit = self.resources.acquire(WorkKind::Decode)?;
        let Some(latest) = read_source_version(&record.path) else {
            return Ok(placeholder(
                record,
                ThumbnailPlaceholderReason::MissingAsset,
                "asset disappeared before thumbnail decoding".into(),
            ));
        };
        if latest != version {
            version = latest;
            key = thumbnail_key(record, &version, max_edge, provider);
            source_identity = source_token(record, &version);
        }
        if let Some(entry) = self
            .cache
            .lookup(&key, &source_identity, max_edge, provider)?
        {
            return Ok(ready(
                record,
                key,
                entry.width,
                entry.height,
                &version,
                true,
                provider,
            ));
        }

        let decoded = match decode_thumbnail(&record.path, max_edge, provider, worker) {
            Ok(decoded) => decoded,
            Err(failure) => return Ok(placeholder(record, failure.reason, failure.message)),
        };
        if read_source_version(&record.path).as_ref() != Some(&version) {
            return Ok(placeholder(
                record,
                ThumbnailPlaceholderReason::SourceChanged,
                "asset changed while its thumbnail was being generated".into(),
            ));
        }
        self.cache
            .store(&key, &source_identity, max_edge, provider, &decoded.bytes)?;
        let outcome = ready(
            record,
            key,
            decoded.width,
            decoded.height,
            &version,
            false,
            provider,
        );
        drop(cache_guard);
        self.cache.maintain_if_due()?;
        Ok(outcome)
    }

    /// Reads a validated cached PNG as raw bytes for efficient IPC transfer.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] for malformed keys, missing entries, or cache I/O.
    pub fn read(&self, cache_key: &str) -> Result<Vec<u8>, PreviewError> {
        self.cache.read(cache_key)
    }

    /// Removes and recreates only the dedicated derived thumbnail cache directory.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] if the cache boundary cannot be revalidated or cleared.
    pub fn clear(&self) -> Result<CacheClearReport, PreviewError> {
        self.cache.clear()
    }

    #[must_use]
    pub const fn startup_report(&self) -> CacheStartupReport {
        self.startup
    }

    /// Measures the current derived cache without reading or modifying any source asset.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] if the cache boundary or marker is no longer safe.
    pub fn cache_stats(&self) -> Result<CacheStats, PreviewError> {
        self.cache.stats()
    }

    /// Reclaims entries that cannot belong to the current in-memory catalog.
    ///
    /// # Errors
    ///
    /// Returns [`PreviewError`] if the cache boundary becomes unsafe or maintenance fails.
    pub fn maintain(
        &self,
        records: &[AssetRecord],
    ) -> Result<CacheMaintenanceReport, PreviewError> {
        let active_sources = records
            .iter()
            .filter_map(|record| {
                read_source_version(&record.path).map(|version| source_token(record, &version))
            })
            .collect::<BTreeSet<_>>();
        self.cache.maintain(Some(&active_sources))
    }
}

fn apply_heif_properties(record: &mut AssetRecord, properties: HeifProperties) {
    record.dimensions = Some(AssetDimensions {
        width: properties.width,
        height: properties.height,
    });
    if let Some(orientation) = properties.orientation {
        let metadata = record
            .native_metadata
            .get_or_insert_with(NativeImageMetadata::default);
        metadata.orientation = Some(u32::from(orientation));
    }
    record.media = Some(MediaProperties {
        frame_count: Some(properties.image_count),
        color_space: properties.color_space,
        has_alpha: properties.has_alpha,
        ..MediaProperties::default()
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceVersion {
    size: u64,
    modified_unix_ms: i64,
    modified_unix_ns: i128,
}

fn read_source_version(path: &Path) -> Option<SourceVersion> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata.modified().ok()?;
    Some(SourceVersion {
        size: metadata.len(),
        modified_unix_ms: unix_milliseconds(modified),
        modified_unix_ns: unix_nanoseconds(modified),
    })
}

fn unix_milliseconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

fn unix_nanoseconds(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX),
        Err(error) => -i128::try_from(error.duration().as_nanos()).unwrap_or(i128::MAX),
    }
}

fn thumbnail_key(
    record: &AssetRecord,
    version: &SourceVersion,
    max_edge: u32,
    provider: PreviewProviderIdentity,
) -> String {
    let mut digest = Sha256::new();
    for part in [
        format!("path:{}", record.key),
        record
            .id
            .map_or_else(|| "id:none".into(), |id| format!("id:{id}")),
        version.size.to_string(),
        version.modified_unix_ns.to_string(),
        max_edge.to_string(),
        format!("provider:{}", provider.id),
        format!("provider-version:{}", provider.version),
    ] {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())
}

fn source_token(record: &AssetRecord, version: &SourceVersion) -> String {
    let mut digest = Sha256::new();
    for part in [
        format!("path:{}", record.key),
        record
            .id
            .map_or_else(|| "id:none".into(), |id| format!("id:{id}")),
        version.size.to_string(),
        version.modified_unix_ns.to_string(),
    ] {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())
}

fn ready(
    record: &AssetRecord,
    cache_key: String,
    width: u32,
    height: u32,
    version: &SourceVersion,
    cache_hit: bool,
    provider: PreviewProviderIdentity,
) -> ThumbnailOutcome {
    ThumbnailOutcome::Ready {
        thumbnail: ThumbnailReady {
            asset_key: record.key.clone(),
            cache_key,
            mime: "image/png".into(),
            width,
            height,
            source_size: version.size,
            source_modified_unix_ms: version.modified_unix_ms,
            cache_hit,
            provider_id: provider.id.into(),
            provider_version: provider.version.into(),
            decoder_version: provider.version.into(),
        },
    }
}

fn preview_provider(
    record: &AssetRecord,
    libheif_available: bool,
) -> Result<PreviewProviderIdentity, (ThumbnailPlaceholderReason, &'static str)> {
    match record.mime.as_str() {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" => Ok(BUILTIN_RASTER_PROVIDER),
        "image/avif" | "image/heic" | "image/heif" if libheif_available => {
            Ok(PreviewProviderIdentity {
                id: LIBHEIF_PROVIDER_ID,
                version: LIBHEIF_PROVIDER_VERSION,
            })
        }
        "image/avif" | "image/heic" | "image/heif" => Err((
            ThumbnailPlaceholderReason::CodecUnavailable,
            "the pinned optional image worker is unavailable",
        )),
        "image/svg+xml" => Ok(SAFE_SVG_PROVIDER),
        "video/mp4" | "video/quicktime" | "video/webm" | "audio/mpeg" | "audio/wav"
        | "audio/flac" | "application/pdf" => Err((
            ThumbnailPlaceholderReason::PreviewUnavailable,
            "this registered format does not yet have a preview provider",
        )),
        _ if record.kind != AssetKind::Other => Err((
            ThumbnailPlaceholderReason::PreviewUnavailable,
            "this asset type does not have a preview provider",
        )),
        _ => Err((
            ThumbnailPlaceholderReason::UnsupportedFormat,
            "asset format is not registered for previews",
        )),
    }
}

pub(crate) fn is_current_preview_provider(id: &str, version: &str) -> bool {
    (id == BUILTIN_RASTER_PROVIDER.id && version == BUILTIN_RASTER_PROVIDER.version)
        || (id == SAFE_SVG_PROVIDER.id && version == SAFE_SVG_PROVIDER.version)
        || (id == LIBHEIF_PROVIDER_ID && version == LIBHEIF_PROVIDER_VERSION)
}

fn placeholder(
    record: &AssetRecord,
    reason: ThumbnailPlaceholderReason,
    message: String,
) -> ThumbnailOutcome {
    ThumbnailOutcome::Placeholder {
        asset_key: record.key.clone(),
        reason,
        message,
    }
}

fn worker_asset_issue(error: &WorkerRunError) -> Option<AssetIssue> {
    match error {
        WorkerRunError::Worker {
            code: WorkerErrorCode::CodecUnavailable | WorkerErrorCode::UnsupportedFeature,
            ..
        } => None,
        WorkerRunError::Worker {
            code: WorkerErrorCode::ResourceLimited | WorkerErrorCode::TimedOut,
            ..
        }
        | WorkerRunError::TimedOut { .. }
        | WorkerRunError::OutputTooLarge
        | WorkerRunError::ResourceLimitViolation => Some(AssetIssue::ResourceLimited(
            "optional image metadata exceeded its fixed worker limits".into(),
        )),
        WorkerRunError::Worker {
            code: WorkerErrorCode::Unreadable,
            ..
        }
        | WorkerRunError::InvalidSource
        | WorkerRunError::SourceOutsideRoot => Some(AssetIssue::UnreadableFile(
            "optional image metadata source is unavailable".into(),
        )),
        WorkerRunError::Worker {
            code: WorkerErrorCode::SourceChanged,
            ..
        }
        | WorkerRunError::SourceChanged => Some(AssetIssue::InvalidNativeMetadata(
            "source changed during optional image metadata extraction".into(),
        )),
        WorkerRunError::Worker {
            code:
                WorkerErrorCode::InvalidContent
                | WorkerErrorCode::DecodeFailed
                | WorkerErrorCode::Internal,
            ..
        }
        | WorkerRunError::InvalidConfiguration(_)
        | WorkerRunError::ExecutableChanged
        | WorkerRunError::Io(_)
        | WorkerRunError::Protocol(_)
        | WorkerRunError::Crashed { .. }
        | WorkerRunError::IdentityMismatch
        | WorkerRunError::InvalidPng
        | WorkerRunError::ThreadPanicked => Some(AssetIssue::InvalidNativeMetadata(
            "optional image metadata could not be decoded by the fixed worker".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, FileTimes, OpenOptions};
    use std::io::BufWriter;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, SystemTime};

    use asset_core::{AssetKind, AssetRecord};
    use format_worker::HeifProperties;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{DynamicImage, Frame, ImageBuffer, ImageFormat, Rgba};
    use metadata::{digest_file, sidecar_path_for};
    use tempfile::tempdir;

    use super::{
        CachePolicy, CacheStartupDisposition, THUMBNAIL_DECODER_VERSION, ThumbnailOutcome,
        ThumbnailPlaceholderReason, ThumbnailService, apply_heif_properties,
    };

    #[test]
    fn lazily_decodes_four_formats_and_preserves_originals() {
        let directory = tempdir().expect("tempdir");
        let cache = directory.path().join("cache");
        let assets = directory.path().join("assets");
        fs::create_dir(&assets).expect("asset directory");
        let service = ThumbnailService::open(&cache, 2).expect("preview service");
        assert_eq!(png_files(service.cache.root()), 0);

        for (extension, format) in [
            ("png", ImageFormat::Png),
            ("jpg", ImageFormat::Jpeg),
            ("webp", ImageFormat::WebP),
            ("gif", ImageFormat::Gif),
        ] {
            let path = assets.join(format!("asset.{extension}"));
            write_image(&path, format);
            let digest = digest_file(&path).expect("source digest");
            let outcome = service
                .request(&record(&path), 32)
                .expect("generate thumbnail");
            let thumbnail = expect_ready(outcome);
            assert_eq!((thumbnail.width, thumbnail.height), (32, 16));
            assert!(!thumbnail.cache_hit);
            assert_eq!(digest_file(&path).expect("source digest after"), digest);
            let bytes = service.read(&thumbnail.cache_key).expect("thumbnail bytes");
            assert_eq!(
                image::guess_format(&bytes).expect("format"),
                ImageFormat::Png
            );
        }
        assert_eq!(png_files(service.cache.root()), 4);
    }

    #[test]
    fn animated_gif_uses_the_first_frame() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("animated.gif");
        write_animated_gif(&asset);
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");

        let thumbnail = expect_ready(service.request(&record(&asset), 32).expect("thumbnail"));
        let bytes = service.read(&thumbnail.cache_key).expect("cached bytes");
        let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .expect("decode cached thumbnail")
            .to_rgba8();

        assert!(decoded.get_pixel(0, 0).0[0] > 200);
        assert!(decoded.get_pixel(0, 0).0[2] < 30);
    }

    #[test]
    fn cache_hit_and_source_change_use_distinct_keys() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.png");
        write_image(&asset, ImageFormat::Png);
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");
        let record = record(&asset);

        let first = expect_ready(service.request(&record, 32).expect("first"));
        let second = expect_ready(service.request(&record, 32).expect("second"));
        assert_eq!(first.cache_key, second.cache_key);
        assert!(second.cache_hit);

        thread::sleep(Duration::from_millis(20));
        write_solid_image(&asset, ImageFormat::Png, 120, 60, [0, 0, 255, 255]);
        let changed = expect_ready(service.request(&record, 32).expect("changed"));
        assert_ne!(first.cache_key, changed.cache_key);
        assert!(!changed.cache_hit);
    }

    #[test]
    fn damaged_image_returns_a_placeholder_without_cache_entry() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("damaged.png");
        fs::write(&asset, b"not an image").expect("damaged image");
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");

        let outcome = service.request(&record(&asset), 32).expect("placeholder");
        assert!(matches!(
            &outcome,
            ThumbnailOutcome::Placeholder {
                reason: ThumbnailPlaceholderReason::InvalidContent,
                message,
                ..
            } if !message.is_empty()
        ));
        assert_eq!(png_files(service.cache.root()), 0);
    }

    #[test]
    fn clear_removes_only_derived_cache_files() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.png");
        write_image(&asset, ImageFormat::Png);
        let sidecar = sidecar_path_for(&asset);
        fs::write(&sidecar, "user metadata").expect("sidecar");
        let asset_digest = digest_file(&asset).expect("asset digest");
        let sidecar_contents = fs::read(&sidecar).expect("sidecar contents");
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");
        let thumbnail = expect_ready(service.request(&record(&asset), 32).expect("thumbnail"));

        let report = service.clear().expect("clear cache");

        assert_eq!(report.removed_files, 2);
        assert!(report.removed_bytes > 0);
        assert!(service.read(&thumbnail.cache_key).is_err());
        assert_eq!(
            digest_file(&asset).expect("asset digest after"),
            asset_digest
        );
        assert_eq!(fs::read(&sidecar).expect("sidecar after"), sidecar_contents);
        assert_eq!(png_files(service.cache.root()), 0);
    }

    #[test]
    fn capacity_uses_recent_access_and_keeps_the_entry_bound() {
        let directory = tempdir().expect("tempdir");
        let assets = directory.path().join("assets");
        fs::create_dir(&assets).expect("asset directory");
        let records = (0..3)
            .map(|index| {
                let path = assets.join(format!("asset-{index}.png"));
                write_solid_image(&path, ImageFormat::Png, 80, 40, [index * 40, 0, 255, 255]);
                record(&path)
            })
            .collect::<Vec<_>>();
        let service = ThumbnailService::open_with_policy(
            &directory.path().join("cache"),
            1,
            CachePolicy {
                max_bytes: u64::MAX,
                max_entries: 2,
                max_age: Duration::from_secs(60 * 60),
            },
        )
        .expect("service");

        let first = expect_ready(service.request(&records[0], 32).expect("first"));
        let second = expect_ready(service.request(&records[1], 32).expect("second"));
        thread::sleep(Duration::from_millis(20));
        assert!(expect_ready(service.request(&records[0], 32).expect("touch first")).cache_hit);
        let third = expect_ready(service.request(&records[2], 32).expect("third"));

        assert_eq!(service.cache_stats().expect("stats").entry_count, 2);
        assert!(service.read(&first.cache_key).is_ok());
        assert!(service.read(&second.cache_key).is_err());
        assert!(service.read(&third.cache_key).is_ok());
    }

    #[test]
    fn maintenance_reclaims_expired_incompatible_and_orphan_entries() {
        let directory = tempdir().expect("tempdir");
        let assets = directory.path().join("assets");
        fs::create_dir(&assets).expect("asset directory");
        let records = (0..4)
            .map(|index| {
                let path = assets.join(format!("asset-{index}.png"));
                write_image(&path, ImageFormat::Png);
                record(&path)
            })
            .collect::<Vec<_>>();
        let service = ThumbnailService::open_with_policy(
            &directory.path().join("cache"),
            1,
            CachePolicy {
                max_bytes: u64::MAX,
                max_entries: 100,
                max_age: Duration::from_secs(60),
            },
        )
        .expect("service");
        let thumbnails = records
            .iter()
            .map(|record| expect_ready(service.request(record, 32).expect("thumbnail")))
            .collect::<Vec<_>>();

        let expired = cache_entry_path(&service, &thumbnails[0].cache_key, "png");
        OpenOptions::new()
            .write(true)
            .open(&expired)
            .expect("open expired entry")
            .set_times(FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(120)))
            .expect("age cache entry");
        let incompatible = cache_entry_path(&service, &thumbnails[1].cache_key, "json");
        let mut descriptor: serde_json::Value =
            serde_json::from_slice(&fs::read(&incompatible).expect("descriptor"))
                .expect("descriptor json");
        descriptor["providerVersion"] = "retired-provider".into();
        fs::write(
            &incompatible,
            serde_json::to_vec(&descriptor).expect("descriptor bytes"),
        )
        .expect("write incompatible descriptor");

        let report = service.maintain(&records[..3]).expect("maintenance");

        assert_eq!(report.removed_entries, 3);
        assert_eq!(report.removed_files, 6);
        assert_eq!(report.expired_entries, 1);
        assert_eq!(report.incompatible_entries, 1);
        assert_eq!(report.orphan_entries, 1);
        assert_eq!(report.capacity_entries, 0);
        assert_eq!(report.stats.entry_count, 1);
        assert!(service.read(&thumbnails[0].cache_key).is_err());
        assert!(service.read(&thumbnails[1].cache_key).is_err());
        assert!(service.read(&thumbnails[2].cache_key).is_ok());
        assert!(service.read(&thumbnails[3].cache_key).is_err());
    }

    #[test]
    fn startup_recovers_partial_entries_and_an_interrupted_rotating_clear() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.png");
        let cache = directory.path().join("cache");
        write_image(&asset, ImageFormat::Png);
        let sidecar = sidecar_path_for(&asset);
        fs::write(&sidecar, "user metadata").expect("sidecar");
        let asset_digest = digest_file(&asset).expect("asset digest");
        let sidecar_contents = fs::read(&sidecar).expect("sidecar contents");
        let service = ThumbnailService::open(&cache, 1).expect("service");
        let thumbnail = expect_ready(service.request(&record(&asset), 32).expect("thumbnail"));
        let root = service.cache.root().to_path_buf();
        fs::remove_file(cache_entry_path(&service, &thumbnail.cache_key, "json"))
            .expect("remove descriptor");
        drop(service);

        let recovered = ThumbnailService::open(&cache, 1).expect("recover partial entry");
        assert_eq!(
            recovered.startup_report().disposition,
            CacheStartupDisposition::Maintained
        );
        assert_eq!(recovered.cache_stats().expect("stats").entry_count, 0);
        drop(recovered);

        fs::write(
            root.join(".material-eagle-thumbnail-cache-tombstone"),
            "material-eagle-thumbnail-cache-tombstone-v1\n",
        )
        .expect("tombstone ownership marker");
        let tombstone =
            cache.join(".material-eagle-thumbnail-cache-gc-0198a9b2-43c0-7cb0-a733-6dc58f829814");
        fs::rename(&root, &tombstone).expect("simulate interrupted clear");
        let reopened = ThumbnailService::open(&cache, 1).expect("recover interrupted clear");

        assert!(!tombstone.exists());
        assert_eq!(reopened.cache_stats().expect("stats").entry_count, 0);
        assert_eq!(
            digest_file(&asset).expect("asset digest after"),
            asset_digest
        );
        assert_eq!(fs::read(&sidecar).expect("sidecar after"), sidecar_contents);
        let rebuilt = expect_ready(reopened.request(&record(&asset), 32).expect("rebuild"));
        assert!(!rebuilt.cache_hit);
    }

    #[test]
    fn startup_does_not_delete_an_unowned_tombstone_named_directory() {
        let directory = tempdir().expect("tempdir");
        let cache = directory.path().join("cache");
        let service = ThumbnailService::open(&cache, 1).expect("service");
        drop(service);
        let unowned =
            cache.join(".material-eagle-thumbnail-cache-gc-0198a9b2-43c0-7cb0-a733-6dc58f829814");
        fs::create_dir(&unowned).expect("unowned directory");
        fs::write(unowned.join("private.txt"), "not cache data").expect("unowned content");

        let reopened = ThumbnailService::open(&cache, 1).expect("reopen service");

        assert!(unowned.join("private.txt").is_file());
        assert_eq!(reopened.cache_stats().expect("stats").entry_count, 0);
    }

    #[test]
    fn deleted_cache_is_rebuilt_from_the_unchanged_source() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.png");
        let cache = directory.path().join("cache");
        write_image(&asset, ImageFormat::Png);
        let digest = digest_file(&asset).expect("asset digest");
        let service = ThumbnailService::open(&cache, 1).expect("service");
        let first = expect_ready(service.request(&record(&asset), 32).expect("thumbnail"));
        let cache_root = service.cache.root().to_path_buf();
        drop(service);

        fs::remove_dir_all(&cache_root).expect("delete entire derived cache");
        let reopened = ThumbnailService::open(&cache, 1).expect("reopen service");
        let rebuilt = expect_ready(reopened.request(&record(&asset), 32).expect("rebuild"));

        assert_eq!(first.cache_key, rebuilt.cache_key);
        assert!(!rebuilt.cache_hit);
        assert_eq!(digest_file(&asset).expect("asset digest after"), digest);
    }

    #[test]
    fn incompatible_cache_marker_is_automatically_discarded_on_startup() {
        let directory = tempdir().expect("tempdir");
        let cache = directory.path().join("cache");
        let service = ThumbnailService::open(&cache, 1).expect("service");
        let root = service.cache.root().to_path_buf();
        drop(service);
        fs::write(
            root.join(".material-eagle-thumbnail-cache"),
            "material-eagle-thumbnail-cache-v0\n",
        )
        .expect("old marker");
        fs::create_dir(root.join("aa")).expect("old shard");
        fs::write(root.join("aa/old.png"), b"obsolete").expect("old entry");

        let reopened = ThumbnailService::open(&cache, 1).expect("compatible rebuild");

        assert_eq!(
            reopened.startup_report().disposition,
            CacheStartupDisposition::RebuiltIncompatible
        );
        assert_eq!(reopened.startup_report().removed_files, 1);
        assert_eq!(reopened.cache_stats().expect("stats").file_count, 0);
        assert!(!root.join("aa/old.png").exists());
    }

    #[test]
    fn nonempty_cache_without_a_marker_is_rebuilt_on_startup() {
        let directory = tempdir().expect("tempdir");
        let cache = directory.path().join("cache");
        let service = ThumbnailService::open(&cache, 1).expect("service");
        let root = service.cache.root().to_path_buf();
        drop(service);
        fs::remove_file(root.join(".material-eagle-thumbnail-cache")).expect("remove marker");
        fs::write(root.join("orphan.bin"), b"obsolete").expect("orphan");

        let reopened = ThumbnailService::open(&cache, 1).expect("missing marker rebuild");

        assert_eq!(
            reopened.startup_report().disposition,
            CacheStartupDisposition::RebuiltMissingMarker
        );
        assert_eq!(reopened.startup_report().removed_files, 1);
        assert!(!root.join("orphan.bin").exists());
    }

    #[cfg(unix)]
    #[test]
    fn cache_marker_symlinks_are_rejected_without_writing_the_target() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("tempdir");
        let cache = directory.path().join("cache");
        let external = directory.path().join("external-marker");
        fs::write(&external, b"private").expect("external marker target");
        let service = ThumbnailService::open(&cache, 1).expect("service");
        let root = service.cache.root().to_path_buf();
        drop(service);
        fs::remove_file(root.join(".material-eagle-thumbnail-cache")).expect("remove marker");
        symlink(&external, root.join(".material-eagle-thumbnail-cache"))
            .expect("malicious marker symlink");

        let error = ThumbnailService::open(&cache, 1).expect_err("marker symlink must fail");

        assert!(matches!(error, super::PreviewError::UnsafeCacheRoot(_)));
        assert_eq!(fs::read(&external).expect("external target"), b"private");
    }

    #[cfg(unix)]
    #[test]
    fn cache_shard_symlinks_are_rejected_without_writing_the_target() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.png");
        let external = directory.path().join("external");
        fs::create_dir(&external).expect("external directory");
        write_image(&asset, ImageFormat::Png);
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");
        let first = expect_ready(service.request(&record(&asset), 32).expect("thumbnail"));
        service.clear().expect("clear cache");
        symlink(&external, service.cache.root().join(&first.cache_key[..2]))
            .expect("malicious shard symlink");

        let error = service
            .request(&record(&asset), 32)
            .expect_err("symlink shard must be rejected");

        assert!(matches!(error, super::PreviewError::UnsafeCacheRoot(_)));
        assert_eq!(
            fs::read_dir(&external).expect("external entries").count(),
            0
        );
    }

    #[test]
    fn only_assets_requested_by_the_viewport_are_generated() {
        let directory = tempdir().expect("tempdir");
        let assets = directory.path().join("assets");
        fs::create_dir(&assets).expect("asset directory");
        let records = (0..100)
            .map(|index| {
                let path = assets.join(format!("asset-{index:03}.png"));
                write_image(&path, ImageFormat::Png);
                record(&path)
            })
            .collect::<Vec<_>>();
        let service = ThumbnailService::open(&directory.path().join("cache"), 2).expect("service");

        assert_eq!(png_files(service.cache.root()), 0);
        for record in records.iter().take(12) {
            expect_ready(service.request(record, 64).expect("viewport thumbnail"));
        }

        assert_eq!(png_files(service.cache.root()), 12);
    }

    #[test]
    fn outcomes_use_the_frontend_wire_shape() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.png");
        write_image(&asset, ImageFormat::Png);
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");
        let outcome = service.request(&record(&asset), 32).expect("thumbnail");
        let asset_key = asset.to_string_lossy().into_owned();

        let value = serde_json::to_value(outcome).expect("serialize thumbnail outcome");

        assert_eq!(value["status"], "ready");
        assert_eq!(value["thumbnail"]["assetKey"], asset_key);
        assert_eq!(value["thumbnail"]["mime"], "image/png");
        assert_eq!(value["thumbnail"]["cacheHit"], false);
        assert_eq!(value["thumbnail"]["providerId"], "builtin-raster");
        assert_eq!(
            value["thumbnail"]["providerVersion"],
            THUMBNAIL_DECODER_VERSION
        );

        let mut unsupported = record(&asset);
        unsupported.kind = AssetKind::Other;
        unsupported.mime = "application/octet-stream".into();
        let value = serde_json::to_value(
            service
                .request(&unsupported, 32)
                .expect("placeholder outcome"),
        )
        .expect("serialize placeholder outcome");
        assert_eq!(value["status"], "placeholder");
        assert_eq!(value["assetKey"], asset_key);
        assert_eq!(value["reason"], "unsupported-format");
    }

    #[test]
    fn unavailable_optional_providers_are_distinct_from_invalid_content() {
        let directory = tempdir().expect("tempdir");
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");
        let cases = [
            (
                "asset.avif",
                "image/avif",
                ThumbnailPlaceholderReason::CodecUnavailable,
            ),
            (
                "asset.mp4",
                "video/mp4",
                ThumbnailPlaceholderReason::PreviewUnavailable,
            ),
            (
                "asset.pdf",
                "application/pdf",
                ThumbnailPlaceholderReason::PreviewUnavailable,
            ),
        ];
        for (name, mime, expected) in cases {
            let path = directory.path().join(name);
            fs::write(&path, b"registered content").expect("write provider fixture");
            let metadata = fs::metadata(&path).expect("metadata");
            let record = AssetRecord::untagged(
                path.to_string_lossy().into_owned(),
                path,
                mime.into(),
                metadata.len(),
                0,
            );
            assert!(matches!(
                service.request(&record, 32).expect("provider outcome"),
                ThumbnailOutcome::Placeholder { reason, .. } if reason == expected
            ));
        }
        assert_eq!(png_files(service.cache.root()), 0);
    }

    #[test]
    fn pinned_libheif_images_degrade_without_writing_preview_cache() {
        let directory = tempdir().expect("tempdir");
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/formats/sources");

        for (relative, mime) in [
            ("avif/libheif-example.avif", "image/avif"),
            ("heic/libheif-example.heic", "image/heic"),
        ] {
            let path = root.join(relative);
            let metadata = fs::metadata(&path).expect("pinned libheif metadata");
            let source_digest = digest_file(&path).expect("source digest");
            let mut record = AssetRecord::untagged(
                path.to_string_lossy().into_owned(),
                path,
                mime.into(),
                metadata.len(),
                0,
            );
            assert!(
                !service
                    .enrich_media_properties(&mut record, &root)
                    .expect("core-only property downgrade")
            );
            assert!(record.dimensions.is_none());
            assert!(record.media.is_none());
            assert!(matches!(
                service.request(&record, 32).expect("provider outcome"),
                ThumbnailOutcome::Placeholder {
                    reason: ThumbnailPlaceholderReason::CodecUnavailable,
                    ..
                }
            ));
            assert_eq!(
                digest_file(&record.path).expect("source digest after"),
                source_digest
            );
        }
        assert_eq!(png_files(service.cache.root()), 0);
    }

    #[test]
    fn heif_properties_update_only_file_derived_record_fields() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.heic");
        fs::write(&asset, b"derived property fixture").expect("fixture");
        let mut record =
            AssetRecord::untagged("asset.heic".into(), asset, "image/heic".into(), 24, 0);
        record.tags.insert("user/tag".into());

        apply_heif_properties(
            &mut record,
            HeifProperties {
                width: 1280,
                height: 854,
                orientation: Some(6),
                color_space: Some("srgb".into()),
                has_alpha: Some(false),
                image_count: 2,
            },
        );

        assert_eq!(
            record.dimensions.map(|value| (value.width, value.height)),
            Some((1280, 854))
        );
        assert_eq!(
            record
                .native_metadata
                .as_ref()
                .and_then(|value| value.orientation),
            Some(6)
        );
        assert_eq!(
            record.media.as_ref().and_then(|value| value.frame_count),
            Some(2)
        );
        assert_eq!(record.tags, ["user/tag".into()].into_iter().collect());
        assert!(record.sidecar_path.is_none());
        assert!(record.sidecar_state.is_none());
    }

    #[test]
    fn safe_svg_provider_generates_a_bounded_png() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.svg");
        fs::write(
            &asset,
            b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 40 20\"><rect width=\"40\" height=\"20\"/></svg>",
        )
        .expect("SVG fixture");
        let metadata = fs::metadata(&asset).expect("metadata");
        let record = AssetRecord::untagged(
            asset.to_string_lossy().into_owned(),
            asset,
            "image/svg+xml".into(),
            metadata.len(),
            0,
        );
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");

        let thumbnail = expect_ready(service.request(&record, 16).expect("SVG thumbnail"));
        assert_eq!((thumbnail.width, thumbnail.height), (16, 8));
        assert_eq!(thumbnail.provider_id, "safe-static-svg");
        assert_eq!(thumbnail.provider_version, asset_svg::SVG_PROVIDER_VERSION);
        let bytes = service.read(&thumbnail.cache_key).expect("SVG PNG bytes");
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn unsafe_svg_is_an_invalid_content_placeholder_without_cache_output() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("script.svg");
        fs::write(
            &asset,
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
        )
        .expect("unsafe SVG fixture");
        let metadata = fs::metadata(&asset).expect("metadata");
        let record = AssetRecord::untagged(
            asset.to_string_lossy().into_owned(),
            asset,
            "image/svg+xml".into(),
            metadata.len(),
            0,
        );
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");

        assert!(matches!(
            service.request(&record, 32).expect("unsafe SVG outcome"),
            ThumbnailOutcome::Placeholder {
                reason: ThumbnailPlaceholderReason::InvalidContent,
                message,
                ..
            } if message.contains("forbidden")
        ));
        assert_eq!(png_files(service.cache.root()), 0);
    }

    #[test]
    fn cache_descriptor_binds_provider_identity_and_version() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.png");
        write_image(&asset, ImageFormat::Png);
        let service = ThumbnailService::open(&directory.path().join("cache"), 1).expect("service");

        let thumbnail = expect_ready(service.request(&record(&asset), 32).expect("thumbnail"));
        let descriptor: serde_json::Value = serde_json::from_slice(
            &fs::read(cache_entry_path(&service, &thumbnail.cache_key, "json"))
                .expect("descriptor bytes"),
        )
        .expect("descriptor JSON");

        assert_eq!(thumbnail.provider_id, "builtin-raster");
        assert_eq!(thumbnail.provider_version, THUMBNAIL_DECODER_VERSION);
        assert_eq!(descriptor["providerId"], thumbnail.provider_id);
        assert_eq!(descriptor["providerVersion"], thumbnail.provider_version);
        assert!(descriptor.get("decoderVersion").is_none());
    }

    fn record(path: &Path) -> AssetRecord {
        let metadata = fs::metadata(path).expect("metadata");
        AssetRecord::untagged(
            path.to_string_lossy().into_owned(),
            path.to_path_buf(),
            mime_for(path).into(),
            metadata.len(),
            0,
        )
    }

    fn mime_for(path: &Path) -> &'static str {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("jpg") => "image/jpeg",
            Some("webp") => "image/webp",
            Some("gif") => "image/gif",
            _ => "image/png",
        }
    }

    fn write_image(path: &Path, format: ImageFormat) {
        write_solid_image(path, format, 80, 40, [255, 0, 0, 255]);
    }

    fn write_solid_image(
        path: &Path,
        format: ImageFormat,
        width: u32,
        height: u32,
        color: [u8; 4],
    ) {
        let pixels = ImageBuffer::from_pixel(width, height, Rgba(color));
        let image = DynamicImage::ImageRgba8(pixels);
        let file = File::create(path).expect("create image");
        image
            .write_to(BufWriter::new(file), format)
            .expect("write image");
    }

    fn write_animated_gif(path: &Path) {
        let file = File::create(path).expect("create gif");
        let mut encoder = GifEncoder::new(file);
        encoder.set_repeat(Repeat::Infinite).expect("repeat");
        let red = Frame::new(ImageBuffer::from_pixel(8, 8, Rgba([255, 0, 0, 255])));
        let blue = Frame::new(ImageBuffer::from_pixel(8, 8, Rgba([0, 0, 255, 255])));
        encoder.encode_frames([red, blue]).expect("encode frames");
    }

    fn expect_ready(outcome: ThumbnailOutcome) -> super::ThumbnailReady {
        match outcome {
            ThumbnailOutcome::Ready { thumbnail } => thumbnail,
            ThumbnailOutcome::Placeholder { message, .. } => panic!("placeholder: {message}"),
        }
    }

    fn cache_entry_path(service: &ThumbnailService, cache_key: &str, extension: &str) -> PathBuf {
        service
            .cache
            .root()
            .join(&cache_key[..2])
            .join(format!("{cache_key}.{extension}"))
    }

    fn png_files(root: &Path) -> usize {
        let mut count = 0;
        let mut pending = vec![PathBuf::from(root)];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).expect("cache directory") {
                let entry = entry.expect("cache entry");
                if entry.file_type().expect("file type").is_dir() {
                    pending.push(entry.path());
                } else if entry.path().extension().is_some_and(|value| value == "png") {
                    count += 1;
                }
            }
        }
        count
    }
}
