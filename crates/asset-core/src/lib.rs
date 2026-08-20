use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetKind {
    Image,
    Video,
    Audio,
    Pdf,
    Other,
}

impl AssetKind {
    #[must_use]
    pub fn from_mime(mime: &str) -> Self {
        if mime.starts_with("image/") {
            Self::Image
        } else if mime.starts_with("video/") {
            Self::Video
        } else if mime.starts_with("audio/") {
            Self::Audio
        } else if mime == "application/pdf" {
            Self::Pdf
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "message")]
pub enum AssetIssue {
    InvalidSidecar(String),
    MismatchedSidecar(String),
    UnreadableFile(String),
    InvalidImageMetadata(String),
    InvalidNativeMetadata(String),
    MimeMismatch(String),
    UnsafeEmbeddedContent(String),
    ResourceLimited(String),
    MissingAsset,
    UnsupportedFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarState {
    pub schema: u32,
    pub digest: String,
    pub size: u64,
    pub modified_unix_ms: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeImageMetadata {
    pub orientation: Option<u32>,
    pub captured_at: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_model: Option<String>,
    pub software: Option<String>,
    pub artist: Option<String>,
    pub copyright: Option<String>,
}

impl NativeImageMetadata {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.orientation.is_none()
            && self.captured_at.is_none()
            && self.camera_make.is_none()
            && self.camera_model.is_none()
            && self.lens_model.is_none()
            && self.software.is_none()
            && self.artist.is_none()
            && self.copyright.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRecord {
    pub key: String,
    pub id: Option<Uuid>,
    pub root_id: Option<Uuid>,
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub sidecar_path: Option<PathBuf>,
    pub sidecar_state: Option<SidecarState>,
    pub file_name: String,
    pub extension: Option<String>,
    pub mime: String,
    pub kind: AssetKind,
    pub size: Option<u64>,
    pub created_unix_ms: Option<i64>,
    pub modified_unix_ms: Option<i64>,
    pub file_read_only: Option<bool>,
    pub dimensions: Option<AssetDimensions>,
    pub native_metadata: Option<NativeImageMetadata>,
    pub tags: BTreeSet<String>,
    pub rating: u8,
    pub favorite: bool,
    pub note: String,
    pub aliases: BTreeSet<String>,
    pub issues: Vec<AssetIssue>,
}

impl AssetRecord {
    #[must_use]
    pub fn untagged(
        key: String,
        path: PathBuf,
        mime: String,
        size: u64,
        modified_unix_ms: i64,
    ) -> Self {
        let file_name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
        let kind = AssetKind::from_mime(&mime);

        Self {
            key,
            id: None,
            root_id: None,
            relative_path: path.clone(),
            path,
            sidecar_path: None,
            sidecar_state: None,
            file_name,
            extension,
            mime,
            kind,
            size: Some(size),
            created_unix_ms: None,
            modified_unix_ms: Some(modified_unix_ms),
            file_read_only: None,
            dimensions: None,
            native_metadata: None,
            tags: BTreeSet::new(),
            rating: 0,
            favorite: false,
            note: String::new(),
            aliases: BTreeSet::new(),
            issues: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AssetKind;

    #[test]
    fn classifies_supported_mime_families() {
        assert_eq!(AssetKind::from_mime("image/png"), AssetKind::Image);
        assert_eq!(AssetKind::from_mime("video/mp4"), AssetKind::Video);
        assert_eq!(AssetKind::from_mime("audio/mpeg"), AssetKind::Audio);
        assert_eq!(AssetKind::from_mime("application/pdf"), AssetKind::Pdf);
        assert_eq!(AssetKind::from_mime("text/plain"), AssetKind::Other);
    }
}
