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

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsLanguage {
    En,
    Ru,
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
}

impl Default for SettingsDocument {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION.to_string(),
            legacy_import_complete: true,
            language: None,
            mirror_presets: BTreeMap::new(),
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
    Ok(())
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
        Some("en") => Some(SettingsLanguage::En),
        Some("ru") => Some(SettingsLanguage::Ru),
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
