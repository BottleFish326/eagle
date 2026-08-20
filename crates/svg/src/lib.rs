use std::fs;
use std::path::{Path, PathBuf};

use resvg::tiny_skia::{Pixmap, Transform};
use roxmltree::{Document, ParsingOptions};
use thiserror::Error;
use usvg::{ImageHrefResolver, Options, Tree};

pub const MAX_SVG_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_SVG_NODES: u32 = 100_000;
pub const MAX_SVG_DIMENSION: u32 = 65_535;
const MAX_SVG_DIMENSION_F32: f32 = 65_535.0;
pub const SVG_PROVIDER_ID: &str = "safe-static-svg";
pub const SVG_PROVIDER_VERSION: &str = "resvg-0.48.1-no-text-no-external-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvgInspection {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSvg {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Error)]
pub enum SvgError {
    #[error("SVG source exceeds a fixed safety limit")]
    ResourceLimited,
    #[error("SVG file cannot be read at {path}: {source}")]
    Unreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("SVG source is not UTF-8")]
    InvalidUtf8,
    #[error("SVG XML is invalid: {0}")]
    InvalidXml(String),
    #[error("SVG root element is not svg")]
    InvalidRoot,
    #[error("SVG contains a forbidden active or external feature: {0}")]
    UnsafeFeature(&'static str),
    #[error("SVG uses a safe feature not supported by this fixed provider: {0}")]
    UnsupportedFeature(&'static str),
    #[error("SVG dimensions are empty, non-finite, or exceed {MAX_SVG_DIMENSION}")]
    InvalidDimensions,
    #[error("SVG render target edge must be greater than zero")]
    InvalidTargetSize,
    #[error("SVG render target could not be allocated")]
    AllocationFailed,
    #[error("SVG PNG output could not be encoded: {0}")]
    EncodeFailed(String),
}

/// Inspects a bounded SVG without resolving files, URLs, fonts, scripts, or embedded images.
///
/// # Errors
///
/// Returns a stable error when input is oversized, malformed, unsafe, or has invalid dimensions.
pub fn inspect_svg(bytes: &[u8]) -> Result<SvgInspection, SvgError> {
    let (_, inspection) = parse_svg(bytes)?;
    Ok(inspection)
}

/// Reads and inspects one bounded SVG file.
///
/// # Errors
///
/// Returns a stable error without reading content when the source exceeds the size limit.
pub fn inspect_svg_file(path: &Path) -> Result<SvgInspection, SvgError> {
    let bytes = read_bounded(path)?;
    inspect_svg(&bytes)
}

/// Renders a bounded safe SVG to a transparent PNG no larger than `max_edge`.
///
/// # Errors
///
/// Returns a stable error for unsafe input, invalid dimensions, allocation, or PNG encoding.
pub fn render_svg(bytes: &[u8], max_edge: u32) -> Result<RenderedSvg, SvgError> {
    if max_edge == 0 {
        return Err(SvgError::InvalidTargetSize);
    }
    let (tree, inspection) = parse_svg(bytes)?;
    let (width, height) = fit_dimensions(inspection, max_edge);
    let mut pixmap = Pixmap::new(width, height).ok_or(SvgError::AllocationFailed)?;
    let source_size = tree.size();
    let transform = Transform::from_scale(
        bounded_dimension_f32(width) / source_size.width(),
        bounded_dimension_f32(height) / source_size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let bytes = pixmap
        .encode_png()
        .map_err(|error| SvgError::EncodeFailed(error.to_string()))?;
    Ok(RenderedSvg {
        bytes,
        width,
        height,
    })
}

/// Reads and renders one bounded safe SVG file.
///
/// # Errors
///
/// Returns a stable error without reading oversized input or resolving external resources.
pub fn render_svg_file(path: &Path, max_edge: u32) -> Result<RenderedSvg, SvgError> {
    let bytes = read_bounded(path)?;
    render_svg(&bytes, max_edge)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, SvgError> {
    let metadata = fs::metadata(path).map_err(|source| SvgError::Unreadable {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_SVG_SOURCE_BYTES {
        return Err(SvgError::ResourceLimited);
    }
    fs::read(path).map_err(|source| SvgError::Unreadable {
        path: path.to_path_buf(),
        source,
    })
}

fn parse_svg(bytes: &[u8]) -> Result<(Tree, SvgInspection), SvgError> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SVG_SOURCE_BYTES {
        return Err(SvgError::ResourceLimited);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SvgError::InvalidUtf8)?;
    inspect_xml_safety(text)?;
    let options = Options {
        resources_dir: None,
        image_href_resolver: ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Options::default()
    };
    let tree =
        Tree::from_str(text, &options).map_err(|error| SvgError::InvalidXml(error.to_string()))?;
    let size = tree.size();
    let width = checked_dimension(size.width())?;
    let height = checked_dimension(size.height())?;
    Ok((tree, SvgInspection { width, height }))
}

fn inspect_xml_safety(text: &str) -> Result<(), SvgError> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_SVG_NODES,
        entity_resolver: None,
    };
    let document = Document::parse_with_options(text, options)
        .map_err(|error| SvgError::InvalidXml(error.to_string()))?;
    let root = document.root_element();
    if !root.tag_name().name().eq_ignore_ascii_case("svg") {
        return Err(SvgError::InvalidRoot);
    }
    if root
        .attribute("width")
        .is_some_and(declared_dimension_exceeds_limit)
        || root
            .attribute("height")
            .is_some_and(declared_dimension_exceeds_limit)
        || root
            .attribute("viewBox")
            .is_some_and(declared_view_box_exceeds_limit)
    {
        return Err(SvgError::ResourceLimited);
    }
    for node in document.descendants().filter(roxmltree::Node::is_element) {
        let name = node.tag_name().name();
        if name.eq_ignore_ascii_case("script") {
            return Err(SvgError::UnsafeFeature("script"));
        }
        if name.eq_ignore_ascii_case("text") {
            return Err(SvgError::UnsupportedFeature("text"));
        }
        if matches_ignore_ascii_case(
            name,
            &[
                "foreignObject",
                "iframe",
                "animate",
                "animateMotion",
                "animateTransform",
                "set",
            ],
        ) {
            return Err(SvgError::UnsafeFeature("active-element"));
        }
        for attribute in node.attributes() {
            let name = attribute.name();
            let value = attribute.value().trim();
            if name
                .get(..2)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("on"))
            {
                return Err(SvgError::UnsafeFeature("event-handler"));
            }
            if name.eq_ignore_ascii_case("href") && !value.starts_with('#') {
                return Err(SvgError::UnsafeFeature("external-reference"));
            }
            inspect_css_references(value)?;
        }
        if name.eq_ignore_ascii_case("style") {
            inspect_css_references(node.text().unwrap_or_default())?;
        }
    }
    Ok(())
}

fn inspect_css_references(value: &str) -> Result<(), SvgError> {
    let lowercase = value.to_ascii_lowercase();
    if lowercase.contains("@import") || lowercase.contains("javascript:") {
        return Err(SvgError::UnsafeFeature("external-reference"));
    }
    let mut remainder = lowercase.as_str();
    while let Some(start) = remainder.find("url(") {
        remainder = &remainder[start + 4..];
        let Some(end) = remainder.find(')') else {
            return Err(SvgError::UnsafeFeature("external-reference"));
        };
        let target = remainder[..end].trim().trim_matches(['\'', '"']).trim();
        if !target.starts_with('#') {
            return Err(SvgError::UnsafeFeature("external-reference"));
        }
        remainder = &remainder[end + 1..];
    }
    Ok(())
}

fn matches_ignore_ascii_case(value: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| value.eq_ignore_ascii_case(candidate))
}

