use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration as WallDuration, Instant, SystemTime};

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioCodecId;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_PCM_F32BE, CODEC_ID_PCM_F32LE, CODEC_ID_PCM_F64BE,
    CODEC_ID_PCM_F64LE, CODEC_ID_PCM_S8, CODEC_ID_PCM_S16BE, CODEC_ID_PCM_S16LE,
    CODEC_ID_PCM_S24BE, CODEC_ID_PCM_S24LE, CODEC_ID_PCM_S32BE, CODEC_ID_PCM_S32LE,
    CODEC_ID_PCM_U8, CODEC_ID_PCM_U16BE, CODEC_ID_PCM_U16LE, CODEC_ID_PCM_U24BE,
    CODEC_ID_PCM_U24LE, CODEC_ID_PCM_U32BE, CODEC_ID_PCM_U32LE,
};
use symphonia::core::codecs::video::VideoCodecId;
use symphonia::core::codecs::video::well_known::{
    CODEC_ID_AV1, CODEC_ID_H264, CODEC_ID_HEVC, CODEC_ID_MJPEG, CODEC_ID_MPEG1, CODEC_ID_MPEG2,
    CODEC_ID_MPEG4, CODEC_ID_VP8, CODEC_ID_VP9,
};
use symphonia::core::common::Limit;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use thiserror::Error;

pub const MAX_CONTAINER_METADATA_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_CONTAINER_IO_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_CONTAINER_ELEMENTS: u64 = 4_096;
pub const MAX_MEDIA_DURATION_MS: u64 = 365 * 24 * 60 * 60 * 1_000;
pub const MAX_MEDIA_TRACKS: usize = 256;
pub const MAX_VIDEO_DIMENSION: u32 = 65_535;
pub const MAX_AUDIO_COVER_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_AUDIO_SAMPLE_RATE_HZ: u32 = 768_000;
pub const MAX_AUDIO_CHANNELS: u32 = 64;
pub const MAX_AUDIO_BIT_DEPTH: u32 = 64;

const EBML_ID_HEADER: u64 = 0x1a45_dfa3;
const EBML_ID_SEGMENT: u64 = 0x1853_8067;
const EBML_ID_INFO: u64 = 0x1549_a966;
const EBML_ID_TRACKS: u64 = 0x1654_ae6b;
const EBML_ID_CLUSTER: u64 = 0x1f43_b675;
const EBML_ID_VOID: u64 = 0xec;
const EBML_ID_TRACK_ENTRY: u64 = 0xae;
const EBML_ID_TRACK_TYPE: u64 = 0x83;
const EBML_ID_VIDEO: u64 = 0xe0;
const EBML_ID_PROJECTION: u64 = 0x7670;
const EBML_ID_PROJECTION_POSE_YAW: u64 = 0x7673;
const EBML_ID_PROJECTION_POSE_PITCH: u64 = 0x7674;
const EBML_ID_PROJECTION_POSE_ROLL: u64 = 0x7675;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoContainer {
    Mp4,
    Mov,
    Webm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioContainer {
    Mp3,
    Wav,
    Flac,
}

impl AudioContainer {
    const fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Flac => "flac",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedCover {
    pub media_type: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioInspection {
    pub duration_ms: Option<u64>,
    pub sample_rate_hz: Option<u32>,
    pub channel_count: Option<u32>,
    pub bit_depth: Option<u32>,
    pub codec: Option<&'static str>,
    pub cover: Option<EmbeddedCover>,
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
    pub display_quarter_turns: Option<u8>,
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
    let display_quarter_turns = match container {
        VideoContainer::Mp4 | VideoContainer::Mov => preflight_iso_bmff(path, before.len)?,
        VideoContainer::Webm => preflight_webm(path, before.len)?,
    };

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
        display_quarter_turns,
        width: dimensions.map(|value| value.0),
        height: dimensions.map(|value| value.1),
        video_track_count,
        audio_track_count,
        codec,
    })
}

/// Inspects audio properties and an optional bounded embedded cover without decoding packets.
///
/// The source remains authoritative and is never modified. Container declarations, parser I/O,
/// metadata allocations, track counts, duration, and wall time are all bounded.
///
/// # Errors
///
/// Returns a stable failure when the source is unreadable, changes, is malformed, exceeds an
/// inspection limit, or uses an unsupported container feature.
pub fn inspect_audio_file(
    path: &Path,
    container: AudioContainer,
    timeout: WallDuration,
) -> Result<AudioInspection, MediaInspectError> {
    let before = source_version(path)?;
    if timeout.is_zero() {
        return Err(MediaInspectError::ResourceLimited);
    }
    preflight_audio(path, container, before.len)?;
    if container == AudioContainer::Flac {
        let inspection = inspect_flac_metadata(path, before.len, timeout)?;
        let after = source_version(path)?;
        if before != after {
            return Err(MediaInspectError::SourceChanged);
        }
        return Ok(inspection);
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
    let maximum_metadata_bytes = usize::try_from(MAX_CONTAINER_METADATA_BYTES)
        .map_err(|_| MediaInspectError::ResourceLimited)?;
    let maximum_cover_bytes =
        usize::try_from(MAX_AUDIO_COVER_BYTES).map_err(|_| MediaInspectError::ResourceLimited)?;
    let metadata_options = MetadataOptions::default()
        .limit_tag_bytes(Limit::Maximum(maximum_metadata_bytes))
        .limit_visual_bytes(Limit::Maximum(maximum_cover_bytes));
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), metadata_options)
        .map_err(|error| classify_symphonia_error(&error, &state))?;

    if format.tracks().len() > MAX_MEDIA_TRACKS {
        return Err(MediaInspectError::ResourceLimited);
    }
    let duration_ms = duration_milliseconds(format.media_info())?.or_else(|| {
        (container == AudioContainer::Mp3)
            .then(|| mp3_duration_milliseconds(path, before.len))
            .flatten()
    });
    if duration_ms.is_some_and(|duration| duration > MAX_MEDIA_DURATION_MS) {
        return Err(MediaInspectError::ResourceLimited);
    }

    let mut inspection = symphonia_audio_track_inspection(&*format)?;
    inspection.duration_ms = duration_ms;
    inspection.cover = first_embedded_cover(&mut *format);
    if inspection
        .cover
        .as_ref()
        .is_some_and(|cover| cover.bytes.len() as u64 > MAX_AUDIO_COVER_BYTES)
    {
        return Err(MediaInspectError::ResourceLimited);
    }

    drop(format);
    let after = source_version(path)?;
    if before != after {
        return Err(MediaInspectError::SourceChanged);
    }
    Ok(inspection)
}

