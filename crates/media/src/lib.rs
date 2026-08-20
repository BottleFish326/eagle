use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration as WallDuration, Instant, SystemTime};

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::video::VideoCodecId;
use symphonia::core::codecs::video::well_known::{
    CODEC_ID_AV1, CODEC_ID_H264, CODEC_ID_HEVC, CODEC_ID_MJPEG, CODEC_ID_MPEG1, CODEC_ID_MPEG2,
    CODEC_ID_MPEG4, CODEC_ID_VP8, CODEC_ID_VP9,
};
use symphonia::core::common::Limit;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use thiserror::Error;

pub const MAX_CONTAINER_METADATA_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_CONTAINER_IO_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_CONTAINER_ELEMENTS: u64 = 4_096;
pub const MAX_MEDIA_DURATION_MS: u64 = 365 * 24 * 60 * 60 * 1_000;
pub const MAX_MEDIA_TRACKS: usize = 256;
pub const MAX_VIDEO_DIMENSION: u32 = 65_535;

const EBML_ID_HEADER: u64 = 0x1a45_dfa3;
const EBML_ID_SEGMENT: u64 = 0x1853_8067;
const EBML_ID_INFO: u64 = 0x1549_a966;
const EBML_ID_TRACKS: u64 = 0x1654_ae6b;
const EBML_ID_CLUSTER: u64 = 0x1f43_b675;
const EBML_ID_VOID: u64 = 0xec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoContainer {
    Mp4,
    Mov,
    Webm,
}

impl VideoContainer {
    const fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Webm => "webm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoInspection {
    pub duration_ms: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_track_count: u32,
    pub audio_track_count: u32,
    pub codec: Option<&'static str>,
}

#[derive(Debug, Error)]
pub enum MediaInspectError {
    #[error("media source is unreadable")]
    Unreadable,
    #[error("media source changed during inspection")]
    SourceChanged,
    #[error("media container is malformed")]
    InvalidContent,
    #[error("media container exceeds a fixed inspection limit")]
    ResourceLimited,
    #[error("media container uses an unsupported feature")]
    UnsupportedFeature,
}

