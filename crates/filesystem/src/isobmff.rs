#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IsoBmffKind {
    Avif,
    Heic,
    Heif,
    Mp4,
    Mov,
}

impl IsoBmffKind {
    pub(crate) const fn format_id(self) -> &'static str {
        match self {
            Self::Avif => "avif",
            Self::Heic => "heic",
            Self::Heif => "heif",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
        }
    }
}

/// Classifies an ISO BMFF file from its first, bounded `ftyp` box.
///
/// A declared box may extend beyond the signature window; the classifier only consumes complete
/// four-byte brands already present in the window. Structurally invalid or unknown `ftyp` boxes
/// are not treated as a registered format.
pub(crate) fn classify_file_type(prefix: &[u8]) -> Option<IsoBmffKind> {
    let brands = FileTypeBrands::parse(prefix)?;
    if brands.contains_any(&[b"avif", b"avis"]) {
        Some(IsoBmffKind::Avif)
    } else if brands.contains_any(&[
        b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs",
    ]) {
        Some(IsoBmffKind::Heic)
    } else if brands.contains_any(&[b"heif", b"mif1", b"msf1"]) {
        Some(IsoBmffKind::Heif)
    } else if brands.contains_any(&[b"qt  "]) {
        Some(IsoBmffKind::Mov)
    } else if brands.contains_any(&[
        b"isom", b"iso2", b"iso3", b"iso4", b"iso5", b"iso6", b"avc1", b"dash", b"M4V ", b"mp41",
        b"mp42", b"mp71",
    ]) {
        Some(IsoBmffKind::Mp4)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
struct FileTypeBrands<'a> {
    major: &'a [u8; 4],
    compatible: &'a [u8],
}

impl<'a> FileTypeBrands<'a> {
    fn parse(prefix: &'a [u8]) -> Option<Self> {
        const STANDARD_HEADER_BYTES: usize = 8;
        const EXTENDED_HEADER_BYTES: usize = 16;
        const FIXED_PAYLOAD_BYTES: usize = 8;

        if prefix.len() < STANDARD_HEADER_BYTES || prefix.get(4..8)? != b"ftyp" {
            return None;
        }

        let size32 = u32::from_be_bytes(prefix.get(..4)?.try_into().ok()?);
        let (header_bytes, declared_size) = match size32 {
            0 => (STANDARD_HEADER_BYTES, None),
            1 => {
                let size64 = u64::from_be_bytes(prefix.get(8..16)?.try_into().ok()?);
                (EXTENDED_HEADER_BYTES, Some(size64))
            }
            size => (STANDARD_HEADER_BYTES, Some(u64::from(size))),
        };
        let minimum_size = header_bytes.checked_add(FIXED_PAYLOAD_BYTES)?;
        let minimum_size_u64 = u64::try_from(minimum_size).ok()?;
        if declared_size.is_some_and(|size| size < minimum_size_u64) {
            return None;
        }

        let available_end = declared_size
            .and_then(|size| usize::try_from(size).ok())
            .map_or(prefix.len(), |size| size.min(prefix.len()));
        if available_end < minimum_size {
            return None;
        }

        let major = prefix
            .get(header_bytes..header_bytes + 4)?
            .try_into()
            .ok()?;
        let compatible_start = header_bytes + FIXED_PAYLOAD_BYTES;
        let compatible_len = (available_end - compatible_start) / 4 * 4;
        let compatible = prefix.get(compatible_start..compatible_start + compatible_len)?;
        Some(Self { major, compatible })
    }

    fn contains_any(self, expected: &[&[u8; 4]]) -> bool {
        expected.iter().any(|expected| {
            self.major == *expected
                || self
                    .compatible
                    .chunks_exact(4)
                    .any(|brand| brand == expected.as_slice())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_major_and_compatible_image_brands_without_allocating() {
        assert_eq!(
            classify_file_type(b"\0\0\0\x18ftypavif\0\0\0\0mif1"),
            Some(IsoBmffKind::Avif)
        );
        assert_eq!(
            classify_file_type(b"\0\0\0\x1cftypmif1\0\0\0\0heixmif1"),
            Some(IsoBmffKind::Heic)
        );
        assert_eq!(
            classify_file_type(b"\0\0\0\x18ftypmif1\0\0\0\0mif1"),
            Some(IsoBmffKind::Heif)
        );
    }

    #[test]
    fn honors_ftyp_boundaries_and_rejects_malformed_or_unknown_boxes() {
        assert_eq!(
            classify_file_type(b"\0\0\0\x10ftypisom\0\0\0\0avif"),
            Some(IsoBmffKind::Mp4),
            "bytes after the declared ftyp box must not become compatible brands"
        );
        assert_eq!(classify_file_type(b"\0\0\0\x08ftypavif"), None);
        assert_eq!(classify_file_type(b"\0\0\0\x18freeavif\0\0\0\0avif"), None);
        assert_eq!(classify_file_type(b"\0\0\0\x18ftypzzzz\0\0\0\0zzzz"), None);
    }

    #[test]
    fn supports_extended_and_open_ended_ftyp_boxes_within_the_prefix() {
        assert_eq!(
            classify_file_type(b"\0\0\0\x01ftyp\0\0\0\0\0\0\0\x20avif\0\0\0\0mif1"),
            Some(IsoBmffKind::Avif)
        );
        assert_eq!(
            classify_file_type(b"\0\0\0\0ftypheic\0\0\0\0mif1"),
            Some(IsoBmffKind::Heic)
        );
    }

    #[test]
    fn recognizes_the_additional_heif_sequence_and_still_image_brands() {
        for brand in [b"heim", b"heis", b"hevm", b"hevs"] {
            let mut bytes = b"\0\0\0\x18ftypxxxx\0\0\0\0xxxx".to_vec();
            bytes[8..12].copy_from_slice(brand);
            assert_eq!(classify_file_type(&bytes), Some(IsoBmffKind::Heic));
        }
    }
}
