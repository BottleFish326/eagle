use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

pub const SIDECAR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub algorithm: String,
    pub value: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSidecar {
    pub schema: u32,
    pub id: Uuid,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub rating: u8,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub aliases: BTreeSet<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<Fingerprint>,
    pub updated_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl AssetSidecar {
    #[must_use]
    pub fn new() -> Self {
        Self::with_id(Uuid::now_v7())
    }

    #[must_use]
    pub fn with_id(id: Uuid) -> Self {
        Self {
            schema: SIDECAR_SCHEMA_VERSION,
            id,
            tags: BTreeSet::new(),
            rating: 0,
            favorite: false,
            note: String::new(),
            aliases: BTreeSet::new(),
            fingerprint: None,
            updated_at: now_rfc3339(),
            extra: BTreeMap::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now_rfc3339();
    }

    /// Validates fields whose constraints are stricter than Serde's shape checks.
    ///
    /// # Errors
    ///
    /// Returns [`SidecarError`] when the schema, rating, tags, or timestamp are invalid.
    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.schema != SIDECAR_SCHEMA_VERSION {
            return Err(SidecarError::UnsupportedSchema(self.schema));
        }
        if self.rating > 5 {
            return Err(SidecarError::InvalidRating(self.rating));
        }
        if self.tags.iter().any(|tag| tag.trim().is_empty()) {
            return Err(SidecarError::EmptyTag);
        }
        DateTime::parse_from_rfc3339(&self.updated_at)
            .map_err(|_| SidecarError::InvalidUpdatedAt(self.updated_at.clone()))?;
        Ok(())
    }
}

impl Default for AssetSidecar {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedVersion {
    Missing,
    Digest(String),
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReceipt {
    pub path: PathBuf,
    pub digest: String,
}

#[derive(Debug, Error)]
pub enum SidecarError {
    #[error("sidecar I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid sidecar YAML at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[error("cannot serialize sidecar: {0}")]
    Serialize(#[from] serde_yaml_ng::Error),
    #[error("unsupported sidecar schema: {0}")]
    UnsupportedSchema(u32),
    #[error("rating must be between 0 and 5, got {0}")]
    InvalidRating(u8),
    #[error("tags cannot contain an empty value")]
    EmptyTag,
    #[error("updatedAt is not RFC 3339: {0}")]
    InvalidUpdatedAt(String),
    #[error("sidecar changed since it was read: {path}")]
    Conflict { path: PathBuf },
    #[error("failed to persist sidecar at {path}: {source}")]
    Persist {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[must_use]
pub fn sidecar_path_for(asset_path: &Path) -> PathBuf {
    let mut name: OsString = asset_path
        .file_name()
        .map_or_else(OsString::new, OsString::from);
    name.push(".asset.yml");
    asset_path.with_file_name(name)
}

/// Reads, parses, and validates a sidecar, returning its content digest.
///
/// # Errors
///
/// Returns [`SidecarError`] when the file cannot be read, parsed, or validated.
pub fn read_sidecar(path: &Path) -> Result<(AssetSidecar, String), SidecarError> {
    let bytes = fs::read(path).map_err(|source| SidecarError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let sidecar: AssetSidecar =
        serde_yaml_ng::from_slice(&bytes).map_err(|source| SidecarError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    sidecar.validate()?;
    Ok((sidecar, digest_bytes(&bytes)))
}

/// Writes a sidecar through a same-directory temporary file and atomic replacement.
///
/// # Errors
///
/// Returns [`SidecarError`] when validation, optimistic concurrency checking, writing,
/// flushing, or atomic replacement fails.
pub fn write_sidecar_atomic(
    path: &Path,
    sidecar: &AssetSidecar,
    expected: &ExpectedVersion,
) -> Result<WriteReceipt, SidecarError> {
    sidecar.validate()?;
    verify_expected_version(path, expected)?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut serialized = serde_yaml_ng::to_string(sidecar)?;
    if !serialized.ends_with('\n') {
        serialized.push('\n');
    }

    let mut temp = NamedTempFile::new_in(parent).map_err(|source| SidecarError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    temp.write_all(serialized.as_bytes())
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|source| SidecarError::Io {
            path: temp.path().to_path_buf(),
            source,
        })?;

    verify_expected_version(path, expected)?;
    temp.persist(path).map_err(|error| SidecarError::Persist {
        path: path.to_path_buf(),
        source: error.error,
    })?;
    sync_parent(parent)?;

    Ok(WriteReceipt {
        path: path.to_path_buf(),
        digest: digest_bytes(serialized.as_bytes()),
    })
}

/// Computes the lowercase SHA-256 digest of a file.
///
/// # Errors
///
/// Returns [`SidecarError`] when the file cannot be opened or read.
pub fn digest_file(path: &Path) -> Result<String, SidecarError> {
    let mut file = File::open(path).map_err(|source| SidecarError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| SidecarError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_expected_version(path: &Path, expected: &ExpectedVersion) -> Result<(), SidecarError> {
    match expected {
        ExpectedVersion::Any => Ok(()),
        ExpectedVersion::Missing if !path.exists() => Ok(()),
        ExpectedVersion::Digest(expected_digest) if path.is_file() => {
            let actual = digest_file(path)?;
            if actual == *expected_digest {
                Ok(())
            } else {
                Err(SidecarError::Conflict {
                    path: path.to_path_buf(),
                })
            }
        }
        ExpectedVersion::Missing | ExpectedVersion::Digest(_) => Err(SidecarError::Conflict {
            path: path.to_path_buf(),
        }),
    }
}

fn sync_parent(parent: &Path) -> Result<(), SidecarError> {
    let directory = File::open(parent).map_err(|source| SidecarError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    directory.sync_all().map_err(|source| SidecarError::Io {
        path: parent.to_path_buf(),
        source,
    })
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_yaml_ng::Value;
    use tempfile::tempdir;

    use super::{
        AssetSidecar, ExpectedVersion, SidecarError, read_sidecar, sidecar_path_for,
        write_sidecar_atomic,
    };

    #[test]
    fn sidecar_name_keeps_the_asset_extension() {
        assert_eq!(
            sidecar_path_for(std::path::Path::new("/tmp/logo.png")),
            std::path::PathBuf::from("/tmp/logo.png.asset.yml")
        );
    }

    #[test]
    fn round_trip_preserves_unknown_fields() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("logo.png.asset.yml");
        let mut sidecar = AssetSidecar::new();
        sidecar.tags.insert("ui/icon".into());
        sidecar
            .extra
            .insert("futureField".into(), Value::String("kept".into()));

        write_sidecar_atomic(&path, &sidecar, &ExpectedVersion::Missing).expect("write");
        let (read, digest) = read_sidecar(&path).expect("read");
        assert_eq!(read, sidecar);
        assert_eq!(digest.len(), 64);
    }

    #[test]
    fn rejects_stale_writes_without_changing_the_file() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("logo.png.asset.yml");
        let original = AssetSidecar::new();
        let receipt =
            write_sidecar_atomic(&path, &original, &ExpectedVersion::Missing).expect("write");

        fs::write(&path, "external: edit\n").expect("external edit");
        let error = write_sidecar_atomic(
            &path,
            &AssetSidecar::new(),
            &ExpectedVersion::Digest(receipt.digest),
        )
        .expect_err("stale write must fail");

        assert!(matches!(error, SidecarError::Conflict { .. }));
        assert_eq!(fs::read_to_string(path).expect("read"), "external: edit\n");
    }
}