fn symphonia_audio_track_inspection(
    format: &dyn FormatReader,
) -> Result<AudioInspection, MediaInspectError> {
    let parameters = format
        .tracks()
        .iter()
        .find_map(|track| match &track.codec_params {
            Some(CodecParameters::Audio(parameters)) => Some(parameters),
            _ => None,
        });
    let sample_rate_hz = parameters.and_then(|parameters| parameters.sample_rate);
    let channel_count = parameters
        .and_then(|parameters| parameters.channels.as_ref())
        .map(|channels| u32::try_from(channels.count()).unwrap_or(u32::MAX));
    let bit_depth = parameters.and_then(|parameters| {
        parameters
            .bits_per_coded_sample
            .or(parameters.bits_per_sample)
    });
    if sample_rate_hz.is_some_and(|value| value == 0 || value > MAX_AUDIO_SAMPLE_RATE_HZ)
        || channel_count.is_some_and(|value| value == 0 || value > MAX_AUDIO_CHANNELS)
        || bit_depth.is_some_and(|value| value == 0 || value > MAX_AUDIO_BIT_DEPTH)
    {
        return Err(MediaInspectError::ResourceLimited);
    }
    Ok(AudioInspection {
        duration_ms: None,
        sample_rate_hz,
        channel_count,
        bit_depth,
        codec: parameters.and_then(|parameters| audio_codec_name(parameters.codec)),
        cover: None,
    })
}

fn first_embedded_cover(format: &mut dyn FormatReader) -> Option<EmbeddedCover> {
    let mut metadata = format.metadata();
    metadata.skip_to_latest().and_then(|revision| {
        revision
            .media
            .visuals
            .iter()
            .chain(
                revision
                    .per_track
                    .iter()
                    .flat_map(|track| track.metadata.visuals.iter()),
            )
            .next()
            .map(|visual| EmbeddedCover {
                media_type: visual.media_type.clone(),
                bytes: visual.data.to_vec(),
            })
    })
}

fn audio_codec_name(codec: AudioCodecId) -> Option<&'static str> {
    if codec == CODEC_ID_MP3 {
        Some("mp3")
    } else if codec == CODEC_ID_FLAC {
        Some("flac")
    } else if matches!(
        codec,
        CODEC_ID_PCM_S8
            | CODEC_ID_PCM_S16LE
            | CODEC_ID_PCM_S16BE
            | CODEC_ID_PCM_S24LE
            | CODEC_ID_PCM_S24BE
            | CODEC_ID_PCM_S32LE
            | CODEC_ID_PCM_S32BE
            | CODEC_ID_PCM_U8
            | CODEC_ID_PCM_U16LE
            | CODEC_ID_PCM_U16BE
            | CODEC_ID_PCM_U24LE
            | CODEC_ID_PCM_U24BE
            | CODEC_ID_PCM_U32LE
            | CODEC_ID_PCM_U32BE
            | CODEC_ID_PCM_F32LE
            | CODEC_ID_PCM_F32BE
            | CODEC_ID_PCM_F64LE
            | CODEC_ID_PCM_F64BE
    ) {
        Some("pcm")
    } else {
        None
    }
}

fn preflight_audio(
    path: &Path,
    container: AudioContainer,
    file_len: u64,
) -> Result<(), MediaInspectError> {
    match container {
        AudioContainer::Mp3 => preflight_mp3(path, file_len),
        AudioContainer::Wav => preflight_wav(path, file_len),
        AudioContainer::Flac => preflight_flac(path, file_len),
    }
}

