use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

pub const VAULT_CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultConfig {
    pub schema: u32,
    #[serde(default)]
    pub vaults: Vec<VaultRoot>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            schema: VAULT_CONFIG_SCHEMA_VERSION,
            vaults: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRoot {
    pub id: Uuid,
    pub path: PathBuf,
    pub name: String,
    pub enabled: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VaultAccessStatus {
    Available,
    Missing,
    NotDirectory,
    PermissionDenied,
    Unavailable,
}

impl fmt::Display for VaultAccessStatus {
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
pub struct VaultRootStatus {
    #[serde(flatten)]
    pub vault: VaultRoot,
    pub access_status: VaultAccessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddVaultRoot {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateVaultRoot {
    pub id: Uuid,
    pub name: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultReference {
    pub vault_id: Uuid,
    pub vault_name: String,
    pub asset_path: PathBuf,
    pub relative_path: String,
    pub url_encoded_path: String,
    pub markdown: String,
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Vault configuration I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid Vault configuration YAML at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[error("cannot serialize Vault configuration: {0}")]
    Serialize(#[from] serde_yaml_ng::Error),
    #[error("invalid Vault configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid Vault display name: {0}")]
    InvalidName(String),
    #[error("Vault path must be valid UTF-8 and absolute: {0}")]
    InvalidPath(PathBuf),
    #[error("Vault is not accessible ({status}): {path}")]
    InaccessibleVault {
        path: PathBuf,
        status: VaultAccessStatus,
    },
    #[error("cannot canonicalize {kind} path {path}: {source}")]
    Canonicalize {
        kind: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Vault path is already configured: {0}")]
    DuplicateVault(PathBuf),
    #[error("Vault was not found: {0}")]
    NotFound(Uuid),
    #[error("Vault is disabled: {0}")]
    Disabled(Uuid),
    #[error("asset is outside Vault {vault}: {asset}")]
    OutsideVault { vault: PathBuf, asset: PathBuf },
    #[error("asset path cannot be represented by a portable Obsidian WikiLink: {path}")]
    UnsafeWikilink { path: String },
    #[error("failed to persist Vault configuration at {path}: {source}")]
    Persist {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub struct VaultManager {
    config_path: PathBuf,
    config: VaultConfig,
}

impl VaultManager {
    /// Opens the readable Vault configuration, or creates an empty in-memory model.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError`] when an existing configuration cannot be read or validated.
    pub fn open(config_path: PathBuf) -> Result<Self, VaultError> {
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
    pub fn vaults(&self) -> Vec<VaultRootStatus> {
        self.config
            .vaults
            .iter()
            .cloned()
            .map(|vault| {
                let (access_status, access_message) = inspect_access(&vault.path);
                VaultRootStatus {
                    vault,
                    access_status,
                    access_message,
                }
            })
            .collect()
    }

    /// Adds an accessible Vault root and atomically persists the configuration.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError`] for invalid input, duplicates, inaccessible paths, or I/O failure.
    pub fn add_vault(&mut self, request: AddVaultRoot) -> Result<VaultRootStatus, VaultError> {
        let name = normalize_name(&request.name)?;
        let requested_path = request.path;
        let (access_status, _) = inspect_access(&requested_path);
        if access_status != VaultAccessStatus::Available {
            return Err(VaultError::InaccessibleVault {
                path: requested_path,
                status: access_status,
            });
        }
        let path = requested_path
            .canonicalize()
            .map_err(|source| VaultError::Canonicalize {
                kind: "Vault",
                path: requested_path.clone(),
                source,
            })?;
        validate_absolute_utf8_path(&path)?;
        if self.config.vaults.iter().any(|vault| vault.path == path) {
            return Err(VaultError::DuplicateVault(path));
        }

        let vault = VaultRoot {
            id: Uuid::now_v7(),
            path,
            name,
            enabled: true,
            extra: BTreeMap::new(),
        };
        let mut candidate = self.config.clone();
        candidate.vaults.push(vault.clone());
        write_config_atomic(&self.config_path, &candidate)?;
        self.config = candidate;

        Ok(VaultRootStatus {
            vault,
            access_status: VaultAccessStatus::Available,
            access_message: None,
        })
    }

    /// Updates the editable label or enabled state without changing the authorized path.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError`] if the Vault is missing or persistence fails.
    pub fn update_vault(
        &mut self,
        request: UpdateVaultRoot,
    ) -> Result<VaultRootStatus, VaultError> {
        let mut candidate = self.config.clone();
        let vault = candidate
            .vaults
            .iter_mut()
            .find(|vault| vault.id == request.id)
            .ok_or(VaultError::NotFound(request.id))?;
        if let Some(name) = request.name {
            vault.name = normalize_name(&name)?;
        }
        if let Some(enabled) = request.enabled {
            vault.enabled = enabled;
        }
        let updated = vault.clone();
        write_config_atomic(&self.config_path, &candidate)?;
        self.config = candidate;
        let (access_status, access_message) = inspect_access(&updated.path);
        Ok(VaultRootStatus {
            vault: updated,
            access_status,
            access_message,
        })
    }

    /// Removes only the configured authorization entry; the Vault and its files are untouched.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError`] if the Vault is missing or persistence fails.
    pub fn remove_vault(&mut self, id: Uuid) -> Result<VaultRoot, VaultError> {
        let mut candidate = self.config.clone();
        let index = candidate
            .vaults
            .iter()
            .position(|vault| vault.id == id)
            .ok_or(VaultError::NotFound(id))?;
        let removed = candidate.vaults.remove(index);
        write_config_atomic(&self.config_path, &candidate)?;
        self.config = candidate;
        Ok(removed)
    }

    /// Resolves an existing asset to a portable, Vault-relative Obsidian embed.
    ///
    /// The returned `markdown` follows Obsidian's native `WikiLink` syntax. The separately
    /// returned `url_encoded_path` follows RFC 3986 byte encoding for consumers that use
    /// Markdown links or Obsidian URIs, where URL encoding is required.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError`] for missing/disabled Vaults, inaccessible paths, path escape,
    /// or characters that Obsidian reserves for `WikiLink` structure.
    pub fn resolve_reference(
        &self,
        vault_id: Uuid,
        asset_path: &Path,
    ) -> Result<VaultReference, VaultError> {
        let vault = self
            .config
            .vaults
            .iter()
            .find(|vault| vault.id == vault_id)
            .ok_or(VaultError::NotFound(vault_id))?;
        if !vault.enabled {
            return Err(VaultError::Disabled(vault_id));
        }
        let (access_status, _) = inspect_access(&vault.path);
        if access_status != VaultAccessStatus::Available {
            return Err(VaultError::InaccessibleVault {
                path: vault.path.clone(),
                status: access_status,
            });
        }
        let canonical_vault = canonicalize("Vault", &vault.path)?;
        let canonical_asset = canonicalize("asset", asset_path)?;
        let relative = canonical_asset
            .strip_prefix(&canonical_vault)
            .map_err(|_| VaultError::OutsideVault {
                vault: canonical_vault.clone(),
                asset: canonical_asset.clone(),
            })?;
        let relative_path = portable_relative_path(relative)?;
        validate_wikilink_path(&relative_path)?;
        let url_encoded_path = url_encode_path(&relative_path);
        Ok(VaultReference {
            vault_id,
            vault_name: vault.name.clone(),
            asset_path: canonical_asset,
            markdown: format!("![[{relative_path}]]"),
            relative_path,
            url_encoded_path,
        })
    }
}

fn read_config(path: &Path) -> Result<VaultConfig, VaultError> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_yaml_ng::from_str(&contents).map_err(|source| VaultError::Parse {
            path: path.to_path_buf(),
            source,
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(VaultConfig::default()),
        Err(source) => Err(VaultError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_config(config: &VaultConfig) -> Result<(), VaultError> {
    if config.schema != VAULT_CONFIG_SCHEMA_VERSION {
        return Err(VaultError::InvalidConfig(format!(
            "unsupported schema {}, expected {VAULT_CONFIG_SCHEMA_VERSION}",
            config.schema
        )));
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for vault in &config.vaults {
        if !ids.insert(vault.id) {
            return Err(VaultError::InvalidConfig(format!(
                "duplicate Vault id {}",
                vault.id
            )));
        }
        validate_absolute_utf8_path(&vault.path)?;
        normalize_name(&vault.name)?;
        if !paths.insert(vault.path.clone()) {
            return Err(VaultError::InvalidConfig(format!(
                "duplicate Vault path {}",
                vault.path.display()
            )));
        }
    }
    Ok(())
}

fn normalize_name(name: &str) -> Result<String, VaultError> {
    let normalized = name.trim();
    if normalized.is_empty() || normalized.chars().count() > 100 {
        return Err(VaultError::InvalidName(name.to_owned()));
    }
    if normalized.chars().any(char::is_control) {
        return Err(VaultError::InvalidName(name.to_owned()));
    }
    Ok(normalized.to_owned())
}

fn validate_absolute_utf8_path(path: &Path) -> Result<(), VaultError> {
    if !path.is_absolute() || path.to_str().is_none() {
        return Err(VaultError::InvalidPath(path.to_path_buf()));
    }
    Ok(())
}

fn inspect_access(path: &Path) -> (VaultAccessStatus, Option<String>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => (VaultAccessStatus::Available, None),
        Ok(_) => (
            VaultAccessStatus::NotDirectory,
            Some("configured Vault path is not a directory".into()),
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (
            VaultAccessStatus::Missing,
            Some("configured Vault path does not exist".into()),
        ),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => (
            VaultAccessStatus::PermissionDenied,
            Some("configured Vault path cannot be read".into()),
        ),
        Err(error) => (VaultAccessStatus::Unavailable, Some(error.to_string())),
    }
}

fn canonicalize(kind: &'static str, path: &Path) -> Result<PathBuf, VaultError> {
    path.canonicalize()
        .map_err(|source| VaultError::Canonicalize {
            kind,
            path: path.to_path_buf(),
            source,
        })
}

fn portable_relative_path(path: &Path) -> Result<String, VaultError> {
    let mut components = Vec::new();
    for component in path.components() {
        let value = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| VaultError::InvalidPath(path.to_path_buf()))?;
        if !value.is_empty() {
            components.push(value);
        }
    }
    if components.is_empty() {
        return Err(VaultError::UnsafeWikilink {
            path: String::new(),
        });
    }
    Ok(components.join("/"))
}

fn validate_wikilink_path(path: &str) -> Result<(), VaultError> {
    if path.chars().any(char::is_control) || path.contains(['#', '|', '^', ':', '%', '[', ']']) {
        return Err(VaultError::UnsafeWikilink {
            path: path.to_owned(),
        });
    }
    Ok(())
}

#[must_use]
pub fn url_encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn write_config_atomic(path: &Path, config: &VaultConfig) -> Result<(), VaultError> {
    let parent = path.parent().ok_or_else(|| VaultError::Persist {
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration path has no parent",
        ),
    })?;
    fs::create_dir_all(parent).map_err(|source| VaultError::Persist {
        path: path.to_path_buf(),
        source,
    })?;
    let contents = serde_yaml_ng::to_string(config)?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|source| VaultError::Persist {
        path: path.to_path_buf(),
        source,
    })?;
    temporary
        .write_all(contents.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| VaultError::Persist {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| VaultError::Persist {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    sync_parent(parent).map_err(|source| VaultError::Persist {
        path: path.to_path_buf(),
        source,
    })
}

fn sync_parent(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use serde_yaml_ng::Value;
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{
        AddVaultRoot, UpdateVaultRoot, VaultError, VaultManager, VaultRoot, url_encode_path,
    };

    #[test]
    fn adds_updates_and_removes_vault_without_touching_its_files() {
        let temp = tempdir().expect("tempdir");
        let vault = temp.path().join("My Vault");
        fs::create_dir(&vault).expect("vault");
        let asset = vault.join("keep.png");
        fs::write(&asset, b"png").expect("asset");
        let config = temp.path().join("config/obsidian-vaults.yml");
        let mut manager = VaultManager::open(config.clone()).expect("open");

        let added = manager
            .add_vault(AddVaultRoot {
                path: vault.clone(),
                name: " Notes ".into(),
            })
            .expect("add");
        assert_eq!(added.vault.name, "Notes");
        assert!(config.exists());

        let updated = manager
            .update_vault(UpdateVaultRoot {
                id: added.vault.id,
                name: Some("Archive".into()),
                enabled: Some(false),
            })
            .expect("update");
        assert_eq!(updated.vault.name, "Archive");
        assert!(!updated.vault.enabled);

        manager.remove_vault(added.vault.id).expect("remove");
        assert!(asset.exists());
        assert!(manager.vaults().is_empty());
        assert!(
            VaultManager::open(config)
                .expect("reopen")
                .vaults()
                .is_empty()
        );
    }

    #[test]
    fn rejects_duplicate_canonical_vault_paths() {
        let temp = tempdir().expect("tempdir");
        let vault = temp.path().join("vault");
        fs::create_dir(&vault).expect("vault");
        let mut manager = VaultManager::open(temp.path().join("vaults.yml")).expect("open");
        manager
            .add_vault(AddVaultRoot {
                path: vault.clone(),
                name: "First".into(),
            })
            .expect("first");
        let error = manager
            .add_vault(AddVaultRoot {
                path: vault.join("..").join("vault"),
                name: "Second".into(),
            })
            .expect_err("duplicate");
        assert!(matches!(error, VaultError::DuplicateVault(_)));
    }

    #[test]
    fn resolves_unicode_spaces_and_url_encoded_path() {
        let temp = tempdir().expect("tempdir");
        let vault = temp.path().join("知识库");
        let folder = vault.join("设计 素材");
        fs::create_dir_all(&folder).expect("folder");
        let asset = folder.join("封面 图.png");
        fs::write(&asset, b"png").expect("asset");
        let mut manager = VaultManager::open(temp.path().join("vaults.yml")).expect("open");
        let configured = manager
            .add_vault(AddVaultRoot {
                path: vault,
                name: "中文 Vault".into(),
            })
            .expect("add");

        let reference = manager
            .resolve_reference(configured.vault.id, &asset)
            .expect("reference");
        assert_eq!(reference.relative_path, "设计 素材/封面 图.png");
        assert_eq!(reference.markdown, "![[设计 素材/封面 图.png]]");
        assert_eq!(
            reference.url_encoded_path,
            "%E8%AE%BE%E8%AE%A1%20%E7%B4%A0%E6%9D%90/%E5%B0%81%E9%9D%A2%20%E5%9B%BE.png"
        );
    }

    #[test]
    fn duplicate_filenames_keep_unambiguous_directories() {
        let temp = tempdir().expect("tempdir");
        let vault = temp.path().join("vault");
        let first = vault.join("brand/logo.png");
        let second = vault.join("product/logo.png");
        fs::create_dir_all(first.parent().expect("first parent")).expect("first folder");
        fs::create_dir_all(second.parent().expect("second parent")).expect("second folder");
        fs::write(&first, b"first").expect("first");
        fs::write(&second, b"second").expect("second");
        let mut manager = VaultManager::open(temp.path().join("vaults.yml")).expect("open");
        let configured = manager
            .add_vault(AddVaultRoot {
                path: vault,
                name: "Vault".into(),
            })
            .expect("add");

        assert_eq!(
            manager
                .resolve_reference(configured.vault.id, &first)
                .expect("first reference")
                .markdown,
            "![[brand/logo.png]]"
        );
        assert_eq!(
            manager
                .resolve_reference(configured.vault.id, &second)
                .expect("second reference")
                .markdown,
            "![[product/logo.png]]"
        );
    }

    #[test]
    fn rejects_assets_outside_the_selected_vault() {
        let temp = tempdir().expect("tempdir");
        let vault = temp.path().join("vault");
        fs::create_dir(&vault).expect("vault");
        let outside = temp.path().join("outside.png");
        fs::write(&outside, b"outside").expect("outside");
        let mut manager = VaultManager::open(temp.path().join("vaults.yml")).expect("open");
        let configured = manager
            .add_vault(AddVaultRoot {
                path: vault,
                name: "Vault".into(),
            })
            .expect("add");

        assert!(matches!(
            manager.resolve_reference(configured.vault.id, &outside),
            Err(VaultError::OutsideVault { .. })
        ));
    }

    #[test]
    fn rejects_wikilink_reserved_characters_instead_of_emitting_a_wrong_link() {
        let temp = tempdir().expect("tempdir");
        let vault = temp.path().join("vault");
        fs::create_dir(&vault).expect("vault");
        let asset = vault.join("cover#draft.png");
        fs::write(&asset, b"png").expect("asset");
        let mut manager = VaultManager::open(temp.path().join("vaults.yml")).expect("open");
        let configured = manager
            .add_vault(AddVaultRoot {
                path: vault,
                name: "Vault".into(),
            })
            .expect("add");

        assert!(matches!(
            manager.resolve_reference(configured.vault.id, &asset),
            Err(VaultError::UnsafeWikilink { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_that_escapes_the_vault() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("tempdir");
        let vault = temp.path().join("vault");
        fs::create_dir(&vault).expect("vault");
        let outside = temp.path().join("secret.png");
        fs::write(&outside, b"secret").expect("outside");
        let linked = vault.join("linked.png");
        symlink(&outside, &linked).expect("symlink");
        let mut manager = VaultManager::open(temp.path().join("vaults.yml")).expect("open");
        let configured = manager
            .add_vault(AddVaultRoot {
                path: vault,
                name: "Vault".into(),
            })
            .expect("add");

        assert!(matches!(
            manager.resolve_reference(configured.vault.id, &linked),
            Err(VaultError::OutsideVault { .. })
        ));
    }

    #[test]
    fn preserves_unknown_configuration_fields() {
        let temp = tempdir().expect("tempdir");
        let vault = temp.path().join("vault");
        fs::create_dir(&vault).expect("vault");
        let config = temp.path().join("vaults.yml");
        let id = Uuid::now_v7();
        let fixture = format!(
            "schema: 1\nvaults:\n  - id: {id}\n    path: {}\n    name: Vault\n    enabled: true\n    futureVault: keep\nfutureTop: keep\n",
            vault.display()
        );
        fs::write(&config, fixture).expect("fixture");
        let mut manager = VaultManager::open(config.clone()).expect("open");
        manager
            .update_vault(UpdateVaultRoot {
                id,
                name: Some("Renamed".into()),
                enabled: None,
            })
            .expect("update");
        let persisted = fs::read_to_string(config).expect("read");
        assert!(persisted.contains("futureTop: keep"));
        assert!(persisted.contains("futureVault: keep"));
    }

    #[test]
    fn validates_loaded_duplicate_ids() {
        let temp = tempdir().expect("tempdir");
        let id = Uuid::now_v7();
        let path = temp.path().join("vaults.yml");
        let vault = VaultRoot {
            id,
            path: PathBuf::from("/vault/one"),
            name: "One".into(),
            enabled: true,
            extra: BTreeMap::<String, Value>::new(),
        };
        let yaml = serde_yaml_ng::to_string(&serde_json::json!({
            "schema": 1,
            "vaults": [vault.clone(), vault]
        }))
        .expect("serialize");
        fs::write(&path, yaml).expect("write");
        assert!(matches!(
            VaultManager::open(path),
            Err(VaultError::InvalidConfig(_))
        ));
    }

    #[test]
    fn encodes_each_utf8_byte_and_preserves_path_separators() {
        assert_eq!(url_encode_path("a b/图.png"), "a%20b/%E5%9B%BE.png");
    }
}
