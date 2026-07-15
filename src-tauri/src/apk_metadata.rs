//! Lazy, bounded APK metadata enrichment for package rows.
//!
//! Package enumeration remains cheap. The renderer requests metadata only for
//! rows near the viewport; this module then limits concurrent pulls, validates
//! the live APK identity, and retains a bounded process-local cache.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Take};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

use base64::Engine;
use serde::Serialize;

use crate::adb::actions;
use crate::adb::device::DeviceTarget;
use crate::adb::packages::valid_package_name;
use crate::adb::transport::{AdbTransport, ShellTransport, TransportError};

const MAX_CONCURRENT_PULLS: usize = 3;
const MAX_CACHE_ENTRIES: usize = 256;
const MAX_APK_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RESOURCES_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ICON_BYTES: u64 = 512 * 1024;
const MAX_LABEL_CHARS: usize = 256;
const MAX_ZIP_ENTRIES: usize = 32_768;

const RES_STRING_POOL_TYPE: u16 = 0x0001;
const RES_TABLE_TYPE: u16 = 0x0002;
const RES_XML_RESOURCE_MAP_TYPE: u16 = 0x0180;
const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
const RES_TABLE_PACKAGE_TYPE: u16 = 0x0200;
const RES_TABLE_TYPE_TYPE: u16 = 0x0201;
const UTF8_FLAG: u32 = 1 << 8;
const NO_ENTRY: u32 = 0xffff_ffff;
const ENTRY_FLAG_COMPLEX: u16 = 0x0001;
const TYPE_REFERENCE: u8 = 0x01;
const TYPE_STRING: u8 = 0x03;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AppPackageMetadata {
    pub package: String,
    pub label: Option<String>,
    pub icon_data_uri: Option<String>,
    pub cache_hit: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("invalid package id")]
    InvalidPackage,
    #[error("package manager did not report a base APK for this package")]
    MissingApk,
    #[error("APK metadata source exceeds the {MAX_APK_BYTES}-byte safety limit")]
    ApkTooLarge,
    #[error("could not create temporary APK metadata storage: {0}")]
    TempIo(std::io::Error),
    #[error("APK metadata is unsupported or malformed: {0}")]
    Parse(String),
    #[error("APK metadata service state is unavailable")]
    State,
}

impl MetadataError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Transport(_) => "metadata_transport_failed",
            Self::InvalidPackage => "invalid_package",
            Self::MissingApk => "metadata_apk_missing",
            Self::ApkTooLarge => "metadata_apk_too_large",
            Self::TempIo(_) => "metadata_temp_failed",
            Self::Parse(_) => "metadata_parse_failed",
            Self::State => "metadata_state_unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    serial: String,
    transport_id: Option<u32>,
    connection_generation: u64,
    build_fingerprint: String,
    user_id: u32,
    package: String,
    apk_path: String,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    identity: ApkIdentity,
    metadata: AppPackageMetadata,
    sequence: u64,
}

#[derive(Debug, Default)]
struct MetadataCache {
    entries: HashMap<CacheKey, CacheEntry>,
    sequence: u64,
}

impl MetadataCache {
    fn get(&mut self, key: &CacheKey, identity: &ApkIdentity) -> Option<AppPackageMetadata> {
        let entry = self.entries.get_mut(key)?;
        if &entry.identity != identity {
            self.entries.remove(key);
            return None;
        }
        self.sequence = self.sequence.wrapping_add(1);
        entry.sequence = self.sequence;
        let mut metadata = entry.metadata.clone();
        metadata.cache_hit = true;
        Some(metadata)
    }

    fn insert(&mut self, key: CacheKey, identity: ApkIdentity, metadata: AppPackageMetadata) {
        self.entries.retain(|known, _| {
            known.serial != key.serial
                || known.transport_id != key.transport_id
                || known.user_id != key.user_id
                || known.package != key.package
                || known == &key
        });
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.insert(
            key,
            CacheEntry {
                identity,
                metadata,
                sequence: self.sequence,
            },
        );
        while self.entries.len() > MAX_CACHE_ENTRIES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.sequence)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }
}