fn preflight_mp3(path: &Path, file_len: u64) -> Result<(), MediaInspectError> {
    let mut file = File::open(path).map_err(|_| MediaInspectError::Unreadable)?;
    let mut header = [0_u8; 10];
    file.read_exact(&mut header)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    let audio_start = if &header[..3] == b"ID3" {
        if header[6..10].iter().any(|byte| byte & 0x80 != 0) {
            return Err(MediaInspectError::InvalidContent);
        }
        let tag_size = header[6..10]
            .iter()
            .fold(0_u64, |value, byte| (value << 7) | u64::from(*byte));
        if tag_size > MAX_CONTAINER_METADATA_BYTES {
            return Err(MediaInspectError::ResourceLimited);
        }
        10_u64
            .checked_add(tag_size)
            .filter(|end| *end < file_len)
            .ok_or(MediaInspectError::InvalidContent)?
    } else {
        0
    };
    file.seek(SeekFrom::Start(audio_start))
        .map_err(|_| MediaInspectError::Unreadable)?;
    let mut frame = [0_u8; 2];
    file.read_exact(&mut frame)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    if frame[0] != 0xff || frame[1] & 0xe0 != 0xe0 {
        return Err(MediaInspectError::InvalidContent);
    }
    Ok(())
}

fn mp3_duration_milliseconds(path: &Path, file_len: u64) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let mut header = [0_u8; 10];
    file.read_exact(&mut header).ok()?;
    let audio_start = if &header[..3] == b"ID3" {
        let tag_size = header[6..10]
            .iter()
            .fold(0_u64, |value, byte| (value << 7) | u64::from(*byte));
        10_u64.checked_add(tag_size)?
    } else {
        0
    };
    file.seek(SeekFrom::Start(audio_start)).ok()?;
    let mut frame = [0_u8; 4];
    file.read_exact(&mut frame).ok()?;
    let header = u32::from_be_bytes(frame);
    let version = (header >> 19) & 0b11;
    let layer = (header >> 17) & 0b11;
    let bitrate_index = usize::try_from((header >> 12) & 0x0f).ok()?;
    if layer != 0b01 || matches!(bitrate_index, 0 | 15) {
        return None;
    }
    let bitrate_kbps = if version == 0b11 {
        [
            0_u64, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
        ][bitrate_index]
    } else {
        [
            0_u64, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
        ][bitrate_index]
    };
    file_len
        .checked_sub(audio_start)?
        .checked_mul(8)?
        .checked_div(bitrate_kbps)
}

fn inspect_flac_metadata(
    path: &Path,
    file_len: u64,
    timeout: WallDuration,
) -> Result<AudioInspection, MediaInspectError> {
    let started = Instant::now();
    let mut file = File::open(path).map_err(|_| MediaInspectError::Unreadable)?;
    let mut marker = [0_u8; 4];
    file.read_exact(&mut marker)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    let mut position = 4_u64;
    let mut stream = None;
    let mut cover = None;
    let mut count = 0_u64;
    loop {
        if started.elapsed() >= timeout {
            return Err(MediaInspectError::ResourceLimited);
        }
        count = count.saturating_add(1);
        if count > MAX_CONTAINER_ELEMENTS {
            return Err(MediaInspectError::ResourceLimited);
        }
        file.seek(SeekFrom::Start(position))
            .map_err(|_| MediaInspectError::Unreadable)?;
        let mut header = [0_u8; 4];
        file.read_exact(&mut header)
            .map_err(|_| MediaInspectError::InvalidContent)?;
        let last = header[0] & 0x80 != 0;
        let block_type = header[0] & 0x7f;
        let size = u64::from_be_bytes([0, 0, 0, 0, 0, header[1], header[2], header[3]]);
        let payload = position + 4;
        if block_type == 0 {
            let mut info = [0_u8; 34];
            file.read_exact(&mut info)
                .map_err(|_| MediaInspectError::InvalidContent)?;
            let packed = u64::from_be_bytes(info[10..18].try_into().expect("eight-byte slice"));
            let sample_rate = u32::try_from((packed >> 44) & 0x0f_ffff)
                .map_err(|_| MediaInspectError::InvalidContent)?;
            let channels = u32::try_from(((packed >> 41) & 0x07) + 1)
                .map_err(|_| MediaInspectError::InvalidContent)?;
            let bit_depth = u32::try_from(((packed >> 36) & 0x1f) + 1)
                .map_err(|_| MediaInspectError::InvalidContent)?;
            let total_samples = packed & 0x0f_ffff_ffff;
            if sample_rate == 0 {
                return Err(MediaInspectError::InvalidContent);
            }
            stream = Some((sample_rate, channels, bit_depth, total_samples));
        } else if block_type == 6 && cover.is_none() {
            cover = read_flac_picture(&mut file, payload, size)?;
        }
        position = payload
            .checked_add(size)
            .filter(|end| *end <= file_len)
            .ok_or(MediaInspectError::InvalidContent)?;
        if last {
            break;
        }
    }
    let (sample_rate_hz, channel_count, bit_depth, total_samples) =
        stream.ok_or(MediaInspectError::InvalidContent)?;
    if sample_rate_hz > MAX_AUDIO_SAMPLE_RATE_HZ
        || channel_count > MAX_AUDIO_CHANNELS
        || bit_depth > MAX_AUDIO_BIT_DEPTH
    {
        return Err(MediaInspectError::ResourceLimited);
    }
    let duration_ms = total_samples
        .checked_mul(1_000)
        .and_then(|value| value.checked_div(u64::from(sample_rate_hz)))
        .ok_or(MediaInspectError::ResourceLimited)?;
    if duration_ms > MAX_MEDIA_DURATION_MS {
        return Err(MediaInspectError::ResourceLimited);
    }
    Ok(AudioInspection {
        duration_ms: Some(duration_ms),
        sample_rate_hz: Some(sample_rate_hz),
        channel_count: Some(channel_count),
        bit_depth: Some(bit_depth),
        codec: Some("flac"),
        cover,
    })
}