fn declared_dimension_exceeds_limit(value: &str) -> bool {
    let value = value.trim();
    let number = value
        .strip_suffix("px")
        .unwrap_or(value)
        .trim()
        .parse::<f32>();
    number.is_ok_and(|number| number.is_finite() && number > MAX_SVG_DIMENSION_F32)
}

fn declared_view_box_exceeds_limit(value: &str) -> bool {
    let values = value
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|value| !value.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>();
    values.is_ok_and(|values| {
        values.len() == 4
            && values[2..]
                .iter()
                .any(|dimension| dimension.is_finite() && *dimension > MAX_SVG_DIMENSION_F32)
    })
}

fn fit_dimensions(inspection: SvgInspection, max_edge: u32) -> (u32, u32) {
    if inspection.width <= max_edge && inspection.height <= max_edge {
        return (inspection.width, inspection.height);
    }
    if inspection.width >= inspection.height {
        let height = rounded_ratio(inspection.height, max_edge, inspection.width);
        (max_edge, height)
    } else {
        let width = rounded_ratio(inspection.width, max_edge, inspection.height);
        (width, max_edge)
    }
}

fn rounded_ratio(value: u32, numerator: u32, denominator: u32) -> u32 {
    let value = u64::from(value)
        .saturating_mul(u64::from(numerator))
        .saturating_add(u64::from(denominator) / 2)
        / u64::from(denominator);
    u32::try_from(value).unwrap_or(u32::MAX).max(1)
}

