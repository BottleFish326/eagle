use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder};
use libheif_rs::{
    ColorSpace, HeifContext, HeifError, HeifErrorCode, HeifErrorSubCode, LibHeif, RgbChroma,
    StreamReader,
};
use sha2::{Digest, Sha256};

use crate::{
    HeifProperties, LIBHEIF_PROVIDER_VERSION, PngPayload, WorkerErrorCode, WorkerOperation,
    WorkerRequest,
};

const EXPECTED_LIBHEIF_VERSION: [u8; 3] = [1, 23, 1];
const MAX_COLOR_PROFILE_BYTES: u32 = 1024 * 1024;
const MAX_ITEMS: u32 = 1_024;
const MAX_TILES: u64 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendFailure {
    pub code: WorkerErrorCode,
    pub message: &'static str,
}

impl BackendFailure {
    fn new(code: WorkerErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }
}

/// Reads one already-authorized source with fixed libheif security limits.
///
/// # Errors
///
/// Returns a stable, path-free failure for source drift, invalid content, unavailable codecs,
/// resource limits, or PNG encoding failure.
pub fn process_libheif_request(
    request: &WorkerRequest,
) -> Result<(HeifProperties, Option<PngPayload>, Vec<u8>), BackendFailure> {
    request
        .validate()
        .map_err(|_| BackendFailure::new(WorkerErrorCode::Internal, "worker request is invalid"))?;
    if request.provider_id != crate::LIBHEIF_PROVIDER_ID
        || request.provider_version != LIBHEIF_PROVIDER_VERSION
    {
        return Err(BackendFailure::new(
            WorkerErrorCode::Internal,
            "worker provider identity is not supported",
        ));
    }
    let requested_path = request.source_path.to_path_buf().map_err(|_| {
        BackendFailure::new(WorkerErrorCode::Unreadable, "source path cannot be decoded")
    })?;
    let canonical_path = fs::canonicalize(&requested_path)
        .map_err(|_| BackendFailure::new(WorkerErrorCode::Unreadable, "source is unreadable"))?;
    if canonical_path != requested_path {
        return Err(BackendFailure::new(
            WorkerErrorCode::SourceChanged,
            "source path changed before decoding",
        ));
    }
    verify_source_snapshot(request, &canonical_path)?;

    let (libheif, context) = open_context(request, &canonical_path)?;

    let handle = context.primary_image_handle().map_err(map_heif_error)?;
    let width = handle.width();
    let height = handle.height();
    validate_dimensions(request, width, height)?;
    let image_count = u32::try_from(context.image_ids().len()).map_err(|_| {
        BackendFailure::new(
            WorkerErrorCode::ResourceLimited,
            "image count exceeds the worker limit",
        )
    })?;
    if image_count == 0 || image_count > MAX_ITEMS {
        return Err(BackendFailure::new(
            WorkerErrorCode::ResourceLimited,
            "image count exceeds the worker limit",
        ));
    }
    let has_alpha = handle.has_alpha_channel();
    let color_space = if handle.color_profile_nclx().is_some() {
        Some("nclx".to_owned())
    } else if handle.color_profile_raw().is_some() {
        Some("icc".to_owned())
    } else {
        None
    };
    let properties = HeifProperties {
        width,
        height,
        orientation: Some(1),
        color_space,
        has_alpha: Some(has_alpha),
        image_count,
    };

    let (payload, png) = if request.operation == WorkerOperation::Thumbnail {
        let max_edge = request.limits.max_edge.ok_or_else(|| {
            BackendFailure::new(WorkerErrorCode::Internal, "thumbnail edge is missing")
        })?;
        let chroma = if has_alpha {
            RgbChroma::Rgba
        } else {
            RgbChroma::Rgb
        };
        let decoded = libheif
            .decode(&handle, ColorSpace::Rgb(chroma), Some(strict_options()?))
            .map_err(map_heif_error)?;
        let (output_width, output_height) = fit_dimensions(width, height, max_edge);
        let scaled = if (decoded.width(), decoded.height()) == (output_width, output_height) {
            decoded
        } else {
            decoded
                .scale(output_width, output_height, None)
                .map_err(map_heif_error)?
        };
        let png = encode_png(&scaled, has_alpha, request.limits.max_output_bytes)?;
        let payload = PngPayload {
            byte_length: u64::try_from(png.len()).unwrap_or(u64::MAX),
            sha256: format!("{:x}", Sha256::digest(&png)),
            width: output_width,
            height: output_height,
        };
        (Some(payload), png)
    } else {
        (None, Vec::new())
    };

    verify_source_snapshot(request, &canonical_path)?;
    Ok((properties, payload, png))
}

