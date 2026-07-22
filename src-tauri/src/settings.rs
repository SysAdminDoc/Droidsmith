//! Versioned, typed application preferences owned by the Rust backend.
//!
//! The renderer never receives the internal store path. Legacy localStorage
//! values cross IPC once, are normalized into a redacted backup, and are then
//! committed atomically to the current schema.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fs_util::{ArtifactKind, StagedArtifact};

pub const SETTINGS_VERSION: &str = "1";
const SETTINGS_FILE: &str = "settings.json";
const LEGACY_BACKUP_FILE: &str = "settings-v0-backup.json";
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;
const MAX_LEGACY_PRESETS: usize = 128;
const MAX_LEGACY_VALUE_BYTES: usize = 64 * 1024;
const MAX_DEVICE_IDENTITY_BYTES: usize = 512;
const MAX_QUARANTINED_FILES: usize = 5;
const MAX_LOGCAT_QUERIES: usize = 64;
const MAX_LOGCAT_FILTER_BYTES: usize = 256;
const MAX_LOGCAT_NAME_BYTES: usize = 80;
const MAX_LOGCAT_ID_BYTES: usize = 64;
const MAX_LOGCAT_AGE_SECONDS: u32 = 30 * 24 * 60 * 60;
const LOGCAT_GLOBAL_KEY: &str = "global";
const MAX_WIRELESS_HISTORY: usize = 32;
const MAX_WIRELESS_HOST_BYTES: usize = 255;
const MAX_WIRELESS_LABEL_BYTES: usize = 80;
const MAX_DEVICE_FINGERPRINTS: usize = 128;
const MAX_FINGERPRINT_BYTES: usize = 4_096;

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsLanguage {
    De,
    En,
    Es,
    Ru,
    Zh,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsScope {
    All,
    Language,
    MirrorPresets,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsRecovery {
    Clean,
    LegacyImported,
    CorruptQuarantined,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MirrorPreset {
    pub max_size: String,
    pub bit_rate: String,
    pub no_audio: bool,
    pub recording: bool,
    pub keyboard_mode: KeyboardMode,
    pub video_codec: VideoCodec,
    pub video_encoder: String,
    pub turn_screen_off: bool,
    pub stay_awake: bool,
    pub show_touches: bool,
    #[serde(default)]
    pub flex_display: bool,
    #[serde(default)]
    pub keep_active: bool,
    #[serde(default)]
    pub max_fps: String,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub always_on_top: bool,
    #[serde(default)]
    pub no_control: bool,
    #[serde(default)]
    pub crop: String,
    #[serde(default)]
    pub display_orientation: String,
    #[serde(default)]
    pub screen_off_timeout: String,
    #[serde(default)]
    pub audio_codec: AudioCodec,
    #[serde(default)]
    pub new_display: String,
    #[serde(default)]
    pub audio_source: String,
    #[serde(default = "default_video_source")]
    pub video_source: String,
    #[serde(default = "default_camera_facing")]
    pub camera_facing: String,
    #[serde(default)]
    pub camera_size: String,
    #[serde(default)]
    pub display_ime_policy: String,
    #[serde(default)]
    pub no_vd_destroy_content: bool,
}

fn default_video_source() -> String {
    "display".to_string()
}

fn default_camera_facing() -> String {
    "back".to_string()
}

#[derive(specta::Type, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioCodec {
    #[default]
    Default,
    Opus,
    Aac,
    Flac,
    Raw,
}

impl Default for MirrorPreset {
    fn default() -> Self {
        Self {
            max_size: "1024".to_string(),
            bit_rate: "8M".to_string(),
            no_audio: false,
            recording: false,
            keyboard_mode: KeyboardMode::Default,
            video_codec: VideoCodec::H264,
            video_encoder: String::new(),
            turn_screen_off: false,
            stay_awake: false,
            show_touches: false,
            flex_display: false,
            keep_active: false,
            max_fps: String::new(),
            fullscreen: false,
            always_on_top: false,
            no_control: false,
            crop: String::new(),
            display_orientation: String::new(),
            screen_off_timeout: String::new(),
            audio_codec: AudioCodec::Default,
            new_display: String::new(),
            audio_source: "output".to_string(),
            video_source: default_video_source(),
            camera_facing: default_camera_facing(),
            camera_size: String::new(),
            display_ime_policy: String::new(),
            no_vd_destroy_content: false,
        }
    }
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyboardMode {
    Default,
    Sdk,
    Uhid,
    Aoa,
    Disabled,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
    Vp8,
    Vp9,
}

/// Minimum severity a Logcat query preset matches, using adb's single-letter
/// level codes so the renderer can reuse its existing `V D I W E F` ladder.
#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogcatLevel {
    #[serde(rename = "V")]
    Verbose,
    #[serde(rename = "D")]
    Debug,
    #[serde(rename = "I")]
    Info,
    #[serde(rename = "W")]
    Warn,
    #[serde(rename = "E")]
    Error,
    #[serde(rename = "F")]
    Fatal,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogcatQueryScope {
    Global,
    Device,
}

/// A reusable, versioned Logcat filter. Only the query definition is persisted;
/// captured log lines never enter the store.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogcatQuery {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tag_filter: String,
    #[serde(default)]
    pub message_filter: String,
    #[serde(default)]
    pub pid_filter: String,
    #[serde(default)]
    pub package_filter: String,
    #[serde(default)]
    pub process_filter: String,
    pub min_level: LogcatLevel,
    #[serde(default)]
    pub max_age_seconds: Option<u32>,
    #[serde(default)]
    pub use_regex: bool,
    #[serde(default)]
    pub negate_tag: bool,
    #[serde(default)]
    pub negate_message: bool,
    #[serde(default)]
    pub negate_pid: bool,
    #[serde(default)]
    pub negate_package: bool,
    #[serde(default)]
    pub negate_process: bool,
}

/// A previously connected wireless ADB endpoint. Only the host/port (and an
/// optional user label) are persisted so the renderer can offer one-click
/// reconnect after a device reboots or its Wi-Fi IP churns.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WirelessEndpoint {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub label: Option<String>,
    pub last_connected_ms: u64,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WirelessHistorySnapshot {
    pub endpoints: Vec<WirelessEndpoint>,
    pub auto_reconnect: bool,
}

/// Result of observing a device's current build fingerprint against the last
/// one Droidsmith recorded for it. `changed` is true only when a *different*
/// fingerprint was previously stored — i.e. the device was updated (OTA) since
/// it was last seen — so the renderer can prompt a debloat-drift review.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintObservation {
    pub changed: bool,
    pub previous: Option<String>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogcatQueryLibrary {
    pub version: String,
    pub global: Vec<LogcatQuery>,
    pub device: Vec<LogcatQuery>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyMirrorPresetInput {
    pub device_identity: String,
    pub raw_value: String,
}

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacySettingsImport {
    pub language: Option<String>,
    #[serde(default)]
    pub mirror_presets: Vec<LegacyMirrorPresetInput>,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub version: String,
    pub language: Option<SettingsLanguage>,
    pub mirror_preset_count: u32,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsLoadResult {
    pub settings: SettingsSnapshot,
    pub recovery: SettingsRecovery,
    pub legacy_cleanup_allowed: bool,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsExportResult {
    pub path: String,
    pub byte_size: u64,
    pub scope: SettingsScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsDocument {
    version: String,
    legacy_import_complete: bool,
    language: Option<SettingsLanguage>,
    #[serde(default)]
    mirror_presets: BTreeMap<String, MirrorPreset>,
    #[serde(default)]
    logcat_queries: BTreeMap<String, Vec<LogcatQuery>>,
    #[serde(default)]
    wireless_history: Vec<WirelessEndpoint>,
    #[serde(default)]
    wireless_auto_reconnect: bool,
    #[serde(default)]
    device_fingerprints: BTreeMap<String, String>,
}

impl Default for SettingsDocument {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION.to_string(),
            legacy_import_complete: true,
            language: None,
            mirror_presets: BTreeMap::new(),
            logcat_queries: BTreeMap::new(),
            wireless_history: Vec::new(),
            wireless_auto_reconnect: false,
            device_fingerprints: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyBackup {
    version: &'static str,
    language: Option<SettingsLanguage>,
    mirror_presets: BTreeMap<String, MirrorPreset>,
    corrupt_entry_count: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsExportDocument<'a> {
    version: &'a str,
    scope: SettingsScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<SettingsLanguage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mirror_presets: Option<&'a BTreeMap<String, MirrorPreset>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyMirrorPreset {
    max_size: Option<String>,
    bit_rate: Option<String>,
    no_audio: Option<bool>,
    recording: Option<bool>,
    keyboard_mode: Option<KeyboardMode>,
    video_codec: Option<VideoCodec>,
    video_encoder: Option<String>,
    turn_screen_off: Option<bool>,
    stay_awake: Option<bool>,
    show_touches: Option<bool>,
    /// Removed renderer-authored path. Accepted only so old valid records can
    /// migrate; it is deliberately omitted from both backup and current data.
    #[allow(dead_code)]
    record_path: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings storage failed: {0}")]
    Storage(String),
    #[error("settings data is invalid: {0}")]
    Invalid(String),
}

impl SettingsError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Storage(_) => "settings_storage_failed",
            Self::Invalid(_) => "settings_invalid",
        }
    }
}

pub fn initialize(
    app_data_dir: &Path,
    legacy: LegacySettingsImport,
) -> Result<SettingsLoadResult, SettingsError> {
    with_lock(|| initialize_locked(app_data_dir, &legacy))
}

pub fn set_language(
    app_data_dir: &Path,
    language: SettingsLanguage,
) -> Result<SettingsSnapshot, SettingsError> {
    mutate(app_data_dir, |settings| settings.language = Some(language))
}

pub fn get_mirror_preset(
    app_data_dir: &Path,
    device_identity: &str,
) -> Result<Option<MirrorPreset>, SettingsError> {
    with_lock(|| {
        let loaded = initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let document = read_valid_document(&settings_path(app_data_dir))?;
        debug_assert_eq!(loaded.settings.version, document.version);
        Ok(document
            .mirror_presets
            .get(&device_scope(device_identity)?)
            .cloned())
    })
}

pub fn set_mirror_preset(
    app_data_dir: &Path,
    device_identity: &str,
    preset: MirrorPreset,
) -> Result<SettingsSnapshot, SettingsError> {
    validate_preset(&preset)?;
    let scope = device_scope(device_identity)?;
    mutate(app_data_dir, move |settings| {
        settings.mirror_presets.insert(scope, preset);
    })
}

pub fn reset_mirror_preset(
    app_data_dir: &Path,
    device_identity: &str,
) -> Result<SettingsSnapshot, SettingsError> {
    let scope = device_scope(device_identity)?;
    mutate(app_data_dir, move |settings| {
        settings.mirror_presets.remove(&scope);
    })
}

pub fn list_logcat_queries(
    app_data_dir: &Path,
    device_identity: Option<&str>,
) -> Result<LogcatQueryLibrary, SettingsError> {
    let device_key = device_identity.map(device_scope).transpose()?;
    with_lock(|| {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let document = read_valid_document(&settings_path(app_data_dir))?;
        Ok(logcat_library(&document, device_key.as_deref()))
    })
}

pub fn save_logcat_queries(
    app_data_dir: &Path,
    scope: LogcatQueryScope,
    device_identity: Option<&str>,
    queries: Vec<LogcatQuery>,
) -> Result<LogcatQueryLibrary, SettingsError> {
    let key = match scope {
        LogcatQueryScope::Global => LOGCAT_GLOBAL_KEY.to_string(),
        LogcatQueryScope::Device => {
            let identity = device_identity.ok_or_else(|| {
                SettingsError::Invalid(
                    "device-scoped queries require a device identity".to_string(),
                )
            })?;
            device_scope(identity)?
        }
    };
    if queries.len() > MAX_LOGCAT_QUERIES {
        return Err(SettingsError::Invalid(format!(
            "too many Logcat queries; keep at most {MAX_LOGCAT_QUERIES}"
        )));
    }
    let mut seen = std::collections::BTreeSet::new();
    for query in &queries {
        validate_logcat_query(query)?;
        if !seen.insert(query.id.clone()) {
            return Err(SettingsError::Invalid(format!(
                "duplicate Logcat query id {:?}",
                query.id
            )));
        }
    }
    let device_key = device_identity.map(device_scope).transpose()?;
    with_lock(move || {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let path = settings_path(app_data_dir);
        let mut settings = read_valid_document(&path)?;
        if queries.is_empty() {
            settings.logcat_queries.remove(&key);
        } else {
            settings.logcat_queries.insert(key, queries);
        }
        validate_document(&settings)?;
        write_document(&path, &settings)?;
        Ok(logcat_library(&settings, device_key.as_deref()))
    })
}

pub fn list_wireless_history(
    app_data_dir: &Path,
) -> Result<WirelessHistorySnapshot, SettingsError> {
    with_lock(|| {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let document = read_valid_document(&settings_path(app_data_dir))?;
        Ok(wireless_snapshot(&document))
    })
}

/// Record (or refresh) a successfully connected wireless endpoint. Re-recording
/// an existing host:port preserves any user label and moves it to the front by
/// updating its timestamp; the list is bounded to the most-recent entries.
pub fn record_wireless_endpoint(
    app_data_dir: &Path,
    host: &str,
    port: u16,
    now_ms: u64,
) -> Result<WirelessHistorySnapshot, SettingsError> {
    let host = normalize_wireless_host(host)?;
    with_lock(move || {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let path = settings_path(app_data_dir);
        let mut settings = read_valid_document(&path)?;
        let existing_label = settings
            .wireless_history
            .iter()
            .find(|entry| wireless_matches(entry, &host, port))
            .and_then(|entry| entry.label.clone());
        settings
            .wireless_history
            .retain(|entry| !wireless_matches(entry, &host, port));
        settings.wireless_history.push(WirelessEndpoint {
            host,
            port,
            label: existing_label,
            last_connected_ms: now_ms,
        });
        sort_and_bound_wireless(&mut settings.wireless_history);
        validate_document(&settings)?;
        write_document(&path, &settings)?;
        Ok(wireless_snapshot(&settings))
    })
}

pub fn forget_wireless_endpoint(
    app_data_dir: &Path,
    host: &str,
    port: u16,
) -> Result<WirelessHistorySnapshot, SettingsError> {
    let host = normalize_wireless_host(host)?;
    with_lock(move || {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let path = settings_path(app_data_dir);
        let mut settings = read_valid_document(&path)?;
        settings
            .wireless_history
            .retain(|entry| !wireless_matches(entry, &host, port));
        validate_document(&settings)?;
        write_document(&path, &settings)?;
        Ok(wireless_snapshot(&settings))
    })
}

pub fn set_wireless_auto_reconnect(
    app_data_dir: &Path,
    enabled: bool,
) -> Result<WirelessHistorySnapshot, SettingsError> {
    with_lock(move || {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let path = settings_path(app_data_dir);
        let mut settings = read_valid_document(&path)?;
        settings.wireless_auto_reconnect = enabled;
        validate_document(&settings)?;
        write_document(&path, &settings)?;
        Ok(wireless_snapshot(&settings))
    })
}

/// Record a device's current build fingerprint and report whether it differs
/// from the previously stored one (i.e. the device was updated since last seen).
pub fn record_device_fingerprint(
    app_data_dir: &Path,
    device_identity: &str,
    fingerprint: &str,
) -> Result<FingerprintObservation, SettingsError> {
    let key = device_scope(device_identity)?;
    let fingerprint = fingerprint.trim();
    if fingerprint.is_empty()
        || fingerprint.len() > MAX_FINGERPRINT_BYTES
        || fingerprint.chars().any(char::is_control)
    {
        return Err(SettingsError::Invalid(
            "device fingerprint is empty, oversized, or has control characters".to_string(),
        ));
    }
    let fingerprint = fingerprint.to_string();
    with_lock(move || {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let path = settings_path(app_data_dir);
        let mut settings = read_valid_document(&path)?;
        let previous = settings.device_fingerprints.get(&key).cloned();
        let changed = previous
            .as_deref()
            .is_some_and(|stored| stored != fingerprint);
        if previous.as_deref() != Some(fingerprint.as_str()) {
            // Bound the map before inserting a genuinely new device key.
            if !settings.device_fingerprints.contains_key(&key)
                && settings.device_fingerprints.len() >= MAX_DEVICE_FINGERPRINTS
            {
                if let Some(oldest) = settings.device_fingerprints.keys().next().cloned() {
                    settings.device_fingerprints.remove(&oldest);
                }
            }
            settings
                .device_fingerprints
                .insert(key, fingerprint.clone());
            validate_document(&settings)?;
            write_document(&path, &settings)?;
        }
        Ok(FingerprintObservation { changed, previous })
    })
}

fn wireless_snapshot(document: &SettingsDocument) -> WirelessHistorySnapshot {
    WirelessHistorySnapshot {
        endpoints: document.wireless_history.clone(),
        auto_reconnect: document.wireless_auto_reconnect,
    }
}

fn wireless_matches(entry: &WirelessEndpoint, host: &str, port: u16) -> bool {
    entry.port == port && entry.host.eq_ignore_ascii_case(host)
}

fn sort_and_bound_wireless(history: &mut Vec<WirelessEndpoint>) {
    history.sort_by(|a, b| {
        b.last_connected_ms
            .cmp(&a.last_connected_ms)
            .then_with(|| a.host.cmp(&b.host))
            .then_with(|| a.port.cmp(&b.port))
    });
    history.truncate(MAX_WIRELESS_HISTORY);
}

fn normalize_wireless_host(host: &str) -> Result<String, SettingsError> {
    let trimmed = host.trim();
    if !valid_wireless_host(trimmed) {
        return Err(SettingsError::Invalid(
            "wireless host is empty, oversized, or malformed".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn valid_wireless_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_WIRELESS_HOST_BYTES
        && !host
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        && !host.contains('/')
        && !host.contains('\\')
        && !host.contains("://")
}

fn logcat_library(document: &SettingsDocument, device_key: Option<&str>) -> LogcatQueryLibrary {
    LogcatQueryLibrary {
        version: document.version.clone(),
        global: document
            .logcat_queries
            .get(LOGCAT_GLOBAL_KEY)
            .cloned()
            .unwrap_or_default(),
        device: device_key
            .and_then(|key| document.logcat_queries.get(key))
            .cloned()
            .unwrap_or_default(),
    }
}

pub fn reset(app_data_dir: &Path, scope: SettingsScope) -> Result<SettingsSnapshot, SettingsError> {
    mutate(app_data_dir, move |settings| match scope {
        SettingsScope::All => *settings = SettingsDocument::default(),
        SettingsScope::Language => settings.language = None,
        SettingsScope::MirrorPresets => settings.mirror_presets.clear(),
    })
}

pub fn export(
    app_data_dir: &Path,
    scope: SettingsScope,
    destination: &Path,
) -> Result<SettingsExportResult, SettingsError> {
    with_lock(|| {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let settings = read_valid_document(&settings_path(app_data_dir))?;
        if destination.extension().and_then(|value| value.to_str()) != Some("json") {
            return Err(SettingsError::Invalid(
                "settings exports must use a .json extension".to_string(),
            ));
        }
        if !destination.parent().is_some_and(Path::is_dir) {
            return Err(SettingsError::Invalid(
                "settings export parent directory does not exist".to_string(),
            ));
        }
        if fs::symlink_metadata(destination).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(SettingsError::Invalid(
                "settings export target must not be a symbolic link".to_string(),
            ));
        }
        let document = SettingsExportDocument {
            version: SETTINGS_VERSION,
            scope,
            language: matches!(scope, SettingsScope::All | SettingsScope::Language)
                .then_some(settings.language)
                .flatten(),
            mirror_presets: matches!(scope, SettingsScope::All | SettingsScope::MirrorPresets)
                .then_some(&settings.mirror_presets),
        };
        let bytes = serialize_pretty(&document)?;
        write_atomic(destination, &bytes)?;
        Ok(SettingsExportResult {
            path: crate::fs_util::display_path(destination),
            byte_size: bytes.len() as u64,
            scope,
        })
    })
}

fn mutate(
    app_data_dir: &Path,
    operation: impl FnOnce(&mut SettingsDocument),
) -> Result<SettingsSnapshot, SettingsError> {
    with_lock(|| {
        initialize_locked(app_data_dir, &LegacySettingsImport::default())?;
        let path = settings_path(app_data_dir);
        let mut settings = read_valid_document(&path)?;
        operation(&mut settings);
        validate_document(&settings)?;
        write_document(&path, &settings)?;
        Ok(snapshot(&settings))
    })
}

fn initialize_locked(
    app_data_dir: &Path,
    legacy: &LegacySettingsImport,
) -> Result<SettingsLoadResult, SettingsError> {
    fs::create_dir_all(app_data_dir).map_err(storage)?;
    let path = settings_path(app_data_dir);
    if path.exists() {
        match read_valid_document(&path) {
            Ok(settings) => {
                return Ok(SettingsLoadResult {
                    settings: snapshot(&settings),
                    recovery: SettingsRecovery::Clean,
                    legacy_cleanup_allowed: settings.legacy_import_complete,
                });
            }
            Err(_) => quarantine_corrupt_file(app_data_dir, &path)?,
        }
        let settings = import_legacy(app_data_dir, legacy)?;
        return Ok(SettingsLoadResult {
            settings: snapshot(&settings),
            recovery: SettingsRecovery::CorruptQuarantined,
            legacy_cleanup_allowed: true,
        });
    }

    let had_legacy = has_legacy_candidates(legacy);
    let settings = import_legacy(app_data_dir, legacy)?;
    Ok(SettingsLoadResult {
        settings: snapshot(&settings),
        recovery: if had_legacy {
            SettingsRecovery::LegacyImported
        } else {
            SettingsRecovery::Clean
        },
        legacy_cleanup_allowed: true,
    })
}

fn import_legacy(
    app_data_dir: &Path,
    legacy: &LegacySettingsImport,
) -> Result<SettingsDocument, SettingsError> {
    let language = legacy.language.as_deref().and_then(parse_language);
    let mut mirror_presets = BTreeMap::new();
    let mut corrupt_entry_count = u32::from(legacy.language.is_some() && language.is_none());
    for entry in legacy.mirror_presets.iter().take(MAX_LEGACY_PRESETS) {
        if entry.raw_value.len() > MAX_LEGACY_VALUE_BYTES {
            corrupt_entry_count = corrupt_entry_count.saturating_add(1);
            continue;
        }
        let Ok(scope) = device_scope(&entry.device_identity) else {
            corrupt_entry_count = corrupt_entry_count.saturating_add(1);
            continue;
        };
        let Ok(legacy_preset) = serde_json::from_str::<LegacyMirrorPreset>(&entry.raw_value) else {
            corrupt_entry_count = corrupt_entry_count.saturating_add(1);
            continue;
        };
        let preset = normalize_legacy_preset(legacy_preset);
        if validate_preset(&preset).is_err() {
            corrupt_entry_count = corrupt_entry_count.saturating_add(1);
            continue;
        }
        mirror_presets.insert(scope, preset);
    }
    corrupt_entry_count = corrupt_entry_count.saturating_add(
        legacy
            .mirror_presets
            .len()
            .saturating_sub(MAX_LEGACY_PRESETS) as u32,
    );

    if has_legacy_candidates(legacy) {
        let backup_path = app_data_dir.join(LEGACY_BACKUP_FILE);
        if !backup_path.exists() {
            let backup = LegacyBackup {
                version: "0",
                language,
                mirror_presets: mirror_presets.clone(),
                corrupt_entry_count,
            };
            write_atomic(&backup_path, &serialize_pretty(&backup)?)?;
        }
    }

    let settings = SettingsDocument {
        version: SETTINGS_VERSION.to_string(),
        legacy_import_complete: true,
        language,
        mirror_presets,
        logcat_queries: BTreeMap::new(),
        wireless_history: Vec::new(),
        wireless_auto_reconnect: false,
        device_fingerprints: BTreeMap::new(),
    };
    write_document(&settings_path(app_data_dir), &settings)?;
    Ok(settings)
}

fn read_valid_document(path: &Path) -> Result<SettingsDocument, SettingsError> {
    let text = crate::fs_util::read_to_string_limited(path, MAX_SETTINGS_BYTES).map_err(storage)?;
    let settings: SettingsDocument =
        serde_json::from_str(&text).map_err(|error| SettingsError::Invalid(error.to_string()))?;
    validate_document(&settings)?;
    Ok(settings)
}

fn validate_document(settings: &SettingsDocument) -> Result<(), SettingsError> {
    if settings.version != SETTINGS_VERSION {
        return Err(SettingsError::Invalid(format!(
            "unsupported settings version {:?}; expected {:?}",
            settings.version, SETTINGS_VERSION
        )));
    }
    if !settings.legacy_import_complete {
        return Err(SettingsError::Invalid(
            "legacy import did not complete".to_string(),
        ));
    }
    if settings.mirror_presets.len() > MAX_LEGACY_PRESETS {
        return Err(SettingsError::Invalid(
            "too many mirror presets".to_string(),
        ));
    }
    for (scope, preset) in &settings.mirror_presets {
        if scope.len() != 64 || !scope.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SettingsError::Invalid(
                "mirror preset scope is not a device hash".to_string(),
            ));
        }
        validate_preset(preset)?;
    }
    for (scope, queries) in &settings.logcat_queries {
        if scope != LOGCAT_GLOBAL_KEY
            && (scope.len() != 64 || !scope.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(SettingsError::Invalid(
                "logcat query scope is neither global nor a device hash".to_string(),
            ));
        }
        if queries.len() > MAX_LOGCAT_QUERIES {
            return Err(SettingsError::Invalid(
                "too many Logcat queries in a scope".to_string(),
            ));
        }
        for query in queries {
            validate_logcat_query(query)?;
        }
    }
    if settings.wireless_history.len() > MAX_WIRELESS_HISTORY {
        return Err(SettingsError::Invalid(
            "too many wireless history entries".to_string(),
        ));
    }
    for endpoint in &settings.wireless_history {
        if !valid_wireless_host(&endpoint.host) {
            return Err(SettingsError::Invalid(
                "wireless history host is malformed".to_string(),
            ));
        }
        if endpoint.port == 0 {
            return Err(SettingsError::Invalid(
                "wireless history port is out of range".to_string(),
            ));
        }
        if let Some(label) = &endpoint.label {
            if label.len() > MAX_WIRELESS_LABEL_BYTES || label.chars().any(char::is_control) {
                return Err(SettingsError::Invalid(
                    "wireless history label is oversized or has control characters".to_string(),
                ));
            }
        }
    }
    if settings.device_fingerprints.len() > MAX_DEVICE_FINGERPRINTS {
        return Err(SettingsError::Invalid(
            "too many device fingerprints".to_string(),
        ));
    }
    for (scope, fingerprint) in &settings.device_fingerprints {
        if scope.len() != 64 || !scope.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SettingsError::Invalid(
                "device fingerprint scope is not a device hash".to_string(),
            ));
        }
        if fingerprint.is_empty()
            || fingerprint.len() > MAX_FINGERPRINT_BYTES
            || fingerprint.chars().any(char::is_control)
        {
            return Err(SettingsError::Invalid(
                "device fingerprint is empty, oversized, or has control characters".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_logcat_query(query: &LogcatQuery) -> Result<(), SettingsError> {
    if query.id.is_empty()
        || query.id.len() > MAX_LOGCAT_ID_BYTES
        || !query
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(SettingsError::Invalid(
            "logcat query id must be 1-64 chars of [A-Za-z0-9_-]".to_string(),
        ));
    }
    let name = query.name.trim();
    if name.is_empty()
        || query.name.len() > MAX_LOGCAT_NAME_BYTES
        || query.name.chars().any(char::is_control)
    {
        return Err(SettingsError::Invalid(
            "logcat query name is empty, oversized, or has control characters".to_string(),
        ));
    }
    for (label, value) in [
        ("tagFilter", &query.tag_filter),
        ("messageFilter", &query.message_filter),
        ("packageFilter", &query.package_filter),
        ("processFilter", &query.process_filter),
    ] {
        if value.len() > MAX_LOGCAT_FILTER_BYTES || value.chars().any(char::is_control) {
            return Err(SettingsError::Invalid(format!(
                "logcat query {label} is oversized or has control characters"
            )));
        }
        if query.use_regex && !value.is_empty() {
            validate_linear_regex(label, value)?;
        }
    }
    if !query.pid_filter.is_empty()
        && (query.pid_filter.len() > 7 || !query.pid_filter.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(SettingsError::Invalid(
            "logcat query pidFilter must be up to 7 ASCII digits".to_string(),
        ));
    }
    if query
        .max_age_seconds
        .is_some_and(|seconds| seconds == 0 || seconds > MAX_LOGCAT_AGE_SECONDS)
    {
        return Err(SettingsError::Invalid(
            "logcat query maxAgeSeconds is out of range".to_string(),
        ));
    }
    Ok(())
}

/// Reject regex constructs that a backtracking engine (the renderer's JS
/// `RegExp`) can evaluate in super-linear time. The renderer mirrors this and
/// also compiles the pattern; this backend copy is the durable guard so a
/// hand-edited store can never smuggle a catastrophic pattern back in.
fn validate_linear_regex(label: &str, pattern: &str) -> Result<(), SettingsError> {
    let reject = |reason: &str| {
        Err(SettingsError::Invalid(format!(
            "logcat query {label} rejected: {reason}"
        )))
    };
    let bytes = pattern.as_bytes();
    let mut depth: i32 = 0;
    let mut index = 0;
    let mut prev_was_group_close = false;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'\\' => {
                let Some(&next) = bytes.get(index + 1) else {
                    return reject("dangling escape");
                };
                if next.is_ascii_digit() {
                    return reject("backreferences are not allowed");
                }
                if next == b'k' {
                    return reject("named backreferences are not allowed");
                }
                index += 2;
                prev_was_group_close = false;
                continue;
            }
            b'[' => {
                // Character-class contents (including parens and quantifier
                // characters) are literals; skip to the first unescaped `]`.
                // Mirrors the renderer's `regexError` scanner so both agree.
                index += 1;
                while index < bytes.len() && bytes[index] != b']' {
                    index += if bytes[index] == b'\\' { 2 } else { 1 };
                }
                if index >= bytes.len() {
                    return reject("unterminated character class");
                }
                index += 1; // consume the closing ']'
                prev_was_group_close = false;
                continue;
            }
            b'(' => {
                if bytes.get(index + 1) == Some(&b'?') {
                    match bytes.get(index + 2) {
                        Some(b'=' | b'!') => return reject("lookahead is not allowed"),
                        Some(b'<') if matches!(bytes.get(index + 3), Some(b'=' | b'!')) => {
                            return reject("lookbehind is not allowed")
                        }
                        _ => {}
                    }
                }
                depth += 1;
                prev_was_group_close = false;
            }
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return reject("unbalanced parentheses");
                }
                prev_was_group_close = true;
                index += 1;
                continue;
            }
            b'*' | b'+' | b'{' if prev_was_group_close => {
                return reject("a quantifier applied to a group can backtrack catastrophically");
            }
            _ => {
                prev_was_group_close = false;
            }
        }
        index += 1;
    }
    if depth != 0 {
        return reject("unbalanced parentheses");
    }
    Ok(())
}

fn validate_preset(preset: &MirrorPreset) -> Result<(), SettingsError> {
    if !valid_numeric_setting(&preset.max_size, 5) {
        return Err(SettingsError::Invalid(
            "mirror maxSize must be 1 to 5 ASCII digits".to_string(),
        ));
    }
    if preset.bit_rate.is_empty()
        || preset.bit_rate.len() > 16
        || !preset
            .bit_rate
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'k' | b'K' | b'm' | b'M'))
    {
        return Err(SettingsError::Invalid(
            "mirror bitRate is invalid".to_string(),
        ));
    }
    if preset.video_encoder.len() > 255
        || !preset
            .video_encoder
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(SettingsError::Invalid(
            "mirror videoEncoder is invalid".to_string(),
        ));
    }
    // An empty maxFps means "unset"; a set value is 1-3 ASCII digits (fps caps
    // never exceed a few hundred).
    if !preset.max_fps.is_empty() && !valid_numeric_setting(&preset.max_fps, 3) {
        return Err(SettingsError::Invalid(
            "mirror maxFps must be empty or 1 to 3 ASCII digits".to_string(),
        ));
    }
    if !preset.screen_off_timeout.is_empty()
        && !valid_numeric_setting(&preset.screen_off_timeout, 6)
    {
        return Err(SettingsError::Invalid(
            "mirror screenOffTimeout must be empty or 1 to 6 ASCII digits".to_string(),
        ));
    }
    if !preset.crop.is_empty() && !valid_crop_setting(&preset.crop) {
        return Err(SettingsError::Invalid(
            "mirror crop must be empty or width:height:x:y digits".to_string(),
        ));
    }
    if !preset.display_orientation.is_empty()
        && !matches!(
            preset.display_orientation.as_str(),
            "0" | "90" | "180" | "270" | "flip0" | "flip90" | "flip180" | "flip270"
        )
    {
        return Err(SettingsError::Invalid(
            "mirror displayOrientation is invalid".to_string(),
        ));
    }
    if !preset.new_display.is_empty() && !valid_new_display_setting(&preset.new_display) {
        return Err(SettingsError::Invalid(
            "mirror newDisplay must be empty or <w>x<h>[/<dpi>] or /<dpi>".to_string(),
        ));
    }
    if !matches!(
        preset.audio_source.as_str(),
        "output"
            | "mic"
            | "mic-unprocessed"
            | "mic-voice-communication"
            | "mic-voice-recognition"
            | "voice-call"
            | "playback"
    ) {
        return Err(SettingsError::Invalid(
            "mirror audioSource is invalid".to_string(),
        ));
    }
    if !matches!(preset.video_source.as_str(), "display" | "camera") {
        return Err(SettingsError::Invalid(
            "mirror videoSource is invalid".to_string(),
        ));
    }
    if !matches!(preset.camera_facing.as_str(), "front" | "back" | "external") {
        return Err(SettingsError::Invalid(
            "mirror cameraFacing is invalid".to_string(),
        ));
    }
    if !preset.camera_size.is_empty() {
        match preset.camera_size.split_once('x') {
            Some((w, h)) if valid_numeric_setting(w, 5) && valid_numeric_setting(h, 5) => {}
            _ => {
                return Err(SettingsError::Invalid(
                    "mirror cameraSize must be empty or <width>x<height>".to_string(),
                ))
            }
        }
    }
    if !matches!(
        preset.display_ime_policy.as_str(),
        "" | "local" | "hide" | "fallback"
    ) {
        return Err(SettingsError::Invalid(
            "mirror displayImePolicy must be empty, local, hide, or fallback".to_string(),
        ));
    }
    Ok(())
}

