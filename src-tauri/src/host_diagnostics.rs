//! Read-only host connection diagnostics.
//!
//! The doctor inspects ADB resolution/version/server state, anonymized
//! device-state counts, bounded platform USB evidence, Linux access
//! prerequisites, and ADB environment overrides. It never installs drivers,
//! changes udev or services, restarts ADB, reads ADB keys, or retains device
//! identifiers.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

#[cfg(target_os = "linux")]
use std::{fs, path::Path};

use serde::Serialize;

use crate::adb::device::DeviceState;
use crate::adb::health::AdbHealth;
use crate::adb::version_policy::{PlatformToolsAssessment, PlatformToolsStatus};
use crate::adb::{self, AdbTransport, ShellTransport};

const DEVICE_SETUP_URL: &str = "https://developer.android.com/studio/run/device";
const OEM_DRIVER_URL: &str = "https://developer.android.com/studio/run/oem-usb";
const PLATFORM_TOOLS_URL: &str = "https://developer.android.com/tools/releases/platform-tools";
const ADB_URL: &str = "https://developer.android.com/tools/adb";
const ANDROID_USB_VENDORS: &[&str] = &[
    "0502", "0bb4", "0fce", "1004", "12d1", "18d1", "19d2", "22b8", "2717", "2a45", "2a70", "2ae5",
    "2d95", "04e8",
];

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Info,
    Warning,
    Error,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostFinding {
    pub code: &'static str,
    pub severity: FindingSeverity,
    pub title: &'static str,
    pub summary: String,
    pub evidence: Vec<String>,
    pub remediation: Vec<&'static str>,
    pub official_url: &'static str,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostDoctorAdb {
    pub resolved: bool,
    pub source: adb::resolver::ResolveSource,
    pub version: Option<String>,
    pub query_succeeded: bool,
    pub client_version: Option<String>,
    pub server_version: Option<String>,
    pub compatibility: PlatformToolsAssessment,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostDoctorReport {
    pub scanned_at: String,
    pub platform: &'static str,
    pub adb: HostDoctorAdb,
    pub device_state_counts: BTreeMap<String, u32>,
    pub findings: Vec<HostFinding>,
    pub privacy: Vec<&'static str>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct UsbEvidence {
    probe_supported: bool,
    android_devices: u32,
    adb_interfaces: u32,
    linux_group_ready: Option<bool>,
    linux_udev_ready: Option<bool>,
}

#[derive(Debug, Clone)]
struct DoctorSnapshot {
    platform: &'static str,
    resolution: adb::AdbResolution,
    query_succeeded: bool,
    device_state_counts: BTreeMap<String, u32>,
    health: Option<AdbHealth>,
    usb: UsbEvidence,
    environment_overrides: Vec<&'static str>,
}

pub fn scan() -> HostDoctorReport {
    let resolution = adb::locate_adb();
    let mut query_succeeded = false;
    let mut device_state_counts = BTreeMap::new();
    let mut health = None;
    if let Some(path) = resolution.path.as_ref() {
        let transport = ShellTransport::new(path);
        if let Ok(devices) = transport.list_devices() {
            query_succeeded = true;
            for device in devices {
                *device_state_counts
                    .entry(state_key(&device.state))
                    .or_default() += 1;
            }
            health = Some(adb::health::probe(&transport, resolution.version.clone()));
        }
    }

    analyze(DoctorSnapshot {
        platform: platform_name(),
        resolution,
        query_succeeded,
        device_state_counts,
        health,
        usb: probe_usb(),
        environment_overrides: configured_adb_overrides(),
    })
}

fn analyze(snapshot: DoctorSnapshot) -> HostDoctorReport {
    let mut findings = Vec::new();
    let resolved = snapshot.resolution.path.is_some();
    let compatibility = snapshot.resolution.compatibility.clone();
    let client_version = snapshot
        .health
        .as_ref()
        .and_then(|health| health.client_version.clone());
    let server_version = snapshot
        .health
        .as_ref()
        .and_then(|health| health.server_version.clone());

    if !resolved {
        findings.push(finding(
            "adb_missing",
            FindingSeverity::Error,
            "ADB was not found",
            "No Platform Tools installation was found in PATH, Android SDK locations, or platform defaults.",
            vec!["Resolver source: not_found".to_string()],
            vec![
                "Install current Android SDK Platform Tools from the official source.",
                "Restart Droidsmith after PATH or Android SDK environment changes.",
            ],
            PLATFORM_TOOLS_URL,
        ));
    } else if !snapshot.query_succeeded {
        findings.push(finding(
            "adb_unrunnable",
            FindingSeverity::Error,
            "ADB was found but could not enumerate transports",
            "The resolved executable did not complete a read-only devices query.",
            vec![format!("Resolver source: {:?}", snapshot.resolution.source)],
            vec![
                "Run the resolved ADB version command in a terminal and inspect OS execution policy or file permissions.",
                "Replace damaged or incompatible Platform Tools with the current official release.",
            ],
            PLATFORM_TOOLS_URL,
        ));
    } else if snapshot.resolution.version.is_none() {
        findings.push(finding(
            "adb_version_unknown",
            FindingSeverity::Warning,
            "ADB version could not be verified",
            "Transport enumeration worked, but the executable did not report a recognized Platform Tools version.",
            vec![format!("Resolver source: {:?}", snapshot.resolution.source)],
            vec!["Install a current, unmodified Platform Tools release and rescan."],
            PLATFORM_TOOLS_URL,
        ));
    } else {
        findings.push(finding(
            "adb_ready",
            FindingSeverity::Info,
            "ADB executable is ready",
            "Droidsmith resolved ADB, read its version, and completed transport enumeration.",
            vec![
                format!("Resolver source: {:?}", snapshot.resolution.source),
                format!(
                    "Platform Tools version: {}",
                    snapshot.resolution.version.as_deref().unwrap_or("unknown")
                ),
                format!("Compatibility status: {:?}", compatibility.status),
            ],
            vec!["Keep Platform Tools current when Android device support changes."],
            PLATFORM_TOOLS_URL,
        ));
    }

    match compatibility.status {
        PlatformToolsStatus::Blocked if resolved => findings.push(finding(
            "platform_tools_blocked",
            FindingSeverity::Error,
            "This Platform Tools release is known-bad",
            compatibility.rationale.clone(),
            vec![format!(
                "Policy reviewed: {}",
                compatibility.policy_reviewed_on
            )],
            vec!["Replace this release with the policy's recommended official Platform Tools version before continuing."],
            PLATFORM_TOOLS_URL,
        )),
        PlatformToolsStatus::Warn if resolved => findings.push(finding(
            "platform_tools_warning",
            FindingSeverity::Warning,
            "Platform Tools compatibility needs attention",
            compatibility.rationale.clone(),
            vec![format!(
                "Policy reviewed: {}",
                compatibility.policy_reviewed_on
            )],
            vec!["Install the recommended official Platform Tools release, then rescan."],
            PLATFORM_TOOLS_URL,
        )),
        _ => {}
    }

    add_state_findings(&snapshot, &mut findings);
    add_usb_findings(&snapshot, &mut findings);

    if !snapshot.environment_overrides.is_empty() {
        findings.push(finding(
            "server_config_override",
            FindingSeverity::Warning,
            "Custom ADB server configuration is active",
            "Environment variables override ADB's default server socket, port, or vendor-key lookup and can route Droidsmith to a competing server.",
            snapshot
                .environment_overrides
                .iter()
                .map(|name| format!("{name} is set (value redacted)"))
                .collect(),
            vec![
                "Review the named environment variables in the shell that launches Droidsmith.",
                "Use the default local ADB server unless a deliberate remote-server setup is required.",
            ],
            ADB_URL,
        ));
    }

    if let (Some(client), Some(server)) = (client_version.as_deref(), server_version.as_deref()) {
        if version_core(client) != version_core(server) {
            findings.push(finding(
                "client_server_mismatch",
                FindingSeverity::Warning,
                "ADB client and server versions differ",
                "A different ADB installation may own the running server, causing feature and protocol inconsistencies.",
                vec![
                    format!("Client version: {client}"),
                    format!("Server version: {server}"),
                ],
                vec![
                    "Find duplicate ADB installations and make PATH resolve one intended Platform Tools release.",
                    "Review the running server before choosing any separate restart action.",
                ],
                PLATFORM_TOOLS_URL,
            ));
        }
    }
    if snapshot
        .health
        .as_ref()
        .is_some_and(|health| !health.server_status_supported)
    {
        findings.push(finding(
            "server_status_unavailable",
            FindingSeverity::Info,
            "ADB server details are unavailable",
            "This ADB build does not expose server-status, so server ownership and backend details could not be verified.",
            Vec::new(),
            vec!["Update Platform Tools if server diagnostics are needed."],
            PLATFORM_TOOLS_URL,
        ));
    }

    // When no device is present, surface the USB/mDNS backend toggles that
    // changed in platform-tools 37.0.1 — a common cause of "device not detected"
    // that users cannot otherwise discover.
    if snapshot.device_state_counts.is_empty() {
        let detected = detected_backend_toggles();
        let mut evidence = vec![backend_toggle_platform_hint().to_string()];
        if detected.is_empty() {
            evidence.push("No USB/mDNS backend override is currently set.".to_string());
        } else {
            for name in &detected {
                evidence.push(format!("{name} is set (value redacted)"));
            }
        }
        findings.push(finding(
            "usb_mdns_backend_toggles",
            FindingSeverity::Info,
            "USB and mDNS backend troubleshooting toggles",
            "platform-tools 37.0.1 changed the default USB and mDNS backends. If a wired device is not detected or wireless discovery fails, switching backends via these environment variables can help.",
            evidence,
            vec![
                "Set the platform toggle above, then run `adb kill-server` and restart Droidsmith so the new server picks it up.",
                "The openscreen mDNS backend was removed in 37.0.1; ADB_MDNS_OPENSCREEN is now a no-op.",
            ],
            PLATFORM_TOOLS_URL,
        ));
    }

    HostDoctorReport {
        scanned_at: crate::time::iso_utc_now(),
        platform: snapshot.platform,
        adb: HostDoctorAdb {
            resolved,
            source: snapshot.resolution.source,
            version: snapshot.resolution.version,
            query_succeeded: snapshot.query_succeeded,
            client_version,
            server_version,
            compatibility,
        },
        device_state_counts: snapshot.device_state_counts,
        findings,
        privacy: vec![
            "No driver, udev, service, or ADB server changes were made.",
            "The report contains no device serials, USB instance IDs, environment values, or ADB key material.",
            "The report remains local unless the user explicitly copies or exports it.",
        ],
    }
}

fn add_state_findings(snapshot: &DoctorSnapshot, findings: &mut Vec<HostFinding>) {
    let count = |state: &str| {
        snapshot
            .device_state_counts
            .get(state)
            .copied()
            .unwrap_or(0)
    };
    if count("unauthorized") > 0 {
        findings.push(finding(
            "unauthorized",
            FindingSeverity::Warning,
            "Android is waiting for USB debugging authorization",
            "ADB sees a transport, but the device has not trusted this host key.",
            vec![format!("Unauthorized transports: {}", count("unauthorized"))],
            vec![
                "Unlock the device and review the Allow USB debugging prompt.",
                "Verify the RSA fingerprint before accepting; reconnect the cable if no prompt is visible.",
            ],
            DEVICE_SETUP_URL,
        ));
    }
    if count("offline") > 0 {
        findings.push(finding(
            "offline",
            FindingSeverity::Warning,
            "ADB transports are offline",
            "The host server has transport records but cannot communicate with those devices.",
            vec![format!("Offline transports: {}", count("offline"))],
            vec![
                "Wake and unlock the device, then reconnect a known data-capable cable or reselect File transfer mode.",
                "Use Droidsmith's separately reviewed ADB recovery action only after physical checks.",
            ],
            DEVICE_SETUP_URL,
        ));
    }
    if count("no_permissions") > 0 {
        findings.push(finding(
            "no_permissions",
            FindingSeverity::Error,
            "Linux cannot open the Android USB device",
            "ADB detected USB hardware but the current account lacks access through group membership or udev rules.",
            vec![format!(
                "No-permissions transports: {}",
                count("no_permissions")
            )],
            vec![
                "Follow the official Linux device setup to join the appropriate group and install vendor udev rules.",
                "Log out and back in after group changes, then reconnect the device.",
            ],
            DEVICE_SETUP_URL,
        ));
    }
}

fn add_usb_findings(snapshot: &DoctorSnapshot, findings: &mut Vec<HostFinding>) {
    let adb_transports: u32 = snapshot.device_state_counts.values().sum();
    if snapshot.usb.android_devices > 0 && adb_transports == 0 {
        findings.push(finding(
            "usb_visible_adb_missing",
            FindingSeverity::Warning,
            "Android USB hardware is visible but absent from ADB",
            "The OS sees Android-like USB hardware while ADB reports no transport. A charge-only/wrong USB mode, cable, or host driver is likely.",
            vec![format!(
                "Android-like USB devices visible to host: {}",
                snapshot.usb.android_devices
            )],
            vec![
                "Unlock the device, enable USB debugging, and select File transfer or another debugging-capable USB mode.",
                "Try a known data-capable cable and a direct host USB port.",
            ],
            DEVICE_SETUP_URL,
        ));
    } else if snapshot.usb.probe_supported
        && snapshot.usb.android_devices == 0
        && adb_transports == 0
    {
        findings.push(finding(
            "no_usb_transport",
            FindingSeverity::Info,
            "No Android USB transport was detected",
            "Neither ADB nor the bounded host USB probe found recognizable Android hardware.",
            Vec::new(),
            vec![
                "Confirm the device is powered, unlocked, and connected with a data-capable cable.",
                "Select File transfer or another debugging-capable USB mode before rescanning.",
            ],
            DEVICE_SETUP_URL,
        ));
    }

    if snapshot.platform == "windows"
        && snapshot.usb.android_devices > 0
        && snapshot.usb.adb_interfaces == 0
    {
        findings.push(finding(
            "windows_driver_missing",
            FindingSeverity::Warning,
            "Windows has no recognized ADB interface",
            "Android-like USB hardware is present, but the host inventory exposes no ADB interface. The OEM USB driver may be missing or bound incorrectly.",
            vec![format!(
                "Recognized ADB interfaces: {}",
                snapshot.usb.adb_interfaces
            )],
            vec![
                "Install the OEM's official Windows USB driver or Google's driver for supported devices.",
                "Review Device Manager for an Android device with a warning icon; avoid third-party driver bundles.",
            ],
            OEM_DRIVER_URL,
        ));
    }

    if snapshot.platform == "linux"
        && (snapshot.usb.android_devices > 0
            || snapshot.device_state_counts.contains_key("no_permissions"))
        && (snapshot.usb.linux_group_ready == Some(false)
            || snapshot.usb.linux_udev_ready == Some(false))
    {
        let mut evidence = Vec::new();
        if let Some(ready) = snapshot.usb.linux_group_ready {
            evidence.push(format!("Android USB access group present: {ready}"));
        }
        if let Some(ready) = snapshot.usb.linux_udev_ready {
            evidence.push(format!("Android/vendor udev rule found: {ready}"));
        }
        findings.push(finding(
            "linux_access_incomplete",
            FindingSeverity::Warning,
            "Linux Android USB access is incomplete",
            "The current account or udev configuration is missing a common prerequisite for non-root ADB access.",
            evidence,
            vec![
                "Use the distribution's Android udev-rules package or official vendor-rule guidance.",
                "Join the recommended device-access group and re-login; avoid world-writable 0666 rules.",
            ],
            DEVICE_SETUP_URL,
        ));
    }
}

fn finding(
    code: &'static str,
    severity: FindingSeverity,
    title: &'static str,
    summary: impl Into<String>,
    evidence: Vec<String>,
    remediation: Vec<&'static str>,
    official_url: &'static str,
) -> HostFinding {
    HostFinding {
        code,
        severity,
        title,
        summary: summary.into(),
        evidence,
        remediation,
        official_url,
    }
}

fn state_key(state: &DeviceState) -> String {
    match state {
        DeviceState::Device => "device",
        DeviceState::Unauthorized => "unauthorized",
        DeviceState::Offline => "offline",
        DeviceState::Recovery => "recovery",
        DeviceState::Bootloader => "bootloader",
        DeviceState::Sideload => "sideload",
        DeviceState::NoPermissions => "no_permissions",
        DeviceState::Other(_) => "other",
    }
    .to_string()
}

fn version_core(version: &str) -> &str {
    version
        .trim()
        .trim_start_matches("Android Debug Bridge version ")
        .split(['-', ' '])
        .next()
        .unwrap_or(version)
}

fn configured_adb_overrides() -> Vec<&'static str> {
    [
        "ADB_SERVER_SOCKET",
        "ANDROID_ADB_SERVER_PORT",
        "ADB_VENDOR_KEYS",
    ]
    .into_iter()
    .filter(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
    .collect()
}

/// USB/mDNS backend troubleshooting toggles (platform-tools 37.0.1) that are
/// currently set in Droidsmith's environment.
fn detected_backend_toggles() -> Vec<&'static str> {
    backend_toggle_names()
        .into_iter()
        .filter(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
        .collect()
}

const fn backend_toggle_names() -> [&'static str; 4] {
    [
        "ADB_USB_LEGACY",
        "ADB_LIBUSB",
        "ADB_MDNS_OPENSCREEN",
        "ADB_MDNS",
    ]
}

const fn backend_toggle_platform_hint() -> &'static str {
    if cfg!(windows) {
        "On Windows, set ADB_USB_LEGACY=1 to fall back to the legacy USB backend if a device is not detected."
    } else if cfg!(target_os = "macos") {
        "On macOS, set ADB_LIBUSB=1 to re-enable the libusb backend if a device is not detected."
    } else {
        "On Linux, ADB_USB_LEGACY or ADB_LIBUSB switch the USB backend if a device is not detected."
    }
}

