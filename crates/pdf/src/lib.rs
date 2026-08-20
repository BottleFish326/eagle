use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};
use thiserror::Error;

pub const MAX_PDF_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_PDF_OBJECTS: usize = 100_000;
pub const MAX_PDF_PAGES: u32 = 100_000;
pub const MAX_PDF_PAGE_DIMENSION: u32 = 65_535;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfInspection {
    pub page_count: u32,
    pub width: u32,
    pub height: u32,
    pub has_unsafe_content: bool,
}

#[derive(Debug, Error)]
pub enum PdfInspectError {
    #[error("PDF source is unreadable")]
    Unreadable,
    #[error("PDF source changed during inspection")]
    SourceChanged,
    #[error("PDF structure is malformed")]
    InvalidContent,
    #[error("PDF structure exceeds a fixed inspection limit")]
    ResourceLimited,
    #[error("PDF uses an unsupported protected or compressed structure")]
    UnsupportedFeature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceVersion {
    len: u64,
    modified: Option<SystemTime>,
}

/// Inspects a classic cross-reference PDF without rendering pages or decoding content streams.
///
/// Encrypted documents and PDFs using xref/object streams remain visible to the scanner but are
/// intentionally left to a future isolated worker. The source and adjacent Sidecar are never
/// modified.
///
/// # Errors
///
/// Returns a stable failure for unreadable, changing, malformed, oversized, protected, or
/// compressed-structure inputs.
pub fn inspect_pdf_file(path: &Path, timeout: Duration) -> Result<PdfInspection, PdfInspectError> {
    let started = Instant::now();
    if timeout.is_zero() {
        return Err(PdfInspectError::ResourceLimited);
    }
    let before = source_version(path)?;
    if before.len > MAX_PDF_SOURCE_BYTES {
        return Err(PdfInspectError::ResourceLimited);
    }
    let bytes = fs::read(path).map_err(|_| PdfInspectError::Unreadable)?;
    preflight_pdf(&bytes)?;
    if started.elapsed() >= timeout {
        return Err(PdfInspectError::ResourceLimited);
    }

    let options = LoadOptions {
        strict: true,
        ..LoadOptions::default()
    };
    let document = Document::load_mem_with_options(&bytes, options)
        .map_err(|_| PdfInspectError::InvalidContent)?;
    if started.elapsed() >= timeout {
        return Err(PdfInspectError::ResourceLimited);
    }
    if document.objects.len() > MAX_PDF_OBJECTS {
        return Err(PdfInspectError::ResourceLimited);
    }
    validate_declared_page_counts(&document)?;
    let pages = document.get_pages();
    let page_count = u32::try_from(pages.len()).map_err(|_| PdfInspectError::ResourceLimited)?;
    if page_count == 0 {
        return Err(PdfInspectError::InvalidContent);
    }
    if page_count > MAX_PDF_PAGES {
        return Err(PdfInspectError::ResourceLimited);
    }
    let first_page = pages
        .first_key_value()
        .map(|(_, object_id)| *object_id)
        .ok_or(PdfInspectError::InvalidContent)?;
    let (mut width, mut height) = inherited_page_box(&document, first_page)?;
    if inherited_rotation(&document, first_page)? % 180 != 0 {
        std::mem::swap(&mut width, &mut height);
    }
    let has_unsafe_content = document
        .objects
        .values()
        .any(|object| object_has_unsafe_content(object, 0));

    drop(document);
    if started.elapsed() >= timeout {
        return Err(PdfInspectError::ResourceLimited);
    }
    let after = source_version(path)?;
    if before != after {
        return Err(PdfInspectError::SourceChanged);
    }
    Ok(PdfInspection {
        page_count,
        width,
        height,
        has_unsafe_content,
    })
}

fn source_version(path: &Path) -> Result<SourceVersion, PdfInspectError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PdfInspectError::Unreadable)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PdfInspectError::Unreadable);
    }
    Ok(SourceVersion {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn preflight_pdf(bytes: &[u8]) -> Result<(), PdfInspectError> {
    if !matches!(bytes.get(..8), Some(header) if header.starts_with(b"%PDF-1.") || header.starts_with(b"%PDF-2."))
    {
        return Err(PdfInspectError::InvalidContent);
    }
    let trailer_start = bytes.len().saturating_sub(1_024);
    if !bytes[trailer_start..]
        .windows(5)
        .any(|window| window == b"%%EOF")
    {
        return Err(PdfInspectError::InvalidContent);
    }
    if contains_name(bytes, b"Encrypt")
        || contains_name(bytes, b"ObjStm")
        || contains_name(bytes, b"XRef")
    {
        return Err(PdfInspectError::UnsupportedFeature);
    }
    Ok(())
}

fn contains_name(bytes: &[u8], name: &[u8]) -> bool {
    let mut token = Vec::with_capacity(name.len() + 1);
    token.push(b'/');
    token.extend_from_slice(name);
    bytes.windows(token.len()).any(|window| window == token)
}

fn validate_declared_page_counts(document: &Document) -> Result<(), PdfInspectError> {
    for object in document.objects.values() {
        let Some(dictionary) = object_dictionary(object) else {
            continue;
        };
        if dictionary.get(b"Type").and_then(Object::as_name).ok() != Some(b"Pages") {
            continue;
        }
        let count = dictionary
            .get(b"Count")
            .and_then(|object| document.dereference(object).map(|(_, value)| value))
            .and_then(Object::as_i64)
            .map_err(|_| PdfInspectError::InvalidContent)?;
        if count < 0 || count > i64::from(MAX_PDF_PAGES) {
            return Err(PdfInspectError::ResourceLimited);
        }
    }
    Ok(())
}

fn inherited_page_box(
    document: &Document,
    page_id: ObjectId,
) -> Result<(u32, u32), PdfInspectError> {
    let mut current = page_id;
    for _ in 0..128 {
        let dictionary = document
            .get_dictionary(current)
            .map_err(|_| PdfInspectError::InvalidContent)?;
        for key in [b"CropBox".as_slice(), b"MediaBox".as_slice()] {
            if let Ok(value) = dictionary.get(key) {
                let (_, value) = document
                    .dereference(value)
                    .map_err(|_| PdfInspectError::InvalidContent)?;
                return page_box_dimensions(value);
            }
        }
        current = parent_id(dictionary)?;
    }
    Err(PdfInspectError::ResourceLimited)
}

fn inherited_rotation(document: &Document, page_id: ObjectId) -> Result<i64, PdfInspectError> {
    let mut current = page_id;
    for _ in 0..128 {
        let dictionary = document
            .get_dictionary(current)
            .map_err(|_| PdfInspectError::InvalidContent)?;
        if let Ok(value) = dictionary.get(b"Rotate") {
            let (_, value) = document
                .dereference(value)
                .map_err(|_| PdfInspectError::InvalidContent)?;
            let rotation = value
                .as_i64()
                .map_err(|_| PdfInspectError::InvalidContent)?;
            if rotation % 90 != 0 {
                return Err(PdfInspectError::InvalidContent);
            }
            return Ok(rotation.rem_euclid(360));
        }
        let Ok(parent) = parent_id(dictionary) else {
            return Ok(0);
        };
        current = parent;
    }
    Err(PdfInspectError::ResourceLimited)
}

fn parent_id(dictionary: &Dictionary) -> Result<ObjectId, PdfInspectError> {
    dictionary
        .get(b"Parent")
        .and_then(Object::as_reference)
        .map_err(|_| PdfInspectError::InvalidContent)
}

fn page_box_dimensions(value: &Object) -> Result<(u32, u32), PdfInspectError> {
    let values = value
        .as_array()
        .map_err(|_| PdfInspectError::InvalidContent)?;
    if values.len() != 4 {
        return Err(PdfInspectError::InvalidContent);
    }
    let coordinates = values
        .iter()
        .map(Object::as_float)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| PdfInspectError::InvalidContent)?;
    let width = coordinates[2] - coordinates[0];
    let height = coordinates[3] - coordinates[1];
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(PdfInspectError::InvalidContent);
    }
    if width > 65_535.0 || height > 65_535.0 {
        return Err(PdfInspectError::ResourceLimited);
    }
    Ok((
        bounded_page_dimension(width),
        bounded_page_dimension(height),
    ))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn bounded_page_dimension(value: f32) -> u32 {
    // The caller proves that value is finite and within 0 < value <= 65_535 before conversion.
    value.ceil() as u32
}

fn object_has_unsafe_content(object: &Object, depth: usize) -> bool {
    if depth >= 100 {
        return true;
    }
    match object {
        Object::Array(values) => values
            .iter()
            .any(|value| object_has_unsafe_content(value, depth + 1)),
        Object::Dictionary(dictionary) => dictionary_has_unsafe_content(dictionary, depth + 1),
        Object::Stream(stream) => dictionary_has_unsafe_content(&stream.dict, depth + 1),
        _ => false,
    }
}

fn dictionary_has_unsafe_content(dictionary: &Dictionary, depth: usize) -> bool {
    const UNSAFE_KEYS: [&[u8]; 11] = [
        b"AA",
        b"EmbeddedFiles",
        b"Filespec",
        b"JavaScript",
        b"JS",
        b"Launch",
        b"OpenAction",
        b"RichMedia",
        b"SubmitForm",
        b"URI",
        b"GoToR",
    ];
    dictionary.iter().any(|(key, value)| {
        UNSAFE_KEYS.contains(&key.as_slice()) || object_has_unsafe_content(value, depth)
    })
}

fn object_dictionary(object: &Object) -> Option<&Dictionary> {
    match object {
        Object::Dictionary(dictionary) => Some(dictionary),
        Object::Stream(stream) => Some(&stream.dict),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/formats/sources/pdf")
            .join(relative)
    }

    #[test]
    fn inspects_classic_pdf_page_count_and_first_page_box_without_rendering() {
        let inspection = inspect_pdf_file(&fixture("minimal.pdf"), Duration::from_secs(1))
            .expect("classic PDF inspection");
        assert_eq!(inspection.page_count, 2);
        assert_eq!((inspection.width, inspection.height), (612, 792));
        assert!(!inspection.has_unsafe_content);
    }

    #[test]
    fn detects_actions_and_external_references_without_executing_them() {
        for name in ["active-javascript.pdf", "external-uri.pdf"] {
            let inspection = inspect_pdf_file(&fixture(name), Duration::from_secs(1))
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(inspection.page_count, 2, "{name}");
            assert!(inspection.has_unsafe_content, "{name}");
        }
    }

    #[test]
    fn isolates_truncation_protection_compressed_structures_and_resource_bombs() {
        assert!(matches!(
            inspect_pdf_file(&fixture("truncated.pdf"), Duration::from_secs(1)),
            Err(PdfInspectError::InvalidContent)
        ));
        for name in ["encrypted.pdf", "object-stream.pdf"] {
            assert!(
                matches!(
                    inspect_pdf_file(&fixture(name), Duration::from_secs(1)),
                    Err(PdfInspectError::UnsupportedFeature)
                ),
                "{name}"
            );
        }
        for name in ["oversized-page-count.pdf", "oversized-page-dimensions.pdf"] {
            assert!(
                matches!(
                    inspect_pdf_file(&fixture(name), Duration::from_secs(1)),
                    Err(PdfInspectError::ResourceLimited)
                ),
                "{name}"
            );
        }
        assert!(matches!(
            inspect_pdf_file(&fixture("minimal.pdf"), Duration::ZERO),
            Err(PdfInspectError::ResourceLimited)
        ));
    }
}
