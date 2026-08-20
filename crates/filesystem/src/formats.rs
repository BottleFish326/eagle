use asset_core::AssetKind;

use crate::isobmff::{IsoBmffKind, classify_file_type};

/// Maximum prefix read while identifying an asset. Format recognition must stay bounded.
pub const MAX_SIGNATURE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatDescriptor {
    pub id: &'static str,
    pub extensions: &'static [&'static str],
    pub mime: &'static str,
    pub kind: AssetKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatRecognition {
    pub descriptor: &'static FormatDescriptor,
    pub source: RecognitionSource,
    pub extension_mismatch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionSource {
    Content,
    Extension,
}

pub static FORMAT_REGISTRY: [FormatDescriptor; 15] = [
    FormatDescriptor {
        id: "png",
        extensions: &["png"],
        mime: "image/png",
        kind: AssetKind::Image,
    },
    FormatDescriptor {
        id: "jpeg",
        extensions: &["jpg", "jpeg"],
        mime: "image/jpeg",
        kind: AssetKind::Image,
    },
    FormatDescriptor {
        id: "gif",
        extensions: &["gif"],
        mime: "image/gif",
        kind: AssetKind::Image,
    },
    FormatDescriptor {
        id: "webp",
        extensions: &["webp"],
        mime: "image/webp",
        kind: AssetKind::Image,
    },
    FormatDescriptor {
        id: "svg",
        extensions: &["svg"],
        mime: "image/svg+xml",
        kind: AssetKind::Image,
    },
    FormatDescriptor {
        id: "avif",
        extensions: &["avif"],
        mime: "image/avif",
        kind: AssetKind::Image,
    },
    FormatDescriptor {
        id: "heic",
        extensions: &["heic"],
        mime: "image/heic",
        kind: AssetKind::Image,
    },
    FormatDescriptor {
        id: "heif",
        extensions: &["heif"],
        mime: "image/heif",
        kind: AssetKind::Image,
    },
    FormatDescriptor {
        id: "mp4",
        extensions: &["mp4", "m4v"],
        mime: "video/mp4",
        kind: AssetKind::Video,
    },
    FormatDescriptor {
        id: "mov",
        extensions: &["mov"],
        mime: "video/quicktime",
        kind: AssetKind::Video,
    },
    FormatDescriptor {
        id: "webm",
        extensions: &["webm"],
        mime: "video/webm",
        kind: AssetKind::Video,
    },
    FormatDescriptor {
        id: "mp3",
        extensions: &["mp3"],
        mime: "audio/mpeg",
        kind: AssetKind::Audio,
    },
    FormatDescriptor {
        id: "wav",
        extensions: &["wav"],
        mime: "audio/wav",
        kind: AssetKind::Audio,
    },
    FormatDescriptor {
        id: "flac",
        extensions: &["flac"],
        mime: "audio/flac",
        kind: AssetKind::Audio,
    },
    FormatDescriptor {
        id: "pdf",
        extensions: &["pdf"],
        mime: "application/pdf",
        kind: AssetKind::Pdf,
    },
];

#[must_use]
pub fn descriptor_for_extension(extension: Option<&str>) -> Option<&'static FormatDescriptor> {
    let extension = extension?.trim_start_matches('.').to_ascii_lowercase();
    FORMAT_REGISTRY
        .iter()
        .find(|descriptor| descriptor.extensions.contains(&extension.as_str()))
}

/// Recognizes a registered format. A bounded content signature is authoritative; a known
/// extension is only a fallback for truncated or temporarily undecodable files.
#[must_use]
pub fn recognize_format(extension: Option<&str>, prefix: &[u8]) -> Option<FormatRecognition> {
    let extension_candidate = descriptor_for_extension(extension);
    if let Some(descriptor) = descriptor_for_content(prefix) {
        return Some(FormatRecognition {
            descriptor,
            source: RecognitionSource::Content,
            extension_mismatch: extension_candidate
                .is_some_and(|candidate| candidate.id != descriptor.id),
        });
    }
    extension_candidate.map(|descriptor| FormatRecognition {
        descriptor,
        source: RecognitionSource::Extension,
        extension_mismatch: false,
    })
}

fn descriptor_for_content(prefix: &[u8]) -> Option<&'static FormatDescriptor> {
    let id = if prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if prefix.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpeg")
    } else if prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a") {
        Some("gif")
    } else if prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WEBP" {
        Some("webp")
    } else if prefix.starts_with(b"fLaC") {
        Some("flac")
    } else if prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WAVE" {
        Some("wav")
    } else if prefix.starts_with(b"%PDF-") {
        Some("pdf")
    } else if prefix.starts_with(b"ID3") || has_mpeg_audio_sync(prefix) {
        Some("mp3")
    } else if prefix.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        && find_ascii_case_insensitive(prefix, b"webm")
    {
        Some("webm")
    } else if let Some(id) = recognize_iso_bmff(prefix) {
        Some(id)
    } else if looks_like_svg(prefix) {
        Some("svg")
    } else {
        None
    }?;
    FORMAT_REGISTRY
        .iter()
        .find(|descriptor| descriptor.id == id)
}

