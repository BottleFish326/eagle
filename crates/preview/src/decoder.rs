use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use crate::ThumbnailPlaceholderReason;

const MAX_SOURCE_DIMENSION: u32 = 65_535;
const MAX_DECODE_ALLOCATION: u64 = 256 * 1024 * 1024;

pub(crate) struct DecodedThumbnail {
    pub(crate) bytes: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) struct DecodeFailure {
    pub(crate) reason: ThumbnailPlaceholderReason,
    pub(crate) message: String,
}

pub(crate) fn decode_thumbnail(
    path: &Path,
    max_edge: u32,
) -> Result<DecodedThumbnail, DecodeFailure> {
    let file =
        File::open(path).map_err(|error| failure(ThumbnailPlaceholderReason::Unreadable, error))?;
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| failure(ThumbnailPlaceholderReason::Unreadable, error))?;
    let format = reader.format().ok_or_else(|| DecodeFailure {
        reason: ThumbnailPlaceholderReason::UnsupportedFormat,
        message: "image format could not be detected".into(),
    })?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP
    ) {
        return Err(DecodeFailure {
            reason: ThumbnailPlaceholderReason::UnsupportedFormat,
            message: format!("image format is not supported for thumbnails: {format:?}"),
        });
    }

    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_DIMENSION);
    limits.max_image_height = Some(MAX_SOURCE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| failure(ThumbnailPlaceholderReason::DecodeFailed, error))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut source = DynamicImage::from_decoder(decoder)
        .map_err(|error| failure(ThumbnailPlaceholderReason::DecodeFailed, error))?;
    source.apply_orientation(orientation);

    let thumbnail = if source.width() <= max_edge && source.height() <= max_edge {
        source
    } else {
        source.resize(max_edge, max_edge, FilterType::Triangle)
    };
    let width = thumbnail.width();
    let height = thumbnail.height();
    let mut output = Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|error| failure(ThumbnailPlaceholderReason::DecodeFailed, error))?;
    Ok(DecodedThumbnail {
        bytes: output.into_inner(),
        width,
        height,
    })
}

fn failure(reason: ThumbnailPlaceholderReason, error: impl std::fmt::Display) -> DecodeFailure {
    DecodeFailure {
        reason,
        message: error.to_string(),
    }
}