fn open_context(
    request: &WorkerRequest,
    path: &std::path::Path,
) -> Result<(LibHeif, HeifContext<'static>), BackendFailure> {
    let file = File::open(path)
        .map_err(|_| BackendFailure::new(WorkerErrorCode::Unreadable, "source is unreadable"))?;
    let total_size = file
        .metadata()
        .map_err(|_| BackendFailure::new(WorkerErrorCode::Unreadable, "source is unreadable"))?
        .len();
    if total_size != request.source_size {
        return Err(source_changed());
    }

    let libheif = LibHeif::new();
    if libheif.version() != EXPECTED_LIBHEIF_VERSION {
        return Err(BackendFailure::new(
            WorkerErrorCode::CodecUnavailable,
            "bundled libheif version does not match the provider manifest",
        ));
    }
    debug_assert!(LIBHEIF_PROVIDER_VERSION.contains("1.23.1"));

    let mut context = HeifContext::new().map_err(map_heif_error)?;
    let mut security = context.security_limits();
    let max_pixels = (request.limits.max_decode_bytes / 4).max(1);
    security.set_max_image_size_pixels(restrict_u64(security.max_image_size_pixels(), max_pixels));
    security.set_max_memory_block_size(restrict_u64(
        security.max_memory_block_size(),
        request.limits.max_decode_bytes,
    ));
    security.set_max_total_memory(restrict_u64(
        security.max_total_memory(),
        request.limits.max_decode_bytes,
    ));
    security.set_max_color_profile_size(restrict_u32(
        security.max_color_profile_size(),
        MAX_COLOR_PROFILE_BYTES,
    ));
    security.set_max_items(restrict_u32(security.max_items(), MAX_ITEMS));
    security.set_max_number_of_tiles(restrict_u64(security.max_number_of_tiles(), MAX_TILES));
    context
        .set_security_limits(&security)
        .map_err(map_heif_error)?;
    context.set_max_decoding_threads(0);
    context
        .read_reader(Box::new(StreamReader::new(
            BufReader::new(file),
            total_size,
        )))
        .map_err(map_heif_error)?;
    Ok((libheif, context))
}

fn strict_options() -> Result<libheif_rs::DecodingOptions, BackendFailure> {
    let mut options = libheif_rs::DecodingOptions::new().ok_or_else(|| {
        BackendFailure::new(
            WorkerErrorCode::ResourceLimited,
            "libheif decoding options could not be allocated",
        )
    })?;
    options.set_strict_decoding(true);
    options.set_convert_hdr_to_8bit(true);
    Ok(options)
}

fn restrict_u64(existing: u64, hard_cap: u64) -> u64 {
    if existing == 0 {
        hard_cap
    } else {
        existing.min(hard_cap)
    }
}

fn restrict_u32(existing: u32, hard_cap: u32) -> u32 {
    if existing == 0 {
        hard_cap
    } else {
        existing.min(hard_cap)
    }
}

fn validate_dimensions(
    request: &WorkerRequest,
    width: u32,
    height: u32,
) -> Result<(), BackendFailure> {
    let pixels = u64::from(width).checked_mul(u64::from(height));
    if width == 0
        || height == 0
        || width > request.limits.max_source_dimension
        || height > request.limits.max_source_dimension
        || pixels.is_none_or(|value| value > request.limits.max_decode_bytes / 4)
    {
        return Err(BackendFailure::new(
            WorkerErrorCode::ResourceLimited,
            "image dimensions exceed the worker limit",
        ));
    }
    Ok(())
}

fn fit_dimensions(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    if width <= max_edge && height <= max_edge {
        return (width, height);
    }
    if width >= height {
        let scaled_height = (u64::from(height) * u64::from(max_edge) / u64::from(width)).max(1);
        (max_edge, u32::try_from(scaled_height).unwrap_or(1))
    } else {
        let scaled_width = (u64::from(width) * u64::from(max_edge) / u64::from(height)).max(1);
        (u32::try_from(scaled_width).unwrap_or(1), max_edge)
    }
}

fn encode_png(
    image: &libheif_rs::Image,
    has_alpha: bool,
    maximum: u64,
) -> Result<Vec<u8>, BackendFailure> {
    let channels = if has_alpha { 4_usize } else { 3_usize };
    let plane = image.planes().interleaved.ok_or_else(|| {
        BackendFailure::new(
            WorkerErrorCode::DecodeFailed,
            "decoded RGB plane is missing",
        )
    })?;
    let row_bytes = usize::try_from(image.width())
        .ok()
        .and_then(|width| width.checked_mul(channels))
        .ok_or_else(|| {
            BackendFailure::new(WorkerErrorCode::ResourceLimited, "decoded row is too large")
        })?;
    let pixel_bytes = row_bytes
        .checked_mul(usize::try_from(image.height()).unwrap_or(usize::MAX))
        .ok_or_else(|| {
            BackendFailure::new(
                WorkerErrorCode::ResourceLimited,
                "decoded image is too large",
            )
        })?;
    if plane.stride < row_bytes || plane.data.len() < plane.stride.saturating_mul(plane.height as _)
    {
        return Err(BackendFailure::new(
            WorkerErrorCode::DecodeFailed,
            "decoded RGB plane is invalid",
        ));
    }
    let mut pixels = Vec::with_capacity(pixel_bytes);
    for row in plane.data.chunks(plane.stride).take(plane.height as _) {
        pixels.extend_from_slice(&row[..row_bytes]);
    }

    let mut output = BoundedWriter::new(maximum)?;
    let color = if has_alpha {
        ColorType::Rgba8
    } else {
        ColorType::Rgb8
    };
    let encode_result = PngEncoder::new(&mut output).write_image(
        &pixels,
        image.width(),
        image.height(),
        color.into(),
    );
    if output.overflowed {
        return Err(BackendFailure::new(
            WorkerErrorCode::ResourceLimited,
            "PNG output exceeds the worker limit",
        ));
    }
    encode_result
        .map_err(|_| BackendFailure::new(WorkerErrorCode::DecodeFailed, "PNG encoding failed"))?;
    Ok(output.bytes)
}

