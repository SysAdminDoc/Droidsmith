//! ADB server and wireless-discovery health probes.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::adb::transport::ShellTransport;
use crate::adb::version_policy::{self, PlatformToolsAssessment};

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AdbHealth {
    pub server_status_supported: bool,
    pub client_version: Option<String>,
    pub server_version: Option<String>,
    pub server_build: Option<String>,
    pub usb_backend: Option<String>,
    pub mdns_backend: Option<String>,
    pub mdns_enabled: Option<bool>,
    pub mdns_check: Option<String>,
    pub burst_mode: Option<bool>,
    pub recommended_for_wifi_v2: bool,
    pub wifi_v2_state: WifiV2State,
    pub wifi_v2_devices: Vec<String>,
    pub warning: Option<String>,
    pub platform_tools: PlatformToolsAssessment,
}

#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiV2State {
    Supported,
    NotDetected,
    #[default]
    ProbeUnavailable,
}

pub fn probe(transport: &ShellTransport, client_version: Option<String>) -> AdbHealth {
    let mut health = AdbHealth {
        platform_tools: version_policy::assess(client_version.as_deref()),
        client_version,
        ..AdbHealth::default()
    };

    match transport.adb(&["server-status"]) {
        Ok(output) => {
            health.server_status_supported = true;
            health.server_version = field(&output, "version");
            health.server_build = field(&output, "build");
            health.usb_backend = field(&output, "usb_backend");
            health.mdns_backend = field(&output, "mdns_backend");
            health.mdns_enabled = bool_field(&output, "mdns_enabled");
            health.burst_mode = bool_field(&output, "burst_mode");
        }
        Err(error) => {
            health.warning = Some(format!("ADB server-status unavailable: {error}"));
        }
    }

    health.recommended_for_wifi_v2 = health
        .server_version
        .as_deref()
        .is_some_and(version_policy::is_recommended);
    health.mdns_check = transport
        .adb(&["mdns", "check"])
        .ok()
        .and_then(|output| first_nonempty_line(&output));

    if health.recommended_for_wifi_v2 && health.mdns_enabled != Some(false) {
        match probe_wifi_v2(&transport.adb_path) {
            Some(devices) if !devices.is_empty() => {
                health.wifi_v2_state = WifiV2State::Supported;
                health.wifi_v2_devices = devices;
            }
            Some(_) => health.wifi_v2_state = WifiV2State::NotDetected,
            None => health.wifi_v2_state = WifiV2State::ProbeUnavailable,
        }
    }

    if health.warning.is_none() {
        if !health.recommended_for_wifi_v2 {
            health.warning = Some(format!(
                "Platform Tools {} or newer is required for ADB Wi-Fi 2.0 diagnostics",
                health.platform_tools.recommended_version
            ));
        } else if health.mdns_enabled == Some(false) {
            health.warning = Some("ADB mDNS discovery is disabled".to_string());
        } else if health
            .mdns_backend
            .as_deref()
            .is_some_and(|backend| backend.eq_ignore_ascii_case("OPENSCREEN"))
        {
            health.warning = Some(
                "ADB is using the legacy Openscreen mDNS backend; libadbmdns is recommended"
                    .to_string(),
            );
        }
    }

    health
}

fn field(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (key, value) = line.trim().split_once(':')?;
        if key.trim() != name {
            return None;
        }
        let value = value.trim().trim_matches('"');
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn bool_field(output: &str, name: &str) -> Option<bool> {
    match field(output, name)?.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn first_nonempty_line(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(240).collect())
}

fn version_at_least(version: &str, required_major: u32) -> bool {
    version
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= required_major)
}

/// Read one short protobuf-text discovery window. The command is intentionally
/// killed after 900 ms because `mdns track-services` is a live stream.
fn probe_wifi_v2(adb_path: &Path) -> Option<Vec<String>> {
    let mut command = Command::new(adb_path);
    command
        .args(["mdns", "track-services", "--proto-text"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::process_tree::configure(&mut command);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(900) {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = crate::process_tree::terminate(&mut child);
    let output = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
    Some(parse_wifi_v2_services(&output))
}

fn parse_wifi_v2_services(output: &str) -> Vec<String> {
    let mut devices = Vec::new();
    let mut serial: Option<String> = None;
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;

    for line in output.lines().map(str::trim) {
        if line == "service {" {
            serial = None;
            name = None;
            version = None;
            continue;
        }
        if line == "}" {
            if version
                .as_deref()
                .is_some_and(|value| version_at_least(value, 2))
            {
                if let Some(identifier) = name.clone().or_else(|| serial.clone()) {
                    if !devices.contains(&identifier) {
                        devices.push(identifier);
                    }
                }
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key {
            "serial" => serial = Some(value),
            "given_name" | "product_model" if name.is_none() => name = Some(value),
            "mdns_service_version" => version = Some(value),
            _ => {}
        }
    }
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"
usb_backend: NATIVE
mdns_backend: LIBADBMDNS
version: "37.0.0"
build: "123456"
burst_mode: true
mdns_enabled: true
"#;

    #[test]
    fn parses_server_status_fields_without_host_paths() {
        assert_eq!(field(STATUS, "version").as_deref(), Some("37.0.0"));
        assert_eq!(field(STATUS, "mdns_backend").as_deref(), Some("LIBADBMDNS"));
        assert_eq!(bool_field(STATUS, "mdns_enabled"), Some(true));
        assert!(version_at_least("2.0", 2));
        assert!(!version_at_least("1.0", 2));
        assert_eq!(
            version_policy::assess(Some("37.0.0")).status,
            crate::adb::version_policy::PlatformToolsStatus::Supported
        );
    }

    #[test]
    fn identifies_wifi_two_services_from_proto_text() {
        let output = r#"
tls {
  service {
    product_model: "Pixel 10"
    serial: "ABC123"
    mdns_service_version: "2.0"
  }
}
tls {
  service {
    given_name: "Old phone"
    mdns_service_version: "1.0"
  }
}
"#;
        assert_eq!(parse_wifi_v2_services(output), vec!["Pixel 10"]);
    }
}
