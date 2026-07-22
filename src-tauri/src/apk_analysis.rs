//! Offline static analysis of a local APK/APKS file (R-097).
//!
//! This is a device-free inspector: given a host path the user selected through
//! the audited grant model, it opens the archive, parses the binary
//! `AndroidManifest.xml` (reusing the AXML/resource-table primitives in
//! [`crate::apk_metadata`]), reads DEX headers, scans for signing artifacts,
//! and reports a per-entry size breakdown. Nothing is pulled from a device and
//! no code is executed — only bounded reads over a ZIP on disk.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::apk_metadata::{le_u16, le_u32, parse_resource_table, parse_string_pool, ResourceTable};

const MAX_APK_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RESOURCES_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 65_536;
const MAX_PERMISSIONS: usize = 4096;
const MAX_ENTRY_ROWS: usize = 40;
const MAX_DISPLAY_VALUE_CHARS: usize = 512;
const MAX_SIGNING_BLOCK_BYTES: u64 = 32 * 1024 * 1024;
const DEX_METHOD_LIMIT: u32 = 65_536;

const RES_XML_START_ELEMENT_TYPE: u16 = 0x0102;
const RES_XML_RESOURCE_MAP_TYPE: u16 = 0x0180;
const NO_ENTRY: u32 = 0xffff_ffff;
const TYPE_REFERENCE: u8 = 0x01;
const TYPE_STRING: u8 = 0x03;
const TYPE_INT_DEC: u8 = 0x10;
const TYPE_INT_HEX: u8 = 0x11;
const TYPE_INT_BOOL: u8 = 0x12;

// APK Signing Block scheme IDs.
const SIG_V2_ID: u32 = 0x7109_871a;
const SIG_V3_ID: u32 = 0xf053_68c0;
const SIG_V31_ID: u32 = 0x1b93_ad61;
const SIG_BLOCK_MAGIC: &[u8; 16] = b"APK Sig Block 42";

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ComponentCounts {
    pub activities: u32,
    pub services: u32,
    pub receivers: u32,
    pub providers: u32,
}

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DexSummary {
    /// Number of `classes*.dex` files (more than one means multidex).
    pub files: u32,
    /// Sum of `class_defs_size` across all DEX files.
    pub defined_classes: u32,
    /// Sum of `method_ids_size` (method references) across all DEX files.
    pub method_refs: u32,
    /// True when the app is multidex or its total method refs exceed the
    /// single-DEX 65,536 reference ceiling.
    pub exceeds_64k: bool,
}

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SigningInfo {
    /// v1 JAR signing: a `META-INF/*.RSA|.DSA|.EC` block is present.
    pub v1: bool,
    /// v2 APK Signature Scheme (ID 0x7109871a in the APK Signing Block).
    pub v2: bool,
    /// v3 APK Signature Scheme (ID 0xf05368c0).
    pub v3: bool,
    /// v3.1 APK Signature Scheme (ID 0x1b93ad61).
    pub v31: bool,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApkEntrySize {
    pub name: String,
    pub compressed: u64,
    pub uncompressed: u64,
}

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ApkAnalysis {
    pub file_name: String,
    pub file_size: u64,
    pub sha256: String,
    pub package: Option<String>,
    pub version_code: Option<i64>,
    pub version_name: Option<String>,
    pub min_sdk: Option<i64>,
    pub target_sdk: Option<i64>,
    pub compile_sdk: Option<i64>,
    pub permissions: Vec<String>,
    pub components: ComponentCounts,
    pub dex: DexSummary,
    pub signing: SigningInfo,
    /// Optional cryptographic verification performed by the official Android
    /// SDK `apksigner`. Static analysis above remains available without it.
    pub signature_verification: crate::apk_signing::ApkSignatureVerification,
    /// Total number of ZIP entries in the archive.
    pub total_entries: usize,
    /// The largest entries by uncompressed size (bounded).
    pub largest_entries: Vec<ApkEntrySize>,
}

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("could not open {0}")]
    Open(String),
    #[error("APK exceeds the {MAX_APK_BYTES}-byte safety limit")]
    TooLarge,
    #[error("not a valid APK/ZIP: {0}")]
    Archive(String),
    #[error("APK is missing AndroidManifest.xml")]
    MissingManifest,
    #[error("APK manifest is unsupported or malformed: {0}")]
    Parse(String),
}

