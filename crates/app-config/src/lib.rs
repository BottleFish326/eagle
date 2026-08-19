use std::collections::{BTreeMap, VecDeque};
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;
use uuid::Uuid;

pub const APPLICATION_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const DIAGNOSTIC_SCHEMA_VERSION: u32 = 2;
const MAX_QUERY_LENGTH: usize = 4_096;
const MAX_TAG_FILTERS: usize = 512;
const MAX_TAG_LENGTH: usize = 256;
const MAX_DIAGNOSTIC_EVENTS: usize = 256;
const MAX_DIAGNOSTIC_DETAILS: usize = 16;
const MAX_DIAGNOSTIC_NAME_LENGTH: usize = 64;
const MAX_DIAGNOSTIC_VALUE_LENGTH: usize = 256;
const RUNTIME_LOG_FILE: &str = "runtime-events.jsonl";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
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
pub struct DiagnosticPerformanceSummary {
    pub active_scans: usize,
    pub active_watches: usize,
    pub scheduler_active: usize,
    pub scheduler_waiting: usize,
    pub scheduler_peak_active: usize,
    pub scheduler_peak_waiting: usize,
    pub cache_entries: u64,
    pub cache_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEventSummary {
    pub level: DiagnosticLevel,
    pub category: String,
    pub code: String,
    pub count: usize,
    pub last_seen: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticLogSummary {
    pub file_count: usize,
    pub byte_count: u64,
    pub max_file_bytes: u64,
    pub retained_files: usize,
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
    pub performance: DiagnosticPerformanceSummary,
    pub runtime_log: DiagnosticLogSummary,
    pub event_summary: Vec<DiagnosticEventSummary>,
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
            performance: DiagnosticPerformanceSummary::default(),
            runtime_log: DiagnosticLogSummary::default(),
            event_summary: Vec::new(),
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
    #[error("diagnostic state lock is poisoned")]
    PoisonedLock,
    #[error("diagnostic export I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot serialize diagnostic export: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("invalid diagnostic log policy")]
    InvalidLogPolicy,
    #[error("unsafe diagnostic log entry at {0}")]
    UnsafeLogEntry(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticLogPolicy {
    pub max_file_bytes: u64,
    pub retained_files: usize,
}

impl Default for DiagnosticLogPolicy {
    fn default() -> Self {
        Self {
            max_file_bytes: 1024 * 1024,
            retained_files: 5,
        }
    }
}

pub struct DiagnosticService {
    export_directory: PathBuf,
    log_policy: DiagnosticLogPolicy,
    events: Mutex<VecDeque<DiagnosticEvent>>,
    runtime_log: Mutex<()>,
}

impl DiagnosticService {
    #[must_use]
    pub fn new(export_directory: PathBuf) -> Self {
        Self {
            export_directory,
            log_policy: DiagnosticLogPolicy::default(),
            events: Mutex::new(VecDeque::with_capacity(MAX_DIAGNOSTIC_EVENTS)),
            runtime_log: Mutex::new(()),
        }
    }

    /// Creates a diagnostic service with explicit per-file and retention limits.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticError::InvalidLogPolicy`] for zero limits.
    pub fn with_log_policy(
        export_directory: PathBuf,
        log_policy: DiagnosticLogPolicy,
    ) -> Result<Self, DiagnosticError> {
        if log_policy.max_file_bytes == 0 || log_policy.retained_files == 0 {
            return Err(DiagnosticError::InvalidLogPolicy);
        }
        Ok(Self {
            export_directory,
            log_policy,
            events: Mutex::new(VecDeque::with_capacity(MAX_DIAGNOSTIC_EVENTS)),
            runtime_log: Mutex::new(()),
        })
    }

    /// Adds a path-free structured event to the bounded in-memory support log.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticError`] if bounded state is unavailable or the rolling
    /// runtime log cannot be written safely.
    pub fn record(
        &self,
        level: DiagnosticLevel,
        category: impl Into<String>,
        code: impl Into<String>,
        details: BTreeMap<String, String>,
    ) -> Result<(), DiagnosticError> {
        let event = DiagnosticEvent {
            timestamp: now(),
            level,
            category: bounded_text(&category.into(), MAX_DIAGNOSTIC_NAME_LENGTH),
            code: bounded_text(&code.into(), MAX_DIAGNOSTIC_NAME_LENGTH),
            details: sanitize_details(details),
        };
        {
            let mut events = self
                .events
                .lock()
                .map_err(|_| DiagnosticError::PoisonedLock)?;
            if events.len() == MAX_DIAGNOSTIC_EVENTS {
                events.pop_front();
            }
            events.push_back(event.clone());
        }
        self.append_runtime_event(&event)
    }

    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.lock().map_or(0, |events| events.len())
    }

    /// Returns the current bounded rolling-log footprint.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing log entry is unsafe or cannot be inspected.
    pub fn runtime_log_summary(&self) -> Result<DiagnosticLogSummary, DiagnosticError> {
        let _guard = self
            .runtime_log
            .lock()
            .map_err(|_| DiagnosticError::PoisonedLock)?;
        let runtime_directory = self.runtime_log_directory();
        ensure_directory_or_missing(&runtime_directory)?;
        if !runtime_directory.exists() {
            return Ok(DiagnosticLogSummary {
                max_file_bytes: self.log_policy.max_file_bytes,
                retained_files: self.log_policy.retained_files,
                ..DiagnosticLogSummary::default()
            });
        }
        let mut file_count = 0;
        let mut byte_count = 0_u64;
        for index in 0..self.log_policy.retained_files {
            let path = self.runtime_log_path(index);
            if let Some(size) = safe_regular_file_size(&path)? {
                file_count += 1;
                byte_count = byte_count.saturating_add(size);
            }
        }
        Ok(DiagnosticLogSummary {
            file_count,
            byte_count,
            max_file_bytes: self.log_policy.max_file_bytes,
            retained_files: self.log_policy.retained_files,
        })
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
        snapshot.event_summary = summarize_events(&snapshot.recent_events);
        snapshot.runtime_log = self.runtime_log_summary()?;
        let bytes = serde_json::to_vec_pretty(&snapshot)?;
        ensure_directory_or_missing(&self.export_directory)?;
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

    fn append_runtime_event(&self, event: &DiagnosticEvent) -> Result<(), DiagnosticError> {
        let _guard = self
            .runtime_log
            .lock()
            .map_err(|_| DiagnosticError::PoisonedLock)?;
        let runtime_directory = self.runtime_log_directory();
        ensure_directory_or_missing(&self.export_directory)?;
        ensure_directory_or_missing(&runtime_directory)?;
        fs::create_dir_all(&runtime_directory).map_err(|source| DiagnosticError::Io {
            path: runtime_directory.clone(),
            source,
        })?;
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        let current = self.runtime_log_path(0);
        let current_size = safe_regular_file_size(&current)?.unwrap_or(0);
        if current_size > 0
            && current_size.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
                > self.log_policy.max_file_bytes
        {
            self.rotate_runtime_logs()?;
        }
        ensure_regular_or_missing(&current)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&current)
            .map_err(|source| DiagnosticError::Io {
                path: current.clone(),
                source,
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.flush())
            .map_err(|source| DiagnosticError::Io {
                path: current.clone(),
                source,
            })?;
        if event.level != DiagnosticLevel::Info {
            file.sync_data().map_err(|source| DiagnosticError::Io {
                path: current.clone(),
                source,
            })?;
            sync_directory(&runtime_directory).map_err(|source| DiagnosticError::Io {
                path: runtime_directory,
                source,
            })?;
        }
        Ok(())
    }

    fn rotate_runtime_logs(&self) -> Result<(), DiagnosticError> {
        let last = self.log_policy.retained_files.saturating_sub(1);
        if last == 0 {
            remove_regular_file_if_exists(&self.runtime_log_path(0))?;
            return Ok(());
        }
        remove_regular_file_if_exists(&self.runtime_log_path(last))?;
        for index in (1..last).rev() {
            rename_regular_file_if_exists(
                &self.runtime_log_path(index),
                &self.runtime_log_path(index + 1),
            )?;
        }
        rename_regular_file_if_exists(&self.runtime_log_path(0), &self.runtime_log_path(1))
    }

    fn runtime_log_directory(&self) -> PathBuf {
        self.export_directory.join("runtime")
    }

    fn runtime_log_path(&self, index: usize) -> PathBuf {
        if index == 0 {
            self.runtime_log_directory().join(RUNTIME_LOG_FILE)
        } else {
            self.runtime_log_directory()
                .join(format!("runtime-events.{index}.jsonl"))
        }
    }
}

fn sanitize_details(details: BTreeMap<String, String>) -> BTreeMap<String, String> {
    details
        .into_iter()
        .take(MAX_DIAGNOSTIC_DETAILS)
        .map(|(key, value)| {
            (
                bounded_text(&key, MAX_DIAGNOSTIC_NAME_LENGTH),
                sanitize_detail_value(&value),
            )
        })
        .collect()
}

fn sanitize_detail_value(value: &str) -> String {
    if looks_like_path(value) {
        let digest = Sha256::digest(value.as_bytes());
        let fingerprint = format!("{digest:x}");
        return format!("[redacted-path:{}]", &fingerprint[..16]);
    }
    bounded_text(value, MAX_DIAGNOSTIC_VALUE_LENGTH)
}

fn looks_like_path(value: &str) -> bool {
    Path::new(value).is_absolute()
        || value.starts_with("~/")
        || value.starts_with("~\\")
        || value.starts_with("\\\\")
        || value.as_bytes().get(1) == Some(&b':')
        || value.contains("/Users/")
        || value.contains("\\Users\\")
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    let mut characters = value.chars();
    let bounded = characters.by_ref().take(max_chars).collect::<String>();
    if characters.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

fn summarize_events(events: &[DiagnosticEvent]) -> Vec<DiagnosticEventSummary> {
    let mut summaries = BTreeMap::<(DiagnosticLevel, String, String), (usize, String)>::new();
    for event in events
        .iter()
        .filter(|event| event.level != DiagnosticLevel::Info)
    {
        let entry = summaries
            .entry((event.level, event.category.clone(), event.code.clone()))
            .or_insert((0, event.timestamp.clone()));
        entry.0 += 1;
        entry.1.clone_from(&event.timestamp);
    }
    summaries
        .into_iter()
        .map(
            |((level, category, code), (count, last_seen))| DiagnosticEventSummary {
                level,
                category,
                code,
                count,
                last_seen,
            },
        )
        .collect()
}

fn safe_regular_file_size(path: &Path) -> Result<Option<u64>, DiagnosticError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(DiagnosticError::UnsafeLogEntry(path.to_path_buf()))
        }
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(DiagnosticError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn ensure_regular_or_missing(path: &Path) -> Result<(), DiagnosticError> {
    safe_regular_file_size(path).map(|_| ())
}

fn ensure_directory_or_missing(path: &Path) -> Result<(), DiagnosticError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(DiagnosticError::UnsafeLogEntry(path.to_path_buf()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(DiagnosticError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn remove_regular_file_if_exists(path: &Path) -> Result<(), DiagnosticError> {
    if safe_regular_file_size(path)?.is_none() {
        return Ok(());
    }
    fs::remove_file(path).map_err(|source| DiagnosticError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn rename_regular_file_if_exists(from: &Path, to: &Path) -> Result<(), DiagnosticError> {
    if safe_regular_file_size(from)?.is_none() {
        return Ok(());
    }
    ensure_regular_or_missing(to)?;
    fs::rename(from, to).map_err(|source| DiagnosticError::Io {
        path: from.to_path_buf(),
        source,
    })
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
    use std::sync::Arc;
    use std::thread;

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{
        ApplicationConfigError, ApplicationConfigManager, DiagnosticAccessSummary, DiagnosticError,
        DiagnosticLevel, DiagnosticLogPolicy, DiagnosticService, DiagnosticSnapshot,
        TagFilterPreference, UpdateUiPreferences,
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
        assert!(contents.contains("\"schema\": 2"));
    }

    #[test]
    fn runtime_log_rotates_and_redacts_path_values() {
        let temp = tempdir().expect("tempdir");
        let service = DiagnosticService::with_log_policy(
            temp.path().join("diagnostics"),
            DiagnosticLogPolicy {
                max_file_bytes: 220,
                retained_files: 3,
            },
        )
        .expect("service");
        let private_path = "/Users/alice/Secret/logo.png";
        for index in 0..20 {
            service
                .record(
                    DiagnosticLevel::Warning,
                    "scanner",
                    "read-failed",
                    BTreeMap::from([
                        ("sequence".into(), index.to_string()),
                        ("path".into(), private_path.into()),
                    ]),
                )
                .expect("record");
        }

        let summary = service.runtime_log_summary().expect("summary");
        assert_eq!(summary.file_count, 3);
        assert!(summary.byte_count <= summary.max_file_bytes * 3 + 512);
        for entry in fs::read_dir(temp.path().join("diagnostics/runtime")).expect("read logs") {
            let contents = fs::read_to_string(entry.expect("entry").path()).expect("read log");
            assert!(!contents.contains(private_path));
            for line in contents.lines() {
                serde_json::from_str::<serde_json::Value>(line).expect("JSON line");
            }
        }
        let export = service
            .export(DiagnosticSnapshot::default())
            .expect("export");
        let contents = fs::read_to_string(export.path).expect("read export");
        assert!(!contents.contains(private_path));
        assert!(contents.contains("eventSummary"));
    }

    #[test]
    fn concurrent_runtime_events_remain_complete_json_lines() {
        let temp = tempdir().expect("tempdir");
        let service = Arc::new(DiagnosticService::new(temp.path().join("diagnostics")));
        let threads = (0..8)
            .map(|worker| {
                let service = Arc::clone(&service);
                thread::spawn(move || {
                    for sequence in 0..20 {
                        service
                            .record(
                                DiagnosticLevel::Info,
                                "concurrency",
                                "event",
                                BTreeMap::from([
                                    ("worker".into(), worker.to_string()),
                                    ("sequence".into(), sequence.to_string()),
                                ]),
                            )
                            .expect("record");
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in threads {
            worker.join().expect("join");
        }

        let contents =
            fs::read_to_string(temp.path().join("diagnostics/runtime/runtime-events.jsonl"))
                .expect("read log");
        assert_eq!(contents.lines().count(), 160);
        for line in contents.lines() {
            serde_json::from_str::<serde_json::Value>(line).expect("JSON line");
        }
        assert_eq!(service.event_count(), 160);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_log_rejects_symlink_targets_without_modifying_them() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("tempdir");
        let diagnostics = temp.path().join("diagnostics");
        let runtime = diagnostics.join("runtime");
        fs::create_dir_all(&runtime).expect("runtime directory");
        let outside = temp.path().join("outside.jsonl");
        fs::write(&outside, b"owned outside").expect("outside");
        symlink(&outside, runtime.join("runtime-events.jsonl")).expect("symlink");
        let service = DiagnosticService::new(diagnostics);

        assert!(matches!(
            service.record(
                DiagnosticLevel::Error,
                "security",
                "unsafe-log",
                BTreeMap::new(),
            ),
            Err(DiagnosticError::UnsafeLogEntry(_))
        ));
        assert_eq!(
            fs::read(&outside).expect("outside unchanged"),
            b"owned outside"
        );
        assert_eq!(service.event_count(), 1);
    }
}