fn metadata_cache() -> &'static Mutex<MetadataCache> {
    static CACHE: OnceLock<Mutex<MetadataCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(MetadataCache::default()))
}

#[derive(Debug, Default)]
struct PullGateState {
    active: usize,
}

fn pull_gate() -> &'static (Mutex<PullGateState>, Condvar) {
    static GATE: OnceLock<(Mutex<PullGateState>, Condvar)> = OnceLock::new();
    GATE.get_or_init(|| (Mutex::new(PullGateState::default()), Condvar::new()))
}

struct PullPermit;

impl PullPermit {
    fn acquire() -> Result<Self, MetadataError> {
        let (lock, available) = pull_gate();
        let mut state = lock.lock().map_err(|_| MetadataError::State)?;
        while state.active >= MAX_CONCURRENT_PULLS {
            state = available.wait(state).map_err(|_| MetadataError::State)?;
        }
        state.active += 1;
        Ok(Self)
    }
}

impl Drop for PullPermit {
    fn drop(&mut self) {
        let (lock, available) = pull_gate();
        if let Ok(mut state) = lock.lock() {
            state.active = state.active.saturating_sub(1);
            available.notify_one();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApkIdentity {
    size: u64,
    modified: i64,
}

struct TemporaryApk {
    path: PathBuf,
}

impl TemporaryApk {
    fn new() -> Result<Self, MetadataError> {
        let directory = std::env::temp_dir().join("Droidsmith").join("apk-metadata");
        fs::create_dir_all(&directory).map_err(MetadataError::TempIo)?;
        Ok(Self {
            path: directory.join(format!("metadata-{}.apk", uuid::Uuid::new_v4())),
        })
    }
}

impl Drop for TemporaryApk {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Resolve, pull, parse, and cache metadata for one requested package row.
pub fn load_package_metadata(
    transport: &ShellTransport,
    target: &DeviceTarget,
    user_id: u32,
    package: &str,
) -> Result<AppPackageMetadata, MetadataError> {
    if !valid_package_name(package) {
        return Err(MetadataError::InvalidPackage);
    }
    let _permit = PullPermit::acquire()?;
    let user = user_id.to_string();
    let path_output = transport.shell_target(target, &["pm", "path", "--user", &user, package])?;
    let apk_path = parse_base_apk_path(&path_output).ok_or(MetadataError::MissingApk)?;
    let identity_output = transport
        .shell_target(target, &["stat", "-c", "%s:%Y", &apk_path])
        .or_else(|_| {
            transport.shell_target(target, &["toybox", "stat", "-c", "%s:%Y", &apk_path])
        })?;
    let identity = parse_apk_identity(&identity_output).ok_or_else(|| {
        MetadataError::Parse("device did not report a stable APK size and timestamp".into())
    })?;
    if identity.size > MAX_APK_BYTES {
        return Err(MetadataError::ApkTooLarge);
    }

    let key = CacheKey {
        serial: target.serial.clone(),
        transport_id: target.transport_id,
        connection_generation: target.connection_generation,
        build_fingerprint: target.build_fingerprint.clone().unwrap_or_default(),
        user_id,
        package: package.to_string(),
        apk_path: apk_path.clone(),
    };
    if let Some(metadata) = metadata_cache()
        .lock()
        .map_err(|_| MetadataError::State)?
        .get(&key, &identity)
    {
        return Ok(metadata);
    }

    let temporary = TemporaryApk::new()?;
    let local_path = temporary.path.to_string_lossy().into_owned();
    actions::extract_apk(&transport.adb_path, target, &apk_path, &local_path)?;
    let pulled_size = fs::metadata(&temporary.path)
        .map_err(MetadataError::TempIo)?
        .len();
    if pulled_size > MAX_APK_BYTES || pulled_size != identity.size {
        return Err(MetadataError::Parse(
            "pulled APK identity changed during metadata extraction".into(),
        ));
    }

    let parsed = parse_apk(&temporary.path)?;
    let metadata = AppPackageMetadata {
        package: package.to_string(),
        label: parsed.label,
        icon_data_uri: parsed.icon_data_uri,
        cache_hit: false,
    };
    metadata_cache()
        .lock()
        .map_err(|_| MetadataError::State)?
        .insert(key, identity, metadata.clone());
    Ok(metadata)
}

fn parse_base_apk_path(stdout: &str) -> Option<String> {
    let paths = stdout
        .lines()
        .filter_map(|line| line.trim().strip_prefix("package:"))
        .filter(|path| path.starts_with('/') && !path.contains(['\r', '\n', '\0']))
        .collect::<Vec<_>>();
    paths
        .iter()
        .find(|path| path.ends_with("/base.apk"))
        .or_else(|| paths.first())
        .map(|path| (*path).to_string())
}

fn parse_apk_identity(stdout: &str) -> Option<ApkIdentity> {
    let (size, modified) = stdout.trim().split_once(':')?;
    let identity = ApkIdentity {
        size: size.parse().ok()?,
        modified: modified.parse().ok()?,
    };
    (identity.size > 0).then_some(identity)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ParsedMetadata {
    label: Option<String>,
    icon_data_uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AttrValue {
    String(String),
    Reference(u32),
}

#[derive(Debug, Default)]
struct ManifestMetadata {
    label: Option<AttrValue>,
    icon: Option<AttrValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourceValue {
    String(String),
    Reference(u32),
}

#[derive(Debug, Default)]
struct ResourceTable {
    values: HashMap<u32, ResourceValue>,
}

impl ResourceTable {
    fn resolve(&self, id: u32) -> Option<String> {
        self.resolve_inner(id, 0)
    }

    fn resolve_inner(&self, id: u32, depth: u8) -> Option<String> {
        if depth > 8 {
            return None;
        }
        match self.values.get(&id)? {
            ResourceValue::String(value) => Some(value.clone()),
            ResourceValue::Reference(next) if *next != id => self.resolve_inner(*next, depth + 1),
            ResourceValue::Reference(_) => None,
        }
    }
}

fn parse_apk(path: &Path) -> Result<ParsedMetadata, MetadataError> {
    let file = File::open(path).map_err(MetadataError::TempIo)?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|error| MetadataError::Parse(error.to_string()))?;
    if zip.len() > MAX_ZIP_ENTRIES {
        return Err(MetadataError::Parse(format!(
            "APK contains more than {MAX_ZIP_ENTRIES} ZIP entries"
        )));
    }
    let manifest = read_zip_entry_limited(&mut zip, "AndroidManifest.xml", MAX_MANIFEST_BYTES)?;
    let resources =
        read_optional_zip_entry_limited(&mut zip, "resources.arsc", MAX_RESOURCES_BYTES)?
            .unwrap_or_default();
    let table = parse_resource_table(&resources).unwrap_or_default();
    let manifest = parse_manifest(&manifest).map_err(MetadataError::Parse)?;

    let label = manifest
        .label
        .as_ref()
        .and_then(|value| resolve_attr_value(value, &table))
        .and_then(clean_label);
    let icon_path = manifest
        .icon
        .as_ref()
        .and_then(|value| resolve_attr_value(value, &table));
    let icon_data_uri = icon_path.and_then(|declared| read_icon_data_uri(&mut zip, &declared));
    Ok(ParsedMetadata {
        label,
        icon_data_uri,
    })
}

fn resolve_attr_value(value: &AttrValue, table: &ResourceTable) -> Option<String> {
    match value {
        AttrValue::String(value) => Some(value.clone()),
        AttrValue::Reference(id) => table.resolve(*id),
    }
}

fn clean_label(label: String) -> Option<String> {
    let cleaned = label
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_LABEL_CHARS + 1)
        .collect::<String>();
    if cleaned.is_empty() || cleaned.starts_with('@') || cleaned.chars().count() > MAX_LABEL_CHARS {
        None
    } else {
        Some(cleaned)
    }
}

fn read_icon_data_uri(zip: &mut zip::ZipArchive<File>, declared_path: &str) -> Option<String> {
    let path = if icon_mime(declared_path).is_some() {
        declared_path.to_string()
    } else {
        find_density_icon(zip, declared_path)?
    };
    let mime = icon_mime(&path)?;
    let bytes = read_zip_entry_limited(zip, &path, MAX_ICON_BYTES).ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}

fn find_density_icon(zip: &zip::ZipArchive<File>, declared_path: &str) -> Option<String> {
    let stem = Path::new(declared_path).file_stem()?.to_str()?;
    let mut candidates = zip
        .file_names()
        .filter(|name| name.starts_with("res/") && icon_mime(name).is_some())
        .filter(|name| {
            Path::new(name)
                .file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value == stem)
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| density_rank(path));
    candidates.pop()
}

fn density_rank(path: &str) -> u8 {
    if path.contains("xxxhdpi") {
        7
    } else if path.contains("xxhdpi") {
        6
    } else if path.contains("xhdpi") {
        5
    } else if path.contains("hdpi") {
        4
    } else if path.contains("mdpi") {
        3
    } else if path.contains("anydpi") {
        2
    } else {
        1
    }
}

fn icon_mime(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some("image/png")
    } else if lower.ends_with(".webp") {
        Some("image/webp")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some("image/jpeg")
    } else {
        None
    }
}

fn read_optional_zip_entry_limited(
    zip: &mut zip::ZipArchive<File>,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, MetadataError> {
    match zip.by_name(name) {
        Ok(entry) => read_zip_file_limited(entry, name, limit).map(Some),
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(MetadataError::Parse(error.to_string())),
    }
}

fn read_zip_entry_limited(
    zip: &mut zip::ZipArchive<File>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, MetadataError> {
    let entry = zip
        .by_name(name)
        .map_err(|error| MetadataError::Parse(error.to_string()))?;
    read_zip_file_limited(entry, name, limit)
}

fn read_zip_file_limited<R: Read>(
    mut entry: zip::read::ZipFile<'_, R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, MetadataError> {
    if entry.size() > limit {
        return Err(MetadataError::Parse(format!(
            "{name} exceeds the {limit}-byte safety limit"
        )));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    let mut limited: Take<&mut zip::read::ZipFile<'_, R>> = entry.by_ref().take(limit + 1);
    limited
        .read_to_end(&mut bytes)
        .map_err(|error| MetadataError::Parse(error.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(MetadataError::Parse(format!(
            "{name} exceeds the {limit}-byte safety limit"
        )));
    }
    Ok(bytes)
}

fn parse_manifest(data: &[u8]) -> Result<ManifestMetadata, String> {
    if data.len() < 16 {
        return Err("manifest too small".into());
    }
    let mut position = le_u16(data, 2)? as usize;
    let strings = parse_string_pool(data, position)?;
    position = position
        .checked_add(le_u32(data, position + 4)? as usize)
        .ok_or_else(|| "manifest string pool overflow".to_string())?;
    let mut metadata = ManifestMetadata::default();
    while position + 8 <= data.len() {
        let chunk_type = le_u16(data, position)?;
        let chunk_size = le_u32(data, position + 4)? as usize;
        if chunk_size < 8 || position.saturating_add(chunk_size) > data.len() {
            return Err("manifest chunk is truncated".into());
        }
        if chunk_type == RES_XML_START_ELEMENT_TYPE {
            parse_start_element(data, position, &strings, &mut metadata)?;
        } else if chunk_type != RES_XML_RESOURCE_MAP_TYPE {
            // Other well-formed XML chunks do not carry application attrs.
        }
        position += chunk_size;
    }
    Ok(metadata)
}

fn parse_start_element(
    data: &[u8],
    chunk_start: usize,
    strings: &[String],
    metadata: &mut ManifestMetadata,
) -> Result<(), String> {
    let name_index = le_u32(data, chunk_start + 20)? as usize;
    if strings.get(name_index).map(String::as_str) != Some("application") {
        return Ok(());
    }
    let attribute_start = le_u16(data, chunk_start + 24)? as usize;
    let attribute_size = le_u16(data, chunk_start + 26)? as usize;
    let attribute_count = le_u16(data, chunk_start + 28)? as usize;
    if attribute_size < 20 || attribute_count > 4096 {
        return Err("manifest attribute table is invalid".into());
    }
    let attributes_start = chunk_start
        .checked_add(16 + attribute_start)
        .ok_or_else(|| "manifest attribute offset overflow".to_string())?;
    for index in 0..attribute_count {
        let attribute = attributes_start
            .checked_add(index.saturating_mul(attribute_size))
            .ok_or_else(|| "manifest attribute offset overflow".to_string())?;
        let name_index = le_u32(data, attribute + 4)? as usize;
        let Some(name) = strings.get(name_index).map(String::as_str) else {
            continue;
        };
        if name != "label" && name != "icon" {
            continue;
        }
        let raw_index = le_u32(data, attribute + 8)?;
        let data_type = *data
            .get(attribute + 15)
            .ok_or_else(|| "manifest attribute is truncated".to_string())?;
        let value_data = le_u32(data, attribute + 16)?;
        let value = if raw_index != NO_ENTRY {
            strings
                .get(raw_index as usize)
                .cloned()
                .map(AttrValue::String)
        } else if data_type == TYPE_STRING {
            strings
                .get(value_data as usize)
                .cloned()
                .map(AttrValue::String)
        } else if data_type == TYPE_REFERENCE {
            Some(AttrValue::Reference(value_data))
        } else {
            None
        };
        match (name, value) {
            ("label", Some(value)) => metadata.label = Some(value),
            ("icon", Some(value)) => metadata.icon = Some(value),
            _ => {}
        }
    }
    Ok(())
}

fn parse_resource_table(data: &[u8]) -> Result<ResourceTable, String> {
    if data.len() < 12 || le_u16(data, 0)? != RES_TABLE_TYPE {
        return Err("resources table header missing".into());
    }
    let mut table = ResourceTable::default();
    let mut position = le_u16(data, 2)? as usize;
    let global_strings = parse_string_pool(data, position)?;
    position = position
        .checked_add(le_u32(data, position + 4)? as usize)
        .ok_or_else(|| "resources string pool overflow".to_string())?;
    while position + 8 <= data.len() {
        let chunk_type = le_u16(data, position)?;
        let chunk_size = le_u32(data, position + 4)? as usize;
        if chunk_size < 8 || position.saturating_add(chunk_size) > data.len() {
            return Err("resources chunk is truncated".into());
        }
        if chunk_type == RES_TABLE_PACKAGE_TYPE {
            parse_package_chunk(data, position, chunk_size, &global_strings, &mut table)?;
        }
        position += chunk_size;
    }
    Ok(table)
}

fn parse_package_chunk(
    data: &[u8],
    package_start: usize,
    package_size: usize,
    global_strings: &[String],
    table: &mut ResourceTable,
) -> Result<(), String> {
    let package_id = le_u32(data, package_start + 8)? & 0xff;
    let header_size = le_u16(data, package_start + 2)? as usize;
    let package_end = package_start.saturating_add(package_size);
    let mut position = package_start.saturating_add(header_size);
    while position + 8 <= package_end {
        let chunk_type = le_u16(data, position)?;
        let chunk_size = le_u32(data, position + 4)? as usize;
        if chunk_size < 8 || position.saturating_add(chunk_size) > package_end {
            return Err("resource package chunk is truncated".into());
        }
        if chunk_type == RES_TABLE_TYPE_TYPE {
            parse_type_chunk(data, position, package_id, global_strings, table)?;
        }
        position += chunk_size;
    }
    Ok(())
}

fn parse_type_chunk(
    data: &[u8],
    chunk_start: usize,
    package_id: u32,
    global_strings: &[String],
    table: &mut ResourceTable,
) -> Result<(), String> {
    let type_id = *data
        .get(chunk_start + 8)
        .ok_or_else(|| "resource type id missing".to_string())? as u32;
    let entry_count = le_u32(data, chunk_start + 12)? as usize;
    if entry_count > 1_000_000 {
        return Err("resource entry count exceeds safety limit".into());
    }
    let entries_start = chunk_start.saturating_add(le_u32(data, chunk_start + 16)? as usize);
    let offsets_start = chunk_start.saturating_add(le_u16(data, chunk_start + 2)? as usize);
    for entry_id in 0..entry_count {
        let offset = le_u32(
            data,
            offsets_start.saturating_add(entry_id.saturating_mul(4)),
        )?;
        if offset == NO_ENTRY {
            continue;
        }
        let entry = entries_start.saturating_add(offset as usize);
        let entry_size = le_u16(data, entry)? as usize;
        let flags = le_u16(data, entry + 2)?;
        if flags & ENTRY_FLAG_COMPLEX != 0 {
            continue;
        }
        let value = entry.saturating_add(entry_size);
        let data_type = *data
            .get(value + 3)
            .ok_or_else(|| "resource value is truncated".to_string())?;
        let value_data = le_u32(data, value + 4)?;
        let resource_id = (package_id << 24) | (type_id << 16) | entry_id as u32;
        let parsed = if data_type == TYPE_STRING {
            global_strings
                .get(value_data as usize)
                .cloned()
                .map(ResourceValue::String)
        } else if data_type == TYPE_REFERENCE {
            Some(ResourceValue::Reference(value_data))
        } else {
            None
        };
        if let Some(value) = parsed {
            table.values.entry(resource_id).or_insert(value);
        }
    }
    Ok(())
}

fn parse_string_pool(data: &[u8], start: usize) -> Result<Vec<String>, String> {
    if le_u16(data, start)? != RES_STRING_POOL_TYPE {
        return Err("string pool header missing".into());
    }
    let chunk_size = le_u32(data, start + 4)? as usize;
    let string_count = le_u32(data, start + 8)? as usize;
    if string_count > 1_000_000 || start.saturating_add(chunk_size) > data.len() {
        return Err("string pool exceeds safety limit".into());
    }
    let flags = le_u32(data, start + 16)?;
    let strings_start = start.saturating_add(le_u32(data, start + 20)? as usize);
    let offsets_start = start.saturating_add(le_u16(data, start + 2)? as usize);
    let is_utf8 = flags & UTF8_FLAG != 0;
    let mut strings = Vec::with_capacity(string_count);
    for index in 0..string_count {
        let offset = le_u32(data, offsets_start.saturating_add(index.saturating_mul(4)))? as usize;
        let string_at = strings_start.saturating_add(offset);
        strings.push(if is_utf8 {
            read_utf8_string(data, string_at)?
        } else {
            read_utf16_string(data, string_at)?
        });
    }
    Ok(strings)
}

fn read_utf8_string(data: &[u8], mut position: usize) -> Result<String, String> {
    let (_, used) = read_utf8_len(data, position)?;
    position += used;
    let (byte_len, used) = read_utf8_len(data, position)?;
    position += used;
    let bytes = data
        .get(position..position.saturating_add(byte_len))
        .ok_or_else(|| "utf8 string is truncated".to_string())?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn read_utf8_len(data: &[u8], position: usize) -> Result<(usize, usize), String> {
    let first = *data
        .get(position)
        .ok_or_else(|| "utf8 length is truncated".to_string())?;
    if first & 0x80 == 0 {
        Ok((first as usize, 1))
    } else {
        let second = *data
            .get(position + 1)
            .ok_or_else(|| "utf8 length is truncated".to_string())?;
        Ok(((((first & 0x7f) as usize) << 8) | second as usize, 2))
    }
}

fn read_utf16_string(data: &[u8], mut position: usize) -> Result<String, String> {
    let (units, used) = read_utf16_len(data, position)?;
    position += used;
    if units > data.len() / 2 {
        return Err("utf16 string exceeds safety limit".into());
    }
    let mut output = Vec::with_capacity(units);
    for index in 0..units {
        output.push(le_u16(
            data,
            position.saturating_add(index.saturating_mul(2)),
        )?);
    }
    Ok(String::from_utf16_lossy(&output))
}

fn read_utf16_len(data: &[u8], position: usize) -> Result<(usize, usize), String> {
    let first = le_u16(data, position)?;
    if first & 0x8000 == 0 {
        Ok((first as usize, 2))
    } else {
        let second = le_u16(data, position + 2)?;
        Ok(((((first & 0x7fff) as usize) << 16) | second as usize, 4))
    }
}

fn le_u16(data: &[u8], position: usize) -> Result<u16, String> {
    let bytes = data
        .get(position..position.saturating_add(2))
        .ok_or_else(|| "u16 read is out of range".to_string())?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn le_u32(data: &[u8], position: usize) -> Result<u32, String> {
    let bytes = data
        .get(position..position.saturating_add(4))
        .ok_or_else(|| "u32 read is out of range".to_string())?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn base_apk_path_prefers_base_and_rejects_relative_rows() {
        let output = "package:/data/app/pkg/split_config.en.apk\npackage:relative.apk\npackage:/data/app/pkg/base.apk\n";
        assert_eq!(
            parse_base_apk_path(output).as_deref(),
            Some("/data/app/pkg/base.apk")
        );
        assert_eq!(parse_base_apk_path("package:relative.apk\n"), None);
    }

    #[test]
    fn apk_identity_requires_positive_size_and_timestamp() {
        assert_eq!(
            parse_apk_identity("42:1712345678\n"),
            Some(ApkIdentity {
                size: 42,
                modified: 1_712_345_678,
            })
        );
        assert_eq!(parse_apk_identity("0:1"), None);
        assert_eq!(parse_apk_identity("42"), None);
    }

    #[test]
    fn clean_label_is_bounded_and_drops_control_characters() {
        assert_eq!(
            clean_label(" Droid\nsmith ".into()).as_deref(),
            Some("Droidsmith")
        );
        assert_eq!(clean_label("@string/app_name".into()), None);
        assert_eq!(clean_label("x".repeat(MAX_LABEL_CHARS + 1)), None);
    }

    #[test]
    fn density_ranking_prefers_larger_raster_assets() {
        let mut paths = [
            "res/mipmap-mdpi/ic_launcher.png",
            "res/mipmap-xxxhdpi/ic_launcher.png",
            "res/mipmap-hdpi/ic_launcher.png",
        ];
        paths.sort_by_key(|path| density_rank(path));
        assert_eq!(paths.last(), Some(&"res/mipmap-xxxhdpi/ic_launcher.png"));
    }

    #[test]
    fn resource_table_resolves_references_but_not_cycles() {
        let mut table = ResourceTable::default();
        table
            .values
            .insert(0x7f01_0001, ResourceValue::String("Droidsmith".into()));
        table
            .values
            .insert(0x7f02_0001, ResourceValue::Reference(0x7f01_0001));
        assert_eq!(table.resolve(0x7f02_0001).as_deref(), Some("Droidsmith"));
        table
            .values
            .insert(0x7f03_0001, ResourceValue::Reference(0x7f03_0001));
        assert_eq!(table.resolve(0x7f03_0001), None);
    }

    #[test]
    fn cache_invalidates_changed_identity_and_marks_hits() {
        let key = CacheKey {
            serial: "device".into(),
            transport_id: Some(1),
            connection_generation: 2,
            build_fingerprint: "build".into(),
            user_id: 0,
            package: "com.example".into(),
            apk_path: "/data/app/base.apk".into(),
        };
        let identity = ApkIdentity {
            size: 10,
            modified: 20,
        };
        let mut cache = MetadataCache::default();
        cache.insert(
            key.clone(),
            identity.clone(),
            AppPackageMetadata {
                package: "com.example".into(),
                label: Some("Example".into()),
                icon_data_uri: None,
                cache_hit: false,
            },
        );
        assert!(cache
            .get(&key, &identity)
            .is_some_and(|value| value.cache_hit));
        assert!(cache
            .get(
                &key,
                &ApkIdentity {
                    size: 11,
                    modified: 20,
                }
            )
            .is_none());
    }

    #[test]
    fn zip_entries_are_rejected_before_crossing_their_size_budget() {
        let path =
            std::env::temp_dir().join(format!("apk-metadata-test-{}.zip", uuid::Uuid::new_v4()));
        {
            let file = File::create(&path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("oversized.bin", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&[0_u8; 33]).unwrap();
            writer.finish().unwrap();
        }
        let file = File::open(&path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let result = read_zip_entry_limited(&mut archive, "oversized.bin", 32);
        let _ = fs::remove_file(path);
        assert!(matches!(result, Err(MetadataError::Parse(_))));
    }

    #[test]
    fn temporary_apk_drop_removes_partial_files() {
        let temporary = TemporaryApk::new().unwrap();
        let path = temporary.path.clone();
        fs::write(&path, b"partial").unwrap();
        drop(temporary);
        assert!(!path.exists());
    }
}
