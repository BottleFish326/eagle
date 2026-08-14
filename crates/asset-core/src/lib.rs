use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    UnreadableFile(String),
    MissingAsset,
    UnsupportedFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRecord {
    pub key: String,
    pub id: Option<Uuid>,
    pub path: PathBuf,
    pub sidecar_path: Option<PathBuf>,
    pub file_name: String,
    pub extension: Option<String>,
    pub mime: String,
    pub kind: AssetKind,
    pub size: u64,
    pub modified_unix_ms: i64,
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
            path,
            sidecar_path: None,
            file_name,
            extension,
            mime,
            kind,
            size,
            modified_unix_ms,
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