/// Inspects container-level video properties without decoding media packets.
///
/// The source remains authoritative and is never modified. The parser uses only the explicitly
/// enabled Symphonia ISO MP4 and Matroska/WebM demuxers, after a bounded structural preflight. A
/// read/seek budget and wall-clock deadline remain active while the demuxer constructs track data.
///
/// # Errors
///
/// Returns a stable failure when the source is unreadable, changes, is malformed, exceeds an
/// inspection limit, or uses an unsupported container feature.
pub fn inspect_video_file(
    path: &Path,
    container: VideoContainer,
    timeout: WallDuration,
) -> Result<VideoInspection, MediaInspectError> {
    let before = source_version(path)?;
    if timeout.is_zero() {
        return Err(MediaInspectError::ResourceLimited);
    }
    match container {
        VideoContainer::Mp4 | VideoContainer::Mov => preflight_iso_bmff(path, before.len)?,
        VideoContainer::Webm => preflight_webm(path, before.len)?,
    }

    let state = Arc::new(SourceLimitState::new(timeout));
    let file = File::open(path).map_err(|_| MediaInspectError::Unreadable)?;
    let source = BoundedMediaSource {
        file,
        len: before.len,
        state: Arc::clone(&state),
    };
    let stream = MediaSourceStream::new(
        Box::new(source),
        MediaSourceStreamOptions {
            buffer_len: 64 * 1024,
        },
    );
    let mut hint = Hint::new();
    hint.with_extension(container.extension());
    let metadata_options = MetadataOptions::default()
        .limit_tag_bytes(Limit::Maximum(64 * 1024))
        .limit_visual_bytes(Limit::Maximum(0));
    let probed = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        metadata_options,
    );
    let format = match probed {
        Ok(format) => format,
        Err(error) => return Err(classify_symphonia_error(&error, &state)),
    };

    if format.tracks().len() > MAX_MEDIA_TRACKS {
        return Err(MediaInspectError::ResourceLimited);
    }
    let duration_ms = duration_milliseconds(format.media_info())?;
    if duration_ms.is_some_and(|duration| duration > MAX_MEDIA_DURATION_MS) {
        return Err(MediaInspectError::ResourceLimited);
    }

    let mut video_track_count = 0_u32;
    let mut audio_track_count = 0_u32;
    let mut untyped_track_count = 0_u32;
    let mut dimensions = None;
    let mut codec = None;
    for track in format.tracks() {
        match track.track_type() {
            Some(TrackType::Video) => {
                video_track_count = video_track_count.saturating_add(1);
                if let Some(CodecParameters::Video(parameters)) = &track.codec_params {
                    if dimensions.is_none() {
                        dimensions = parameters
                            .width
                            .zip(parameters.height)
                            .map(|(width, height)| (u32::from(width), u32::from(height)));
                    }
                    codec = codec.or_else(|| video_codec_name(parameters.codec));
                }
            }
            Some(TrackType::Audio) => {
                audio_track_count = audio_track_count.saturating_add(1);
            }
            Some(TrackType::Subtitle) => {}
            Some(_) | None => untyped_track_count = untyped_track_count.saturating_add(1),
        }
    }
    if video_track_count == 0 {
        // Unknown visual sample entries have no public Symphonia TrackType. For a registered video
        // container, preserve them as neutral video tracks instead of treating codec absence as
        // corruption or hiding the file.
        video_track_count = untyped_track_count;
    }
    if let Some((width, height)) = dimensions {
        if width == 0 || height == 0 || width > MAX_VIDEO_DIMENSION || height > MAX_VIDEO_DIMENSION
        {
            return Err(MediaInspectError::ResourceLimited);
        }
    }

    drop(format);
    let after = source_version(path)?;
    if before != after {
        return Err(MediaInspectError::SourceChanged);
    }
    Ok(VideoInspection {
        duration_ms,
        width: dimensions.map(|value| value.0),
        height: dimensions.map(|value| value.1),
        video_track_count,
        audio_track_count,
        codec,
    })
}

fn duration_milliseconds(
    media: &symphonia::core::formats::MediaInfo,
) -> Result<Option<u64>, MediaInspectError> {
    let Some((time_base, duration)) = media.time_base.zip(media.duration) else {
        return Ok(None);
    };
    let time = time_base
        .calc_duration(duration)
        .ok_or(MediaInspectError::ResourceLimited)?;
    u64::try_from(time.as_millis())
        .map(Some)
        .map_err(|_| MediaInspectError::ResourceLimited)
}

fn video_codec_name(codec: VideoCodecId) -> Option<&'static str> {
    match codec {
        CODEC_ID_H264 => Some("h264"),
        CODEC_ID_HEVC => Some("hevc"),
        CODEC_ID_VP8 => Some("vp8"),
        CODEC_ID_VP9 => Some("vp9"),
        CODEC_ID_AV1 => Some("av1"),
        CODEC_ID_MPEG1 => Some("mpeg1-video"),
        CODEC_ID_MPEG2 => Some("mpeg2-video"),
        CODEC_ID_MPEG4 => Some("mpeg4-video"),
        CODEC_ID_MJPEG => Some("mjpeg"),
        _ => None,
    }
}