impl AnalysisError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Open(_) => "apk_open_failed",
            Self::TooLarge => "apk_too_large",
            Self::Archive(_) => "apk_not_archive",
            Self::MissingManifest => "apk_manifest_missing",
            Self::Parse(_) => "apk_parse_failed",
        }
    }
}

/// Analyze a local APK file. `path` is a validated host path (grant-consumed).
pub fn analyze(path: &Path) -> Result<ApkAnalysis, AnalysisError> {
    // Every parser and verifier must inspect the same immutable bytes. Opening
    // the renderer-selected path independently for metadata, hashing, ZIP
    // parsing, signing-block inspection, and apksigner allowed a concurrent
    // replacement to produce one report assembled from different files.
    let snapshot = AnalysisSnapshot::create(path)?;

    let file =
        File::open(&snapshot.path).map_err(|error| AnalysisError::Open(error.to_string()))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|error| AnalysisError::Archive(error.to_string()))?;
    if zip.len() > MAX_ZIP_ENTRIES {
        return Err(AnalysisError::Archive(format!(
            "APK contains more than {MAX_ZIP_ENTRIES} ZIP entries"
        )));
    }

    let manifest_bytes = read_entry(&mut zip, "AndroidManifest.xml", MAX_MANIFEST_BYTES)?
        .ok_or(AnalysisError::MissingManifest)?;
    let resources =
        read_entry(&mut zip, "resources.arsc", MAX_RESOURCES_BYTES)?.unwrap_or_default();
    let table = parse_resource_table(&resources).unwrap_or_default();
    let elements = parse_axml_elements(&manifest_bytes).map_err(AnalysisError::Parse)?;
    let manifest = summarize_manifest(&elements, &table);

    let (total_entries, largest_entries, dex, signing_v1) = scan_entries(&mut zip)?;
    let mut signing = detect_signing_block(&snapshot.path)?;
    signing.v1 = signing_v1;

    Ok(ApkAnalysis {
        file_name: snapshot.file_name.clone(),
        file_size: snapshot.size,
        sha256: snapshot.sha256.clone(),
        package: manifest.package,
        version_code: manifest.version_code,
        version_name: manifest.version_name,
        min_sdk: manifest.min_sdk,
        target_sdk: manifest.target_sdk.or(manifest.min_sdk),
        compile_sdk: manifest.compile_sdk,
        permissions: manifest.permissions,
        components: manifest.components,
        dex,
        signing,
        signature_verification: crate::apk_signing::verify(&snapshot.path),
        total_entries,
        largest_entries,
    })
}

struct AnalysisSnapshot {
    path: PathBuf,
    file_name: String,
    size: u64,
    sha256: String,
}