fn read_flac_picture(
    file: &mut File,
    payload: u64,
    size: u64,
) -> Result<Option<EmbeddedCover>, MediaInspectError> {
    file.seek(SeekFrom::Start(payload + 4))
        .map_err(|_| MediaInspectError::Unreadable)?;
    let media_type_len = u64::from(read_be_u32(file)?);
    if media_type_len > 256 {
        return Err(MediaInspectError::ResourceLimited);
    }
    let media_type_len_usize =
        usize::try_from(media_type_len).map_err(|_| MediaInspectError::ResourceLimited)?;
    let mut media_type = vec![0_u8; media_type_len_usize];
    file.read_exact(&mut media_type)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    let media_type =
        String::from_utf8(media_type).map_err(|_| MediaInspectError::InvalidContent)?;
    let description_len = u64::from(read_be_u32(file)?);
    if description_len > 64 * 1024 {
        return Err(MediaInspectError::ResourceLimited);
    }
    file.seek(SeekFrom::Current(
        i64::try_from(description_len + 16).map_err(|_| MediaInspectError::ResourceLimited)?,
    ))
    .map_err(|_| MediaInspectError::InvalidContent)?;
    let data_len = u64::from(read_be_u32(file)?);
    if data_len > MAX_AUDIO_COVER_BYTES {
        return Err(MediaInspectError::ResourceLimited);
    }
    let consumed = 4_u64
        .checked_add(4 + media_type_len)
        .and_then(|value| value.checked_add(4 + description_len + 16 + 4 + data_len))
        .ok_or(MediaInspectError::ResourceLimited)?;
    if consumed > size {
        return Err(MediaInspectError::InvalidContent);
    }
    let data_len = usize::try_from(data_len).map_err(|_| MediaInspectError::ResourceLimited)?;
    let mut bytes = vec![0_u8; data_len];
    file.read_exact(&mut bytes)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    Ok(Some(EmbeddedCover {
        media_type: (!media_type.is_empty()).then_some(media_type),
        bytes,
    }))
}

fn read_be_u32(file: &mut File) -> Result<u32, MediaInspectError> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    Ok(u32::from_be_bytes(bytes))
}

fn preflight_wav(path: &Path, file_len: u64) -> Result<(), MediaInspectError> {
    let mut file = File::open(path).map_err(|_| MediaInspectError::Unreadable)?;
    let mut header = [0_u8; 12];
    file.read_exact(&mut header)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err(MediaInspectError::InvalidContent);
    }
    let declared_end = 8_u64
        .checked_add(u64::from(u32::from_le_bytes(
            header[4..8].try_into().expect("four-byte slice"),
        )))
        .filter(|end| *end <= file_len)
        .ok_or(MediaInspectError::InvalidContent)?;
    let mut position = 12_u64;
    let mut count = 0_u64;
    let mut found_fmt = false;
    let mut found_data = false;
    while position < declared_end {
        count = count.saturating_add(1);
        if count > MAX_CONTAINER_ELEMENTS {
            return Err(MediaInspectError::ResourceLimited);
        }
        file.seek(SeekFrom::Start(position))
            .map_err(|_| MediaInspectError::Unreadable)?;
        let mut chunk = [0_u8; 8];
        file.read_exact(&mut chunk)
            .map_err(|_| MediaInspectError::InvalidContent)?;
        let size = u64::from(u32::from_le_bytes(
            chunk[4..].try_into().expect("four-byte slice"),
        ));
        if &chunk[..4] != b"data" && size > MAX_CONTAINER_METADATA_BYTES {
            return Err(MediaInspectError::ResourceLimited);
        }
        let padded = size
            .checked_add(size & 1)
            .ok_or(MediaInspectError::ResourceLimited)?;
        let end = position
            .checked_add(8)
            .and_then(|value| value.checked_add(padded))
            .filter(|end| *end <= declared_end)
            .ok_or(MediaInspectError::InvalidContent)?;
        found_fmt |= &chunk[..4] == b"fmt ";
        found_data |= &chunk[..4] == b"data";
        position = end;
    }
    if !found_fmt || !found_data {
        return Err(MediaInspectError::InvalidContent);
    }
    Ok(())
}