fn platform_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "other"
    }
}

#[cfg(windows)]
fn probe_usb() -> UsbEvidence {
    let Some(output) = run_probe(
        "pnputil.exe",
        &["/enum-devices", "/connected"],
        Duration::from_secs(5),
    ) else {
        return UsbEvidence::default();
    };
    parse_windows_usb(&output)
}

#[cfg(not(windows))]
fn probe_usb() -> UsbEvidence {
    #[cfg(target_os = "linux")]
    {
        return UsbEvidence {
            probe_supported: true,
            android_devices: linux_android_usb_count(),
            adb_interfaces: 0,
            linux_group_ready: linux_group_ready(),
            linux_udev_ready: Some(linux_udev_ready()),
        };
    }
    #[cfg(not(target_os = "linux"))]
    UsbEvidence::default()
}

fn parse_windows_usb(output: &str) -> UsbEvidence {
    let android_devices = output
        .lines()
        .filter(|line| {
            let line = line.to_ascii_lowercase();
            ANDROID_USB_VENDORS
                .iter()
                .any(|vendor| line.contains(&format!("vid_{vendor}")))
        })
        .count() as u32;
    let adb_interfaces = output
        .to_ascii_lowercase()
        .lines()
        .filter(|line| {
            line.contains("adb interface")
                || line.contains("android adb")
                || line.contains("androidusbdeviceclass")
                || line.contains("3f966bd9-fa04-4ec5-991c-d326973b5128")
        })
        .count() as u32;
    UsbEvidence {
        probe_supported: true,
        android_devices,
        adb_interfaces,
        linux_group_ready: None,
        linux_udev_ready: None,
    }
}