impl AnalysisSnapshot {
    fn create(source_path: &Path) -> Result<Self, AnalysisError> {
        let file_name = source_path
            .file_name()
            .map(|name| bounded_display(&name.to_string_lossy()))
            .unwrap_or_default();
        let mut source =
            File::open(source_path).map_err(|error| AnalysisError::Open(error.to_string()))?;
        if source
            .metadata()
            .map_err(|error| AnalysisError::Open(error.to_string()))?
            .len()
            > MAX_APK_BYTES
        {
            return Err(AnalysisError::TooLarge);
        }

        let path = std::env::temp_dir().join(format!(
            ".droidsmith-apk-analysis-{}-{}.apk",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let mut created = false;
        let result = (|| {
            let mut destination = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map_err(|error| AnalysisError::Open(error.to_string()))?;
            created = true;
            let mut hasher = Sha256::new();
            let mut size = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = source
                    .read(&mut buffer)
                    .map_err(|error| AnalysisError::Open(error.to_string()))?;
                if read == 0 {
                    break;
                }
                size = size.saturating_add(read as u64);
                if size > MAX_APK_BYTES {
                    return Err(AnalysisError::TooLarge);
                }
                hasher.update(&buffer[..read]);
                destination
                    .write_all(&buffer[..read])
                    .map_err(|error| AnalysisError::Open(error.to_string()))?;
            }
            destination
                .flush()
                .and_then(|()| destination.sync_data())
                .map_err(|error| AnalysisError::Open(error.to_string()))?;
            Ok(Self {
                path: path.clone(),
                file_name,
                size,
                sha256: format!("{:x}", hasher.finalize()),
            })
        })();
        if result.is_err() && created {
            let _ = std::fs::remove_file(&path);
        }
        result
    }
}

impl Drop for AnalysisSnapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn read_entry(
    zip: &mut zip::ZipArchive<File>,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, AnalysisError> {
    let mut entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(AnalysisError::Archive(error.to_string())),
    };
    if entry.size() > limit {
        return Err(AnalysisError::Parse(format!(
            "{name} exceeds the {limit}-byte safety limit"
        )));
    }
    let mut bytes = Vec::with_capacity(entry.size().min(limit) as usize);
    entry
        .by_ref()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AnalysisError::Parse(error.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(AnalysisError::Parse(format!(
            "{name} exceeds the {limit}-byte safety limit"
        )));
    }
    Ok(Some(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypedValue {
    Str(String),
    Int(i64),
    Ref(u32),
    Other,
}

impl TypedValue {
    fn resolve(&self, table: &ResourceTable) -> Option<String> {
        match self {
            Self::Str(value) => Some(value.clone()),
            Self::Ref(id) => table.resolve(*id),
            _ => None,
        }
    }

    fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Element {
    name: String,
    attrs: Vec<(String, TypedValue)>,
}

#[derive(Debug, Default)]
struct ManifestSummary {
    package: Option<String>,
    version_code: Option<i64>,
    version_name: Option<String>,
    min_sdk: Option<i64>,
    target_sdk: Option<i64>,
    compile_sdk: Option<i64>,
    permissions: Vec<String>,
    components: ComponentCounts,
}

/// Walk every START_ELEMENT chunk in a binary AXML document and return each
/// element's tag name plus its typed attributes.
fn parse_axml_elements(data: &[u8]) -> Result<Vec<Element>, String> {
    if data.len() < 16 {
        return Err("manifest too small".into());
    }
    let pool_start = le_u16(data, 2)? as usize;
    let strings = parse_string_pool(data, pool_start)?;
    let mut position = pool_start
        .checked_add(le_u32(data, pool_start + 4)? as usize)
        .ok_or_else(|| "manifest string pool overflow".to_string())?;

    let mut elements = Vec::new();
    while position + 8 <= data.len() {
        let chunk_type = le_u16(data, position)?;
        let chunk_size = le_u32(data, position + 4)? as usize;
        if chunk_size < 8 || position.saturating_add(chunk_size) > data.len() {
            return Err("manifest chunk is truncated".into());
        }
        if chunk_type == RES_XML_START_ELEMENT_TYPE {
            if let Some(element) = read_start_element(data, position, &strings)? {
                elements.push(element);
                if elements.len() > 200_000 {
                    return Err("manifest element count exceeds safety limit".into());
                }
            }
        } else if chunk_type != RES_XML_RESOURCE_MAP_TYPE {
            // Namespace / end-element / CDATA chunks carry no element attrs.
        }
        position += chunk_size;
    }
    Ok(elements)
}

fn read_start_element(
    data: &[u8],
    chunk_start: usize,
    strings: &[String],
) -> Result<Option<Element>, String> {
    let name_index = le_u32(data, chunk_start + 20)? as usize;
    let Some(name) = strings.get(name_index).cloned() else {
        return Ok(None);
    };
    let attribute_start = le_u16(data, chunk_start + 24)? as usize;
    let attribute_size = le_u16(data, chunk_start + 26)? as usize;
    let attribute_count = le_u16(data, chunk_start + 28)? as usize;
    if attribute_count == 0 {
        return Ok(Some(Element {
            name,
            attrs: Vec::new(),
        }));
    }
    if attribute_size < 20 || attribute_count > 4096 {
        return Err("manifest attribute table is invalid".into());
    }
    let attributes_start = chunk_start
        .checked_add(16 + attribute_start)
        .ok_or_else(|| "manifest attribute offset overflow".to_string())?;
    let mut attrs = Vec::with_capacity(attribute_count.min(64));
    for index in 0..attribute_count {
        let attribute = attributes_start
            .checked_add(index.saturating_mul(attribute_size))
            .ok_or_else(|| "manifest attribute offset overflow".to_string())?;
        let attr_name_index = le_u32(data, attribute + 4)? as usize;
        let Some(attr_name) = strings.get(attr_name_index).cloned() else {
            continue;
        };
        let raw_index = le_u32(data, attribute + 8)?;
        let data_type = *data
            .get(attribute + 15)
            .ok_or_else(|| "manifest attribute is truncated".to_string())?;
        let value_data = le_u32(data, attribute + 16)?;
        let value = match data_type {
            _ if raw_index != NO_ENTRY => strings
                .get(raw_index as usize)
                .cloned()
                .map(TypedValue::Str)
                .unwrap_or(TypedValue::Other),
            TYPE_STRING => strings
                .get(value_data as usize)
                .cloned()
                .map(TypedValue::Str)
                .unwrap_or(TypedValue::Other),
            TYPE_REFERENCE => TypedValue::Ref(value_data),
            TYPE_INT_DEC | TYPE_INT_HEX | TYPE_INT_BOOL => {
                TypedValue::Int(value_data as i32 as i64)
            }
            _ => TypedValue::Other,
        };
        attrs.push((attr_name, value));
    }
    Ok(Some(Element { name, attrs }))
}

fn attr<'a>(element: &'a Element, name: &str) -> Option<&'a TypedValue> {
    element
        .attrs
        .iter()
        .find(|(attr_name, _)| attr_name == name)
        .map(|(_, value)| value)
}

fn summarize_manifest(elements: &[Element], table: &ResourceTable) -> ManifestSummary {
    let mut summary = ManifestSummary::default();
    for element in elements {
        match element.name.as_str() {
            "manifest" => {
                summary.package = attr(element, "package")
                    .and_then(|v| v.resolve(table))
                    .map(|value| bounded_display(&value));
                let version_code = attr(element, "versionCode").and_then(TypedValue::as_int);
                let version_code_major =
                    attr(element, "versionCodeMajor").and_then(TypedValue::as_int);
                summary.version_code = long_version_code(version_code, version_code_major);
                summary.version_name = attr(element, "versionName")
                    .and_then(|v| v.resolve(table))
                    .map(|value| bounded_display(&value));
                summary.compile_sdk =
                    attr(element, "compileSdkVersion").and_then(TypedValue::as_int);
            }
            "uses-sdk" => {
                summary.min_sdk = attr(element, "minSdkVersion").and_then(TypedValue::as_int);
                summary.target_sdk = attr(element, "targetSdkVersion").and_then(TypedValue::as_int);
            }
            "uses-permission" | "uses-permission-sdk-23" => {
                if summary.permissions.len() < MAX_PERMISSIONS {
                    if let Some(name) = attr(element, "name").and_then(|v| v.resolve(table)) {
                        let name = bounded_display(&name);
                        if !summary.permissions.contains(&name) {
                            summary.permissions.push(name);
                        }
                    }
                }
            }
            "activity" | "activity-alias" => summary.components.activities += 1,
            "service" => summary.components.services += 1,
            "receiver" => summary.components.receivers += 1,
            "provider" => summary.components.providers += 1,
            _ => {}
        }
    }
    summary.permissions.sort();
    summary
}

fn long_version_code(lower: Option<i64>, major: Option<i64>) -> Option<i64> {
    let lower = lower?;
    let Some(major) = major else {
        return Some(lower);
    };
    let lower = u32::try_from(lower).ok()? as u64;
    let major = u32::try_from(major).ok()? as u64;
    i64::try_from((major << 32) | lower).ok()
}

fn bounded_display(value: &str) -> String {
    let mut output = String::with_capacity(value.len().min(MAX_DISPLAY_VALUE_CHARS));
    let mut truncated = false;
    for (index, character) in value.chars().enumerate() {
        if index >= MAX_DISPLAY_VALUE_CHARS {
            truncated = true;
            break;
        }
        output.push(if character.is_control() {
            '\u{fffd}'
        } else {
            character
        });
    }
    if truncated {
        output.push('…');
    }
    output
}

type EntryScan = (usize, Vec<ApkEntrySize>, DexSummary, bool);

fn scan_entries(zip: &mut zip::ZipArchive<File>) -> Result<EntryScan, AnalysisError> {
    let total_entries = zip.len();
    let mut sizes: Vec<ApkEntrySize> = Vec::with_capacity(MAX_ENTRY_ROWS + 1);
    let mut dex = DexSummary::default();
    let mut v1 = false;

    for index in 0..total_entries {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| AnalysisError::Archive(error.to_string()))?;
        let name = entry.name().to_string();
        sizes.push(ApkEntrySize {
            name: bounded_display(&name),
            compressed: entry.compressed_size(),
            uncompressed: entry.size(),
        });
        sizes.sort_by(|a, b| {
            b.uncompressed
                .cmp(&a.uncompressed)
                .then(a.name.cmp(&b.name))
        });
        sizes.truncate(MAX_ENTRY_ROWS);
        if is_v1_signature(&name) {
            v1 = true;
        }
        if is_dex_name(&name) {
            let mut header = [0_u8; 112];
            let read = fill(&mut entry, &mut header);
            if let Some((classes, methods)) = dex_counts(&header[..read]) {
                dex.files += 1;
                dex.defined_classes = dex.defined_classes.saturating_add(classes);
                dex.method_refs = dex.method_refs.saturating_add(methods);
            }
        }
    }

    dex.exceeds_64k = dex.files > 1 || dex.method_refs > DEX_METHOD_LIMIT;
    Ok((total_entries, sizes, dex, v1))
}

fn fill<R: Read>(reader: &mut R, buffer: &mut [u8]) -> usize {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(_) => break,
        }
    }
    filled
}

