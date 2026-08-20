use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use asset_core::{
    AssetDimensions, AssetIssue, AssetRecord, MediaProperties, NativeImageMetadata, SidecarState,
};
use asset_media::{
    AudioContainer, MediaInspectError, VideoContainer, inspect_audio_file, inspect_video_file,
};
use asset_svg::{SvgError, inspect_svg_file};
use exif::{In, Tag, Value};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use metadata::{quick_fingerprint_file, read_sidecar_versioned, sidecar_path_for};
use resource_control::{ResourceController, ResourceError, WorkKind};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::library::{RootAccessStatus, inspect_root_access};
use crate::{MAX_SIGNATURE_BYTES, descriptor_for_extension, recognize_format};

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub recursive: bool,
    pub ignore_hidden: bool,
    pub ignore: Vec<String>,
    pub batch_size: usize,
    pub max_native_metadata_bytes: u64,
    pub max_sidecar_bytes: u64,
    pub file_parse_timeout: Duration,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            ignore_hidden: true,
            ignore: Vec::new(),
            batch_size: 64,
            max_native_metadata_bytes: 256 * 1024 * 1024,
            max_sidecar_bytes: 4 * 1024 * 1024,
            file_parse_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProblem {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub root: PathBuf,
    pub assets: Vec<AssetRecord>,
    pub problems: Vec<ScanProblem>,
    pub visited_files: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanBatch {
    pub sequence: usize,
    pub assets: Vec<AssetRecord>,
    pub problems: Vec<ScanProblem>,
    pub visited_files: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanCompletion {
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub root_id: Option<Uuid>,
    pub root: PathBuf,
    pub completion: ScanCompletion,
    pub visited_files: usize,
    pub asset_count: usize,
    pub problem_count: usize,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Default)]
pub struct ScanCancellation {
    cancelled: Arc<AtomicBool>,
}

impl ScanCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
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
    #[error("scan root became unavailable ({status}): {path}")]
    RootUnavailable {
        path: PathBuf,
        status: RootAccessStatus,
    },
    #[error("scan batch size must be greater than zero")]
    InvalidBatchSize,
    #[error("file watcher batch size must be greater than zero")]
    InvalidWatchBatchSize,
    #[error("invalid ignore rule {rule}: {message}")]
    InvalidIgnoreRule { rule: String, message: String },
    #[error("file watcher error: {0}")]
    Watch(#[from] notify::Error),
    #[error("scan resource control error: {0}")]
    Resource(#[from] ResourceError),
}

/// Scans one authorized root and returns a complete compatibility report.
///
/// # Errors
///
/// Returns [`FilesystemError`] when the root or scan options are invalid.
pub fn scan_root(root: &Path, options: &ScanOptions) -> Result<ScanReport, FilesystemError> {
    let cancellation = ScanCancellation::new();
    let mut assets = Vec::new();
    let mut problems = Vec::new();
    let summary = scan_root_incremental(None, root, options, &cancellation, |batch| {
        assets.extend(batch.assets);
        problems.extend(batch.problems);
    })?;
    assets.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(ScanReport {
        root: summary.root,
        assets,
        problems,
        visited_files: summary.visited_files,
        elapsed_ms: summary.elapsed_ms,
    })
}

/// Scans an authorized root incrementally, emitting bounded ordered batches.
///
/// Cancellation is cooperative and is observed between traversal entries and batches.
/// A cancelled scan emits all fully parsed batches produced before cancellation.
///
/// # Errors
///
/// Returns [`FilesystemError`] when the root, batch size, or ignore rules are invalid.
pub fn scan_root_incremental<F>(
    root_id: Option<Uuid>,
    root: &Path,
    options: &ScanOptions,
    cancellation: &ScanCancellation,
    emit: F,
) -> Result<ScanSummary, FilesystemError>
where
    F: FnMut(ScanBatch),
{
    let resources = ResourceController::with_defaults();
    scan_root_incremental_controlled(root_id, root, options, cancellation, &resources, emit)
}

/// Scans using a shared scheduler so scan, hash, and decode work obey one process bound.
///
/// # Errors
///
/// Returns [`FilesystemError`] when inputs are invalid or a bounded resource wait fails.
pub fn scan_root_incremental_controlled<F>(
    root_id: Option<Uuid>,
    root: &Path,
    options: &ScanOptions,
    cancellation: &ScanCancellation,
    resources: &ResourceController,
    mut emit: F,
) -> Result<ScanSummary, FilesystemError>
where
    F: FnMut(ScanBatch),
{
    if options.batch_size == 0 {
        return Err(FilesystemError::InvalidBatchSize);
    }
    let root = canonical_scan_root(root)?;
    let ignore = compile_ignore_rules(&options.ignore)?;
    let started = Instant::now();
    let mut sequence = 0;
    let mut visited_files = 0;
    let mut asset_count = 0;
    let mut problem_count = 0;
    let mut paths = Vec::with_capacity(options.batch_size);
    let mut pending_problems = Vec::new();

    let mut walker = WalkDir::new(&root).follow_links(false);
    if !options.recursive {
        walker = walker.max_depth(1);
    }

    let entries = walker.into_iter().filter_entry(|entry| {
        should_visit_entry(entry.path(), entry.depth(), &root, options, &ignore)
    });
    for entry in entries {
        if cancellation.is_cancelled() {
            break;
        }
        match entry {
            Ok(entry) if entry.file_type().is_file() => {
                let path = entry.into_path();
                if !is_metadata_file(&path) {
                    visited_files += 1;
                    paths.push(path);
                }
            }
            Ok(entry) if entry.file_type().is_symlink() => {
                pending_problems.push(ScanProblem {
                    path: entry.into_path(),
                    message: "symbolic link skipped because link traversal is disabled".into(),
                });
            }
            Ok(_) => {}
            Err(error) => pending_problems.push(ScanProblem {
                path: error.path().map_or_else(|| root.clone(), Path::to_path_buf),
                message: error.to_string(),
            }),
        }

        if paths.len() >= options.batch_size || pending_problems.len() >= options.batch_size {
            let batch = parse_batch(
                sequence,
                root_id,
                &root,
                &mut paths,
                &mut pending_problems,
                cancellation,
                options,
                resources,
            )?;
            asset_count += batch.assets.len();
            problem_count += batch.problems.len();
            emit(batch);
            sequence += 1;
            if !cancellation.is_cancelled() {
                ensure_scan_root_available(&root)?;
            }
        }
    }

    if !paths.is_empty() || !pending_problems.is_empty() {
        let batch = parse_batch(
            sequence,
            root_id,
            &root,
            &mut paths,
            &mut pending_problems,
            cancellation,
            options,
            resources,
        )?;
        asset_count += batch.assets.len();
        problem_count += batch.problems.len();
        emit(batch);
    }

    if !cancellation.is_cancelled() {
        ensure_scan_root_available(&root)?;
    }

    Ok(ScanSummary {
        root_id,
        root,
        completion: if cancellation.is_cancelled() {
            ScanCompletion::Cancelled
        } else {
            ScanCompletion::Completed
        },
        visited_files,
        asset_count,
        problem_count,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

fn canonical_scan_root(root: &Path) -> Result<PathBuf, FilesystemError> {
    if !root.is_dir() {
        return Err(FilesystemError::InvalidRoot(root.to_path_buf()));
    }
    root.canonicalize()
        .map_err(|source| FilesystemError::Canonicalize {
            path: root.to_path_buf(),
            source,
        })
}

fn ensure_scan_root_available(root: &Path) -> Result<(), FilesystemError> {
    let (status, _) = inspect_root_access(root);
    if status == RootAccessStatus::Available {
        Ok(())
    } else {
        Err(FilesystemError::RootUnavailable {
            path: root.to_path_buf(),
            status,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_batch(
    sequence: usize,
    root_id: Option<Uuid>,
    root: &Path,
    paths: &mut Vec<PathBuf>,
    problems: &mut Vec<ScanProblem>,
    cancellation: &ScanCancellation,
    options: &ScanOptions,
    resources: &ResourceController,
) -> Result<ScanBatch, FilesystemError> {
    let batch_paths = std::mem::take(paths);
    let permit = match resources.acquire_cancellable(WorkKind::Scan, || cancellation.is_cancelled())
    {
        Ok(permit) => Some(permit),
        Err(ResourceError::Cancelled(WorkKind::Scan)) => None,
        Err(error) => return Err(error.into()),
    };
    let assets = if permit.is_some() {
        batch_paths
            .iter()
            .filter_map(|path| {
                if cancellation.is_cancelled() {
                    None
                } else {
                    parse_asset(root_id, root, path, options, cancellation)
                }
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    Ok(ScanBatch {
        sequence,
        assets,
        problems: std::mem::take(problems),
        visited_files: batch_paths.len(),
    })
}

#[allow(clippy::too_many_lines)]
fn parse_asset(
    root_id: Option<Uuid>,
    root: &Path,
    path: &Path,
    options: &ScanOptions,
    cancellation: &ScanCancellation,
) -> Option<AssetRecord> {
    let started = Instant::now();
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    let extension_candidate = descriptor_for_extension(extension.as_deref());

    let canonical = match path.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) => {
            let descriptor = extension_candidate?;
            return Some(unavailable_asset(
                root_id,
                root,
                path,
                descriptor.mime.to_owned(),
                error.to_string(),
            ));
        }
    };
    if cancellation.is_cancelled() {
        return None;
    }
    let mut prefix = Vec::new();
    if let Err(error) = File::open(&canonical)
        .and_then(|file| file.take(MAX_SIGNATURE_BYTES).read_to_end(&mut prefix))
    {
        let descriptor = extension_candidate?;
        return Some(unavailable_asset(
            root_id,
            root,
            &canonical,
            descriptor.mime.to_owned(),
            error.to_string(),
        ));
    }
    let recognition = recognize_format(extension.as_deref(), &prefix)?;
    let mime = recognition.descriptor.mime.to_owned();
    let file_metadata = match fs::metadata(&canonical) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Some(unavailable_asset(
                root_id,
                root,
                &canonical,
                mime,
                error.to_string(),
            ));
        }
    };
    let modified_unix_ms = file_metadata
        .modified()
        .ok()
        .and_then(system_time_to_unix_ms)
        .unwrap_or(0);
    let key = canonical.to_string_lossy().into_owned();
    let mut asset = AssetRecord::untagged(
        key,
        canonical.clone(),
        mime,
        file_metadata.len(),
        modified_unix_ms,
    );
    asset.root_id = root_id;
    asset.relative_path = relative_path(root, &canonical);
    asset.created_unix_ms = file_metadata
        .created()
        .ok()
        .and_then(system_time_to_unix_ms);
    asset.modified_unix_ms = file_metadata
        .modified()
        .ok()
        .and_then(system_time_to_unix_ms);
    asset.file_read_only = Some(file_metadata.permissions().readonly());
    if recognition.extension_mismatch {
        asset.issues.push(AssetIssue::MimeMismatch(format!(
            "content identifies {} but the .{} extension identifies {}",
            recognition.descriptor.id,
            extension.as_deref().unwrap_or(""),
            extension_candidate.map_or("an unknown format", |descriptor| descriptor.id)
        )));
    }

    if parse_deadline_exceeded(&mut asset, started, options.file_parse_timeout) {
        return Some(asset);
    }
    if cancellation.is_cancelled() {
        return None;
    }

    if is_raster_dimension_format(&asset.mime) {
        match imagesize::size(&canonical) {
            Ok(size) => match (u32::try_from(size.width), u32::try_from(size.height)) {
                (Ok(width), Ok(height)) => {
                    asset.dimensions = Some(AssetDimensions { width, height });
                }
                _ => asset.issues.push(AssetIssue::InvalidImageMetadata(
                    "image dimensions exceed the supported range".into(),
                )),
            },
            Err(error) => asset
                .issues
                .push(AssetIssue::InvalidImageMetadata(error.to_string())),
        }
    } else if asset.mime == "image/svg+xml" {
        match inspect_svg_file(&canonical) {
            Ok(inspection) => {
                asset.dimensions = Some(AssetDimensions {
                    width: inspection.width,
                    height: inspection.height,
                });
            }
            Err(SvgError::ResourceLimited) => asset.issues.push(AssetIssue::ResourceLimited(
                SvgError::ResourceLimited.to_string(),
            )),
            Err(SvgError::UnsafeFeature(feature)) => asset
                .issues
                .push(AssetIssue::UnsafeEmbeddedContent(feature.into())),
            Err(SvgError::UnsupportedFeature(_)) => {
                asset.issues.push(AssetIssue::UnsupportedFormat);
            }
            Err(SvgError::Unreadable { .. }) => asset.issues.push(AssetIssue::UnreadableFile(
                "SVG enrichment could not reread the source".into(),
            )),
            Err(error) => asset
                .issues
                .push(AssetIssue::InvalidImageMetadata(error.to_string())),
        }
    } else if let Some(container) = video_container(&asset.mime) {
        let remaining = options.file_parse_timeout.saturating_sub(started.elapsed());
        match inspect_video_file(&canonical, container, remaining) {
            Ok(inspection) => {
                if let Some((width, height)) = inspection.width.zip(inspection.height) {
                    asset.dimensions = Some(AssetDimensions { width, height });
                }
                asset.media = Some(MediaProperties {
                    duration_ms: inspection.duration_ms,
                    video_track_count: Some(inspection.video_track_count),
                    audio_track_count: Some(inspection.audio_track_count),
                    codec: inspection.codec.map(str::to_owned),
                    ..MediaProperties::default()
                });
            }
            Err(MediaInspectError::Unreadable) => asset.issues.push(AssetIssue::UnreadableFile(
                "video enrichment could not reread the source".into(),
            )),
            Err(MediaInspectError::SourceChanged) => {
                asset.issues.push(AssetIssue::InvalidNativeMetadata(
                    "video source changed during container inspection".into(),
                ));
            }
            Err(MediaInspectError::InvalidContent) => asset.issues.push(
                AssetIssue::InvalidNativeMetadata("video container metadata is malformed".into()),
            ),
            Err(MediaInspectError::ResourceLimited) => asset.issues.push(
                AssetIssue::ResourceLimited("video container inspection limit reached".into()),
            ),
            Err(MediaInspectError::UnsupportedFeature) => {}
        }
    } else if let Some(container) = audio_container(&asset.mime) {
        let remaining = options.file_parse_timeout.saturating_sub(started.elapsed());
        match inspect_audio_file(&canonical, container, remaining) {
            Ok(inspection) => {
                asset.media = Some(MediaProperties {
                    duration_ms: inspection.duration_ms,
                    sample_rate_hz: inspection.sample_rate_hz,
                    channel_count: inspection.channel_count,
                    bit_depth: inspection.bit_depth,
                    codec: inspection.codec.map(str::to_owned),
                    ..MediaProperties::default()
                });
            }
            Err(MediaInspectError::Unreadable) => asset.issues.push(AssetIssue::UnreadableFile(
                "audio enrichment could not reread the source".into(),
            )),
            Err(MediaInspectError::SourceChanged) => {
                asset.issues.push(AssetIssue::InvalidNativeMetadata(
                    "audio source changed during container inspection".into(),
                ));
            }
            Err(MediaInspectError::InvalidContent) => asset.issues.push(
                AssetIssue::InvalidNativeMetadata("audio container metadata is malformed".into()),
            ),
            Err(MediaInspectError::ResourceLimited) => asset.issues.push(
                AssetIssue::ResourceLimited("audio container inspection limit reached".into()),
            ),
            Err(MediaInspectError::UnsupportedFeature) => {}
        }
    }

    if parse_deadline_exceeded(&mut asset, started, options.file_parse_timeout) {
        return Some(asset);
    }
    if cancellation.is_cancelled() {
        return None;
    }

    if matches!(
        asset.mime.as_str(),
        "image/jpeg" | "image/png" | "image/webp"
    ) && file_metadata.len() <= options.max_native_metadata_bytes
    {
        match read_native_image_metadata(&canonical) {
            Ok(metadata) => asset.native_metadata = metadata,
            Err(exif::Error::NotFound(_) | exif::Error::NotSupported(_)) => {}
            Err(error) => asset
                .issues
                .push(AssetIssue::InvalidNativeMetadata(error.to_string())),
        }
    } else if matches!(
        asset.mime.as_str(),
        "image/jpeg" | "image/png" | "image/webp"
    ) {
        asset.issues.push(AssetIssue::ResourceLimited(format!(
            "native metadata skipped because the source exceeds the {} byte safety limit",
            options.max_native_metadata_bytes
        )));
    }

    if parse_deadline_exceeded(&mut asset, started, options.file_parse_timeout) {
        return Some(asset);
    }
    if cancellation.is_cancelled() {
        return None;
    }
    merge_adjacent_sidecar(
        &mut asset,
        &canonical,
        file_metadata.len(),
        options.max_sidecar_bytes,
    );
    let _ = parse_deadline_exceeded(&mut asset, started, options.file_parse_timeout);
    Some(asset)
}

fn parse_deadline_exceeded(asset: &mut AssetRecord, started: Instant, timeout: Duration) -> bool {
    if started.elapsed() < timeout {
        return false;
    }
    if !asset
        .issues
        .iter()
        .any(|issue| matches!(issue, AssetIssue::ResourceLimited(_)))
    {
        asset.issues.push(AssetIssue::ResourceLimited(format!(
            "file enrichment stopped after the {} ms cooperative deadline",
            timeout.as_millis()
        )));
    }
    true
}

fn video_container(mime: &str) -> Option<VideoContainer> {
    match mime {
        "video/mp4" => Some(VideoContainer::Mp4),
        "video/quicktime" => Some(VideoContainer::Mov),
        "video/webm" => Some(VideoContainer::Webm),
        _ => None,
    }
}

fn audio_container(mime: &str) -> Option<AudioContainer> {
    match mime {
        "audio/mpeg" => Some(AudioContainer::Mp3),
        "audio/wav" => Some(AudioContainer::Wav),
        "audio/flac" => Some(AudioContainer::Flac),
        _ => None,
    }
}

fn merge_adjacent_sidecar(
    asset: &mut AssetRecord,
    asset_path: &Path,
    asset_size: u64,
    max_sidecar_bytes: u64,
) {
    let sidecar_path = sidecar_path_for(asset_path);
    if !sidecar_path.is_file() {
        return;
    }
    asset.sidecar_path = Some(sidecar_path.clone());
    if sidecar_path
        .metadata()
        .is_ok_and(|metadata| metadata.len() > max_sidecar_bytes)
    {
        asset.issues.push(AssetIssue::ResourceLimited(format!(
            "Sidecar parsing skipped because the file exceeds the {max_sidecar_bytes} byte safety limit"
        )));
        return;
    }
    match read_sidecar_versioned(&sidecar_path) {
        Ok((sidecar, version)) => {
            if sidecar.fingerprint.as_ref().is_some_and(|fingerprint| {
                fingerprint.size != asset_size
                    || fingerprint.quick_value.as_ref().is_some_and(|expected| {
                        quick_fingerprint_file(asset_path).as_ref().ok() != Some(expected)
                    })
            }) {
                asset.issues.push(AssetIssue::MismatchedSidecar(
                    "Sidecar fingerprint does not match the adjacent asset".into(),
                ));
                return;
            }
            // File-derived fields remain authoritative. Sidecars only provide user metadata.
            asset.sidecar_state = Some(SidecarState {
                schema: sidecar.schema,
                digest: version.digest,
                size: version.size,
                modified_unix_ms: version.modified_unix_ms,
                updated_at: sidecar.updated_at.clone(),
            });
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

fn unavailable_asset(
    root_id: Option<Uuid>,
    root: &Path,
    path: &Path,
    mime: String,
    message: String,
) -> AssetRecord {
    let mut asset = AssetRecord::untagged(
        path.to_string_lossy().into_owned(),
        path.to_path_buf(),
        mime,
        0,
        0,
    );
    asset.root_id = root_id;
    asset.relative_path = relative_path(root, path);
    asset.size = None;
    asset.modified_unix_ms = None;
    asset.issues.push(AssetIssue::UnreadableFile(message));
    asset
}

fn read_native_image_metadata(path: &Path) -> Result<Option<NativeImageMetadata>, exif::Error> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader)?;
    let metadata = NativeImageMetadata {
        orientation: exif
            .get_field(Tag::Orientation, In::PRIMARY)
            .and_then(|field| field.value.get_uint(0)),
        captured_at: exif_text(&exif, Tag::DateTimeOriginal)
            .or_else(|| exif_text(&exif, Tag::DateTimeDigitized))
            .or_else(|| exif_text(&exif, Tag::DateTime)),
        camera_make: exif_text(&exif, Tag::Make),
        camera_model: exif_text(&exif, Tag::Model),
        lens_model: exif_text(&exif, Tag::LensModel),
        software: exif_text(&exif, Tag::Software),
        artist: exif_text(&exif, Tag::Artist),
        copyright: exif_text(&exif, Tag::Copyright),
    };
    Ok((!metadata.is_empty()).then_some(metadata))
}

fn exif_text(exif: &exif::Exif, tag: Tag) -> Option<String> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let Value::Ascii(values) = &field.value else {
        return None;
    };
    let value = values.first()?;
    let value = String::from_utf8_lossy(value)
        .trim_matches(['\0', ' '])
        .to_owned();
    (!value.is_empty()).then_some(value)
}

fn is_raster_dimension_format(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

fn compile_ignore_rules(rules: &[String]) -> Result<GlobSet, FilesystemError> {
    let mut builder = GlobSetBuilder::new();
    for rule in rules {
        let glob = GlobBuilder::new(rule)
            .literal_separator(true)
            .build()
            .map_err(|error| FilesystemError::InvalidIgnoreRule {
                rule: rule.clone(),
                message: error.to_string(),
            })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| FilesystemError::InvalidIgnoreRule {
            rule: rules.join(", "),
            message: error.to_string(),
        })
}

fn should_visit_entry(
    path: &Path,
    depth: usize,
    root: &Path,
    options: &ScanOptions,
    ignore: &GlobSet,
) -> bool {
    if depth == 0 {
        return true;
    }
    if options.ignore_hidden
        && path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
    {
        return false;
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    !ignore.is_match(relative)
}

fn relative_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
}

fn is_metadata_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    name.ends_with(".asset.yml") || name == ".asset-library.yml"
}

fn system_time_to_unix_ms(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::time::Duration;

    use asset_core::{AssetDimensions, AssetIssue, AssetKind};
    use exif::{Field, In, Tag, Value as ExifValue};
    use metadata::{AssetSidecar, ExpectedVersion, sidecar_path_for, write_sidecar_atomic};
    use resource_control::{ResourceController, ResourceLimits};
    use serde_yaml_ng::Value;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[cfg(unix)]
    use super::RootAccessStatus;
    use super::{
        FilesystemError, ScanCancellation, ScanCompletion, ScanOptions, scan_root,
        scan_root_incremental, scan_root_incremental_controlled,
    };

    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5,
        0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn reads_file_fields_and_merges_only_user_sidecar_metadata() {
        let directory = tempdir().expect("tempdir");
        let image = directory.path().join("logo.png");
        fs::write(&image, PNG).expect("write png");
        let mut sidecar = AssetSidecar::new();
        sidecar.tags.insert("ui/icon".into());
        sidecar
            .extra
            .insert("mime".into(), Value::String("text/plain".into()));
        write_sidecar_atomic(
            &sidecar_path_for(&image),
            &sidecar,
            &ExpectedVersion::Missing,
        )
        .expect("write sidecar");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan");
        assert_eq!(report.assets.len(), 1);
        let asset = &report.assets[0];
        assert_eq!(asset.id, Some(sidecar.id));
        assert_eq!(
            asset
                .sidecar_state
                .as_ref()
                .expect("sidecar state")
                .digest
                .len(),
            64
        );
        assert!(asset.tags.contains("ui/icon"));
        assert_eq!(asset.mime, "image/png");
        assert_eq!(asset.relative_path, std::path::Path::new("logo.png"));
        assert_eq!(asset.dimensions.expect("dimensions").width, 1);
        assert_eq!(asset.dimensions.expect("dimensions").height, 1);
        assert_eq!(asset.size, Some(PNG.len() as u64));
        assert!(asset.modified_unix_ms.is_some());
        assert!(asset.issues.is_empty());
    }

    #[test]
    fn reads_dimensions_for_all_phase_one_image_formats() {
        let directory = tempdir().expect("tempdir");
        let fixtures = [
            ("sample.png", PNG.to_vec(), "image/png", (1, 1)),
            ("sample.jpg", jpeg_header(2, 3), "image/jpeg", (2, 3)),
            ("sample.gif", gif_header(4, 5), "image/gif", (4, 5)),
            ("sample.webp", webp_header(6, 7), "image/webp", (6, 7)),
        ];
        for (name, bytes, _, _) in &fixtures {
            fs::write(directory.path().join(name), bytes).expect("write image fixture");
        }

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan formats");
        assert_eq!(report.assets.len(), fixtures.len());
        for (name, _, mime, dimensions) in fixtures {
            let asset = report
                .assets
                .iter()
                .find(|asset| asset.file_name == name)
                .expect("asset by name");
            assert_eq!(asset.mime, mime);
            let actual = asset
                .dimensions
                .unwrap_or_else(|| panic!("missing dimensions for {name}: {:?}", asset.issues));
            assert_eq!((actual.width, actual.height), dimensions);
        }
    }

    #[test]
    fn retains_registered_assets_and_sidecars_when_optional_capabilities_are_unavailable() {
        let directory = tempdir().expect("tempdir");
        let fixtures: [(&str, &[u8], &str, AssetKind); 2] = [
            (
                "photo.avif",
                b"\x00\x00\x00\x18ftypavif\x00\x00\x00\x00avif",
                "image/avif",
                AssetKind::Image,
            ),
            (
                "document.pdf",
                b"%PDF-1.7",
                "application/pdf",
                AssetKind::Pdf,
            ),
        ];
        for (name, bytes, _, _) in fixtures {
            let path = directory.path().join(name);
            fs::write(&path, bytes).expect("write registered fixture");
            let mut sidecar = AssetSidecar::new();
            sidecar.tags.insert("registered/without-provider".into());
            write_sidecar_atomic(
                &sidecar_path_for(&path),
                &sidecar,
                &ExpectedVersion::Missing,
            )
            .expect("write registered fixture sidecar");
        }

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan formats");
        assert_eq!(report.assets.len(), fixtures.len());
        for (name, _, mime, kind) in fixtures {
            let asset = report
                .assets
                .iter()
                .find(|asset| asset.file_name == name)
                .expect("registered asset");
            assert_eq!(asset.mime, mime);
            assert_eq!(asset.kind, kind);
            assert!(asset.tags.contains("registered/without-provider"));
            assert!(asset.sidecar_state.is_some());
            assert!(asset.dimensions.is_none());
            assert!(asset.issues.is_empty());
        }
    }

    #[test]
    fn enriches_normal_video_fixtures_and_keeps_unknown_codecs_neutral() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/formats/sources/video");
        let report = scan_root(&root, &ScanOptions::default()).expect("scan video fixtures");

        for (name, mime, codec) in [
            ("minimal.mp4", "video/mp4", "h264"),
            ("minimal.mov", "video/quicktime", "h264"),
            ("minimal.webm", "video/webm", "vp9"),
        ] {
            let asset = report
                .assets
                .iter()
                .find(|asset| asset.file_name == name)
                .unwrap_or_else(|| panic!("missing normal video fixture {name}"));
            assert_eq!(asset.mime, mime, "{name}");
            assert_eq!(asset.kind, AssetKind::Video, "{name}");
            assert_eq!(
                asset.dimensions,
                Some(AssetDimensions {
                    width: 320,
                    height: 180
                }),
                "{name}"
            );
            let media = asset.media.as_ref().expect("normal video media properties");
            assert_eq!(media.duration_ms, Some(2_000), "{name}");
            assert_eq!(media.video_track_count, Some(1), "{name}");
            assert_eq!(media.audio_track_count, Some(1), "{name}");
            assert_eq!(media.codec.as_deref(), Some(codec), "{name}");
            assert!(asset.issues.is_empty(), "{name}: {:?}", asset.issues);
        }

        let unknown = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "unknown-codec.mp4")
            .expect("unknown codec fixture");
        assert_eq!(unknown.mime, "video/mp4");
        let media = unknown
            .media
            .as_ref()
            .expect("unknown codec media properties");
        assert_eq!(media.duration_ms, Some(2_000));
        assert_eq!(media.video_track_count, Some(1));
        assert_eq!(media.audio_track_count, Some(1));
        assert!(media.codec.is_none());
        assert!(unknown.issues.is_empty());
    }

    #[test]
    fn isolates_adversarial_video_fixtures_without_mutating_source_bytes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/formats/sources/video");
        let before = fs::read_dir(&root)
            .expect("read video fixture directory")
            .map(|entry| {
                let path = entry.expect("video fixture entry").path();
                let bytes = fs::read(&path).expect("read video fixture");
                (path, bytes)
            })
            .collect::<Vec<_>>();
        let report = scan_root(&root, &ScanOptions::default()).expect("scan video fixtures");
        assert_eq!(report.assets.len(), before.len());

        let truncated = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "truncated.mp4")
            .expect("truncated fixture");
        assert!(matches!(
            truncated.issues.as_slice(),
            [AssetIssue::InvalidNativeMetadata(_)]
        ));

        for name in ["oversized-duration.mp4", "oversized-dimensions.webm"] {
            let asset = report
                .assets
                .iter()
                .find(|asset| asset.file_name == name)
                .unwrap_or_else(|| panic!("missing resource-limited fixture {name}"));
            assert!(
                matches!(asset.issues.as_slice(), [AssetIssue::ResourceLimited(_)]),
                "{name}: {:?}",
                asset.issues
            );
        }

        let disguised_png = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "png-disguised-as-mp4.mp4")
            .expect("disguised PNG fixture");
        assert_eq!(disguised_png.mime, "image/png");
        assert_eq!(
            disguised_png.dimensions,
            Some(AssetDimensions {
                width: 16,
                height: 16
            })
        );
        assert!(matches!(
            disguised_png.issues.as_slice(),
            [AssetIssue::MimeMismatch(_)]
        ));

        let disguised_mp4 = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "mp4-disguised-as-webm.webm")
            .expect("disguised MP4 fixture");
        assert_eq!(disguised_mp4.mime, "video/mp4");
        assert_eq!(
            disguised_mp4.dimensions,
            Some(AssetDimensions {
                width: 320,
                height: 180
            })
        );
        assert!(matches!(
            disguised_mp4.issues.as_slice(),
            [AssetIssue::MimeMismatch(_)]
        ));

        for (path, expected) in before {
            assert_eq!(
                fs::read(&path).expect("reread video fixture"),
                expected,
                "{} changed",
                path.display()
            );
        }
    }

    #[test]
    fn enriches_normal_audio_fixtures_and_keeps_unknown_codecs_visible() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/formats/sources/audio");
        let report = scan_root(&root, &ScanOptions::default()).expect("scan audio fixtures");

        for (name, mime, duration, rate, channels, depth, codec) in [
            ("minimal.mp3", "audio/mpeg", 521, 44_100, 2, None, "mp3"),
            ("minimal.wav", "audio/wav", 1_000, 8_000, 1, Some(16), "pcm"),
            (
                "minimal.flac",
                "audio/flac",
                1_000,
                8_000,
                1,
                Some(16),
                "flac",
            ),
        ] {
            let asset = report
                .assets
                .iter()
                .find(|asset| asset.file_name == name)
                .unwrap_or_else(|| panic!("missing normal audio fixture {name}"));
            assert_eq!(asset.mime, mime, "{name}");
            assert_eq!(asset.kind, AssetKind::Audio, "{name}");
            let media = asset.media.as_ref().expect("normal audio properties");
            assert_eq!(media.duration_ms, Some(duration), "{name}");
            assert_eq!(media.sample_rate_hz, Some(rate), "{name}");
            assert_eq!(media.channel_count, Some(channels), "{name}");
            assert_eq!(media.bit_depth, depth, "{name}");
            assert_eq!(media.codec.as_deref(), Some(codec), "{name}");
            assert!(asset.issues.is_empty(), "{name}: {:?}", asset.issues);
        }

        let unknown = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "unknown-codec.wav")
            .expect("unknown WAV codec fixture");
        assert_eq!(unknown.kind, AssetKind::Audio);
        assert!(unknown.media.is_none());
        assert!(unknown.issues.is_empty());
    }

    #[test]
    fn isolates_adversarial_audio_fixtures_without_mutating_source_bytes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/formats/sources/audio");
        let before = fs::read_dir(&root)
            .expect("read audio fixture directory")
            .map(|entry| {
                let path = entry.expect("audio fixture entry").path();
                let bytes = fs::read(&path).expect("read audio fixture");
                (path, bytes)
            })
            .collect::<Vec<_>>();
        let report = scan_root(&root, &ScanOptions::default()).expect("scan audio fixtures");
        assert_eq!(report.assets.len(), before.len());

        let truncated = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "truncated.mp3")
            .expect("truncated audio fixture");
        assert!(matches!(
            truncated.issues.as_slice(),
            [AssetIssue::InvalidNativeMetadata(_)]
        ));
        let oversized = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "oversized-cover.mp3")
            .expect("oversized cover fixture");
        assert!(matches!(
            oversized.issues.as_slice(),
            [AssetIssue::ResourceLimited(_)]
        ));
        for (name, mime) in [
            ("png-disguised-as-mp3.mp3", "image/png"),
            ("mp3-disguised-as-wav.wav", "audio/mpeg"),
        ] {
            let asset = report
                .assets
                .iter()
                .find(|asset| asset.file_name == name)
                .unwrap_or_else(|| panic!("missing disguised fixture {name}"));
            assert_eq!(asset.mime, mime, "{name}");
            assert!(matches!(
                asset.issues.first(),
                Some(AssetIssue::MimeMismatch(_))
            ));
        }
        for (path, expected) in before {
            assert_eq!(
                fs::read(&path).expect("reread audio fixture"),
                expected,
                "{} changed",
                path.display()
            );
        }
    }

    #[test]
    fn scans_pinned_libheif_images_without_requiring_a_codec() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/formats/sources");
        let report = scan_root(&root, &ScanOptions::default()).expect("scan format fixtures");

        for (name, mime) in [
            ("libheif-example.avif", "image/avif"),
            ("libheif-example.heic", "image/heic"),
        ] {
            let asset = report
                .assets
                .iter()
                .find(|asset| asset.file_name == name)
                .expect("pinned libheif asset");
            assert_eq!(asset.mime, mime);
            assert_eq!(asset.kind, AssetKind::Image);
            assert!(asset.dimensions.is_none());
            assert!(asset.issues.is_empty());
        }
    }

    #[test]
    fn core_scan_isolates_adversarial_avif_fixtures_by_content() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/formats/sources");
        let report = scan_root(&root, &ScanOptions::default()).expect("scan format fixtures");

        for name in [
            "corrupted-bitstream.avif",
            "truncated-ftyp.avif",
            "unknown-codec.avif",
            "oversized-ispe.avif",
            "resource-limited-output.avif",
        ] {
            let asset = report
                .assets
                .iter()
                .find(|asset| asset.file_name == name)
                .unwrap_or_else(|| panic!("missing adversarial fixture {name}"));
            assert_eq!(asset.mime, "image/avif", "{name}");
            assert_eq!(asset.kind, AssetKind::Image, "{name}");
            assert!(asset.dimensions.is_none(), "{name}");
            assert!(asset.issues.is_empty(), "{name}: {:?}", asset.issues);
        }

        let disguised_png = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "png-disguised-as-avif.avif")
            .expect("PNG disguised as AVIF");
        assert_eq!(disguised_png.mime, "image/png");
        assert_eq!(
            disguised_png.dimensions,
            Some(AssetDimensions {
                width: 16,
                height: 16,
            })
        );
        assert!(matches!(
            disguised_png.issues.as_slice(),
            [AssetIssue::MimeMismatch(_)]
        ));

        let disguised_avif = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "avif-disguised-as-jpeg.jpg")
            .expect("AVIF disguised as JPEG");
        assert_eq!(disguised_avif.mime, "image/avif");
        assert!(disguised_avif.dimensions.is_none());
        assert!(matches!(
            disguised_avif.issues.as_slice(),
            [AssetIssue::MimeMismatch(_)]
        ));
    }

    #[test]
    fn extracts_safe_svg_dimensions_and_isolates_active_content() {
        let directory = tempdir().expect("tempdir");
        fs::write(
            directory.path().join("safe.svg"),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 18\"><rect width=\"32\" height=\"18\"/></svg>",
        )
        .expect("safe svg");
        fs::write(
            directory.path().join("script.svg"),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
        )
        .expect("active svg");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan SVGs");
        let safe = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "safe.svg")
            .expect("safe asset");
        assert_eq!(
            safe.dimensions,
            Some(AssetDimensions {
                width: 32,
                height: 18
            })
        );
        assert!(safe.issues.is_empty());
        let active = report
            .assets
            .iter()
            .find(|asset| asset.file_name == "script.svg")
            .expect("active asset remains visible");
        assert!(active.dimensions.is_none());
        assert!(active.issues.iter().any(|issue| matches!(
            issue,
            AssetIssue::UnsafeEmbeddedContent(feature) if feature == "script"
        )));
    }

    #[test]
    fn content_signature_wins_over_a_conflicting_registered_extension() {
        let directory = tempdir().expect("tempdir");
        fs::write(directory.path().join("renamed.jpg"), b"%PDF-1.7")
            .expect("write mismatched fixture");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan mismatch");
        let asset = report.assets.first().expect("mismatched asset retained");
        assert_eq!(asset.mime, "application/pdf");
        assert_eq!(asset.kind, AssetKind::Pdf);
        assert!(asset.dimensions.is_none());
        assert!(
            asset
                .issues
                .iter()
                .any(|issue| matches!(issue, AssetIssue::MimeMismatch(_)))
        );
    }

    #[test]
    fn reads_selected_exif_fields_without_modifying_the_image() {
        let directory = tempdir().expect("tempdir");
        let image = directory.path().join("camera.jpg");
        let bytes = jpeg_with_exif();
        fs::write(&image, &bytes).expect("write exif jpeg");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan exif jpeg");
        let asset = &report.assets[0];
        let native = asset.native_metadata.as_ref().expect("native metadata");
        assert_eq!(native.orientation, Some(6));
        assert_eq!(native.camera_make.as_deref(), Some("Material Camera"));
        assert_eq!(native.captured_at.as_deref(), Some("2026:08:14 10:20:30"));
        assert_eq!(fs::read(&image).expect("read image after scan"), bytes);
    }

    #[test]
    fn keeps_damaged_images_and_sidecars_visible_as_asset_issues() {
        let directory = tempdir().expect("tempdir");
        let damaged = directory.path().join("damaged.png");
        let healthy = directory.path().join("healthy.png");
        fs::write(&damaged, []).expect("write damaged image");
        fs::write(&healthy, PNG).expect("write healthy image");
        fs::write(sidecar_path_for(&healthy), "not: [valid").expect("write broken sidecar");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan");
        assert_eq!(report.assets.len(), 2);
        assert!(report.problems.is_empty());
        assert!(report.assets.iter().any(|asset| {
            asset.file_name == "damaged.png"
                && asset
                    .issues
                    .iter()
                    .any(|issue| matches!(issue, AssetIssue::InvalidImageMetadata(_)))
        }));
        assert!(report.assets.iter().any(|asset| {
            asset.file_name == "healthy.png"
                && asset
                    .issues
                    .iter()
                    .any(|issue| matches!(issue, AssetIssue::InvalidSidecar(_)))
        }));
    }

    #[test]
    fn emits_incremental_batches_and_honors_cancellation() {
        let directory = tempdir().expect("tempdir");
        for index in 0..10 {
            fs::write(directory.path().join(format!("{index}.png")), PNG).expect("write png");
        }
        let options = ScanOptions {
            batch_size: 2,
            ..ScanOptions::default()
        };
        let cancellation = ScanCancellation::new();
        let callback_token = cancellation.clone();
        let mut batches = Vec::new();
        let summary = scan_root_incremental(
            Some(Uuid::now_v7()),
            directory.path(),
            &options,
            &cancellation,
            |batch| {
                batches.push(batch);
                callback_token.cancel();
            },
        )
        .expect("incremental scan");

        assert_eq!(summary.completion, ScanCompletion::Cancelled);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].sequence, 0);
        assert_eq!(batches[0].assets.len(), 2);
        assert_eq!(summary.asset_count, 2);
    }

    #[test]
    fn applies_ignore_rules_and_rejects_invalid_patterns() {
        let directory = tempdir().expect("tempdir");
        fs::create_dir(directory.path().join("temp")).expect("create ignored directory");
        fs::write(directory.path().join("keep.png"), PNG).expect("write kept image");
        fs::write(directory.path().join("temp/skip.png"), PNG).expect("write ignored image");
        let options = ScanOptions {
            ignore: vec!["temp/**".into()],
            ..ScanOptions::default()
        };
        let report = scan_root(directory.path(), &options).expect("scan with ignore rule");
        assert_eq!(report.assets.len(), 1);
        assert_eq!(report.assets[0].file_name, "keep.png");

        let error = scan_root(
            directory.path(),
            &ScanOptions {
                ignore: vec!["[broken".into()],
                ..ScanOptions::default()
            },
        )
        .expect_err("invalid glob must fail");
        assert!(matches!(error, FilesystemError::InvalidIgnoreRule { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn p2_platform_symlink_loop_is_skipped_once_with_an_explicit_diagnostic() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("tempdir");
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        fs::write(nested.join("logo.png"), PNG).expect("write png");
        symlink(directory.path(), nested.join("loop")).expect("create symlink loop");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan loop");
        assert_eq!(report.assets.len(), 1);
        assert_eq!(report.visited_files, 1);
        assert_eq!(report.problems.len(), 1);
        assert!(report.problems[0].message.contains("symbolic link skipped"));
    }

    #[cfg(windows)]
    #[test]
    fn p2_platform_windows_symlink_loop_is_skipped_when_native_creation_is_available() {
        use std::os::windows::fs::symlink_dir;

        let directory = tempdir().expect("tempdir");
        let nested = directory.path().join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        fs::write(nested.join("logo.png"), PNG).expect("write png");
        if let Err(error) = symlink_dir(directory.path(), nested.join("loop")) {
            assert!(
                std::env::var("MATERIAL_EAGLE_REQUIRE_WINDOWS_SYMLINK").as_deref() != Ok("1"),
                "hosted Windows runner must permit the native symlink fixture: {error}"
            );
            eprintln!("native Windows symlink fixture skipped: {error}");
            return;
        }

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan loop");
        assert_eq!(report.assets.len(), 1);
        assert_eq!(report.visited_files, 1);
        assert_eq!(report.problems.len(), 1);
        assert!(report.problems[0].message.contains("symbolic link skipped"));
    }

    #[cfg(windows)]
    #[test]
    fn p2_platform_windows_long_path_scans_and_atomically_updates_sidecar() {
        use std::os::windows::ffi::OsStrExt;

        let directory = tempdir().expect("tempdir");
        let mut nested = directory.path().to_path_buf();
        for index in 0..4 {
            nested.push(format!("segment-{index}-{}", "a".repeat(64)));
        }
        fs::create_dir_all(&nested).expect("create long directory path");
        let image = nested.join("Caf\u{e9}-\u{7d20}\u{6750}.png");
        assert!(
            image.as_os_str().encode_wide().count() > 260,
            "fixture must exceed the legacy MAX_PATH boundary"
        );
        fs::write(&image, PNG).expect("write long-path png");

        let sidecar_path = sidecar_path_for(&image);
        let mut sidecar = AssetSidecar::new();
        sidecar.tags.insert("windows/long-path".into());
        let first = write_sidecar_atomic(&sidecar_path, &sidecar, &ExpectedVersion::Missing)
            .expect("create long-path sidecar");
        sidecar.tags.insert("atomic-replacement".into());
        write_sidecar_atomic(
            &sidecar_path,
            &sidecar,
            &ExpectedVersion::Digest(first.digest),
        )
        .expect("atomically replace long-path sidecar");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan long path");
        assert_eq!(report.assets.len(), 1);
        assert_eq!(
            report.assets[0].path,
            image.canonicalize().expect("canonical long-path image")
        );
        assert_eq!(
            report.assets[0].relative_path,
            nested
                .strip_prefix(directory.path())
                .expect("nested path remains inside fixture")
                .join("Caf\u{e9}-\u{7d20}\u{6750}.png")
        );
        assert_eq!(
            report.assets[0].tags,
            [
                "atomic-replacement".to_owned(),
                "windows/long-path".to_owned()
            ]
            .into_iter()
            .collect()
        );
        assert!(sidecar_path.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn p2_platform_disconnect_during_scan_is_non_authoritative() {
        let directory = tempdir().expect("tempdir");
        let root = directory.path().join("removable");
        fs::create_dir(&root).expect("create root");
        fs::write(root.join("first.png"), PNG).expect("write first png");
        fs::write(root.join("second.png"), PNG).expect("write second png");
        let cancellation = ScanCancellation::new();
        let mut batches = 0;

        let error = scan_root_incremental(
            None,
            &root,
            &ScanOptions {
                batch_size: 1,
                ..ScanOptions::default()
            },
            &cancellation,
            |_| {
                batches += 1;
                if root.exists() {
                    fs::remove_dir_all(&root).expect("disconnect root");
                }
            },
        )
        .expect_err("disconnected scan must not complete authoritatively");

        assert_eq!(batches, 1);
        assert!(matches!(
            error,
            FilesystemError::RootUnavailable {
                status: RootAccessStatus::Missing,
                ..
            }
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn p2_platform_linux_moved_mount_root_is_non_authoritative() {
        let directory = tempdir().expect("tempdir");
        let root = directory.path().join("mounted-library");
        let detached = directory.path().join("detached-library");
        fs::create_dir(&root).expect("create root");
        fs::write(root.join("first.png"), PNG).expect("write first png");
        fs::write(root.join("second.png"), PNG).expect("write second png");
        let cancellation = ScanCancellation::new();

        let error = scan_root_incremental(
            None,
            &root,
            &ScanOptions {
                batch_size: 1,
                ..ScanOptions::default()
            },
            &cancellation,
            |_| {
                if root.exists() {
                    fs::rename(&root, &detached).expect("detach mounted root");
                }
            },
        )
        .expect_err("detached mount must not complete authoritatively");

        assert!(matches!(
            error,
            FilesystemError::RootUnavailable {
                status: RootAccessStatus::Missing,
                ..
            }
        ));
        assert!(detached.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn p2_platform_permission_revocation_during_scan_is_non_authoritative() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("tempdir");
        let root = directory.path().join("network-share");
        fs::create_dir(&root).expect("create root");
        fs::write(root.join("first.png"), PNG).expect("write first png");
        fs::write(root.join("second.png"), PNG).expect("write second png");
        let cancellation = ScanCancellation::new();

        let result = scan_root_incremental(
            None,
            &root,
            &ScanOptions {
                batch_size: 1,
                ..ScanOptions::default()
            },
            &cancellation,
            |_| {
                fs::set_permissions(&root, fs::Permissions::from_mode(0o000))
                    .expect("revoke root permissions");
            },
        );
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("restore root permissions");

        assert!(matches!(
            result,
            Err(FilesystemError::RootUnavailable {
                status: RootAccessStatus::PermissionDenied,
                ..
            })
        ));
    }

    #[test]
    fn p2_platform_scans_native_unicode_path_without_rewriting_it() {
        let directory = tempdir().expect("tempdir");
        let file_name = "Caf\u{e9}-\u{7d20}\u{6750}.png";
        fs::write(directory.path().join(file_name), PNG).expect("write unicode png");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan unicode");
        assert_eq!(report.assets.len(), 1);
        assert_eq!(
            report.assets[0].relative_path,
            std::path::Path::new(file_name)
        );
        assert!(directory.path().join(file_name).is_file());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn p2_platform_linux_scans_case_distinct_files_as_separate_assets() {
        let directory = tempdir().expect("tempdir");
        fs::write(directory.path().join("Logo.png"), PNG).expect("write uppercase asset");
        fs::write(directory.path().join("logo.png"), PNG).expect("write lowercase asset");

        let report = scan_root(directory.path(), &ScanOptions::default()).expect("scan root");
        let mut names = report
            .assets
            .iter()
            .map(|asset| asset.file_name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(names, ["Logo.png", "logo.png"]);
    }

    #[test]
    fn shared_scheduler_accounts_scan_work_and_bounds_enrichment() {
        let directory = tempdir().expect("tempdir");
        let image = directory.path().join("large.png");
        fs::write(&image, PNG).expect("write png");
        let sidecar = AssetSidecar::new();
        write_sidecar_atomic(
            &sidecar_path_for(&image),
            &sidecar,
            &ExpectedVersion::Missing,
        )
        .expect("write sidecar");
        let resources = ResourceController::new(ResourceLimits {
            foreground_total: 1,
            background_total: 1,
            scan: 1,
            hash: 1,
            decode: 1,
            max_waiters: 2,
            wait_timeout: Duration::from_secs(1),
        })
        .expect("resources");
        let cancellation = ScanCancellation::new();
        let mut assets = Vec::new();
        scan_root_incremental_controlled(
            None,
            directory.path(),
            &ScanOptions {
                max_native_metadata_bytes: 1,
                max_sidecar_bytes: 1,
                ..ScanOptions::default()
            },
            &cancellation,
            &resources,
            |batch| assets.extend(batch.assets),
        )
        .expect("controlled scan");

        assert_eq!(resources.snapshot().expect("snapshot").scan.completed, 1);
        assert!(assets[0].dimensions.is_some());
        assert!(assets[0].id.is_none());
        assert!(
            assets[0]
                .issues
                .iter()
                .any(|issue| matches!(issue, AssetIssue::ResourceLimited(_)))
        );
    }

    #[test]
    fn zero_parse_deadline_stops_optional_file_enrichment() {
        let directory = tempdir().expect("tempdir");
        fs::write(directory.path().join("deadline.png"), PNG).expect("write png");
        let report = scan_root(
            directory.path(),
            &ScanOptions {
                file_parse_timeout: Duration::ZERO,
                ..ScanOptions::default()
            },
        )
        .expect("scan");

        assert!(report.assets[0].dimensions.is_none());
        assert!(matches!(
            report.assets[0].issues.as_slice(),
            [AssetIssue::ResourceLimited(_)]
        ));
    }

    fn jpeg_header(width: u16, height: u16) -> Vec<u8> {
        let [height_high, height_low] = height.to_be_bytes();
        let [width_high, width_low] = width.to_be_bytes();
        vec![
            0xff,
            0xd8,
            0xff,
            0xc0,
            0x00,
            0x0b,
            0x08,
            height_high,
            height_low,
            width_high,
            width_low,
            0x01,
            0x01,
            0x11,
            0x00,
            0xff,
            0xd9,
        ]
    }

    fn gif_header(width: u16, height: u16) -> Vec<u8> {
        let [width_low, width_high] = width.to_le_bytes();
        let [height_low, height_high] = height.to_le_bytes();
        vec![
            b'G',
            b'I',
            b'F',
            b'8',
            b'9',
            b'a',
            width_low,
            width_high,
            height_low,
            height_high,
            0,
            0,
            0,
        ]
    }

    fn webp_header(width: u32, height: u32) -> Vec<u8> {
        let width = (width - 1).to_le_bytes();
        let height = (height - 1).to_le_bytes();
        vec![
            b'R', b'I', b'F', b'F', 22, 0, 0, 0, b'W', b'E', b'B', b'P', b'V', b'P', b'8', b'X',
            10, 0, 0, 0, 0, 0, 0, 0, width[0], width[1], width[2], height[0], height[1], height[2],
        ]
    }

    fn jpeg_with_exif() -> Vec<u8> {
        let fields = [
            Field {
                tag: Tag::Orientation,
                ifd_num: In::PRIMARY,
                value: ExifValue::Short(vec![6]),
            },
            Field {
                tag: Tag::Make,
                ifd_num: In::PRIMARY,
                value: ExifValue::Ascii(vec![b"Material Camera\0".to_vec()]),
            },
            Field {
                tag: Tag::DateTimeOriginal,
                ifd_num: In::PRIMARY,
                value: ExifValue::Ascii(vec![b"2026:08:14 10:20:30\0".to_vec()]),
            },
        ];
        let mut writer = exif::experimental::Writer::new();
        for field in &fields {
            writer.push_field(field);
        }
        let mut tiff = Cursor::new(Vec::new());
        writer.write(&mut tiff, false).expect("write exif block");
        let tiff = tiff.into_inner();
        let app1_length = u16::try_from(tiff.len() + 8).expect("app1 length");
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend(app1_length.to_be_bytes());
        jpeg.extend(b"Exif\0\0");
        jpeg.extend(tiff);
        jpeg.extend(jpeg_header(8, 9).into_iter().skip(2));
        jpeg
    }
}
