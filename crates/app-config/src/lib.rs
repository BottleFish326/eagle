use std::collections::{BTreeMap, VecDeque};
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

pub const APPLICATION_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const DIAGNOSTIC_SCHEMA_VERSION: u32 = 1;
const MAX_QUERY_LENGTH: usize = 4_096;
const MAX_TAG_FILTERS: usize = 512;
const MAX_TAG_LENGTH: usize = 256;
const MAX_DIAGNOSTIC_EVENTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationConfig {
    pub schema: u32,
    #[serde(default)]
    pub ui: UiPreferences,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            schema: APPLICATION_CONFIG_SCHEMA_VERSION,
            ui: UiPreferences::default(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPreferences {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub tag_filters: BTreeMap<String, TagFilterPreference>,
    #[serde(default)]
    pub active_vault_id: Option<Uuid>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TagFilterPreference {
    Include,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUiPreferences {
    pub query: String,
    pub tag_filters: BTreeMap<String, TagFilterPreference>,
    pub active_vault_id: Option<Uuid>,
}

#[derive(Debug, Error)]
pub enum ApplicationConfigError {
    #[error("application configuration I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid application configuration YAML at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[error("cannot serialize application configuration: {0}")]
    Serialize(#[from] serde_yaml_ng::Error),
    #[error("invalid application configuration: {0}")]
    Invalid(String),
    #[error("failed to persist application configuration at {path}: {source}")]
    Persist {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub struct ApplicationConfigManager {
    config_path: PathBuf,
    config: ApplicationConfig,
}

impl ApplicationConfigManager {
    /// Opens the readable application preference file or an empty in-memory default.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationConfigError`] when an existing file cannot be read or validated.
    pub fn open(config_path: PathBuf) -> Result<Self, ApplicationConfigError> {
        let config = read_application_config(&config_path)?;
        validate_application_config(&config)?;
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
    pub fn config(&self) -> ApplicationConfig {
        self.config.clone()
    }

    /// Replaces the persisted UI preferences while preserving unknown future fields.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationConfigError`] for invalid input or atomic persistence failure.
    pub fn update_ui(
        &mut self,
        update: UpdateUiPreferences,
    ) -> Result<ApplicationConfig, ApplicationConfigError> {
        validate_ui_update(&update)?;
        let mut candidate = self.config.clone();
        candidate.ui.query = update.query;
        candidate.ui.tag_filters = update.tag_filters;
        candidate.ui.active_vault_id = update.active_vault_id;
        write_yaml_atomic(&self.config_path, &candidate)?;
        self.config = candidate;
        Ok(self.config.clone())
    }
}

fn read_application_config(path: &Path) -> Result<ApplicationConfig, ApplicationConfigError> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            serde_yaml_ng::from_str(&contents).map_err(|source| ApplicationConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(ApplicationConfig::default()),
        Err(source) => Err(ApplicationConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_application_config(config: &ApplicationConfig) -> Result<(), ApplicationConfigError> {
    if config.schema != APPLICATION_CONFIG_SCHEMA_VERSION {
        return Err(ApplicationConfigError::Invalid(format!(
            "unsupported schema {}, expected {APPLICATION_CONFIG_SCHEMA_VERSION}",
            config.schema
        )));
    }
    validate_ui_values(&config.ui.query, &config.ui.tag_filters)
}

fn validate_ui_update(update: &UpdateUiPreferences) -> Result<(), ApplicationConfigError> {
    validate_ui_values(&update.query, &update.tag_filters)
}

fn validate_ui_values(
    query: &str,
    filters: &BTreeMap<String, TagFilterPreference>,
) -> Result<(), ApplicationConfigError> {
    if query.chars().count() > MAX_QUERY_LENGTH
        || query
            .chars()
            .any(|character| character.is_control() && !character.is_whitespace())
        || query.contains(['\r', '\n'])
    {
        return Err(ApplicationConfigError::Invalid(
            "saved query must be one line and at most 4096 characters".into(),
        ));
    }
    if filters.len() > MAX_TAG_FILTERS {
        return Err(ApplicationConfigError::Invalid(format!(
            "saved tag filters cannot exceed {MAX_TAG_FILTERS} entries"
        )));
    }
    for tag in filters.keys() {
        if tag.trim() != tag
            || tag.is_empty()
            || tag.chars().count() > MAX_TAG_LENGTH
            || tag.chars().any(char::is_control)
        {
            return Err(ApplicationConfigError::Invalid(format!(
                "invalid saved tag filter: {tag:?}"
            )));
        }
    }
    Ok(())
}

fn write_yaml_atomic(
    path: &Path,
    config: &ApplicationConfig,
) -> Result<(), ApplicationConfigError> {
    let parent = path
        .parent()
        .ok_or_else(|| ApplicationConfigError::Persist {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "configuration path has no parent",
            ),
        })?;
    fs::create_dir_all(parent).map_err(|source| ApplicationConfigError::Persist {
        path: path.to_path_buf(),
        source,
    })?;
    let contents = serde_yaml_ng::to_string(config)?;
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|source| ApplicationConfigError::Persist {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .write_all(contents.as_bytes())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| ApplicationConfigError::Persist {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| ApplicationConfigError::Persist {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    sync_directory(parent).map_err(|source| ApplicationConfigError::Persist {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub timestamp: String,
    pub level: DiagnosticLevel,
    pub category: String,
    pub code: String,
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticBuild {
    pub version: String,
    pub git_commit: String,
    pub target: String,
    pub profile: String,
    pub rustc: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRuntime {
    pub operating_system: String,
    pub architecture: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticConfigurationSummary {
    pub application_schema: u32,
    pub library_root_count: usize,
    pub enabled_library_root_count: usize,
    pub obsidian_vault_count: usize,
    pub enabled_obsidian_vault_count: usize,
    pub saved_query_present: bool,
    pub saved_tag_filter_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCacheSummary {
    pub layout_version: u32,
    pub startup_disposition: String,
    pub file_count: u64,
    pub byte_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCatalogSummary {
    pub asset_count: usize,
    pub active_scan_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticAccessSummary {
    pub name: String,
    pub enabled: bool,
    pub access_status: String,
    pub path_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSnapshot {
    pub schema: u32,
    pub generated_at: String,
    pub build: DiagnosticBuild,
    pub runtime: DiagnosticRuntime,
    pub configuration: DiagnosticConfigurationSummary,
    pub cache: DiagnosticCacheSummary,
    pub catalog: DiagnosticCatalogSummary,
    pub library_roots: Vec<DiagnosticAccessSummary>,
    pub obsidian_vaults: Vec<DiagnosticAccessSummary>,
    pub recent_events: Vec<DiagnosticEvent>,
}

impl Default for DiagnosticSnapshot {
    fn default() -> Self {
        Self {
            schema: DIAGNOSTIC_SCHEMA_VERSION,
            generated_at: String::new(),
            build: DiagnosticBuild::default(),
            runtime: DiagnosticRuntime::default(),
            configuration: DiagnosticConfigurationSummary::default(),
            cache: DiagnosticCacheSummary::default(),
            catalog: DiagnosticCatalogSummary::default(),
            library_roots: Vec::new(),
            obsidian_vaults: Vec::new(),
            recent_events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticExportReport {
    pub path: PathBuf,
    pub generated_at: String,
    pub event_count: usize,
    pub size_bytes: u64,
}

#[derive(Debug, Error)]
pub enum DiagnosticError {
    #[error("diagnostic event buffer lock is poisoned")]
    PoisonedLock,
    #[error("diagnostic export I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot serialize diagnostic export: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub struct DiagnosticService {
    export_directory: PathBuf,
    events: Mutex<VecDeque<DiagnosticEvent>>,
}

impl DiagnosticService {
    #[must_use]
    pub fn new(export_directory: PathBuf) -> Self {
        Self {
            export_directory,
            events: Mutex::new(VecDeque::with_capacity(MAX_DIAGNOSTIC_EVENTS)),
        }
    }

    /// Adds a path-free structured event to the bounded in-memory support log.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticError`] only if the event buffer lock is poisoned.
    pub fn record(
        &self,
        level: DiagnosticLevel,
        category: impl Into<String>,
        code: impl Into<String>,
        details: BTreeMap<String, String>,
    ) -> Result<(), DiagnosticError> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| DiagnosticError::PoisonedLock)?;
        if events.len() == MAX_DIAGNOSTIC_EVENTS {
            events.pop_front();
        }
        events.push_back(DiagnosticEvent {
            timestamp: now(),
            level,
            category: category.into(),
            code: code.into(),
            details,
        });
        Ok(())
    }

    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.lock().map_or(0, |events| events.len())
    }

    /// Writes a redacted JSON support snapshot with a bounded recent-event log.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticError`] for lock, serialization, or atomic persistence failures.
    pub fn export(
        &self,
        mut snapshot: DiagnosticSnapshot,
    ) -> Result<DiagnosticExportReport, DiagnosticError> {
        snapshot.schema = DIAGNOSTIC_SCHEMA_VERSION;
        snapshot.generated_at = now();
        snapshot.recent_events = self
            .events
            .lock()
            .map_err(|_| DiagnosticError::PoisonedLock)?
            .iter()
            .cloned()
            .collect();
        let bytes = serde_json::to_vec_pretty(&snapshot)?;
        fs::create_dir_all(&self.export_directory).map_err(|source| DiagnosticError::Io {
            path: self.export_directory.clone(),
            source,
        })?;
        let file_name = format!(
            "material-eagle-diagnostic-{}-{}.json",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            &Uuid::now_v7().to_string()[..8]
        );
        let path = self.export_directory.join(file_name);
        let mut temporary = NamedTempFile::new_in(&self.export_directory).map_err(|source| {
            DiagnosticError::Io {
                path: path.clone(),
                source,
            }
        })?;
        temporary
            .write_all(&bytes)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|source| DiagnosticError::Io {
                path: path.clone(),
                source,
            })?;
        temporary
            .persist_noclobber(&path)
            .map_err(|error| DiagnosticError::Io {
                path: path.clone(),
                source: error.error,
            })?;
        sync_directory(&self.export_directory).map_err(|source| DiagnosticError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(DiagnosticExportReport {
            path,
            generated_at: snapshot.generated_at,
            event_count: snapshot.recent_events.len(),
            size_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        })
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
const fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{
        ApplicationConfigError, ApplicationConfigManager, DiagnosticAccessSummary, DiagnosticLevel,
        DiagnosticService, DiagnosticSnapshot, TagFilterPreference, UpdateUiPreferences,
    };

    #[test]
    fn missing_application_config_opens_with_readable_defaults() {
        let temp = tempdir().expect("tempdir");
        let manager =
            ApplicationConfigManager::open(temp.path().join("application.yml")).expect("open");
        let config = manager.config();
        assert_eq!(config.schema, 1);
        assert_eq!(config.ui.query, "");
        assert!(config.ui.tag_filters.is_empty());
        assert!(config.ui.active_vault_id.is_none());
    }

    #[test]
    fn atomically_persists_and_reloads_ui_preferences() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config/application.yml");
        let vault_id = Uuid::now_v7();
        let mut manager = ApplicationConfigManager::open(path.clone()).expect("open");
        let config = manager
            .update_ui(UpdateUiPreferences {
                query: "type:image favorite:true".into(),
                tag_filters: BTreeMap::from([
                    ("color/blue".into(), TagFilterPreference::Include),
                    ("state/draft".into(), TagFilterPreference::Exclude),
                ]),
                active_vault_id: Some(vault_id),
            })
            .expect("update");
        assert_eq!(config.ui.active_vault_id, Some(vault_id));

        let reopened = ApplicationConfigManager::open(path)
            .expect("reopen")
            .config();
        assert_eq!(reopened.ui, config.ui);
    }

    #[test]
    fn preserves_unknown_top_level_and_ui_fields() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("application.yml");
        fs::write(
            &path,
            "schema: 1\nui:\n  query: old\n  tagFilters: {}\n  activeVaultId: null\n  futureUi: keep\nfutureTop: keep\n",
        )
        .expect("fixture");
        let mut manager = ApplicationConfigManager::open(path.clone()).expect("open");
        manager
            .update_ui(UpdateUiPreferences {
                query: "new".into(),
                tag_filters: BTreeMap::new(),
                active_vault_id: None,
            })
            .expect("update");
        let contents = fs::read_to_string(path).expect("read");
        assert!(contents.contains("futureUi: keep"));
        assert!(contents.contains("futureTop: keep"));
    }

    #[test]
    fn rejects_invalid_or_unbounded_preferences() {
        let temp = tempdir().expect("tempdir");
        let mut manager =
            ApplicationConfigManager::open(temp.path().join("application.yml")).expect("open");
        let error = manager
            .update_ui(UpdateUiPreferences {
                query: "line one\nline two".into(),
                tag_filters: BTreeMap::new(),
                active_vault_id: None,
            })
            .expect_err("newline");
        assert!(matches!(error, ApplicationConfigError::Invalid(_)));
    }

    #[test]
    fn persistence_failure_does_not_commit_the_in_memory_candidate() {
        let temp = tempdir().expect("tempdir");
        let blocked_parent = temp.path().join("not-a-directory");
        fs::create_dir(&blocked_parent).expect("initial parent");
        let mut manager =
            ApplicationConfigManager::open(blocked_parent.join("application.yml")).expect("open");
        fs::remove_dir(&blocked_parent).expect("remove initial parent");
        fs::write(&blocked_parent, b"file").expect("blocker");
        assert!(
            manager
                .update_ui(UpdateUiPreferences {
                    query: "favorite:true".into(),
                    tag_filters: BTreeMap::new(),
                    active_vault_id: None,
                })
                .is_err()
        );
        assert_eq!(manager.config().ui.query, "");
    }

    #[test]
    fn diagnostic_export_is_bounded_redacted_and_atomic() {
        let temp = tempdir().expect("tempdir");
        let service = DiagnosticService::new(temp.path().join("diagnostics"));
        for index in 0..300 {
            service
                .record(
                    DiagnosticLevel::Info,
                    "test",
                    "bounded",
                    BTreeMap::from([("sequence".into(), index.to_string())]),
                )
                .expect("record");
        }
        let private_path = "/Users/alice/Secret/logo.png";
        let fingerprint = format!("{:x}", Sha256::digest(private_path.as_bytes()));
        let snapshot = DiagnosticSnapshot {
            library_roots: vec![DiagnosticAccessSummary {
                name: "Design".into(),
                enabled: true,
                access_status: "available".into(),
                path_fingerprint: fingerprint[..16].into(),
            }],
            ..DiagnosticSnapshot::default()
        };
        let report = service.export(snapshot).expect("export");
        assert_eq!(report.event_count, 256);
        let contents = fs::read_to_string(report.path).expect("read");
        assert!(!contents.contains(private_path));
        assert!(contents.contains(&fingerprint[..16]));
        assert!(contents.contains("\"schema\": 1"));
    }
}