fn is_dex_name(name: &str) -> bool {
    // `classes.dex`, `classes2.dex`, ... at the archive root.
    let Some(stem) = name.strip_suffix(".dex") else {
        return false;
    };
    stem == "classes"
        || stem
            .strip_prefix("classes")
            .is_some_and(|n| n.parse::<u32>().is_ok())
}

fn is_v1_signature(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("META-INF/")
        && (upper.ends_with(".RSA") || upper.ends_with(".DSA") || upper.ends_with(".EC"))
}

/// DEX header: `class_defs_size` at 0x60, `method_ids_size` at 0x58. The magic
/// is `dex\n0NN\0`.
fn dex_counts(header: &[u8]) -> Option<(u32, u32)> {
    if header.len() < 0x70 || &header[0..4] != b"dex\n" {
        return None;
    }
    let methods = u32::from_le_bytes(header[0x58..0x5c].try_into().ok()?);
    let classes = u32::from_le_bytes(header[0x60..0x64].try_into().ok()?);
    Some((classes, methods))
}

/// Locate the APK Signing Block (immediately before the ZIP central directory)
/// and report which v2/v3 scheme IDs it contains. Best-effort: any read/parse
/// failure yields all-false, since v1 is detected separately from the entries.
fn detect_signing_block(path: &Path) -> Result<SigningInfo, AnalysisError> {
    let mut info = SigningInfo::default();
    let Ok(mut file) = File::open(path) else {
        return Ok(info);
    };
    let Some(cd_offset) = find_central_directory_offset(&mut file) else {
        return Ok(info);
    };
    if cd_offset < 24 {
        return Ok(info);
    }
    // The 16-byte magic sits in the last 16 bytes before the central directory;
    // the 8 bytes before that repeat the block size.
    if file.seek(SeekFrom::Start(cd_offset - 24)).is_err() {
        return Ok(info);
    }
    let mut footer = [0_u8; 24];
    if fill(&mut file, &mut footer) != 24 || &footer[8..24] != SIG_BLOCK_MAGIC {
        return Ok(info);
    }
    let block_size = u64::from_le_bytes(footer[0..8].try_into().unwrap());
    if !(24..=MAX_SIGNING_BLOCK_BYTES).contains(&block_size) || block_size + 8 > cd_offset {
        return Ok(info);
    }
    // Block layout: u64 size | (id-value pairs) | u64 size | 16-byte magic.
    // The pairs region starts 8 bytes into the block and ends before the
    // trailing size+magic (24 bytes).
    let pairs_start = cd_offset - block_size - 8 + 8;
    let pairs_len = block_size.saturating_sub(24);
    if file.seek(SeekFrom::Start(pairs_start)).is_err() {
        return Ok(info);
    }
    let mut pairs = vec![0_u8; pairs_len as usize];
    if fill(&mut file, &mut pairs) != pairs.len() {
        return Ok(info);
    }
    let mut cursor = 0_usize;
    while cursor + 12 <= pairs.len() {
        let pair_len = u64::from_le_bytes(pairs[cursor..cursor + 8].try_into().unwrap());
        if pair_len < 4 || (cursor + 8).saturating_add(pair_len as usize) > pairs.len() {
            break;
        }
        let id = u32::from_le_bytes(pairs[cursor + 8..cursor + 12].try_into().unwrap());
        match id {
            SIG_V2_ID => info.v2 = true,
            SIG_V3_ID => info.v3 = true,
            SIG_V31_ID => info.v31 = true,
            _ => {}
        }
        cursor += 8 + pair_len as usize;
    }
    Ok(info)
}