fn preflight_flac(path: &Path, file_len: u64) -> Result<(), MediaInspectError> {
    let mut file = File::open(path).map_err(|_| MediaInspectError::Unreadable)?;
    let mut marker = [0_u8; 4];
    file.read_exact(&mut marker)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    if &marker != b"fLaC" {
        return Err(MediaInspectError::InvalidContent);
    }
    let mut position = 4_u64;
    let mut count = 0_u64;
    let mut total = 0_u64;
    loop {
        count = count.saturating_add(1);
        if count > MAX_CONTAINER_ELEMENTS {
            return Err(MediaInspectError::ResourceLimited);
        }
        file.seek(SeekFrom::Start(position))
            .map_err(|_| MediaInspectError::Unreadable)?;
        let mut header = [0_u8; 4];
        file.read_exact(&mut header)
            .map_err(|_| MediaInspectError::InvalidContent)?;
        let last = header[0] & 0x80 != 0;
        let block_type = header[0] & 0x7f;
        let size = u64::from_be_bytes([0, 0, 0, 0, 0, header[1], header[2], header[3]]);
        if count == 1 && (block_type != 0 || size != 34) {
            return Err(MediaInspectError::InvalidContent);
        }
        if size > MAX_CONTAINER_METADATA_BYTES {
            return Err(MediaInspectError::ResourceLimited);
        }
        total = total
            .checked_add(size + 4)
            .filter(|value| *value <= MAX_CONTAINER_IO_BYTES)
            .ok_or(MediaInspectError::ResourceLimited)?;
        position = position
            .checked_add(4 + size)
            .filter(|end| *end <= file_len)
            .ok_or(MediaInspectError::InvalidContent)?;
        if last {
            break;
        }
    }
    Ok(())
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

fn preflight_iso_bmff(path: &Path, file_len: u64) -> Result<Option<u8>, MediaInspectError> {
    let mut file = File::open(path).map_err(|_| MediaInspectError::Unreadable)?;
    let mut position = 0_u64;
    let mut count = 0_u64;
    let mut found_ftyp = false;
    let mut found_moov = false;
    let mut display_quarter_turns = None;
    while position < file_len {
        count_container_element(&mut count)?;
        let atom = read_iso_box(&mut file, position, file_len)?;
        match &atom.kind {
            b"ftyp" => found_ftyp = true,
            b"moov" => {
                if found_moov {
                    return Err(MediaInspectError::InvalidContent);
                }
                if atom.end.saturating_sub(position) > MAX_CONTAINER_METADATA_BYTES {
                    return Err(MediaInspectError::ResourceLimited);
                }
                found_moov = true;
                display_quarter_turns = iso_video_display_quarter_turns(
                    &mut file,
                    atom.payload_start,
                    atom.end,
                    &mut count,
                )?;
            }
            b"meta" | b"sidx"
                if atom.end.saturating_sub(position) > MAX_CONTAINER_METADATA_BYTES =>
            {
                return Err(MediaInspectError::ResourceLimited);
            }
            _ => {}
        }
        position = atom.end;
    }
    if !found_ftyp || !found_moov {
        return Err(MediaInspectError::InvalidContent);
    }
    Ok(display_quarter_turns)
}

#[derive(Debug, Clone, Copy)]
struct IsoBox {
    kind: [u8; 4],
    payload_start: u64,
    end: u64,
}

fn read_iso_box(
    file: &mut File,
    position: u64,
    parent_end: u64,
) -> Result<IsoBox, MediaInspectError> {
    file.seek(SeekFrom::Start(position))
        .map_err(|_| MediaInspectError::Unreadable)?;
    let mut header = [0_u8; 16];
    file.read_exact(&mut header[..8])
        .map_err(|_| MediaInspectError::InvalidContent)?;
    let size32 = u32::from_be_bytes(header[..4].try_into().expect("four-byte slice"));
    let kind = header[4..8].try_into().expect("four-byte slice");
    let (header_len, size) = match size32 {
        0 => (8_u64, parent_end.saturating_sub(position)),
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
    let payload_start = position
        .checked_add(header_len)
        .ok_or(MediaInspectError::InvalidContent)?;
    let end = position
        .checked_add(size)
        .filter(|end| *end <= parent_end && *end > position)
        .ok_or(MediaInspectError::InvalidContent)?;
    Ok(IsoBox {
        kind,
        payload_start,
        end,
    })
}

fn iso_video_display_quarter_turns(
    file: &mut File,
    start: u64,
    end: u64,
    count: &mut u64,
) -> Result<Option<u8>, MediaInspectError> {
    let mut position = start;
    while position < end {
        count_container_element(count)?;
        let atom = read_iso_box(file, position, end)?;
        if &atom.kind == b"trak" {
            let (is_video, turns) = iso_track_display(file, atom.payload_start, atom.end, count)?;
            if is_video {
                return Ok(turns);
            }
        }
        position = atom.end;
    }
    Ok(None)
}

fn iso_track_display(
    file: &mut File,
    start: u64,
    end: u64,
    count: &mut u64,
) -> Result<(bool, Option<u8>), MediaInspectError> {
    let mut position = start;
    let mut is_video = false;
    let mut turns = None;
    while position < end {
        count_container_element(count)?;
        let atom = read_iso_box(file, position, end)?;
        match &atom.kind {
            b"tkhd" => turns = iso_track_header_quarter_turns(file, atom)?,
            b"mdia" => {
                is_video = iso_media_is_video(file, atom.payload_start, atom.end, count)?;
            }
            _ => {}
        }
        position = atom.end;
    }
    Ok((is_video, turns))
}

fn iso_media_is_video(
    file: &mut File,
    start: u64,
    end: u64,
    count: &mut u64,
) -> Result<bool, MediaInspectError> {
    let mut position = start;
    while position < end {
        count_container_element(count)?;
        let atom = read_iso_box(file, position, end)?;
        if &atom.kind == b"hdlr" {
            if atom.end.saturating_sub(atom.payload_start) < 12 {
                return Err(MediaInspectError::InvalidContent);
            }
            file.seek(SeekFrom::Start(atom.payload_start + 8))
                .map_err(|_| MediaInspectError::Unreadable)?;
            let mut handler = [0_u8; 4];
            file.read_exact(&mut handler)
                .map_err(|_| MediaInspectError::InvalidContent)?;
            return Ok(&handler == b"vide");
        }
        position = atom.end;
    }
    Ok(false)
}

fn iso_track_header_quarter_turns(
    file: &mut File,
    atom: IsoBox,
) -> Result<Option<u8>, MediaInspectError> {
    file.seek(SeekFrom::Start(atom.payload_start))
        .map_err(|_| MediaInspectError::Unreadable)?;
    let mut version = [0_u8; 1];
    file.read_exact(&mut version)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    let matrix_offset = match version[0] {
        0 => 40_u64,
        1 => 52,
        _ => return Ok(None),
    };
    let matrix_start = atom
        .payload_start
        .checked_add(matrix_offset)
        .filter(|start| start.saturating_add(36) <= atom.end)
        .ok_or(MediaInspectError::InvalidContent)?;
    file.seek(SeekFrom::Start(matrix_start))
        .map_err(|_| MediaInspectError::Unreadable)?;
    let mut bytes = [0_u8; 36];
    file.read_exact(&mut bytes)
        .map_err(|_| MediaInspectError::InvalidContent)?;
    let component = |index: usize| {
        i32::from_be_bytes(
            bytes[index * 4..index * 4 + 4]
                .try_into()
                .expect("four-byte matrix component"),
        )
    };
    if component(2) != 0 || component(5) != 0 || component(8) != 0x4000_0000 {
        return Ok(None);
    }
    let rotation = match (component(0), component(1), component(3), component(4)) {
        (0x0001_0000, 0, 0, 0x0001_0000) => Some(0),
        (0, 0x0001_0000, -0x0001_0000, 0) => Some(1),
        (-0x0001_0000, 0, 0, -0x0001_0000) => Some(2),
        (0, -0x0001_0000, 0x0001_0000, 0) => Some(3),
        _ => None,
    };
    Ok(rotation)
}

fn preflight_webm(path: &Path, file_len: u64) -> Result<Option<u8>, MediaInspectError> {
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
    let mut display_quarter_turns = None;
    while position < segment_end {
        count_container_element(&mut count)?;
        let child = read_ebml_header(&mut file, position, segment_end)?;
        let child_end = child.end(segment_end)?;
        if child.id == EBML_ID_INFO {
            found_info = true;
        } else if child.id == EBML_ID_TRACKS {
            found_tracks = true;
            display_quarter_turns = webm_video_display_quarter_turns(
                &mut file,
                child.payload_start,
                child_end,
                &mut count,
            )?;
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
    Ok(display_quarter_turns)
}

fn webm_video_display_quarter_turns(
    file: &mut File,
    start: u64,
    end: u64,
    count: &mut u64,
) -> Result<Option<u8>, MediaInspectError> {
    let mut position = start;
    while position < end {
        count_container_element(count)?;
        let child = read_ebml_header(file, position, end)?;
        let child_end = child.end(end)?;
        if child.id == EBML_ID_TRACK_ENTRY {
            let (is_video, turns) =
                webm_track_display(file, child.payload_start, child_end, count)?;
            if is_video {
                return Ok(turns);
            }
        }
        if child.size.is_none() {
            return Err(MediaInspectError::UnsupportedFeature);
        }
        position = child_end;
    }
    Ok(None)
}

fn webm_track_display(
    file: &mut File,
    start: u64,
    end: u64,
    count: &mut u64,
) -> Result<(bool, Option<u8>), MediaInspectError> {
    let mut position = start;
    let mut track_type = None;
    let mut turns = Some(0);
    while position < end {
        count_container_element(count)?;
        let child = read_ebml_header(file, position, end)?;
        let child_end = child.end(end)?;
        match child.id {
            EBML_ID_TRACK_TYPE => track_type = Some(read_ebml_unsigned(file, child)?),
            EBML_ID_VIDEO => {
                turns = webm_video_rotation(file, child.payload_start, child_end, count)?;
            }
            _ => {}
        }
        if child.size.is_none() {
            return Err(MediaInspectError::UnsupportedFeature);
        }
        position = child_end;
    }
    Ok((track_type == Some(1), turns))
}

fn webm_video_rotation(
    file: &mut File,
    start: u64,
    end: u64,
    count: &mut u64,
) -> Result<Option<u8>, MediaInspectError> {
    let mut position = start;
    while position < end {
        count_container_element(count)?;
        let child = read_ebml_header(file, position, end)?;
        let child_end = child.end(end)?;
        if child.id == EBML_ID_PROJECTION {
            return webm_projection_rotation(file, child.payload_start, child_end, count);
        }
        if child.size.is_none() {
            return Err(MediaInspectError::UnsupportedFeature);
        }
        position = child_end;
    }
    Ok(Some(0))
}

fn webm_projection_rotation(
    file: &mut File,
    start: u64,
    end: u64,
    count: &mut u64,
) -> Result<Option<u8>, MediaInspectError> {
    let mut position = start;
    let mut yaw = 0_f64;
    let mut pitch = 0_f64;
    let mut roll = 0_f64;
    while position < end {
        count_container_element(count)?;
        let child = read_ebml_header(file, position, end)?;
        let child_end = child.end(end)?;
        match child.id {
            EBML_ID_PROJECTION_POSE_YAW => yaw = read_ebml_float(file, child)?,
            EBML_ID_PROJECTION_POSE_PITCH => pitch = read_ebml_float(file, child)?,
            EBML_ID_PROJECTION_POSE_ROLL => roll = read_ebml_float(file, child)?,
            _ => {}
        }
        if child.size.is_none() {
            return Err(MediaInspectError::UnsupportedFeature);
        }
        position = child_end;
    }
    if !yaw.is_finite()
        || !pitch.is_finite()
        || !roll.is_finite()
        || yaw.abs() > 180.0
        || pitch.abs() > 90.0
        || roll.abs() > 180.0
    {
        return Err(MediaInspectError::InvalidContent);
    }
    if yaw.abs() > 0.001 || pitch.abs() > 0.001 {
        return Ok(None);
    }
    Ok(clockwise_quarter_turns(-roll))
}

fn read_ebml_unsigned(file: &mut File, element: EbmlHeader) -> Result<u64, MediaInspectError> {
    let size = element.size.ok_or(MediaInspectError::UnsupportedFeature)?;
    if !(1..=8).contains(&size) {
        return Err(MediaInspectError::InvalidContent);
    }
    file.seek(SeekFrom::Start(element.payload_start))
        .map_err(|_| MediaInspectError::Unreadable)?;
    let mut bytes = [0_u8; 8];
    let offset = 8_usize
        .saturating_sub(usize::try_from(size).map_err(|_| MediaInspectError::InvalidContent)?);
    file.read_exact(&mut bytes[offset..])
        .map_err(|_| MediaInspectError::InvalidContent)?;
    Ok(u64::from_be_bytes(bytes))
}

fn read_ebml_float(file: &mut File, element: EbmlHeader) -> Result<f64, MediaInspectError> {
    let size = element.size.ok_or(MediaInspectError::UnsupportedFeature)?;
    file.seek(SeekFrom::Start(element.payload_start))
        .map_err(|_| MediaInspectError::Unreadable)?;
    match size {
        4 => {
            let mut bytes = [0_u8; 4];
            file.read_exact(&mut bytes)
                .map_err(|_| MediaInspectError::InvalidContent)?;
            Ok(f64::from(f32::from_bits(u32::from_be_bytes(bytes))))
        }
        8 => {
            let mut bytes = [0_u8; 8];
            file.read_exact(&mut bytes)
                .map_err(|_| MediaInspectError::InvalidContent)?;
            Ok(f64::from_bits(u64::from_be_bytes(bytes)))
        }
        _ => Err(MediaInspectError::InvalidContent),
    }
}

fn clockwise_quarter_turns(degrees: f64) -> Option<u8> {
    let normalized = degrees.rem_euclid(360.0);
    [(0.0, 0), (90.0, 1), (180.0, 2), (270.0, 3)]
        .into_iter()
        .find_map(|(candidate, turns)| ((normalized - candidate).abs() <= 0.001).then_some(turns))
}

fn count_container_element(count: &mut u64) -> Result<(), MediaInspectError> {
    *count = count.saturating_add(1);
    if *count > MAX_CONTAINER_ELEMENTS {
        return Err(MediaInspectError::ResourceLimited);
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
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    fn fixture(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/formats/sources/video")
            .join(relative)
    }

    fn audio_fixture(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/formats/sources/audio")
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
            assert_eq!(inspection.display_quarter_turns, Some(0), "{name}");
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
    fn reads_iso_track_matrix_and_webm_projection_rotation() {
        let directory = tempfile::tempdir().expect("temp directory");
        let rotated_mp4 = directory.path().join("rotated.mp4");
        let mut mp4 = fs::read(fixture("minimal.mp4")).expect("read MP4 fixture");
        let tkhd = mp4
            .windows(4)
            .position(|bytes| bytes == b"tkhd")
            .expect("video track header");
        let matrix_start = tkhd + 44;
        let rotated_matrix = [0_i32, 0x0001_0000, 0, -0x0001_0000, 0, 0, 0, 0, 0x4000_0000]
            .into_iter()
            .flat_map(i32::to_be_bytes)
            .collect::<Vec<_>>();
        mp4[matrix_start..matrix_start + 36].copy_from_slice(&rotated_matrix);
        fs::write(&rotated_mp4, mp4).expect("write rotated MP4");
        let inspection = inspect_video_file(
            &rotated_mp4,
            VideoContainer::Mp4,
            WallDuration::from_secs(1),
        )
        .expect("inspect rotated MP4");
        assert_eq!(inspection.display_quarter_turns, Some(1));
        assert_eq!(
            (inspection.width, inspection.height),
            (Some(320), Some(180))
        );

        let rotated_webm = directory.path().join("rotated.webm");
        let track_type = test_ebml_element(&[0x83], &[1]);
        let roll = test_ebml_element(&[0x76, 0x75], &90_f64.to_be_bytes());
        let projection = test_ebml_element(&[0x76, 0x70], &roll);
        let video = test_ebml_element(&[0xe0], &projection);
        let track = test_ebml_element(&[0xae], &[track_type, video].concat());
        let tracks = test_ebml_element(&[0x16, 0x54, 0xae, 0x6b], &track);
        let info = test_ebml_element(&[0x15, 0x49, 0xa9, 0x66], &[]);
        let cluster = test_ebml_element(&[0x1f, 0x43, 0xb6, 0x75], &[]);
        let segment =
            test_ebml_element(&[0x18, 0x53, 0x80, 0x67], &[info, tracks, cluster].concat());
        let header = test_ebml_element(&[0x1a, 0x45, 0xdf, 0xa3], &[]);
        fs::write(&rotated_webm, [header, segment].concat()).expect("write rotated WebM");
        assert_eq!(
            preflight_webm(
                &rotated_webm,
                fs::metadata(&rotated_webm).expect("WebM metadata").len(),
            )
            .expect("preflight rotated WebM"),
            Some(3),
        );
    }

    fn test_ebml_element(id: &[u8], payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() < 127);
        [
            id,
            &[0x80 | u8::try_from(payload.len()).expect("small payload")],
            payload,
        ]
        .concat()
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

    #[test]
    fn inspects_mp3_wav_and_flac_without_decoding_packets() {
        for (name, container, expected) in [
            (
                "minimal.mp3",
                AudioContainer::Mp3,
                (Some(521), Some(44_100), Some(2), None, Some("mp3")),
            ),
            (
                "minimal.wav",
                AudioContainer::Wav,
                (Some(1_000), Some(8_000), Some(1), Some(16), Some("pcm")),
            ),
            (
                "minimal.flac",
                AudioContainer::Flac,
                (Some(1_000), Some(8_000), Some(1), Some(16), Some("flac")),
            ),
        ] {
            let inspection =
                inspect_audio_file(&audio_fixture(name), container, WallDuration::from_secs(1))
                    .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(
                (
                    inspection.duration_ms,
                    inspection.sample_rate_hz,
                    inspection.channel_count,
                    inspection.bit_depth,
                    inspection.codec,
                ),
                expected,
                "{name}"
            );
            assert!(inspection.cover.is_none(), "{name}");
        }
    }

    #[test]
    fn extracts_bounded_mp3_and_flac_covers() {
        for (name, container) in [
            ("cover.mp3", AudioContainer::Mp3),
            ("cover.flac", AudioContainer::Flac),
        ] {
            let inspection =
                inspect_audio_file(&audio_fixture(name), container, WallDuration::from_secs(1))
                    .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            let cover = inspection.cover.expect("embedded cover");
            assert_eq!(cover.media_type.as_deref(), Some("image/png"));
            assert_eq!(
                cover.bytes,
                fs::read(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../fixtures/formats/references/svg/minimal.png")
                )
                .expect("reference PNG")
            );
        }
    }

    #[test]
    fn isolates_audio_truncation_disguises_unknown_codecs_and_resource_bombs() {
        for (name, container) in [
            ("truncated.mp3", AudioContainer::Mp3),
            ("png-disguised-as-mp3.mp3", AudioContainer::Mp3),
            ("mp3-disguised-as-wav.wav", AudioContainer::Wav),
        ] {
            assert!(matches!(
                inspect_audio_file(&audio_fixture(name), container, WallDuration::from_secs(1)),
                Err(MediaInspectError::InvalidContent)
            ));
        }
        assert!(matches!(
            inspect_audio_file(
                &audio_fixture("unknown-codec.wav"),
                AudioContainer::Wav,
                WallDuration::from_secs(1)
            ),
            Err(MediaInspectError::UnsupportedFeature)
        ));
        assert!(matches!(
            inspect_audio_file(
                &audio_fixture("oversized-cover.mp3"),
                AudioContainer::Mp3,
                WallDuration::from_secs(1)
            ),
            Err(MediaInspectError::ResourceLimited)
        ));
    }
}