#[cfg(target_os = "linux")]
fn linux_android_usb_count() -> u32 {
    let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path().join("idVendor")).ok())
        .filter(|vendor| ANDROID_USB_VENDORS.contains(&vendor.trim().to_ascii_lowercase().as_str()))
        .count() as u32
}

#[cfg(target_os = "linux")]
fn linux_group_ready() -> Option<bool> {
    run_probe("id", &["-nG"], Duration::from_secs(2)).map(|groups| {
        groups
            .split_whitespace()
            .any(|group| matches!(group, "plugdev" | "adbusers" | "uucp"))
    })
}

#[cfg(target_os = "linux")]
fn linux_udev_ready() -> bool {
    [
        "/etc/udev/rules.d",
        "/lib/udev/rules.d",
        "/usr/lib/udev/rules.d",
    ]
    .into_iter()
    .any(|directory| directory_has_android_rule(Path::new(directory)))
}

#[cfg(target_os = "linux")]
fn directory_has_android_rule(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "rules")
        })
        .take(256)
        .any(|entry| {
            fs::metadata(entry.path())
                .ok()
                .filter(|metadata| metadata.len() <= 512 * 1024)
                .and_then(|_| fs::read_to_string(entry.path()).ok())
                .is_some_and(|contents| {
                    let lower = contents.to_ascii_lowercase();
                    lower.contains("android")
                        || ANDROID_USB_VENDORS
                            .iter()
                            .any(|vendor| lower.contains(vendor))
                })
        })
}