fn bounded_dimension_f32(value: u32) -> f32 {
    f32::from(u16::try_from(value).expect("validated SVG dimension must fit u16"))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn checked_dimension(value: f32) -> Result<u32, SvgError> {
    if !value.is_finite() || value <= 0.0 || value > MAX_SVG_DIMENSION_F32 {
        return Err(SvgError::InvalidDimensions);
    }
    let rounded = value.ceil() as u32;
    (rounded > 0 && rounded <= MAX_SVG_DIMENSION)
        .then_some(rounded)
        .ok_or(SvgError::InvalidDimensions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspects_and_renders_a_static_svg_without_upscaling() {
        let source = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 8"><rect width="16" height="8" fill="#6b7cff"/></svg>"##;
        assert_eq!(
            inspect_svg(source).expect("inspection"),
            SvgInspection {
                width: 16,
                height: 8
            }
        );
        let rendered = render_svg(source, 64).expect("render");
        assert_eq!((rendered.width, rendered.height), (16, 8));
        assert!(rendered.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn downscales_with_a_bounded_output_edge() {
        let source = br#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200"><rect width="400" height="200"/></svg>"#;
        let rendered = render_svg(source, 100).expect("render");
        assert_eq!((rendered.width, rendered.height), (100, 50));
        assert!(matches!(
            render_svg(source, 0),
            Err(SvgError::InvalidTargetSize)
        ));
    }

    #[test]
    fn rejects_active_and_external_features_but_allows_local_fragments() {
        let rejected = [
            br#"<svg xmlns="http://www.w3.org/2000/svg"><script/></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg" onload="run()"/>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.test/x.png"/></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="file.png"/></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"><rect style="fill:url(https://example.test/x.svg)"/></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"><animate attributeName="x"/></svg>"#.as_slice(),
            br#"<svg xmlns="http://www.w3.org/2000/svg"><foreignObject/></svg>"#.as_slice(),
        ];
        for source in rejected {
            assert!(matches!(
                inspect_svg(source),
                Err(SvgError::UnsafeFeature(_))
            ));
        }
        let local = br#"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="g"/></defs><rect fill="url(#g)"/></svg>"#;
        assert!(inspect_svg(local).is_ok());
        assert!(matches!(
            inspect_svg(br#"<svg xmlns="http://www.w3.org/2000/svg"><text>safe</text></svg>"#),
            Err(SvgError::UnsupportedFeature("text"))
        ));
    }

    #[test]
    fn rejects_dtd_invalid_xml_and_oversized_input() {
        assert!(matches!(
            inspect_svg(b"<!DOCTYPE svg><svg/>"),
            Err(SvgError::InvalidXml(_))
        ));
        assert!(matches!(
            inspect_svg(b"<svg>"),
            Err(SvgError::InvalidXml(_))
        ));
        let oversized = vec![b' '; usize::try_from(MAX_SVG_SOURCE_BYTES).unwrap() + 1];
        assert!(matches!(
            inspect_svg(&oversized),
            Err(SvgError::ResourceLimited)
        ));
        assert!(matches!(
            inspect_svg(br#"<svg xmlns="http://www.w3.org/2000/svg" width="70000" height="1"/>"#),
            Err(SvgError::ResourceLimited)
        ));
        assert!(matches!(
            inspect_svg(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 70000"/>"#),
            Err(SvgError::ResourceLimited)
        ));
    }

    #[test]
    fn tracked_svg_fixtures_and_reference_match_the_safe_provider() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/formats");
        let minimal = fs::read(root.join("sources/svg/minimal.svg")).expect("minimal SVG");
        let rendered = render_svg(&minimal, 2_048).expect("tracked SVG render");
        assert_eq!((rendered.width, rendered.height), (16, 16));
        assert_eq!(
            rendered.bytes,
            fs::read(root.join("references/svg/minimal.png")).expect("reference PNG")
        );

        for fixture in ["script.svg", "external-reference.svg"] {
            let source = fs::read(root.join("sources/svg").join(fixture)).expect("unsafe SVG");
            assert!(matches!(
                inspect_svg(&source),
                Err(SvgError::UnsafeFeature(_))
            ));
        }
        let truncated = fs::read(root.join("sources/svg/truncated.svg")).expect("truncated SVG");
        assert!(matches!(
            inspect_svg(&truncated),
            Err(SvgError::InvalidXml(_))
        ));
    }
}
