use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

mod conflict;
mod edit;

pub use conflict::{
    ConflictAnalysis, ConflictField, ConflictResolutionError, FieldConflictResolution,
    MetadataConflictResolution, TagConflictResolution, UserMetadata, analyze_metadata_conflict,
    resolve_metadata_conflict,
};

pub use edit::{
    MetadataEdit, MetadataPatch, PreparedMetadataEdit, commit_prepared_metadata_edit,
    edit_asset_metadata, edit_asset_metadata_versioned, prepare_asset_metadata_edit,
    prepare_asset_metadata_edit_versioned, validate_metadata_patch,
};

pub const SIDECAR_SCHEMA_VERSION: u32 = 1;
pub const QUICK_FINGERPRINT_ALGORITHM: &str = "sha256-sample-64k-v1";
const QUICK_FINGERPRINT_SAMPLE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub algorithm: String,
    pub value: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_algorithm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_value: Option<String>,
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
    /// Returns [`SidecarError`] when the schema, user metadata, or timestamp are invalid.
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
        if self.tags.iter().any(|tag| tag.chars().count() > 128) {
            return Err(SidecarError::TagTooLong);
        }
        if self.aliases.iter().any(|alias| alias.trim().is_empty()) {
            return Err(SidecarError::EmptyAlias);
        }
        if self.aliases.iter().any(|alias| alias.chars().count() > 256) {
            return Err(SidecarError::AliasTooLong);
        }
        if self.note.chars().count() > 10_000 {
            return Err(SidecarError::NoteTooLong);
        }
        if let Some(fingerprint) = &self.fingerprint {
            fingerprint.validate()?;
        }
        DateTime::parse_from_rfc3339(&self.updated_at)
            .map_err(|_| SidecarError::InvalidUpdatedAt(self.updated_at.clone()))?;
        Ok(())
    }
}

