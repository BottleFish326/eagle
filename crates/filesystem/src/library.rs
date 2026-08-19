use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

use crate::platform::{PathRelation, PlatformFamily, path_relation_for_platform};

pub const LIBRARY_CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryConfig {
    pub schema: u32,
    #[serde(default)]
    pub roots: Vec<LibraryRoot>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            schema: LIBRARY_CONFIG_SCHEMA_VERSION,
            roots: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRoot {
    pub id: Uuid,
    pub path: PathBuf,
    pub name: String,
    pub enabled: bool,
    pub scan: RootScanSettings,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootScanSettings {
    pub recursive: bool,
    pub follow_symlinks: bool,
    #[serde(default)]
    pub ignore: Vec<String>,
}

impl Default for RootScanSettings {
    fn default() -> Self {
        Self {
            recursive: true,
            follow_symlinks: false,
            ignore: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RootAccessStatus {
    Available,
    Missing,
    NotDirectory,
    PermissionDenied,
    Unavailable,
}

impl fmt::Display for RootAccessStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Available => "available",
            Self::Missing => "missing",
            Self::NotDirectory => "not-directory",
            Self::PermissionDenied => "permission-denied",
            Self::Unavailable => "unavailable",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRootStatus {
    #[serde(flatten)]
    pub root: LibraryRoot,
    pub access_status: RootAccessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddLibraryRoot {
    pub path: PathBuf,
    pub name: String,
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLibraryRoot {
    pub id: Uuid,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub ignore: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootOverlapKind {
    Duplicate,
    InsideExisting,
    ContainsExisting,
}

impl fmt::Display for RootOverlapKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Duplicate => "duplicates",
            Self::InsideExisting => "is inside",
            Self::ContainsExisting => "contains",
        })
    }
}

#[derive(Debug, Error)]
pub enum LibraryRootError {
    #[error("library configuration I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid library configuration YAML at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[error("cannot serialize library configuration: {0}")]
    Serialize(#[from] serde_yaml_ng::Error),
    #[error("invalid library configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid root display name: {0}")]
    InvalidName(String),
    #[error("invalid root ignore rule: {0}")]
    InvalidIgnore(String),
    #[error("root path must be valid UTF-8 and absolute: {0}")]
    InvalidPath(PathBuf),
    #[error("root is not accessible ({status}): {path}")]
    InaccessibleRoot {
        path: PathBuf,
        status: RootAccessStatus,
    },
    #[error("cannot canonicalize root {path}: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("root {path} {kind} configured root {existing_path}")]
    Overlap {
        path: PathBuf,
        kind: RootOverlapKind,
        existing_path: PathBuf,
    },
    #[error("library root was not found: {0}")]
    NotFound(Uuid),
    #[error("failed to persist library configuration at {path}: {source}")]
    Persist {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub struct LibraryRootManager {
    config_path: PathBuf,
    config: LibraryConfig,
}

impl LibraryRootManager {
    /// Opens the readable YAML root configuration, or creates an empty in-memory model.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryRootError`] if an existing configuration cannot be read or validated.
    pub fn open(config_path: PathBuf) -> Result<Self, LibraryRootError> {
        let config = read_config(&config_path)?;
        validate_config(&config)?;
        Ok(Self {
            config_path,
            config,
        })
    }

    #[must_use]
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    #[must_use]
    pub fn roots(&self) -> Vec<LibraryRootStatus> {
        self.config
            .roots
            .iter()
            .cloned()
            .map(|root| {
                let (access_status, access_message) = inspect_root_access(&root.path);
                LibraryRootStatus {
                    root,
                    access_status,
                    access_message,
                }
            })
            .collect()
    }

    /// Adds an accessible, non-overlapping root and atomically persists the configuration.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryRootError`] for invalid input, inaccessible paths, overlap, or I/O failure.
    pub fn add_root(
        &mut self,
        request: AddLibraryRoot,
    ) -> Result<LibraryRootStatus, LibraryRootError> {
        let name = normalize_name(&request.name)?;
        let ignore = normalize_ignore(&request.ignore)?;
        let requested_path = request.path;
        let (access_status, _) = inspect_root_access(&requested_path);
        if access_status != RootAccessStatus::Available {
            return Err(LibraryRootError::InaccessibleRoot {
                path: requested_path,
                status: access_status,
            });
        }
        let path =
            requested_path
                .canonicalize()
                .map_err(|source| LibraryRootError::Canonicalize {
                    path: requested_path.clone(),
                    source,
                })?;
        validate_absolute_utf8_path(&path)?;
        ensure_no_overlap(&path, &self.config.roots)?;

        let root = LibraryRoot {
            id: Uuid::now_v7(),
            path,
            name,
            enabled: true,
            scan: RootScanSettings {
                ignore,
                ..RootScanSettings::default()
            },
            extra: BTreeMap::new(),
        };
        let mut candidate = self.config.clone();
        candidate.roots.push(root.clone());
        write_config_atomic(&self.config_path, &candidate)?;
        self.config = candidate;

        Ok(LibraryRootStatus {
            root,
            access_status: RootAccessStatus::Available,
            access_message: None,
        })
    }

    /// Updates editable root settings without changing the authorized path.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryRootError`] when the ID is missing, input is invalid, or persistence fails.
    pub fn update_root(
        &mut self,
        request: UpdateLibraryRoot,
    ) -> Result<LibraryRootStatus, LibraryRootError> {
        let mut candidate = self.config.clone();
        let root = candidate
            .roots
            .iter_mut()
            .find(|root| root.id == request.id)
            .ok_or(LibraryRootError::NotFound(request.id))?;
        if let Some(name) = request.name {
            root.name = normalize_name(&name)?;
        }
        if let Some(enabled) = request.enabled {
            root.enabled = enabled;
        }
        if let Some(ignore) = request.ignore {
            root.scan.ignore = normalize_ignore(&ignore)?;
        }
        let updated = root.clone();
        write_config_atomic(&self.config_path, &candidate)?;
        self.config = candidate;
        let (access_status, access_message) = inspect_root_access(&updated.path);
        Ok(LibraryRootStatus {
            root: updated,
            access_status,
            access_message,
        })
    }

    /// Removes only the root record from application configuration.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryRootError`] when the ID is missing or persistence fails.
    pub fn remove_root(&mut self, id: Uuid) -> Result<LibraryRoot, LibraryRootError> {
        let mut candidate = self.config.clone();
        let position = candidate
            .roots
            .iter()
            .position(|root| root.id == id)
            .ok_or(LibraryRootError::NotFound(id))?;
        let removed = candidate.roots.remove(position);
        write_config_atomic(&self.config_path, &candidate)?;
        self.config = candidate;
        Ok(removed)
    }
}

#[must_use]
pub fn inspect_root_access(path: &Path) -> (RootAccessStatus, Option<String>) {
    match fs::metadata(path) {
        Ok(metadata) if !metadata.is_dir() => (RootAccessStatus::NotDirectory, None),
        Ok(_) => match fs::read_dir(path) {
            Ok(_) => (RootAccessStatus::Available, None),
            Err(error) => status_for_io_error(&error),
        },
        Err(error) => status_for_io_error(&error),
    }
}

fn status_for_io_error(error: &io::Error) -> (RootAccessStatus, Option<String>) {
    let status = match error.kind() {
        io::ErrorKind::NotFound => RootAccessStatus::Missing,
        io::ErrorKind::PermissionDenied => RootAccessStatus::PermissionDenied,
        _ => RootAccessStatus::Unavailable,
    };
    (status, Some(error.to_string()))
}

fn read_config(path: &Path) -> Result<LibraryConfig, LibraryRootError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LibraryConfig::default());
        }
        Err(source) => {
            return Err(LibraryRootError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    serde_yaml_ng::from_slice(&bytes).map_err(|source| LibraryRootError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_config(config: &LibraryConfig) -> Result<(), LibraryRootError> {
    if config.schema != LIBRARY_CONFIG_SCHEMA_VERSION {
        return Err(LibraryRootError::InvalidConfig(format!(
            "unsupported schema {}",
            config.schema
        )));
    }
    let mut ids = BTreeSet::new();
    for root in &config.roots {
        if !ids.insert(root.id) {
            return Err(LibraryRootError::InvalidConfig(format!(
                "duplicate root id {}",
                root.id
            )));
        }
        normalize_name(&root.name)?;
        normalize_ignore(&root.scan.ignore)?;
        validate_absolute_utf8_path(&root.path)?;
        if root.scan.follow_symlinks {
            return Err(LibraryRootError::InvalidConfig(format!(
                "root {} enables forbidden symlink traversal",
                root.path.display()
            )));
        }
    }
    for (index, root) in config.roots.iter().enumerate() {
        ensure_no_overlap(&root.path, &config.roots[..index])?;
    }
    Ok(())
}

fn validate_absolute_utf8_path(path: &Path) -> Result<(), LibraryRootError> {
    if path.is_absolute() && path.to_str().is_some() {
        Ok(())
    } else {
        Err(LibraryRootError::InvalidPath(path.to_path_buf()))
    }
}

fn normalize_name(name: &str) -> Result<String, LibraryRootError> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 || length > 128 {
        return Err(LibraryRootError::InvalidName(name.to_owned()));
    }
    Ok(name.to_owned())
}

fn normalize_ignore(ignore: &[String]) -> Result<Vec<String>, LibraryRootError> {
    let mut normalized = BTreeSet::new();
    for rule in ignore {
        let rule = rule.trim();
        if rule.is_empty() {
            return Err(LibraryRootError::InvalidIgnore(
                "rules cannot be empty".into(),
            ));
        }
        normalized.insert(rule.to_owned());
    }
    Ok(normalized.into_iter().collect())
}

fn ensure_no_overlap(path: &Path, existing: &[LibraryRoot]) -> Result<(), LibraryRootError> {
    for root in existing {
        let kind = match path_relation_for_platform(path, &root.path, PlatformFamily::current()) {
            PathRelation::Same => Some(RootOverlapKind::Duplicate),
            PathRelation::Descendant => Some(RootOverlapKind::InsideExisting),
            PathRelation::Ancestor => Some(RootOverlapKind::ContainsExisting),
            PathRelation::Distinct => None,
        };
        if let Some(kind) = kind {
            return Err(LibraryRootError::Overlap {
                path: path.to_path_buf(),
                kind,
                existing_path: root.path.clone(),
            });
        }
    }
    Ok(())
}

fn write_config_atomic(path: &Path, config: &LibraryConfig) -> Result<(), LibraryRootError> {
    validate_config(config)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| LibraryRootError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut serialized = serde_yaml_ng::to_string(config)?;
    if !serialized.ends_with('\n') {
        serialized.push('\n');
    }
    let mut temporary = NamedTempFile::new_in(parent).map_err(|source| LibraryRootError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    temporary
        .write_all(serialized.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| LibraryRootError::Io {
            path: temporary.path().to_path_buf(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| LibraryRootError::Persist {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    sync_parent(parent)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), LibraryRootError> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| LibraryRootError::Io {
            path: parent.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
const fn sync_parent(_parent: &Path) -> Result<(), LibraryRootError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use metadata::digest_file;
    use tempfile::tempdir;

    use super::{
        AddLibraryRoot, LibraryRootError, LibraryRootManager, RootAccessStatus, RootOverlapKind,
        UpdateLibraryRoot, inspect_root_access,
    };

    #[test]
    fn persists_updates_and_removes_without_touching_library_files() {
        let directory = tempdir().expect("tempdir");
        let root = directory.path().join("素材");
        fs::create_dir(&root).expect("create root");
        let asset = root.join("logo.png");
        let sidecar = root.join("logo.png.asset.yml");
        fs::write(&asset, b"original image bytes").expect("write asset");
        fs::write(&sidecar, b"schema: 1\n").expect("write sidecar");
        let asset_digest = digest_file(&asset).expect("asset digest");
        let sidecar_digest = digest_file(&sidecar).expect("sidecar digest");
        let config_path = directory.path().join("config/library-roots.yml");

        let mut manager = LibraryRootManager::open(config_path.clone()).expect("open manager");
        let added = manager
            .add_root(AddLibraryRoot {
                path: root.clone(),
                name: "  Design Assets  ".into(),
                ignore: vec!["**/.git/**".into(), "**/.git/**".into()],
            })
            .expect("add root");
        assert_eq!(added.root.name, "Design Assets");
        assert_eq!(added.root.scan.ignore, ["**/.git/**"]);
        assert!(config_path.is_file());

        let mut reloaded = LibraryRootManager::open(config_path).expect("reload manager");
        let updated = reloaded
            .update_root(UpdateLibraryRoot {
                id: added.root.id,
                name: Some("Archive".into()),
                enabled: Some(false),
                ignore: Some(vec!["temp/**".into()]),
            })
            .expect("update root");
        assert_eq!(updated.root.name, "Archive");
        assert!(!updated.root.enabled);
        assert_eq!(updated.root.scan.ignore, ["temp/**"]);

        reloaded.remove_root(added.root.id).expect("remove root");
        assert!(reloaded.roots().is_empty());
        assert!(
            LibraryRootManager::open(reloaded.config_path().to_path_buf())
                .expect("reload removed root")
                .roots()
                .is_empty()
        );
        assert_eq!(digest_file(&asset).expect("asset digest"), asset_digest);
        assert_eq!(
            digest_file(&sidecar).expect("sidecar digest"),
            sidecar_digest
        );
        assert!(asset.is_file());
        assert!(sidecar.is_file());
    }

    #[test]
    fn p2_platform_rejects_duplicate_and_overlapping_roots() {
        let directory = tempdir().expect("tempdir");
        let parent = directory.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).expect("create roots");
        let mut manager =
            LibraryRootManager::open(directory.path().join("roots.yml")).expect("open manager");
        manager
            .add_root(AddLibraryRoot {
                path: parent.clone(),
                name: "Parent".into(),
                ignore: Vec::new(),
            })
            .expect("add parent");

        let duplicate = manager
            .add_root(AddLibraryRoot {
                path: parent.join("."),
                name: "Duplicate".into(),
                ignore: Vec::new(),
            })
            .expect_err("reject duplicate");
        assert!(matches!(
            duplicate,
            LibraryRootError::Overlap {
                kind: RootOverlapKind::Duplicate,
                ..
            }
        ));

        let nested = manager
            .add_root(AddLibraryRoot {
                path: child,
                name: "Child".into(),
                ignore: Vec::new(),
            })
            .expect_err("reject nested root");
        assert!(matches!(
            nested,
            LibraryRootError::Overlap {
                kind: RootOverlapKind::InsideExisting,
                ..
            }
        ));

        let second_config = directory.path().join("second-roots.yml");
        let mut child_first = LibraryRootManager::open(second_config).expect("open second manager");
        child_first
            .add_root(AddLibraryRoot {
                path: parent.join("child"),
                name: "Child".into(),
                ignore: Vec::new(),
            })
            .expect("add child first");
        let containing = child_first
            .add_root(AddLibraryRoot {
                path: parent,
                name: "Parent".into(),
                ignore: Vec::new(),
            })
            .expect_err("reject containing root");
        assert!(matches!(
            containing,
            LibraryRootError::Overlap {
                kind: RootOverlapKind::ContainsExisting,
                ..
            }
        ));
    }

    #[test]
    fn reports_live_access_status_instead_of_persisting_stale_state() {
        let directory = tempdir().expect("tempdir");
        let root = directory.path().join("removable-drive");
        fs::create_dir(&root).expect("create root");
        let mut manager =
            LibraryRootManager::open(directory.path().join("roots.yml")).expect("open manager");
        manager
            .add_root(AddLibraryRoot {
                path: root.clone(),
                name: "Drive".into(),
                ignore: Vec::new(),
            })
            .expect("add root");

        fs::remove_dir(&root).expect("disconnect root");
        assert_eq!(manager.roots()[0].access_status, RootAccessStatus::Missing);

        let ordinary_file = directory.path().join("not-a-directory");
        fs::write(&ordinary_file, b"file").expect("write file");
        assert_eq!(
            inspect_root_access(&ordinary_file).0,
            RootAccessStatus::NotDirectory
        );

        let missing = manager
            .add_root(AddLibraryRoot {
                path: directory.path().join("does-not-exist"),
                name: "Missing".into(),
                ignore: Vec::new(),
            })
            .expect_err("reject missing root");
        assert!(matches!(
            missing,
            LibraryRootError::InaccessibleRoot {
                status: RootAccessStatus::Missing,
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn identifies_permission_denied_roots() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().expect("tempdir");
        let root = directory.path().join("private");
        fs::create_dir(&root).expect("create root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o000)).expect("remove permissions");
        let status = inspect_root_access(&root).0;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("restore permissions");
        assert_eq!(status, RootAccessStatus::PermissionDenied);
    }
}