fn valid_new_display_setting(value: &str) -> bool {
    let (size, dpi) = match value.split_once('/') {
        Some((size, dpi)) => (size, Some(dpi)),
        None => (value, None),
    };
    let small_int = |candidate: &str| {
        !candidate.is_empty()
            && candidate.len() <= 5
            && candidate.bytes().all(|byte| byte.is_ascii_digit())
    };
    let size_ok = if size.is_empty() {
        dpi.is_some()
    } else {
        match size.split_once('x') {
            Some((w, h)) => small_int(w) && small_int(h),
            None => false,
        }
    };
    let dpi_ok = match dpi {
        Some(dpi) => small_int(dpi),
        None => true,
    };
    size_ok && dpi_ok
}

fn valid_crop_setting(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 6 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn valid_numeric_setting(value: &str, max_len: usize) -> bool {
    !value.is_empty() && value.len() <= max_len && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn normalize_legacy_preset(value: LegacyMirrorPreset) -> MirrorPreset {
    let defaults = MirrorPreset::default();
    MirrorPreset {
        max_size: value.max_size.unwrap_or(defaults.max_size),
        bit_rate: value.bit_rate.unwrap_or(defaults.bit_rate),
        no_audio: value.no_audio.unwrap_or(defaults.no_audio),
        recording: value.recording.unwrap_or(defaults.recording),
        keyboard_mode: value.keyboard_mode.unwrap_or(defaults.keyboard_mode),
        video_codec: value.video_codec.unwrap_or(defaults.video_codec),
        video_encoder: value.video_encoder.unwrap_or(defaults.video_encoder),
        turn_screen_off: value.turn_screen_off.unwrap_or(defaults.turn_screen_off),
        stay_awake: value.stay_awake.unwrap_or(defaults.stay_awake),
        show_touches: value.show_touches.unwrap_or(defaults.show_touches),
        flex_display: defaults.flex_display,
        keep_active: defaults.keep_active,
        max_fps: defaults.max_fps,
        fullscreen: defaults.fullscreen,
        always_on_top: defaults.always_on_top,
        no_control: defaults.no_control,
        crop: defaults.crop,
        display_orientation: defaults.display_orientation,
        screen_off_timeout: defaults.screen_off_timeout,
        audio_codec: defaults.audio_codec,
        new_display: defaults.new_display,
        audio_source: defaults.audio_source,
        video_source: defaults.video_source,
        camera_facing: defaults.camera_facing,
        camera_size: defaults.camera_size,
        display_ime_policy: defaults.display_ime_policy,
        no_vd_destroy_content: defaults.no_vd_destroy_content,
    }
}

fn device_scope(device_identity: &str) -> Result<String, SettingsError> {
    if device_identity.is_empty()
        || device_identity.len() > MAX_DEVICE_IDENTITY_BYTES
        || device_identity.chars().any(char::is_control)
    {
        return Err(SettingsError::Invalid(
            "device identity is empty, oversized, or contains control characters".to_string(),
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(b"droidsmith-settings-device-v1\0");
    hasher.update(device_identity.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn parse_language(value: &str) -> Option<SettingsLanguage> {
    match value.to_ascii_lowercase().split(['-', '_']).next() {
        Some("de") => Some(SettingsLanguage::De),
        Some("en") => Some(SettingsLanguage::En),
        Some("es") => Some(SettingsLanguage::Es),
        Some("ru") => Some(SettingsLanguage::Ru),
        Some("zh") => Some(SettingsLanguage::Zh),
        _ => None,
    }
}

fn snapshot(settings: &SettingsDocument) -> SettingsSnapshot {
    SettingsSnapshot {
        version: settings.version.clone(),
        language: settings.language,
        mirror_preset_count: settings.mirror_presets.len() as u32,
    }
}

fn has_legacy_candidates(legacy: &LegacySettingsImport) -> bool {
    legacy.language.is_some() || !legacy.mirror_presets.is_empty()
}

fn settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SETTINGS_FILE)
}

fn write_document(path: &Path, settings: &SettingsDocument) -> Result<(), SettingsError> {
    validate_document(settings)?;
    write_atomic(path, &serialize_pretty(settings)?)
}

fn serialize_pretty<T: Serialize>(value: &T) -> Result<Vec<u8>, SettingsError> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| SettingsError::Invalid(error.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SettingsError> {
    let parent = path
        .parent()
        .ok_or_else(|| SettingsError::Invalid("settings path has no parent".to_string()))?;
    fs::create_dir_all(parent).map_err(storage)?;
    let staged = StagedArtifact::new(path).map_err(|error| storage(error.to_string()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(staged.path())
        .map_err(storage)?;
    file.write_all(bytes).map_err(storage)?;
    file.flush().map_err(storage)?;
    drop(file);
    staged
        .commit(ArtifactKind::AnyFile)
        .map_err(|error| storage(error.to_string()))?;
    Ok(())
}

fn quarantine_corrupt_file(app_data_dir: &Path, path: &Path) -> Result<(), SettingsError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut destination = app_data_dir.join(format!("settings-corrupt-{timestamp}.json"));
    let mut suffix = 0_u8;
    while destination.exists() && suffix < 100 {
        suffix += 1;
        destination = app_data_dir.join(format!("settings-corrupt-{timestamp}-{suffix}.json"));
    }
    fs::rename(path, &destination).map_err(storage)?;
    trim_quarantine(app_data_dir);
    Ok(())
}

fn trim_quarantine(app_data_dir: &Path) {
    let Ok(entries) = fs::read_dir(app_data_dir) else {
        return;
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("settings-corrupt-") && name.ends_with(".json")
                })
        })
        .collect::<Vec<_>>();
    paths.sort();
    let remove_count = paths.len().saturating_sub(MAX_QUARANTINED_FILES);
    for path in paths.into_iter().take(remove_count) {
        let _ = fs::remove_file(path);
    }
}

fn with_lock<T>(operation: impl FnOnce() -> Result<T, SettingsError>) -> Result<T, SettingsError> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    operation()
}

fn storage(error: impl std::fmt::Display) -> SettingsError {
    SettingsError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "droidsmith-settings-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn legacy() -> LegacySettingsImport {
        LegacySettingsImport {
            language: Some("ru-RU".to_string()),
            mirror_presets: vec![LegacyMirrorPresetInput {
                device_identity: "SERIAL-123".to_string(),
                raw_value: serde_json::json!({
                    "maxSize": "1920",
                    "bitRate": "12M",
                    "noAudio": true,
                    "recording": true,
                    "keyboardMode": "uhid",
                    "videoCodec": "h265",
                    "videoEncoder": "c2.vendor.encoder",
                    "turnScreenOff": true,
                    "stayAwake": true,
                    "showTouches": false,
                    "recordPath": "C:/legacy/secret.mp4"
                })
                .to_string(),
            }],
        }
    }

    #[test]
    fn imports_legacy_once_after_writing_a_redacted_backup() {
        let dir = temp_dir("legacy");
        let result = initialize(&dir, legacy()).unwrap();
        assert_eq!(result.recovery, SettingsRecovery::LegacyImported);
        assert_eq!(result.settings.language, Some(SettingsLanguage::Ru));
        assert_eq!(result.settings.mirror_preset_count, 1);

        let backup = fs::read_to_string(dir.join(LEGACY_BACKUP_FILE)).unwrap();
        assert!(!backup.contains("SERIAL-123"));
        assert!(!backup.contains("secret.mp4"));
        let first_settings = fs::read(dir.join(SETTINGS_FILE)).unwrap();

        let second = initialize(&dir, LegacySettingsImport::default()).unwrap();
        assert_eq!(second.recovery, SettingsRecovery::Clean);
        assert_eq!(fs::read(dir.join(SETTINGS_FILE)).unwrap(), first_settings);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_settings_are_quarantined_without_blocking_launch() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join(SETTINGS_FILE), b"{broken").unwrap();

        let result = initialize(&dir, LegacySettingsImport::default()).unwrap();
        assert_eq!(result.recovery, SettingsRecovery::CorruptQuarantined);
        assert_eq!(result.settings, snapshot(&SettingsDocument::default()));
        assert!(fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("settings-corrupt-")
        }));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mirror_presets_are_scoped_by_hash_and_reset_independently() {
        let dir = temp_dir("scope");
        initialize(&dir, LegacySettingsImport::default()).unwrap();
        let preset = MirrorPreset {
            bit_rate: "16M".to_string(),
            ..MirrorPreset::default()
        };
        set_mirror_preset(&dir, "device-a", preset.clone()).unwrap();
        assert_eq!(get_mirror_preset(&dir, "device-a").unwrap(), Some(preset));
        assert_eq!(get_mirror_preset(&dir, "device-b").unwrap(), None);

        let raw = fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(!raw.contains("device-a"));
        reset_mirror_preset(&dir, "device-a").unwrap();
        assert_eq!(get_mirror_preset(&dir, "device-a").unwrap(), None);
        fs::remove_dir_all(dir).unwrap();
    }

    fn sample_query(id: &str) -> LogcatQuery {
        LogcatQuery {
            id: id.to_string(),
            name: format!("Query {id}"),
            tag_filter: "ActivityManager".to_string(),
            message_filter: String::new(),
            pid_filter: String::new(),
            min_level: LogcatLevel::Info,
            max_age_seconds: Some(3600),
            package_filter: String::new(),
            process_filter: String::new(),
            use_regex: false,
            negate_tag: false,
            negate_message: false,
            negate_pid: false,
            negate_package: false,
            negate_process: false,
        }
    }

    #[test]
    fn logcat_queries_persist_ordered_and_scoped_by_hash() {
        let dir = temp_dir("logcat");
        initialize(&dir, LegacySettingsImport::default()).unwrap();

        let global = vec![sample_query("crash-watch"), sample_query("net-noise")];
        let library =
            save_logcat_queries(&dir, LogcatQueryScope::Global, None, global.clone()).unwrap();
        assert_eq!(library.global, global);
        assert!(library.device.is_empty());

        let device = vec![sample_query("device-only")];
        save_logcat_queries(
            &dir,
            LogcatQueryScope::Device,
            Some("SERIAL-1"),
            device.clone(),
        )
        .unwrap();
        let listed = list_logcat_queries(&dir, Some("SERIAL-1")).unwrap();
        assert_eq!(listed.global, global);
        assert_eq!(listed.device, device);

        // A different device never sees another device's queries.
        let other = list_logcat_queries(&dir, Some("SERIAL-2")).unwrap();
        assert!(other.device.is_empty());

        // The raw serial is hashed, never stored in the clear.
        let raw = fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(!raw.contains("SERIAL-1"));

        // Saving an empty list clears the scope.
        save_logcat_queries(&dir, LogcatQueryScope::Global, None, Vec::new()).unwrap();
        assert!(list_logcat_queries(&dir, None).unwrap().global.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn logcat_queries_reject_catastrophic_regex_and_duplicate_ids() {
        let dir = temp_dir("logcat-reject");
        initialize(&dir, LegacySettingsImport::default()).unwrap();

        let mut bad = sample_query("redos");
        bad.use_regex = true;
        bad.message_filter = "(a+)+".to_string();
        assert!(save_logcat_queries(&dir, LogcatQueryScope::Global, None, vec![bad]).is_err());

        let mut backref = sample_query("backref");
        backref.use_regex = true;
        backref.message_filter = r"(\w)\1".to_string();
        assert!(save_logcat_queries(&dir, LogcatQueryScope::Global, None, vec![backref]).is_err());

        let dupes = vec![sample_query("same"), sample_query("same")];
        assert!(save_logcat_queries(&dir, LogcatQueryScope::Global, None, dupes).is_err());

        // A safe regex is accepted.
        let mut ok = sample_query("ok");
        ok.use_regex = true;
        ok.message_filter = "FATAL EXCEPTION|ANR in".to_string();
        assert!(save_logcat_queries(&dir, LogcatQueryScope::Global, None, vec![ok]).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn linear_regex_matches_renderer_subset() {
        // Rejected: catastrophic backtracking, backreferences, lookaround.
        assert!(validate_linear_regex("m", "(a+)+").is_err());
        assert!(validate_linear_regex("m", "(ab)+").is_err());
        assert!(validate_linear_regex("m", r"(\w)\1").is_err());
        assert!(validate_linear_regex("m", "(?=foo)").is_err());
        assert!(validate_linear_regex("m", "(unterminated").is_err());
        assert!(validate_linear_regex("m", "[unterminated").is_err());
        // Accepted: literal parens (escaped or in a class) may be quantified;
        // these previously diverged from the renderer's `regexError`.
        assert!(validate_linear_regex("m", r"foo\)*").is_ok());
        assert!(validate_linear_regex("m", r"foo\)+").is_ok());
        assert!(validate_linear_regex("m", r"err\){2}").is_ok());
        assert!(validate_linear_regex("m", "[)]+").is_ok());
        assert!(validate_linear_regex("m", "[(]+").is_ok());
        assert!(validate_linear_regex("m", "[a-z()]+").is_ok());
        assert!(validate_linear_regex("m", "FATAL EXCEPTION|ANR in").is_ok());
    }

    #[test]
    fn wireless_history_dedupes_orders_and_persists_auto_reconnect() {
        let dir = temp_dir("wireless");
        initialize(&dir, LegacySettingsImport::default()).unwrap();

        record_wireless_endpoint(&dir, "192.168.1.10", 5555, 1_000).unwrap();
        record_wireless_endpoint(&dir, "192.168.1.20", 5556, 2_000).unwrap();
        // Re-recording the first endpoint (case-insensitive host) refreshes its
        // timestamp and moves it to the front without creating a duplicate.
        let snapshot = record_wireless_endpoint(&dir, "192.168.1.10", 5555, 3_000).unwrap();
        assert_eq!(snapshot.endpoints.len(), 2);
        assert_eq!(snapshot.endpoints[0].host, "192.168.1.10");
        assert_eq!(snapshot.endpoints[0].last_connected_ms, 3_000);
        assert!(!snapshot.auto_reconnect);

        // The opt-in reconnect flag persists across a fresh load.
        set_wireless_auto_reconnect(&dir, true).unwrap();
        let reloaded = list_wireless_history(&dir).unwrap();
        assert!(reloaded.auto_reconnect);
        assert_eq!(reloaded.endpoints.len(), 2);

        // Forgetting removes only the matching endpoint.
        let after = forget_wireless_endpoint(&dir, "192.168.1.20", 5556).unwrap();
        assert_eq!(after.endpoints.len(), 1);
        assert_eq!(after.endpoints[0].host, "192.168.1.10");

        // A malformed host is rejected rather than stored.
        assert!(record_wireless_endpoint(&dir, "has space", 5555, 4_000).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wireless_history_is_bounded_to_the_most_recent_entries() {
        let dir = temp_dir("wireless-bound");
        initialize(&dir, LegacySettingsImport::default()).unwrap();
        for index in 0..(MAX_WIRELESS_HISTORY as u64 + 8) {
            record_wireless_endpoint(&dir, &format!("10.0.0.{index}"), 5555, index + 1).unwrap();
        }
        let snapshot = list_wireless_history(&dir).unwrap();
        assert_eq!(snapshot.endpoints.len(), MAX_WIRELESS_HISTORY);
        // The newest timestamp survives; the oldest is evicted.
        assert_eq!(
            snapshot.endpoints[0].last_connected_ms,
            MAX_WIRELESS_HISTORY as u64 + 8
        );
        assert!(snapshot
            .endpoints
            .iter()
            .all(|entry| entry.last_connected_ms > 8));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn device_fingerprint_change_is_detected_across_reconnects() {
        let dir = temp_dir("fingerprint");
        initialize(&dir, LegacySettingsImport::default()).unwrap();

        // First observation: no prior fingerprint, so nothing "changed".
        let first = record_device_fingerprint(&dir, "SERIAL-9", "acme/pixel/1:13/A").unwrap();
        assert!(!first.changed);
        assert_eq!(first.previous, None);

        // Same fingerprint again: still not a change.
        let again = record_device_fingerprint(&dir, "SERIAL-9", "acme/pixel/1:13/A").unwrap();
        assert!(!again.changed);
        assert_eq!(again.previous.as_deref(), Some("acme/pixel/1:13/A"));

        // A new fingerprint (OTA) is reported as changed with the prior value.
        let updated = record_device_fingerprint(&dir, "SERIAL-9", "acme/pixel/2:14/B").unwrap();
        assert!(updated.changed);
        assert_eq!(updated.previous.as_deref(), Some("acme/pixel/1:13/A"));

        // The raw serial is never stored in the clear.
        let raw = fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
        assert!(!raw.contains("SERIAL-9"));

        // A different device tracks independently.
        let other = record_device_fingerprint(&dir, "SERIAL-2", "acme/pixel/2:14/B").unwrap();
        assert!(!other.changed);
        assert_eq!(other.previous, None);

        // An empty fingerprint is rejected.
        assert!(record_device_fingerprint(&dir, "SERIAL-9", "  ").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_shipped_language_persists_across_reload() {
        let dir = temp_dir("languages");
        initialize(&dir, LegacySettingsImport::default()).unwrap();

        for language in [
            SettingsLanguage::De,
            SettingsLanguage::En,
            SettingsLanguage::Es,
            SettingsLanguage::Ru,
            SettingsLanguage::Zh,
        ] {
            let saved = set_language(&dir, language).unwrap();
            assert_eq!(saved.language, Some(language));
            let reloaded = initialize(&dir, LegacySettingsImport::default()).unwrap();
            assert_eq!(reloaded.settings.language, Some(language));
        }

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scoped_export_omits_unselected_preferences() {
        let dir = temp_dir("export");
        initialize(&dir, legacy()).unwrap();
        let destination = dir.join("language.json");
        let exported = export(&dir, SettingsScope::Language, &destination).unwrap();
        assert!(exported.byte_size > 0);
        let content = fs::read_to_string(destination).unwrap();
        assert!(content.contains("\"language\": \"ru\""));
        assert!(!content.contains("mirrorPresets"));
        fs::remove_dir_all(dir).unwrap();
    }
}
