use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use asset_core::{AssetKind, AssetRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use cache::ThumbnailCache;
use decoder::decode_thumbnail;
use limiter::DecodeLimiter;

mod cache;
mod decoder;
mod limiter;

pub const THUMBNAIL_DECODER_VERSION: &str = "image-0.25.9-triangle-png-v1";
pub const THUMBNAIL_CACHE_LAYOUT_VERSION: u32 = 1;
pub const MIN_THUMBNAIL_EDGE: u32 = 16;
pub const MAX_THUMBNAIL_EDGE: u32 = 2_048;

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
    pub decoder_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThumbnailPlaceholderReason {
    MissingAsset,
    UnsupportedFormat,
    Unreadable,
    DecodeFailed,
    SourceChanged,
}

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
    RebuiltMissingMarker,
    RebuiltIncompatible,
}

impl std::fmt::Display for CacheStartupDisposition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Created => "created",
            Self::Reused => "reused",
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
    pub byte_count: u64,
}

#[derive(Debug, Error)]
pub enum PreviewError {
    #[error(
        "thumbnail edge must be between {MIN_THUMBNAIL_EDGE} and {MAX_THUMBNAIL_EDGE}, got {0}"
    )]
    InvalidMaxEdge(u32),
    #[error("thumbnail concurrency must be between 1 and 32, got {0}")]
    InvalidConcurrency(usize),
    #[error("unsafe thumbnail cache root: {0}")]
    UnsafeCacheRoot(PathBuf),
    #[error("thumbnail cache I/O error at {path}: {source}")]
    CacheIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid thumbnail cache key: {0}")]
    InvalidCacheKey(String),
    #[error("thumbnail cache entry does not exist: {0}")]
    MissingCacheEntry(String),
    #[error("shared state lock is poisoned: {0}")]
    PoisonedLock(&'static str),
}

#[derive(Debug)]
pub struct ThumbnailService {
    cache: ThumbnailCache,
    limiter: DecodeLimiter,
    startup: CacheStartupReport,
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
        let (cache, startup) = ThumbnailCache::open(base_cache_directory)?;
        Ok(Self {
            cache,
            limiter: DecodeLimiter::new(max_concurrent),
            startup,
        })
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
        if !(MIN_THUMBNAIL_EDGE..=MAX_THUMBNAIL_EDGE).contains(&max_edge) {
            return Err(PreviewError::InvalidMaxEdge(max_edge));
        }
        if record.kind != AssetKind::Image {
            return Ok(placeholder(
                record,
                ThumbnailPlaceholderReason::UnsupportedFormat,
                "asset type does not have an image thumbnail decoder".into(),
            ));
        }

        let _cache_guard = self.cache.read_guard()?;
        let Some(mut version) = read_source_version(&record.path) else {
            return Ok(placeholder(
                record,
                ThumbnailPlaceholderReason::MissingAsset,
                "asset is missing or cannot be read".into(),
            ));
        };
        let mut key = thumbnail_key(record, &version, max_edge);
        if let Some(entry) = self.cache.lookup(&key)? {
            return Ok(ready(
                record,
                key,
                entry.width,
                entry.height,
                &version,
                true,
            ));
        }

        let _permit = self.limiter.acquire()?;
        let Some(latest) = read_source_version(&record.path) else {
            return Ok(placeholder(
                record,
                ThumbnailPlaceholderReason::MissingAsset,
                "asset disappeared before thumbnail decoding".into(),
            ));
        };
        if latest != version {
            version = latest;
            key = thumbnail_key(record, &version, max_edge);
        }
        if let Some(entry) = self.cache.lookup(&key)? {
            return Ok(ready(
                record,
                key,
                entry.width,
                entry.height,
                &version,
                true,
            ));
        }

        let decoded = match decode_thumbnail(&record.path, max_edge) {
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
        self.cache.store(&key, &decoded.bytes)?;
        Ok(ready(
            record,
            key,
            decoded.width,
            decoded.height,
            &version,
            false,
        ))
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

fn thumbnail_key(record: &AssetRecord, version: &SourceVersion, max_edge: u32) -> String {
    let mut digest = Sha256::new();
    for part in [
        format!("path:{}", record.key),
        record
            .id
            .map_or_else(|| "id:none".into(), |id| format!("id:{id}")),
        version.size.to_string(),
        version.modified_unix_ns.to_string(),
        max_edge.to_string(),
        THUMBNAIL_DECODER_VERSION.into(),
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
            decoder_version: THUMBNAIL_DECODER_VERSION.into(),
        },
    }
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

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::BufWriter;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    use asset_core::{AssetKind, AssetRecord};
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{DynamicImage, Frame, ImageBuffer, ImageFormat, Rgba};
    use metadata::{digest_file, sidecar_path_for};
    use tempfile::tempdir;

    use super::{
        CacheStartupDisposition, DecodeLimiter, ThumbnailOutcome, ThumbnailPlaceholderReason,
        ThumbnailService,
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
                reason: ThumbnailPlaceholderReason::UnsupportedFormat,
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

        assert_eq!(report.removed_files, 1);
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

        let mut unsupported = record(&asset);
        unsupported.kind = AssetKind::Other;
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
    fn limiter_never_exceeds_configured_concurrency() {
        let limiter = Arc::new(DecodeLimiter::new(2));
        let barrier = Arc::new(Barrier::new(9));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let limiter = Arc::clone(&limiter);
            let barrier = Arc::clone(&barrier);
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            threads.push(thread::spawn(move || {
                barrier.wait();
                let _permit = limiter.acquire().expect("permit");
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        barrier.wait();
        for thread in threads {
            thread.join().expect("join");
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2);
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