/// Return only a count of likely active tunnel/VPN interfaces. Adapter names
/// and descriptions are intentionally discarded so wireless failure
/// diagnostics remain copyable without exposing host network identifiers.
#[cfg(target_os = "windows")]
pub(crate) fn active_vpn_interface_count() -> u32 {
    run_probe(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-NetAdapter -IncludeHidden -ErrorAction SilentlyContinue | Where-Object Status -eq 'Up' | Select-Object -ExpandProperty Name",
        ],
        Duration::from_secs(2),
    )
    .map_or(0, |output| count_vpn_interfaces(&output))
}

#[cfg(target_os = "linux")]
pub(crate) fn active_vpn_interface_count() -> u32 {
    fs::read_dir("/sys/class/net")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let state = fs::read_to_string(entry.path().join("operstate")).unwrap_or_default();
            matches!(state.trim(), "up" | "unknown") && looks_like_vpn_interface(&name)
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

#[cfg(target_os = "macos")]
pub(crate) fn active_vpn_interface_count() -> u32 {
    run_probe("ifconfig", &["-l"], Duration::from_secs(2)).map_or(0, |output| {
        output
            .split_whitespace()
            .filter(|value| looks_like_vpn_interface(value))
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub(crate) const fn active_vpn_interface_count() -> u32 {
    0
}

fn count_vpn_interfaces(output: &str) -> u32 {
    output
        .lines()
        .filter(|line| line.split_whitespace().any(looks_like_vpn_interface))
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

fn looks_like_vpn_interface(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let compact: String = lower
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();
    lower.starts_with("utun")
        || lower.starts_with("tun")
        || lower.starts_with("tap")
        || lower.starts_with("wg")
        || lower.starts_with("ppp")
        || lower.starts_with("ipsec")
        || [
            "vpn",
            "wireguard",
            "wintun",
            "tailscale",
            "zerotier",
            "nordlynx",
            "anyconnect",
            "globalprotect",
            "fortinet",
            "hamachi",
        ]
        .iter()
        .any(|marker| compact.contains(marker))
}

fn run_probe(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    let output = crate::process_capture::run(
        &mut command,
        timeout,
        crate::process_capture::CaptureLimits::default(),
    )
    .ok()?;
    match output.termination {
        crate::process_capture::CaptureTermination::Exited(status) if status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::resolver::ResolveSource;

    fn snapshot(platform: &'static str) -> DoctorSnapshot {
        DoctorSnapshot {
            platform,
            resolution: adb::AdbResolution {
                path: Some("/fixture/adb".to_string()),
                source: ResolveSource::Path,
                version: Some("37.0.0".to_string()),
                compatibility: crate::adb::version_policy::assess(Some("37.0.0")),
            },
            query_succeeded: true,
            device_state_counts: BTreeMap::new(),
            health: Some(AdbHealth {
                client_version: Some("37.0.0".to_string()),
                server_version: Some("37.0.0".to_string()),
                server_status_supported: true,
                ..AdbHealth::default()
            }),
            usb: UsbEvidence::default(),
            environment_overrides: Vec::new(),
        }
    }

    #[test]
    fn missing_adb_is_an_actionable_root_finding() {
        let mut input = snapshot("windows");
        input.resolution = adb::AdbResolution {
            path: None,
            source: ResolveSource::NotFound,
            version: None,
            compatibility: crate::adb::version_policy::assess(None),
        };
        input.query_succeeded = false;
        let report = analyze(input);
        assert_eq!(report.findings[0].code, "adb_missing");
        assert_eq!(report.findings[0].severity, FindingSeverity::Error);
        assert!(report.findings[0]
            .official_url
            .starts_with("https://developer.android.com/"));
    }

    #[test]
    fn windows_usb_without_adb_yields_mode_and_driver_findings() {
        let mut input = snapshot("windows");
        input.usb = UsbEvidence {
            probe_supported: true,
            android_devices: 1,
            adb_interfaces: 0,
            ..UsbEvidence::default()
        };
        let report = analyze(input);
        let codes: Vec<_> = report.findings.iter().map(|finding| finding.code).collect();
        assert!(codes.contains(&"usb_visible_adb_missing"));
        assert!(codes.contains(&"windows_driver_missing"));
    }

    #[test]
    fn device_state_and_linux_access_are_normalized_without_serials() {
        let mut input = snapshot("linux");
        input
            .device_state_counts
            .insert("unauthorized".to_string(), 1);
        input
            .device_state_counts
            .insert("no_permissions".to_string(), 2);
        input.usb = UsbEvidence {
            probe_supported: true,
            android_devices: 2,
            linux_group_ready: Some(false),
            linux_udev_ready: Some(false),
            ..UsbEvidence::default()
        };
        let serialized = serde_json::to_string(&analyze(input)).unwrap();
        assert!(serialized.contains("unauthorized"));
        assert!(serialized.contains("linux_access_incomplete"));
        assert!(!serialized.contains("R58M-SECRET-SERIAL"));
        assert!(!serialized.contains("ADB_VENDOR_KEYS="));
    }

    #[test]
    fn server_overrides_and_version_mismatch_are_explicit_and_redacted() {
        let mut input = snapshot("windows");
        input.environment_overrides = vec!["ADB_SERVER_SOCKET"];
        input.health.as_mut().unwrap().server_version = Some("36.0.0".to_string());
        let serialized = serde_json::to_string(&analyze(input)).unwrap();
        assert!(serialized.contains("server_config_override"));
        assert!(serialized.contains("client_server_mismatch"));
        assert!(serialized.contains("value redacted"));
    }

    #[test]
    fn backend_toggle_advice_appears_only_without_devices() {
        // No device present: surface the USB/mDNS backend troubleshooting toggles.
        let empty = serde_json::to_string(&analyze(snapshot("windows"))).unwrap();
        assert!(empty.contains("usb_mdns_backend_toggles"));
        assert!(empty.contains("ADB_USB_LEGACY"));

        // A device is present: the guidance is omitted as unneeded.
        let mut with_device = snapshot("windows");
        with_device
            .device_state_counts
            .insert("device".to_string(), 1);
        let json = serde_json::to_string(&analyze(with_device)).unwrap();
        assert!(!json.contains("usb_mdns_backend_toggles"));
    }

    #[test]
    fn windows_usb_parser_counts_known_vendors_and_adb_interfaces() {
        let evidence = parse_windows_usb(
            "Instance ID: USB\\VID_18D1&PID_4EE7\\SERIAL\nDevice Description: Android Composite ADB Interface\nInstance ID: USB\\VID_04E8&PID_6860\\OTHER\n",
        );
        assert_eq!(evidence.android_devices, 2);
        assert_eq!(evidence.adb_interfaces, 1);
    }

    #[test]
    fn vpn_interface_detection_counts_tunnels_without_matching_ethernet() {
        let output = "Ethernet\nWi-Fi\nWireGuard Tunnel\nutun4\nTailscale\n";
        assert_eq!(count_vpn_interfaces(output), 3);
        assert!(!looks_like_vpn_interface("Ethernet"));
        assert!(!looks_like_vpn_interface("Bluetooth Network Connection"));
    }
}