impl Fingerprint {
    fn validate(&self) -> Result<(), SidecarError> {
        if self.algorithm != "sha256" || !is_sha256_hex(&self.value) {
            return Err(SidecarError::InvalidFingerprint);
        }
        match (&self.quick_algorithm, &self.quick_value) {
            (None, None) => Ok(()),
            (Some(algorithm), Some(value))
                if algorithm == QUICK_FINGERPRINT_ALGORITHM && is_sha256_hex(value) =>
            {
                Ok(())
            }
            _ => Err(SidecarError::InvalidFingerprint),
        }
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
    Snapshot(SidecarFileVersion),
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarFileVersion {
    pub digest: String,
    pub size: u64,
    pub modified_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReceipt {
    pub path: PathBuf,
    pub digest: String,
    pub size: u64,
    pub modified_unix_ms: i64,
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
    #[error("tags cannot exceed 128 characters")]
    TagTooLong,
    #[error("aliases cannot contain an empty value")]
    EmptyAlias,
    #[error("aliases cannot exceed 256 characters")]
    AliasTooLong,
    #[error("note cannot exceed 10000 characters")]
    NoteTooLong,
    #[error("asset does not exist or is not a file: {0}")]
    InvalidAsset(PathBuf),
    #[error("metadata edit must change at least one field")]
    EmptyEdit,
    #[error("setTags cannot be combined with addTags or removeTags")]
    AmbiguousTagEdit,
    #[error("the same tag cannot be both added and removed: {0}")]
    ConflictingTagEdit(String),
    #[error("updatedAt is not RFC 3339: {0}")]
    InvalidUpdatedAt(String),
    #[error("fingerprint must use the supported SHA-256 and quick fingerprint formats")]
    InvalidFingerprint,
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

/// Computes the persisted size, sampled quick fingerprint, and full SHA-256 for an asset.
///
/// # Errors
///
/// Returns [`SidecarError`] when the asset cannot be opened, inspected, or read.
pub fn fingerprint_asset(path: &Path) -> Result<Fingerprint, SidecarError> {
    let size = fs::metadata(path)
        .map_err(|source| SidecarError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    Ok(Fingerprint {
        algorithm: "sha256".into(),
        value: digest_file(path)?,
        size,
        quick_algorithm: Some(QUICK_FINGERPRINT_ALGORITHM.into()),
        quick_value: Some(quick_fingerprint_file(path)?),
    })
}

/// Computes the bounded sampled fingerprint used before a full SHA-256 comparison.
///
/// # Errors
///
/// Returns [`SidecarError`] when the asset cannot be opened or read.
pub fn quick_fingerprint_file(path: &Path) -> Result<String, SidecarError> {
    let mut file = File::open(path).map_err(|source| SidecarError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let size = file
        .metadata()
        .map_err(|source| SidecarError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    let sample_size = usize::try_from(size.min(QUICK_FINGERPRINT_SAMPLE_BYTES as u64))
        .unwrap_or(QUICK_FINGERPRINT_SAMPLE_BYTES);
    let mut first = vec![0_u8; sample_size];
    file.read_exact(&mut first)
        .map_err(|source| SidecarError::Io {
            path: path.to_path_buf(),
            source,
        })?;

    let mut digest = Sha256::new();
    digest.update(b"material-eagle-quick-fingerprint-v1\0");
    digest.update(size.to_le_bytes());
    digest.update(&first);
    if size > QUICK_FINGERPRINT_SAMPLE_BYTES as u64 {
        let tail_size = size.min(QUICK_FINGERPRINT_SAMPLE_BYTES as u64);
        let tail_offset = i64::try_from(tail_size).unwrap_or(i64::MAX);
        file.seek(SeekFrom::End(-tail_offset))
            .map_err(|source| SidecarError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        let mut last =
            vec![0_u8; usize::try_from(tail_size).unwrap_or(QUICK_FINGERPRINT_SAMPLE_BYTES)];
        file.read_exact(&mut last)
            .map_err(|source| SidecarError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        digest.update(last);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Reads, parses, and validates a sidecar, returning its content digest.
///
/// # Errors
///
/// Returns [`SidecarError`] when the file cannot be read, parsed, or validated.
pub fn read_sidecar(path: &Path) -> Result<(AssetSidecar, String), SidecarError> {
    let (sidecar, version) = read_sidecar_versioned(path)?;
    Ok((sidecar, version.digest))
}

/// Reads, parses, and validates a Sidecar together with a stable file version snapshot.
///
/// # Errors
///
/// Returns [`SidecarError`] when the file changes while being read or cannot be parsed.
pub fn read_sidecar_versioned(
    path: &Path,
) -> Result<(AssetSidecar, SidecarFileVersion), SidecarError> {
    let (bytes, version) = read_versioned_bytes(path)?;
    let sidecar: AssetSidecar =
        serde_yaml_ng::from_slice(&bytes).map_err(|source| SidecarError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    sidecar.validate()?;
    Ok((sidecar, version))
}

/// Reads a stable Sidecar size, modification time, and SHA-256 snapshot.
///
/// # Errors
///
/// Returns [`SidecarError`] when the file changes during inspection or cannot be read.
pub fn inspect_sidecar_version(path: &Path) -> Result<SidecarFileVersion, SidecarError> {
    read_versioned_bytes(path).map(|(_, version)| version)
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
    inject_fault("before-temp");
    sidecar.validate()?;
    let serialized = serialize_sidecar(sidecar)?;
    write_serialized_sidecar_atomic(path, serialized.as_bytes(), expected)
}

/// Restores exact, validated Sidecar YAML bytes with optimistic concurrency control.
///
/// # Errors
///
/// Returns [`SidecarError`] when the content is invalid, the expected version changed,
/// or the atomic write cannot be completed.
pub fn restore_sidecar_content_atomic(
    path: &Path,
    content: &str,
    expected: &ExpectedVersion,
) -> Result<WriteReceipt, SidecarError> {
    let sidecar: AssetSidecar =
        serde_yaml_ng::from_str(content).map_err(|source| SidecarError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    sidecar.validate()?;
    write_serialized_sidecar_atomic(path, content.as_bytes(), expected)
}

/// Removes a Sidecar only if its digest is still the expected version.
///
/// # Errors
///
/// Returns [`SidecarError`] when the version changed or the file cannot be removed.
pub fn remove_sidecar_if_version(
    path: &Path,
    expected: &ExpectedVersion,
) -> Result<(), SidecarError> {
    verify_expected_version(path, expected)?;
    fs::remove_file(path).map_err(|source| SidecarError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    sync_parent(path.parent().unwrap_or_else(|| Path::new(".")))
}

fn serialize_sidecar(sidecar: &AssetSidecar) -> Result<String, SidecarError> {
    let mut serialized = serde_yaml_ng::to_string(sidecar)?;
    if !serialized.ends_with('\n') {
        serialized.push('\n');
    }
    Ok(serialized)
}

fn write_serialized_sidecar_atomic(
    path: &Path,
    serialized: &[u8],
    expected: &ExpectedVersion,
) -> Result<WriteReceipt, SidecarError> {
    verify_expected_version(path, expected)?;
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (persistence_parent, persistence_path) = persistence_paths(parent, path)?;

    let mut temp =
        NamedTempFile::new_in(&persistence_parent).map_err(|source| SidecarError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    temp.write_all(serialized)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|source| SidecarError::Io {
            path: temp.path().to_path_buf(),
            source,
        })?;

    inject_fault("after-temp-sync");
    verify_expected_version(path, expected)?;
    temp.persist(&persistence_path)
        .map_err(|error| SidecarError::Persist {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    inject_fault("after-persist");
    sync_parent(parent)?;

    let version = inspect_sidecar_version(path)?;
    if version.digest != digest_bytes(serialized) {
        return Err(SidecarError::Conflict {
            path: path.to_path_buf(),
        });
    }
    Ok(WriteReceipt {
        path: path.to_path_buf(),
        digest: version.digest,
        size: version.size,
        modified_unix_ms: version.modified_unix_ms,
    })
}

#[cfg(windows)]
fn persistence_paths(parent: &Path, path: &Path) -> Result<(PathBuf, PathBuf), SidecarError> {
    // `canonicalize` returns a verbatim Windows path. Keep both tempfile creation and
    // replacement in that namespace because tempfile's replacement call otherwise
    // receives the legacy-length spelling even when Rust's ordinary file APIs can
    // open the same path.
    let canonical_parent = parent.canonicalize().map_err(|source| SidecarError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let Some(file_name) = path.file_name() else {
        return Err(SidecarError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "sidecar destination has no file name",
            ),
        });
    };
    let persistence_path = canonical_parent.join(file_name);
    Ok((canonical_parent, persistence_path))
}

#[cfg(not(windows))]
#[allow(clippy::unnecessary_wraps)]
fn persistence_paths(parent: &Path, path: &Path) -> Result<(PathBuf, PathBuf), SidecarError> {
    Ok((parent.to_path_buf(), path.to_path_buf()))
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
        ExpectedVersion::Snapshot(expected) if path.is_file() => {
            if inspect_sidecar_version(path)? == *expected {
                Ok(())
            } else {
                Err(SidecarError::Conflict {
                    path: path.to_path_buf(),
                })
            }
        }
        ExpectedVersion::Missing | ExpectedVersion::Digest(_) | ExpectedVersion::Snapshot(_) => {
            Err(SidecarError::Conflict {
                path: path.to_path_buf(),
            })
        }
    }
}

fn read_versioned_bytes(path: &Path) -> Result<(Vec<u8>, SidecarFileVersion), SidecarError> {
    let before = fs::metadata(path).map_err(|source| SidecarError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let before_modified = modified_unix_ms(&before, path)?;
    let bytes = fs::read(path).map_err(|source| SidecarError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let after = fs::metadata(path).map_err(|source| SidecarError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let after_modified = modified_unix_ms(&after, path)?;
    if before.len() != after.len()
        || before_modified != after_modified
        || after.len() != u64::try_from(bytes.len()).unwrap_or(u64::MAX)
    {
        return Err(SidecarError::Conflict {
            path: path.to_path_buf(),
        });
    }
    let digest = digest_bytes(&bytes);
    Ok((
        bytes,
        SidecarFileVersion {
            digest,
            size: after.len(),
            modified_unix_ms: after_modified,
        },
    ))
}

fn modified_unix_ms(metadata: &fs::Metadata, path: &Path) -> Result<i64, SidecarError> {
    let modified = metadata.modified().map_err(|source| SidecarError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(system_time_to_unix_ms(modified))
}

fn system_time_to_unix_ms(value: SystemTime) -> i64 {
    match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

#[cfg(unix)]
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

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn sync_parent(_parent: &Path) -> Result<(), SidecarError> {
    // Rust's standard File API cannot portably open a directory for fsync on Windows.
    // The file itself has already been flushed before atomic replacement. Keep the
    // fallible signature aligned with Unix so callers preserve the same durability flow.
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(feature = "fault-injection")]
fn inject_fault(point: &str) {
    if std::env::var("EAGLE_SIDECAR_FAULT_POINT").as_deref() == Ok(point) {
        std::process::abort();
    }
}

#[cfg(not(feature = "fault-injection"))]
const fn inject_fault(_point: &str) {}

#[cfg(test)]
mod tests {
    use std::fs::{self, FileTimes, OpenOptions};
    use std::time::{Duration, SystemTime};

    use serde_yaml_ng::Value;
    use tempfile::tempdir;

    use super::{
        AssetSidecar, ExpectedVersion, SidecarError, SidecarFileVersion, read_sidecar,
        sidecar_path_for, write_sidecar_atomic,
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

    #[test]
    fn rejects_a_same_content_file_with_a_changed_modification_time() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("logo.png.asset.yml");
        let original = AssetSidecar::new();
        let receipt =
            write_sidecar_atomic(&path, &original, &ExpectedVersion::Missing).expect("write");
        let original_bytes = fs::read(&path).expect("read original");
        let expected = SidecarFileVersion {
            digest: receipt.digest,
            size: receipt.size,
            modified_unix_ms: receipt.modified_unix_ms,
        };

        let file = OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open sidecar");
        file.set_times(
            FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(3_600)),
        )
        .expect("change modification time");

        let mut proposed = original;
        proposed.note = "my edit".into();
        let error = write_sidecar_atomic(&path, &proposed, &ExpectedVersion::Snapshot(expected))
            .expect_err("mtime-only external touch must invalidate the snapshot");

        assert!(matches!(error, SidecarError::Conflict { .. }));
        assert_eq!(fs::read(path).expect("read unchanged"), original_bytes);
    }
}