struct BoundedWriter {
    bytes: Vec<u8>,
    maximum: usize,
    overflowed: bool,
}

impl BoundedWriter {
    fn new(maximum: u64) -> Result<Self, BackendFailure> {
        let maximum = usize::try_from(maximum).map_err(|_| {
            BackendFailure::new(WorkerErrorCode::ResourceLimited, "PNG limit is too large")
        })?;
        Ok(Self {
            bytes: Vec::new(),
            maximum,
            overflowed: false,
        })
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            self.overflowed = true;
            return Err(io::Error::other("bounded PNG output exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn verify_source_snapshot(
    request: &WorkerRequest,
    path: &std::path::Path,
) -> Result<(), BackendFailure> {
    let metadata = fs::metadata(path).map_err(|_| source_changed())?;
    let modified = metadata.modified().map_err(|_| source_changed())?;
    if !metadata.is_file()
        || metadata.len() != request.source_size
        || unix_nanoseconds(modified) != request.source_modified_unix_ns
    {
        return Err(source_changed());
    }
    Ok(())
}

fn unix_nanoseconds(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX),
        Err(error) => -i128::try_from(error.duration().as_nanos()).unwrap_or(i128::MAX),
    }
}

fn source_changed() -> BackendFailure {
    BackendFailure::new(
        WorkerErrorCode::SourceChanged,
        "source changed while the worker request was running",
    )
}

fn map_heif_error(HeifError { code, sub_code, .. }: HeifError) -> BackendFailure {
    if matches!(sub_code, HeifErrorSubCode::SecurityLimitExceeded)
        || matches!(code, HeifErrorCode::MemoryAllocationError)
    {
        return BackendFailure::new(
            WorkerErrorCode::ResourceLimited,
            "libheif resource limit was exceeded",
        );
    }
    if matches!(
        sub_code,
        HeifErrorSubCode::NoMatchingDecoderInstalled | HeifErrorSubCode::UnsupportedCodec
    ) {
        return BackendFailure::new(
            WorkerErrorCode::CodecUnavailable,
            "the bundled worker has no decoder for this codec",
        );
    }
    match code {
        HeifErrorCode::InputDoesNotExist => {
            BackendFailure::new(WorkerErrorCode::Unreadable, "source is unreadable")
        }
        HeifErrorCode::InvalidInput | HeifErrorCode::UnsupportedFileType => BackendFailure::new(
            WorkerErrorCode::InvalidContent,
            "HEIF container content is invalid",
        ),
        HeifErrorCode::UnsupportedFeature => BackendFailure::new(
            WorkerErrorCode::UnsupportedFeature,
            "HEIF content uses an unsupported feature",
        ),
        HeifErrorCode::DecoderPluginError => BackendFailure::new(
            WorkerErrorCode::DecodeFailed,
            "the bundled decoder could not decode the image",
        ),
        _ => BackendFailure::new(
            WorkerErrorCode::DecodeFailed,
            "libheif could not process the image",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_dimensions_never_upscales_and_preserves_a_nonzero_edge() {
        assert_eq!(fit_dimensions(16, 8, 32), (16, 8));
        assert_eq!(fit_dimensions(4_000, 2_000, 1_000), (1_000, 500));
        assert_eq!(fit_dimensions(1, 4_000, 16), (1, 16));
    }

    #[test]
    fn bounded_writer_rejects_before_exceeding_its_allocation() {
        let mut writer = BoundedWriter::new(4).expect("writer");
        assert_eq!(writer.write(b"1234").expect("write"), 4);
        assert!(writer.write(b"5").is_err());
        assert_eq!(writer.bytes, b"1234");
        assert!(writer.overflowed);
    }

    #[test]
    fn zero_defaults_are_replaced_instead_of_disabling_security_limits() {
        assert_eq!(restrict_u64(0, 256), 256);
        assert_eq!(restrict_u64(128, 256), 128);
        assert_eq!(restrict_u32(0, 1_024), 1_024);
        assert_eq!(restrict_u32(2_048, 1_024), 1_024);
    }
}
