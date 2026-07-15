//! Local-only, privacy-filtered diagnostics bundle generation.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::adb::device::DeviceState;
use crate::adb::{self, health::AdbHealth};

const MAX_JSONL_BYTES: u64 = 2 * 1024 * 1024;
const MAX_JOURNAL_FILES: usize = 100;
const MAX_FAILURES: usize = 50;
const MAX_CRASH_BYTES: u64 = 64 * 1024;
const MAX_CRASH_LINES: usize = 200;

#[derive(Debug, Clone)]
pub struct EnvironmentInput {
    pub app_version: String,
    pub tauri_version: String,
    pub rust_version: String,
    pub os_family: String,
    pub os_version: String,
    pub os_arch: String,
    pub adb_available: bool,
    pub adb_source: String,
    pub adb_version: Option<String>,
    pub adb_health: Option<AdbHealth>,
    pub devices: Vec<adb::Device>,
    pub collection_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupportPreview {
    pub generated_at: String,
    pub content: String,
    pub byte_size: usize,
    pub device_count: usize,
    pub failed_operation_count: usize,
    pub crash_line_count: usize,
    pub local_only: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WipeResult {
    pub files_removed: usize,
    pub bytes_removed: u64,
    pub device_journals_preserved: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedResult {
    pub path: String,
    pub byte_size: usize,
    pub generated_at: String,
}

#[derive(Serialize)]
struct SupportBundle {
    schema_version: u32,
    generated_at: String,
    privacy: PrivacySummary,
    environment: SupportEnvironment,
    devices: Vec<SupportDevice>,
    failed_operations: Vec<SupportFailure>,
    crash_logs: Vec<CrashExcerpt>,
    collection_warnings: Vec<String>,
}

#[derive(Serialize)]
struct PrivacySummary {
    local_only: bool,
    uploads_performed: bool,
    redactions: [&'static str; 5],
}

#[derive(Serialize)]
struct SupportEnvironment {
    app_version: String,
    tauri_version: String,
    rust_version: String,
    os_family: String,
    os_version: String,
    os_arch: String,
    adb_available: bool,
    adb_source: String,
    adb_version: Option<String>,
    adb_health: Option<SupportAdbHealth>,
}

#[derive(Serialize)]
struct SupportAdbHealth {
    server_status_supported: bool,
    client_version: Option<String>,
    server_version: Option<String>,
    server_build: Option<String>,
    usb_backend: Option<String>,
    mdns_backend: Option<String>,
    mdns_enabled: Option<bool>,
    mdns_check: Option<String>,
    burst_mode: Option<bool>,
    recommended_for_wifi_v2: bool,
    wifi_v2_state: String,
    wifi_v2_device_count: usize,
    warning: Option<String>,
}

#[derive(Serialize)]
struct SupportDevice {
    id: String,
    state: String,
    model: Option<String>,
    product: Option<String>,
    device: Option<String>,
    wireless: bool,
    transport_kind: crate::adb::DeviceTransportKind,
}

#[derive(Debug, Clone, Serialize)]
struct SupportFailure {
    source: &'static str,
    device_id: Option<String>,
    operation_id: String,
    operation: String,
    outcome: String,
    occurred_at: String,
    failure: Option<String>,
}

#[derive(Serialize)]
struct CrashExcerpt {
    slot: String,
    lines: Vec<String>,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct RawJournalRecord {
    serial: String,
    id: u64,
    incident_id: String,
    operation: String,
    outcome: String,
    occurred_at: String,
    failure: Option<String>,
}

#[derive(Debug, Clone)]
struct RawInstallRecord {
    serial: String,
    operation_id: String,
    outcome: String,
    occurred_at: String,
    failure: Option<String>,
}

pub fn build_preview(
    app_data_dir: &Path,
    crash_log_dir: &Path,
    input: EnvironmentInput,
) -> std::io::Result<SupportPreview> {
    let journal_records = read_journal_records(&app_data_dir.join("journal"))?;
    let host_records = read_host_records(&app_data_dir.join("host-operations.jsonl"))?;
    let install_records = read_install_records(&app_data_dir.join("install-operations.jsonl"))?;
    let mut serials = input
        .devices
        .iter()
        .map(|device| device.serial.clone())
        .chain(journal_records.iter().map(|record| record.serial.clone()))
        .chain(install_records.iter().map(|record| record.serial.clone()))
        .collect::<BTreeSet<_>>();
    serials.retain(|serial| !serial.is_empty());
    let aliases = serials
        .into_iter()
        .enumerate()
        .map(|(index, serial)| (serial, format!("device-{:02}", index + 1)))
        .collect::<BTreeMap<_, _>>();

    let devices = input
        .devices
        .iter()
        .take(100)
        .map(|device| SupportDevice {
            id: aliases
                .get(&device.serial)
                .cloned()
                .unwrap_or_else(|| "device-unmapped".to_string()),
            state: sanitize_text(&serialized_state(&device.state), &aliases),
            model: device
                .model
                .as_deref()
                .map(|value| sanitize_text(value, &aliases)),
            product: device
                .product
                .as_deref()
                .map(|value| sanitize_text(value, &aliases)),
            device: device
                .device
                .as_deref()
                .map(|value| sanitize_text(value, &aliases)),
            wireless: device.wireless,
            transport_kind: device.transport_kind,
        })
        .collect::<Vec<_>>();

    let mut failures = journal_records
        .into_iter()
        .filter(|record| is_failure_outcome(&record.outcome))
        .map(|record| SupportFailure {
            source: "device_journal",
            device_id: aliases.get(&record.serial).cloned(),
            operation_id: if record.incident_id.is_empty() {
                format!("journal-{}", record.id)
            } else {
                sanitize_text(&record.incident_id, &aliases)
            },
            operation: sanitize_text(&record.operation, &aliases),
            outcome: record.outcome,
            occurred_at: record.occurred_at,
            failure: record
                .failure
                .as_deref()
                .map(|failure| sanitize_text(failure, &aliases)),
        })
        .chain(install_records.into_iter().map(|record| {
            SupportFailure {
                source: "install_operation",
                device_id: aliases.get(&record.serial).cloned(),
                operation_id: sanitize_text(&record.operation_id, &aliases),
                operation: "install_package".to_string(),
                outcome: record.outcome,
                occurred_at: record.occurred_at,
                failure: record
                    .failure
                    .as_deref()
                    .map(|failure| sanitize_text(failure, &aliases)),
            }
        }))
        .chain(host_records.into_iter().map(|mut record| {
            record.operation_id = sanitize_text(&record.operation_id, &aliases);
            record.operation = sanitize_text(&record.operation, &aliases);
            record.failure = record
                .failure
                .as_deref()
                .map(|failure| sanitize_text(failure, &aliases));
            record
        }))
        .collect::<Vec<_>>();
    failures.sort_by(|left, right| right.occurred_at.cmp(&left.occurred_at));
    failures.truncate(MAX_FAILURES);

    let crash_logs = read_crash_logs(crash_log_dir, &aliases)?;
    let crash_line_count = crash_logs.iter().map(|log| log.lines.len()).sum();
    let health = input
        .adb_health
        .as_ref()
        .map(|health| sanitize_health(health, &aliases));
    let generated_at = crate::time::iso_utc_now();
    let bundle = SupportBundle {
        schema_version: 1,
        generated_at: generated_at.clone(),
        privacy: PrivacySummary {
            local_only: true,
            uploads_performed: false,
            redactions: [
                "raw device serials",
                "network device addresses",
                "wireless pairing secrets",
                "host filesystem paths",
                "credential-like values",
            ],
        },
        environment: SupportEnvironment {
            app_version: input.app_version,
            tauri_version: input.tauri_version,
            rust_version: input.rust_version,
            os_family: sanitize_text(&input.os_family, &aliases),
            os_version: sanitize_text(&input.os_version, &aliases),
            os_arch: sanitize_text(&input.os_arch, &aliases),
            adb_available: input.adb_available,
            adb_source: sanitize_text(&input.adb_source, &aliases),
            adb_version: input
                .adb_version
                .as_deref()
                .map(|version| sanitize_text(version, &aliases)),
            adb_health: health,
        },
        devices,
        failed_operations: failures,
        crash_logs,
        collection_warnings: input
            .collection_warnings
            .iter()
            .map(|warning| sanitize_text(warning, &aliases))
            .collect(),
    };
    let content = serde_json::to_string_pretty(&bundle)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    Ok(SupportPreview {
        generated_at,
        byte_size: content.len(),
        device_count: bundle.devices.len(),
        failed_operation_count: bundle.failed_operations.len(),
        crash_line_count,
        content,
        local_only: true,
    })
}

pub fn wipe_local_data(app_data_dir: &Path, crash_log_dir: &Path) -> std::io::Result<WipeResult> {
    let mut candidates = crash_log_paths(crash_log_dir)?;
    candidates.push(app_data_dir.join("host-operations.jsonl"));
    let mut files_removed = 0;
    let mut bytes_removed = 0_u64;
    for path in candidates {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        bytes_removed = bytes_removed.saturating_add(metadata.len());
        fs::remove_file(path)?;
        files_removed += 1;
    }
    Ok(WipeResult {
        files_removed,
        bytes_removed,
        device_journals_preserved: true,
    })
}

fn sanitize_health(health: &AdbHealth, aliases: &BTreeMap<String, String>) -> SupportAdbHealth {
    let clean =
        |value: &Option<String>| value.as_deref().map(|value| sanitize_text(value, aliases));
    SupportAdbHealth {
        server_status_supported: health.server_status_supported,
        client_version: clean(&health.client_version),
        server_version: clean(&health.server_version),
        server_build: clean(&health.server_build),
        usb_backend: clean(&health.usb_backend),
        mdns_backend: clean(&health.mdns_backend),
        mdns_enabled: health.mdns_enabled,
        mdns_check: clean(&health.mdns_check),
        burst_mode: health.burst_mode,
        recommended_for_wifi_v2: health.recommended_for_wifi_v2,
        wifi_v2_state: format!("{:?}", health.wifi_v2_state).to_lowercase(),
        wifi_v2_device_count: health.wifi_v2_devices.len(),
        warning: clean(&health.warning),
    }
}

fn serialized_state(state: &DeviceState) -> String {
    serde_json::to_string(state).unwrap_or_else(|_| "unknown".to_string())
}

fn read_journal_records(dir: &Path) -> std::io::Result<Vec<RawJournalRecord>> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let path = entry.path();
            (file_type.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("jsonl"))
            .then_some(path)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.truncate(MAX_JOURNAL_FILES);

    let mut latest = HashMap::<(PathBuf, u64), RawJournalRecord>::new();
    for path in paths {
        for value in read_json_lines(&path, MAX_JSONL_BYTES)? {
            let Some(id) = value.get("id").and_then(Value::as_u64) else {
                continue;
            };
            let request = &value["applied"]["plan"]["request"];
            let Some(serial) = request.get("serial").and_then(Value::as_str) else {
                continue;
            };
            latest.insert(
                (path.clone(), id),
                RawJournalRecord {
                    serial: serial.to_string(),
                    id,
                    incident_id: value["applied"]["plan"]["incident_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    operation: json_scalar(&request["kind"]),
                    outcome: value["outcome"].as_str().unwrap_or("succeeded").to_string(),
                    occurred_at: value["applied"]["applied_at"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    failure: value["failure"].as_str().map(str::to_string),
                },
            );
        }
    }
    Ok(latest.into_values().collect())
}

fn read_host_records(path: &Path) -> std::io::Result<Vec<SupportFailure>> {
    let mut latest = HashMap::<String, SupportFailure>::new();
    for value in read_json_lines(path, MAX_JSONL_BYTES)? {
        let Some(operation_id) = value.get("operation_id").and_then(Value::as_str) else {
            continue;
        };
        let outcome = value["outcome"].as_str().unwrap_or("unknown");
        if !is_failure_outcome(outcome) {
            latest.remove(operation_id);
            continue;
        }
        latest.insert(
            operation_id.to_string(),
            SupportFailure {
                source: "host_operation",
                device_id: None,
                operation_id: operation_id.to_string(),
                operation: value["operation"]
                    .as_str()
                    .unwrap_or("host_operation")
                    .to_string(),
                outcome: outcome.to_string(),
                occurred_at: value["completed_at"]
                    .as_str()
                    .or_else(|| value["started_at"].as_str())
                    .unwrap_or_default()
                    .to_string(),
                failure: value["failure"].as_str().map(str::to_string),
            },
        );
    }
    Ok(latest.into_values().collect())
}

fn read_install_records(path: &Path) -> std::io::Result<Vec<RawInstallRecord>> {
    let mut latest = HashMap::<String, RawInstallRecord>::new();
    for value in read_json_lines(path, MAX_JSONL_BYTES)? {
        let Some(operation_id) = value.get("operation_id").and_then(Value::as_str) else {
            continue;
        };
        let outcome = value["outcome"].as_str().unwrap_or("unknown");
        if !is_failure_outcome(outcome) {
            latest.remove(operation_id);
            continue;
        }
        latest.insert(
            operation_id.to_string(),
            RawInstallRecord {
                serial: value["device_serial"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                operation_id: operation_id.to_string(),
                outcome: outcome.to_string(),
                occurred_at: value["completed_at"]
                    .as_str()
                    .or_else(|| value["started_at"].as_str())
                    .unwrap_or_default()
                    .to_string(),
                failure: value["failure_summary"].as_str().map(str::to_string),
            },
        );
    }
    Ok(latest.into_values().collect())
}

fn read_json_lines(path: &Path, max_bytes: u64) -> std::io::Result<Vec<Value>> {
    let Some(text) = read_regular_file_tail(path, max_bytes)? else {
        return Ok(Vec::new());
    };
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect())
}

fn read_crash_logs(
    dir: &Path,
    aliases: &BTreeMap<String, String>,
) -> std::io::Result<Vec<CrashExcerpt>> {
    let paths = crash_log_paths(dir)?;
    let mut total_lines = 0;
    let mut logs = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        if total_lines >= MAX_CRASH_LINES {
            break;
        }
        let Some(text) = read_regular_file_tail(&path, MAX_CRASH_BYTES)? else {
            continue;
        };
        let available = MAX_CRASH_LINES - total_lines;
        let all_lines = text.lines().collect::<Vec<_>>();
        let start = all_lines.len().saturating_sub(available);
        let lines = all_lines[start..]
            .iter()
            .map(|line| sanitize_text(line, aliases))
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        total_lines += lines.len();
        logs.push(CrashExcerpt {
            slot: if index == 0 {
                "current".to_string()
            } else {
                format!("rotated-{index}")
            },
            truncated: start > 0 || text.len() as u64 >= MAX_CRASH_BYTES,
            lines,
        });
    }
    Ok(logs)
}

fn crash_log_paths(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            (file_type.is_file() && name.starts_with("crash.log")).then_some(entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if name == "crash.log" {
            0
        } else {
            1
        }
    });
    Ok(paths)
}

fn read_regular_file_tail(path: &Path, max_bytes: u64) -> std::io::Result<Option<String>> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(None);
    };
    if !metadata.file_type().is_file() {
        return Ok(None);
    }
    let mut file = File::open(path)?;
    let start = metadata.len().saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if start > 0 {
        if let Some(newline) = bytes.iter().position(|byte| *byte == b'\n') {
            bytes.drain(..=newline);
        }
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn is_failure_outcome(outcome: &str) -> bool {
    matches!(outcome, "pending" | "failed" | "interrupted" | "cancelled")
}

fn json_scalar(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn sanitize_text(value: &str, aliases: &BTreeMap<String, String>) -> String {
    let mut value = value
        .chars()
        .filter(|character| *character == '\n' || *character == '\t' || !character.is_control())
        .take(8_192)
        .collect::<String>();
    for (serial, alias) in aliases {
        value = value.replace(serial, alias);
    }

    value
        .lines()
        .map(sanitize_line)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(8_192)
        .collect()
}

fn sanitize_line(line: &str) -> String {
    let mut output = Vec::new();
    let mut redact_next_device = false;
    let mut redact_next_secret = false;
    for token in line.split_whitespace() {
        let lower = token
            .trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '-' && character != '_'
            })
            .to_ascii_lowercase();
        if redact_next_device {
            output.push("[redacted-device]".to_string());
            redact_next_device = false;
            continue;
        }
        if redact_next_secret {
            output.push("[redacted-secret]".to_string());
            redact_next_secret = false;
            continue;
        }
        if matches!(
            lower.as_str(),
            "serial" | "device" | "device_id" | "target" | "-s"
        ) {
            output.push(token.to_string());
            redact_next_device = true;
            continue;
        }
        if let Some((key, _)) = token.split_once('=') {
            let key_lower = key.to_ascii_lowercase();
            if key_lower.contains("serial") || key_lower.contains("device_id") {
                output.push(format!("{key}=[redacted-device]"));
                continue;
            }
            if sensitive_key(&key_lower) {
                output.push(format!("{key}=[redacted-secret]"));
                continue;
            }
        }
        if sensitive_key(&lower) {
            output.push(token.to_string());
            redact_next_secret = true;
            continue;
        }
        if looks_like_host_path(token) {
            output.push("[redacted-path]".to_string());
            continue;
        }
        if looks_like_network_address(token) {
            output.push("[redacted-network-device]".to_string());
            continue;
        }
        output.push(redact_six_digit_runs(token));
    }
    output.join(" ")
}

fn sensitive_key(key: &str) -> bool {
    [
        "pairing",
        "password",
        "passwd",
        "token",
        "secret",
        "credential",
    ]
    .iter()
    .any(|marker| key.contains(marker))
}

fn looks_like_host_path(token: &str) -> bool {
    let token = token.trim_matches(['"', '\'', '[', ']', '(', ')', ',', ';']);
    let bytes = token.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || token.starts_with("/home/")
        || token.starts_with("/Users/")
        || token.starts_with("/tmp/")
        || token.starts_with("\\\\")
}

fn looks_like_network_address(token: &str) -> bool {
    let token = token.trim_matches(['"', '\'', '(', ')', ',', ';']);
    token.parse::<IpAddr>().is_ok()
        || token.parse::<SocketAddr>().is_ok()
        || token
            .strip_prefix('[')
            .and_then(|value| value.split_once(']'))
            .and_then(|(address, _)| address.parse::<IpAddr>().ok())
            .is_some()
}

fn redact_six_digit_runs(token: &str) -> String {
    let chars = token.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii_digit() {
            let start = index;
            while index < chars.len() && chars[index].is_ascii_digit() {
                index += 1;
            }
            if index - start == 6 {
                output.push_str("[redacted-code]");
            } else {
                output.extend(chars[start..index].iter());
            }
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "droidsmith-support-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn input() -> EnvironmentInput {
        EnvironmentInput {
            app_version: "0.1.0".to_string(),
            tauri_version: "2.0.0".to_string(),
            rust_version: "1.81".to_string(),
            os_family: "Windows".to_string(),
            os_version: "11".to_string(),
            os_arch: "x86_64".to_string(),
            adb_available: true,
            adb_source: "path".to_string(),
            adb_version: Some("37.0.0".to_string()),
            adb_health: Some(AdbHealth::default()),
            devices: vec![adb::Device {
                serial: "ZY224JQ9".to_string(),
                state: DeviceState::Device,
                model: Some("Pixel".to_string()),
                product: None,
                device: None,
                build_fingerprint: None,
                transport_id: Some(7),
                connection_generation: 8,
                transport_kind: adb::DeviceTransportKind::Usb,
                wireless: false,
            }],
            collection_warnings: Vec::new(),
        }
    }

    #[test]
    fn bundle_redacts_serials_paths_network_addresses_and_secrets() {
        let root = temp_dir("redaction");
        let app = root.join("app");
        let logs = root.join("logs");
        fs::create_dir_all(app.join("journal")).unwrap();
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            app.join("journal").join("device.jsonl"),
            r#"{"id":1,"applied":{"plan":{"request":{"serial":"ZY224JQ9","kind":"disable"},"incident_id":"op-1"},"applied_at":"2026-07-14T18:00:00Z"},"outcome":"failed","failure":"serial=ZY224JQ9 pairing_code=123456 path=C:\\Users\\Alice\\adb.exe"}
"#,
        )
        .unwrap();
        fs::write(
            app.join("host-operations.jsonl"),
            r#"{"operation_id":"host-1","operation":"adb_server_recovery","outcome":"failed","started_at":"2026-07-14T18:01:00Z","failure":"token=supersecret /home/alice/adb 192.168.1.4:5555"}
"#,
        )
        .unwrap();
        fs::write(
            app.join("install-operations.jsonl"),
            r#"{"operation_id":"install-1","device_serial":"ZY224JQ9","outcome":"failed","started_at":"2026-07-14T18:01:30Z","failure_summary":"INSTALL_FAILED_MISSING_SPLIT for device ZY224JQ9"}
"#,
        )
        .unwrap();
        fs::write(
            logs.join("crash.log"),
            "panic for device ZY224JQ9 pairing_code=123456 password supersecret at C:\\Users\\Alice\\repo [fe80::1]:5555\n",
        )
        .unwrap();

        let preview = build_preview(&app, &logs, input()).unwrap();
        for secret in [
            "ZY224JQ9",
            "123456",
            "C:\\Users\\Alice",
            "/home/alice",
            "192.168.1.4",
            "fe80::1",
            "supersecret",
        ] {
            assert!(!preview.content.contains(secret), "leaked {secret}");
        }
        assert!(preview.content.contains("device-01"));
        assert!(preview.content.contains("install_operation"));
        assert!(preview.content.contains("\"local_only\": true"));
        assert!(preview.content.contains("\"uploads_performed\": false"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wipe_removes_logs_but_preserves_device_journals() {
        let root = temp_dir("wipe");
        let app = root.join("app");
        let logs = root.join("logs");
        fs::create_dir_all(app.join("journal")).unwrap();
        fs::create_dir_all(&logs).unwrap();
        let journal = app.join("journal").join("device.jsonl");
        fs::write(&journal, "journal").unwrap();
        fs::write(app.join("host-operations.jsonl"), "host").unwrap();
        fs::write(logs.join("crash.log"), "crash").unwrap();
        fs::write(logs.join("crash.log.1"), "older").unwrap();

        let result = wipe_local_data(&app, &logs).unwrap();
        assert_eq!(result.files_removed, 3);
        assert!(result.device_journals_preserved);
        assert!(journal.exists());
        assert!(!logs.join("crash.log").exists());
        assert!(!app.join("host-operations.jsonl").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