fn recognize_iso_bmff(prefix: &[u8]) -> Option<&'static str> {
    classify_file_type(prefix).map(IsoBmffKind::format_id)
}

fn has_mpeg_audio_sync(prefix: &[u8]) -> bool {
    prefix.len() >= 4
        && prefix[0] == 0xff
        && prefix[1] & 0xe0 == 0xe0
        && prefix[1] & 0x18 != 0x08
        && prefix[1] & 0x06 != 0
        && prefix[2] & 0xf0 != 0xf0
        && prefix[2] & 0x0c != 0x0c
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn looks_like_svg(prefix: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(prefix) else {
        return false;
    };
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let text = if text.starts_with("<?xml") {
        let Some(end) = text.find("?>") else {
            return false;
        };
        text[end + 2..].trim_start()
    } else {
        text
    };
    text.get(..4)
        .is_some_and(|start| start.eq_ignore_ascii_case("<svg"))
        && text
            .as_bytes()
            .get(4)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    use super::*;

    #[test]
    fn registry_identifiers_mimes_and_extensions_are_unique_and_kinds_match() {
        let mut ids = BTreeSet::new();
        let mut mimes = BTreeSet::new();
        let mut extensions = BTreeSet::new();
        for descriptor in &FORMAT_REGISTRY {
            assert!(ids.insert(descriptor.id));
            assert!(mimes.insert(descriptor.mime));
            assert_eq!(AssetKind::from_mime(descriptor.mime), descriptor.kind);
            assert!(!descriptor.extensions.is_empty());
            for extension in descriptor.extensions {
                assert!(extensions.insert(*extension));
            }
        }
    }

    #[test]
    fn recognizes_every_registered_signature() {
        let fixtures: [(&str, &[u8]); 15] = [
            ("png", b"\x89PNG\r\n\x1a\n"),
            ("jpeg", b"\xff\xd8\xff\xe0"),
            ("gif", b"GIF89a"),
            ("webp", b"RIFF\x04\x00\x00\x00WEBP"),
            ("svg", b"<?xml version=\"1.0\"?><svg viewBox=\"0 0 1 1\">"),
            ("avif", b"\x00\x00\x00\x18ftypavif\x00\x00\x00\x00avif"),
            ("heic", b"\x00\x00\x00\x18ftypheic\x00\x00\x00\x00heic"),
            ("heif", b"\x00\x00\x00\x18ftypheif\x00\x00\x00\x00heif"),
            ("mp4", b"\x00\x00\x00\x18ftypisom\x00\x00\x00\x00isom"),
            ("mov", b"\x00\x00\x00\x18ftypqt  \x00\x00\x00\x00qt  "),
            ("webm", b"\x1a\x45\xdf\xa3\x08webm"),
            ("mp3", b"ID3\x04\x00"),
            ("wav", b"RIFF\x04\x00\x00\x00WAVE"),
            ("flac", b"fLaC"),
            ("pdf", b"%PDF-1.7"),
        ];
        for (id, bytes) in fixtures {
            let recognition = recognize_format(None, bytes).expect("registered signature");
            assert_eq!(recognition.descriptor.id, id);
            assert_eq!(recognition.source, RecognitionSource::Content);
        }
    }

    #[test]
    fn content_is_authoritative_when_a_known_extension_conflicts() {
        let recognition = recognize_format(Some("jpg"), b"%PDF-1.7").expect("pdf signature");
        assert_eq!(recognition.descriptor.id, "pdf");
        assert_eq!(recognition.source, RecognitionSource::Content);
        assert!(recognition.extension_mismatch);
    }

    #[test]
    fn a_known_extension_retains_truncated_content() {
        let recognition = recognize_format(Some("SVG"), b"").expect("extension fallback");
        assert_eq!(recognition.descriptor.id, "svg");
        assert_eq!(recognition.source, RecognitionSource::Extension);
        assert!(!recognition.extension_mismatch);
    }

    #[test]
    fn recognizes_the_pinned_libheif_avif_and_heic_fixtures_from_content() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/formats/sources");
        for (relative, expected) in [
            ("avif/libheif-example.avif", "avif"),
            ("heic/libheif-example.heic", "heic"),
        ] {
            let bytes = fs::read(root.join(relative)).expect("pinned libheif fixture");
            let prefix_len = bytes
                .len()
                .min(usize::try_from(MAX_SIGNATURE_BYTES).expect("signature bound"));
            let recognition = recognize_format(None, &bytes[..prefix_len]).expect("recognized");
            assert_eq!(recognition.descriptor.id, expected);
            assert_eq!(recognition.source, RecognitionSource::Content);
        }
    }
}