/// Scan the tail of the file for the ZIP End Of Central Directory record and
/// return the central-directory offset it points to.
fn find_central_directory_offset(file: &mut File) -> Option<u64> {
    let len = file.seek(SeekFrom::End(0)).ok()?;
    // EOCD is 22 bytes + up to 65,535 bytes of comment.
    let scan = len.min(22 + 0xffff);
    let start = len - scan;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buffer = vec![0_u8; scan as usize];
    if fill(file, &mut buffer) != buffer.len() {
        return None;
    }
    let signature = [0x50, 0x4b, 0x05, 0x06];
    let mut position = buffer.len().checked_sub(22)?;
    loop {
        if buffer[position..position + 4] == signature {
            let offset = u32::from_le_bytes(buffer[position + 16..position + 20].try_into().ok()?);
            if offset != 0xffff_ffff {
                return Some(offset as u64);
            }
        }
        position = position.checked_sub(1)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn dex_counts_reads_class_and_method_totals() {
        let mut header = vec![0_u8; 0x70];
        header[0..4].copy_from_slice(b"dex\n");
        header[0x58..0x5c].copy_from_slice(&1234_u32.to_le_bytes());
        header[0x60..0x64].copy_from_slice(&56_u32.to_le_bytes());
        assert_eq!(dex_counts(&header), Some((56, 1234)));
        assert_eq!(dex_counts(b"not a dex"), None);
    }

    #[test]
    fn dex_name_matches_root_multidex_only() {
        assert!(is_dex_name("classes.dex"));
        assert!(is_dex_name("classes2.dex"));
        assert!(is_dex_name("classes10.dex"));
        assert!(!is_dex_name("res/classes.dex"));
        assert!(!is_dex_name("classesX.dex"));
        assert!(!is_dex_name("AndroidManifest.xml"));
    }

    #[test]
    fn v1_signature_detection_is_case_insensitive_and_scoped() {
        assert!(is_v1_signature("META-INF/CERT.RSA"));
        assert!(is_v1_signature("meta-inf/cert.dsa"));
        assert!(is_v1_signature("META-INF/KEY.EC"));
        assert!(!is_v1_signature("META-INF/MANIFEST.MF"));
        assert!(!is_v1_signature("res/cert.rsa"));
    }

    #[test]
    fn exceeds_64k_flags_multidex_and_high_method_counts() {
        let scan = |files: u32, methods: u32| {
            let mut dex = DexSummary {
                files,
                method_refs: methods,
                ..DexSummary::default()
            };
            dex.exceeds_64k = dex.files > 1 || dex.method_refs > DEX_METHOD_LIMIT;
            dex.exceeds_64k
        };
        assert!(!scan(1, 100));
        assert!(scan(2, 100));
        assert!(scan(1, 70_000));
    }

    #[test]
    fn long_version_code_combines_the_manifest_major_component() {
        assert_eq!(long_version_code(Some(42), None), Some(42));
        assert_eq!(
            long_version_code(Some(42), Some(3)),
            Some((3_i64 << 32) | 42)
        );
        assert_eq!(long_version_code(Some(-1), Some(3)), None);
        assert_eq!(long_version_code(Some(42), Some(-1)), None);
    }

    #[test]
    fn display_values_are_bounded_and_strip_controls() {
        assert_eq!(bounded_display("safe value"), "safe value");
        assert_eq!(bounded_display("line\nfeed"), "line�feed");
        let bounded = bounded_display(&"x".repeat(MAX_DISPLAY_VALUE_CHARS + 20));
        assert_eq!(bounded.chars().count(), MAX_DISPLAY_VALUE_CHARS + 1);
        assert!(bounded.ends_with('…'));
    }

    #[test]
    fn entry_scan_keeps_only_bounded_largest_rows() {
        let dir = std::env::temp_dir().join(format!("apk-rows-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("rows.apk");
        {
            let file = File::create(&archive).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            for index in 0..(MAX_ENTRY_ROWS + 20) {
                writer
                    .start_file(
                        format!("entry-{index:03}.bin"),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
                writer.write_all(&vec![b'x'; index + 1]).unwrap();
            }
            writer.finish().unwrap();
        }
        let file = File::open(&archive).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let (total, rows, _, _) = scan_entries(&mut zip).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(total, MAX_ENTRY_ROWS + 20);
        assert_eq!(rows.len(), MAX_ENTRY_ROWS);
        assert_eq!(rows[0].uncompressed, (MAX_ENTRY_ROWS + 20) as u64);
        assert_eq!(rows.last().unwrap().uncompressed, 21);
    }

    #[test]
    fn snapshot_keeps_hash_and_parsers_on_one_immutable_file() {
        let dir = std::env::temp_dir().join(format!("apk-snapshot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("selected.apk");
        std::fs::write(&source, b"first bytes").unwrap();
        let snapshot = AnalysisSnapshot::create(&source).unwrap();
        let snapshot_path = snapshot.path.clone();
        std::fs::write(&source, b"replacement bytes").unwrap();

        assert_eq!(std::fs::read(&snapshot.path).unwrap(), b"first bytes");
        assert_eq!(snapshot.size, 11);
        assert_eq!(
            snapshot.sha256,
            format!("{:x}", Sha256::digest(b"first bytes"))
        );
        drop(snapshot);
        assert!(!snapshot_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signing_block_pairs_report_scheme_ids() {
        // Build a synthetic zip, then splice an APK Signing Block carrying a v2
        // and a v3 pair between the local entries and the central directory.
        let dir = std::env::temp_dir().join(format!("apk-sig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("base.zip");
        {
            let file = File::create(&base).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file(
                    "AndroidManifest.xml",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(b"stub").unwrap();
            writer.finish().unwrap();
        }
        let bytes = std::fs::read(&base).unwrap();
        // Central directory offset is the position of the first 0x504b0102.
        let cd_offset = bytes
            .windows(4)
            .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
            .unwrap();
        let pair = |id: u32| {
            let mut p = Vec::new();
            p.extend_from_slice(&12_u64.to_le_bytes()); // pair length = 4 (id) + 8 payload
            p.extend_from_slice(&id.to_le_bytes());
            p.extend_from_slice(&0_u64.to_le_bytes());
            p
        };
        let mut pairs = pair(SIG_V2_ID);
        pairs.extend(pair(SIG_V3_ID));
        // block_size counts everything AFTER the leading size field: the pairs,
        // the trailing size field (8), and the 16-byte magic.
        let block_size = (pairs.len() + 8 + 16) as u64;
        let mut block = Vec::new();
        block.extend_from_slice(&block_size.to_le_bytes());
        block.extend_from_slice(&pairs);
        block.extend_from_slice(&block_size.to_le_bytes());
        block.extend_from_slice(SIG_BLOCK_MAGIC);

        let mut spliced = Vec::new();
        spliced.extend_from_slice(&bytes[..cd_offset]);
        spliced.extend_from_slice(&block);
        spliced.extend_from_slice(&bytes[cd_offset..]);
        // Fix the EOCD central-directory offset to point past the block.
        let new_cd = (cd_offset + block.len()) as u32;
        let eocd = spliced
            .windows(4)
            .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])
            .unwrap();
        spliced[eocd + 16..eocd + 20].copy_from_slice(&new_cd.to_le_bytes());

        let apk = dir.join("signed.apk");
        std::fs::write(&apk, &spliced).unwrap();
        let info = detect_signing_block(&apk).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(info.v2, "v2 id detected");
        assert!(info.v3, "v3 id detected");
        assert!(!info.v31);
    }

    #[test]
    fn missing_manifest_is_reported() {
        let dir = std::env::temp_dir().join(format!("apk-nomani-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let apk = dir.join("empty.apk");
        {
            let file = File::create(&apk).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("res/values.xml", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"<x/>").unwrap();
            writer.finish().unwrap();
        }
        let result = analyze(&apk);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(matches!(result, Err(AnalysisError::MissingManifest)));
    }
}