fn classify_symphonia_error(error: &SymphoniaError, state: &SourceLimitState) -> MediaInspectError {
    if state.limited.load(Ordering::Acquire) {
        return MediaInspectError::ResourceLimited;
    }
    match error {
        SymphoniaError::LimitError(_)
        | SymphoniaError::Unsupported(
            "mkv: video width too large" | "mkv: video height too large",
        ) => MediaInspectError::ResourceLimited,
        SymphoniaError::IoError(_) | SymphoniaError::DecodeError(_) => {
            MediaInspectError::InvalidContent
        }
        SymphoniaError::SeekError(_) | SymphoniaError::ResetRequired => {
            MediaInspectError::UnsupportedFeature
        }
        _ => MediaInspectError::UnsupportedFeature,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceVersion {
    len: u64,
    modified: Option<SystemTime>,
}

fn source_version(path: &Path) -> Result<SourceVersion, MediaInspectError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| MediaInspectError::Unreadable)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MediaInspectError::Unreadable);
    }
    Ok(SourceVersion {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn preflight_iso_bmff(path: &Path, file_len: u64) -> Result<(), MediaInspectError> {
    let mut file = File::open(path).map_err(|_| MediaInspectError::Unreadable)?;
    let mut position = 0_u64;
    let mut count = 0_u64;
    let mut found_ftyp = false;
    let mut found_moov = false;
    while position < file_len {
        count = count.saturating_add(1);
        if count > MAX_CONTAINER_ELEMENTS {
            return Err(MediaInspectError::ResourceLimited);
        }
        file.seek(SeekFrom::Start(position))
            .map_err(|_| MediaInspectError::Unreadable)?;
        let mut header = [0_u8; 16];
        file.read_exact(&mut header[..8])
            .map_err(|_| MediaInspectError::InvalidContent)?;
        let size32 = u32::from_be_bytes(header[..4].try_into().expect("four-byte slice"));
        let atom_type: [u8; 4] = header[4..8].try_into().expect("four-byte slice");
        let (header_len, size) = match size32 {
            0 => (8_u64, file_len.saturating_sub(position)),
            1 => {
                file.read_exact(&mut header[8..16])
                    .map_err(|_| MediaInspectError::InvalidContent)?;
                (
                    16,
                    u64::from_be_bytes(header[8..16].try_into().expect("eight-byte slice")),
                )
            }
            value => (8, u64::from(value)),
        };
        if size < header_len {
            return Err(MediaInspectError::InvalidContent);
        }
        let end = position
            .checked_add(size)
            .filter(|end| *end <= file_len)
            .ok_or(MediaInspectError::InvalidContent)?;
        match &atom_type {
            b"ftyp" => found_ftyp = true,
            b"moov" => {
                if found_moov {
                    return Err(MediaInspectError::InvalidContent);
                }
                if size > MAX_CONTAINER_METADATA_BYTES {
                    return Err(MediaInspectError::ResourceLimited);
                }
                found_moov = true;
            }
            b"meta" | b"sidx" if size > MAX_CONTAINER_METADATA_BYTES => {
                return Err(MediaInspectError::ResourceLimited);
            }
            _ => {}
        }
        if end == position {
            return Err(MediaInspectError::InvalidContent);
        }
        position = end;
    }
    if !found_ftyp || !found_moov {
        return Err(MediaInspectError::InvalidContent);
    }
    Ok(())
}

fn preflight_webm(path: &Path, file_len: u64) -> Result<(), MediaInspectError> {
    let mut file = File::open(path).map_err(|_| MediaInspectError::Unreadable)?;
    let header = read_ebml_header(&mut file, 0, file_len)?;
    if header.id != EBML_ID_HEADER {
        return Err(MediaInspectError::InvalidContent);
    }
    let header_end = header.end(file_len)?;
    let segment = read_ebml_header(&mut file, header_end, file_len)?;
    if segment.id != EBML_ID_SEGMENT {
        return Err(MediaInspectError::InvalidContent);
    }
    let segment_end = segment.end(file_len)?;
    let mut position = segment.payload_start;
    let mut count = 0_u64;
    let mut metadata_bytes = 0_u64;
    let mut found_info = false;
    let mut found_tracks = false;
    while position < segment_end {
        count = count.saturating_add(1);
        if count > MAX_CONTAINER_ELEMENTS {
            return Err(MediaInspectError::ResourceLimited);
        }
        let child = read_ebml_header(&mut file, position, segment_end)?;
        let child_end = child.end(segment_end)?;
        if child.id == EBML_ID_INFO {
            found_info = true;
        } else if child.id == EBML_ID_TRACKS {
            found_tracks = true;
        }
        if !matches!(child.id, EBML_ID_CLUSTER | EBML_ID_VOID) {
            let element_bytes = child_end.saturating_sub(position);
            if element_bytes > MAX_CONTAINER_METADATA_BYTES {
                return Err(MediaInspectError::ResourceLimited);
            }
            metadata_bytes = metadata_bytes
                .checked_add(element_bytes)
                .filter(|bytes| *bytes <= MAX_CONTAINER_IO_BYTES)
                .ok_or(MediaInspectError::ResourceLimited)?;
        }
        if child.size.is_none() {
            if child.id == EBML_ID_CLUSTER {
                break;
            }
            return Err(MediaInspectError::UnsupportedFeature);
        }
        if child_end == position {
            return Err(MediaInspectError::InvalidContent);
        }
        position = child_end;
    }
    if !found_info || !found_tracks {
        return Err(MediaInspectError::InvalidContent);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct EbmlHeader {
    id: u64,
    payload_start: u64,
    size: Option<u64>,
}

impl EbmlHeader {
    fn end(self, parent_end: u64) -> Result<u64, MediaInspectError> {
        self.size.map_or(Ok(parent_end), |size| {
            self.payload_start
                .checked_add(size)
                .filter(|end| *end <= parent_end)
                .ok_or(MediaInspectError::InvalidContent)
        })
    }
}

fn read_ebml_header(
    file: &mut File,
    position: u64,
    parent_end: u64,
) -> Result<EbmlHeader, MediaInspectError> {
    file.seek(SeekFrom::Start(position))
        .map_err(|_| MediaInspectError::Unreadable)?;
    let (id, id_width, _) = read_ebml_vint(file, true)?;
    let (size_value, size_width, unknown) = read_ebml_vint(file, false)?;
    let payload_start = position
        .checked_add(u64::from(id_width) + u64::from(size_width))
        .filter(|start| *start <= parent_end)
        .ok_or(MediaInspectError::InvalidContent)?;
    Ok(EbmlHeader {
        id,
        payload_start,
        size: (!unknown).then_some(size_value),
    })
}

fn read_ebml_vint(
    file: &mut File,
    preserve_marker: bool,
) -> Result<(u64, u8, bool), MediaInspectError> {
    let mut first = [0_u8; 1];
    file.read_exact(&mut first)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    if first[0] == 0 {
        return Err(MediaInspectError::InvalidContent);
    }
    let width =
        u8::try_from(first[0].leading_zeros()).map_err(|_| MediaInspectError::InvalidContent)? + 1;
    let maximum_width = if preserve_marker { 4 } else { 8 };
    if width > maximum_width {
        return Err(MediaInspectError::InvalidContent);
    }
    let marker = 1_u8 << (8 - width);
    let mut value = if preserve_marker {
        u64::from(first[0])
    } else {
        u64::from(first[0] & !marker)
    };
    for _ in 1..width {
        let mut byte = [0_u8; 1];
        file.read_exact(&mut byte)
            .map_err(|_| MediaInspectError::InvalidContent)?;
        value = (value << 8) | u64::from(byte[0]);
    }
    let unknown = !preserve_marker && value == (1_u64 << (7 * width)) - 1;
    Ok((value, width, unknown))
}

struct SourceLimitState {
    started: Instant,
    timeout: WallDuration,
    bytes_read: AtomicU64,
    seeks: AtomicU64,
    limited: AtomicBool,
}

impl SourceLimitState {
    fn new(timeout: WallDuration) -> Self {
        Self {
            started: Instant::now(),
            timeout,
            bytes_read: AtomicU64::new(0),
            seeks: AtomicU64::new(0),
            limited: AtomicBool::new(false),
        }
    }

    fn admit(&self, requested_bytes: usize, seek: bool) -> io::Result<usize> {
        if self.started.elapsed() >= self.timeout {
            return self.reject();
        }
        if seek {
            let seeks = self.seeks.fetch_add(1, Ordering::AcqRel).saturating_add(1);
            if seeks > MAX_CONTAINER_ELEMENTS {
                return self.reject();
            }
        }
        let consumed = self.bytes_read.load(Ordering::Acquire);
        let remaining = MAX_CONTAINER_IO_BYTES.saturating_sub(consumed);
        let admitted = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(requested_bytes);
        if admitted == 0 && requested_bytes > 0 {
            return self.reject();
        }
        Ok(admitted)
    }

    fn account_read(&self, bytes: usize) {
        self.bytes_read.fetch_add(bytes as u64, Ordering::AcqRel);
    }

    fn reject<T>(&self) -> io::Result<T> {
        self.limited.store(true, Ordering::Release);
        Err(io::Error::other("media inspection resource limit reached"))
    }
}

struct BoundedMediaSource {
    file: File,
    len: u64,
    state: Arc<SourceLimitState>,
}

impl Read for BoundedMediaSource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let admitted = self.state.admit(buffer.len(), false)?;
        let read = self.file.read(&mut buffer[..admitted])?;
        self.state.account_read(read);
        Ok(read)
    }
}

impl Seek for BoundedMediaSource {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let _ = self.state.admit(0, true)?;
        self.file.seek(position)
    }
}

impl MediaSource for BoundedMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/formats/sources/video")
            .join(relative)
    }

    #[test]
    fn inspects_generated_mp4_mov_and_webm_without_decoding_packets() {
        for (name, container, codec) in [
            ("minimal.mp4", VideoContainer::Mp4, "h264"),
            ("minimal.mov", VideoContainer::Mov, "h264"),
            ("minimal.webm", VideoContainer::Webm, "vp9"),
        ] {
            let inspection =
                inspect_video_file(&fixture(name), container, WallDuration::from_secs(1))
                    .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(inspection.duration_ms, Some(2_000), "{name}");
            assert_eq!(
                (inspection.width, inspection.height),
                (Some(320), Some(180))
            );
            assert_eq!(inspection.video_track_count, 1);
            assert_eq!(inspection.audio_track_count, 1);
            assert_eq!(inspection.codec, Some(codec));
        }
    }

    #[test]
    fn isolates_truncation_unknown_codecs_and_declared_resource_bombs() {
        assert!(matches!(
            inspect_video_file(
                &fixture("truncated.mp4"),
                VideoContainer::Mp4,
                WallDuration::from_secs(1)
            ),
            Err(MediaInspectError::InvalidContent)
        ));
        let unknown = inspect_video_file(
            &fixture("unknown-codec.mp4"),
            VideoContainer::Mp4,
            WallDuration::from_secs(1),
        )
        .expect("unknown codec must retain neutral container properties");
        assert_eq!(unknown.duration_ms, Some(2_000));
        assert_eq!(unknown.video_track_count, 1);
        assert_eq!(unknown.audio_track_count, 1);
        assert_eq!(unknown.codec, None);
        for (name, container) in [
            ("oversized-duration.mp4", VideoContainer::Mp4),
            ("oversized-dimensions.webm", VideoContainer::Webm),
        ] {
            assert!(matches!(
                inspect_video_file(&fixture(name), container, WallDuration::from_secs(1)),
                Err(MediaInspectError::ResourceLimited)
            ));
        }
    }

    #[test]
    fn rejects_zero_deadlines_and_oversized_iso_metadata_before_demuxing() {
        assert!(matches!(
            inspect_video_file(
                &fixture("minimal.mp4"),
                VideoContainer::Mp4,
                WallDuration::ZERO
            ),
            Err(MediaInspectError::ResourceLimited)
        ));
        let directory = tempfile::tempdir().expect("temp directory");
        let oversized = directory.path().join("oversized.mp4");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16_u32.to_be_bytes());
        bytes.extend_from_slice(b"ftypisom\0\0\0\0");
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(b"moov");
        bytes.extend_from_slice(&(MAX_CONTAINER_METADATA_BYTES + 1).to_be_bytes());
        fs::write(&oversized, bytes).expect("write oversized header");
        assert!(matches!(
            inspect_video_file(&oversized, VideoContainer::Mp4, WallDuration::from_secs(1)),
            Err(MediaInspectError::InvalidContent | MediaInspectError::ResourceLimited)
        ));
    }
}
