//! Device value types returned from the [`crate::adb::transport`] layer.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A single connected device, as seen by `adb devices -l`.
#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    /// Hardware serial or `host:port` for TCP/wireless devices. Stable
    /// per device; survives reboots over USB.
    pub serial: String,
    pub state: DeviceState,
    /// Optional `model:` field from `adb devices -l`. Not all devices /
    /// states populate it.
    pub model: Option<String>,
    /// Optional `product:` field. Same caveat as `model`.
    pub product: Option<String>,
    /// Optional `device:` field — the kernel device codename.
    pub device: Option<String>,
    /// Build fingerprint captured while this connection target is prepared.
    pub build_fingerprint: Option<String>,
    /// Optional `transport_id:`. Stable for the life of the adb-server
    /// session.
    pub transport_id: Option<u32>,
    /// Process-local generation assigned when this exact ADB transport
    /// first appears. A disconnect observed by Droidsmith retires the
    /// generation, so a later reconnect cannot reuse a stale UI target.
    pub connection_generation: u64,
    /// Trust classification derived from USB identity or process-local
    /// wireless connection provenance. An address alone is never enough to
    /// claim Android 11+ TLS Wireless Debugging.
    pub transport_kind: DeviceTransportKind,
    /// True if the device serial parses as a `host:port` (wireless)
    /// rather than a hardware serial.
    pub wireless: bool,
}

#[derive(
    specta::Type, Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DeviceTransportKind {
    Usb,
    TlsWifi,
    LegacyTcp,
    #[default]
    UnknownTcp,
}

impl DeviceTransportKind {
    pub fn requires_override(self) -> bool {
        matches!(self, Self::LegacyTcp | Self::UnknownTcp)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::TlsWifi => "paired TLS Wi-Fi",
            Self::LegacyTcp => "legacy TCP",
            Self::UnknownTcp => "unknown TCP",
        }
    }
}

/// Immutable device identity captured by the renderer before an operation.
///
/// The serial remains useful for display/journal partitioning, but execution
/// prefers `transport_id` and verifies this generation plus the advertised
/// metadata immediately before talking to the device.
#[derive(specta::Type, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceTarget {
    pub serial: String,
    pub transport_id: Option<u32>,
    pub connection_generation: u64,
    pub model: Option<String>,
    pub product: Option<String>,
    pub device: Option<String>,
    pub build_fingerprint: Option<String>,
    #[serde(default)]
    pub transport_kind: DeviceTransportKind,
    /// Explicit renderer acknowledgement for one selected connection. This is
    /// authorization metadata, not identity, and is checked only after the
    /// live target's independently-derived transport kind is revalidated.
    #[serde(default)]
    pub untrusted_transport_override: bool,
}

impl Device {
    pub fn target(&self) -> DeviceTarget {
        DeviceTarget {
            serial: self.serial.clone(),
            transport_id: self.transport_id,
            connection_generation: self.connection_generation,
            model: self.model.clone(),
            product: self.product.clone(),
            device: self.device.clone(),
            build_fingerprint: self.build_fingerprint.clone(),
            transport_kind: self.transport_kind,
            untrusted_transport_override: false,
        }
    }
}

impl DeviceTarget {
    /// Global ADB selector arguments. Transport ids disambiguate duplicate
    /// hardware serials; old ADB builds without an id fall back to `-s` only
    /// after validation has proved the serial is unique.
    pub fn adb_selector(&self) -> Vec<String> {
        match self.transport_id {
            Some(id) => vec!["-t".to_string(), id.to_string()],
            None => vec!["-s".to_string(), self.serial.clone()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DeviceKey {
    serial: String,
    transport_id: Option<u32>,
    model: Option<String>,
    product: Option<String>,
    device: Option<String>,
    build_fingerprint: Option<String>,
    transport_kind: DeviceTransportKind,
}

impl From<&Device> for DeviceKey {
    fn from(value: &Device) -> Self {
        Self {
            serial: value.serial.clone(),
            transport_id: value.transport_id,
            model: value.model.clone(),
            product: value.product.clone(),
            device: value.device.clone(),
            build_fingerprint: value.build_fingerprint.clone(),
            transport_kind: value.transport_kind,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct WirelessProvenance {
    kind: DeviceTransportKind,
    recorded_at: Instant,
    observed_live: bool,
}

fn wireless_provenance() -> &'static Mutex<HashMap<String, WirelessProvenance>> {
    static PROVENANCE: OnceLock<Mutex<HashMap<String, WirelessProvenance>>> = OnceLock::new();
    PROVENANCE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record the protocol provenance of a successful `adb connect`. Only the
/// backend wireless module calls this after matching mDNS TLS discovery or an
/// explicit legacy-mode request.
pub fn remember_wireless_transport(serial: &str, kind: DeviceTransportKind) {
    if !looks_wireless(serial) || kind == DeviceTransportKind::Usb {
        return;
    }
    let Ok(mut known) = wireless_provenance().lock() else {
        return;
    };
    known.insert(
        serial.to_string(),
        WirelessProvenance {
            kind,
            recorded_at: Instant::now(),
            observed_live: false,
        },
    );
}

/// Attach process-local protocol provenance and retire disconnected endpoints.
/// Unknown host:port serials remain `unknown_tcp`; TLS is never inferred from
/// an address, port number, or successful ADB shell alone.
pub fn attach_transport_provenance(devices: &mut [Device]) {
    let live_wireless = devices
        .iter()
        .filter(|device| looks_wireless(&device.serial))
        .map(|device| device.serial.clone())
        .collect::<HashSet<_>>();
    let Ok(mut known) = wireless_provenance().lock() else {
        for device in devices {
            device.transport_kind = if looks_wireless(&device.serial) {
                DeviceTransportKind::UnknownTcp
            } else {
                DeviceTransportKind::Usb
            };
            device.wireless = device.transport_kind != DeviceTransportKind::Usb;
        }
        return;
    };
    // ADB may report `connected to` just before the endpoint appears in a
    // devices snapshot. Keep a short-lived unobserved claim across that race,
    // but retire observed disconnects immediately and never persist trust
    // across a later reconnect.
    known.retain(|serial, provenance| {
        live_wireless.contains(serial)
            || (!provenance.observed_live
                && provenance.recorded_at.elapsed() < Duration::from_secs(30))
    });
    for device in devices {
        device.transport_kind = if looks_wireless(&device.serial) {
            known
                .get_mut(&device.serial)
                .map(|provenance| {
                    provenance.observed_live = true;
                    provenance.kind
                })
                .unwrap_or(DeviceTransportKind::UnknownTcp)
        } else {
            DeviceTransportKind::Usb
        };
        device.wireless = device.transport_kind != DeviceTransportKind::Usb;
    }
}

fn generations() -> &'static Mutex<HashMap<DeviceKey, u64>> {
    static GENERATIONS: OnceLock<Mutex<HashMap<DeviceKey, u64>>> = OnceLock::new();
    GENERATIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Attach process-local connection generations and retire identities that are
/// absent from the latest complete `adb devices -l` snapshot.
pub fn observe_connection_generations(devices: &mut [Device]) {
    let keys: HashSet<DeviceKey> = devices
        .iter()
        .filter(|device| device.transport_id.is_none())
        .map(DeviceKey::from)
        .collect();
    let Ok(mut known) = generations().lock() else {
        // Poisoning must fail closed: generation zero is never executable.
        return;
    };
    known.retain(|key, _| keys.contains(key));
    for device in devices {
        if let Some(transport_id) = device.transport_id {
            // ADB allocates this id per live transport. Offset by one so a
            // valid transport id of zero can never collide with the invalid
            // generation sentinel.
            device.connection_generation = u64::from(transport_id) + 1;
            continue;
        }
        let key = DeviceKey::from(&*device);
        let generation = *known.entry(key).or_insert_with(next_generation);
        device.connection_generation = generation;
    }
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    /// `device` — fully authorized.
    Device,
    /// `unauthorized` — user hasn't tapped "Allow USB debugging" yet.
    Unauthorized,
    /// `offline` — adb-server lost contact (cable dropout, sleep).
    Offline,
    /// `recovery` — booted into recovery mode.
    Recovery,
    /// `bootloader` — in fastboot/bootloader. Not common from
    /// `adb devices` but documented.
    Bootloader,
    /// `sideload` — sideload mode.
    Sideload,
    /// `no permissions` — udev rules missing on Linux. We surface this
    /// distinctly so the UI can show a fix-it tip.
    NoPermissions,
    /// Anything we don't recognise. We keep the raw string so support
    /// requests can include it without losing fidelity.
    Other(String),
}

impl DeviceState {
    pub fn parse(raw: &str) -> Self {
        // `adb devices` outputs the state as a single token. The "no
        // permissions" case is actually two tokens but we collapse
        // before the call site.
        match raw {
            "device" => Self::Device,
            "unauthorized" => Self::Unauthorized,
            "offline" => Self::Offline,
            "recovery" => Self::Recovery,
            "bootloader" => Self::Bootloader,
            "sideload" => Self::Sideload,
            // Some adb builds print `no permissions (user in plugdev group; are your udev rules wrong?); see [http://...]`.
            // We collapse the whole tail.
            s if s.starts_with("no permissions") => Self::NoPermissions,
            other => Self::Other(other.to_string()),
        }
    }

    /// True if the device is in a state where ADB shell calls will
    /// succeed. Used by the upcoming Devices route to gate the action
    /// buttons; kept ahead of UI plumbing so the rule lives next to
    /// the state enum.
    #[allow(dead_code)]
    pub fn is_actionable(&self) -> bool {
        matches!(self, Self::Device)
    }
}

/// True if `s` looks like `host:port` rather than a hardware serial.
/// Examples:
///   - `192.168.1.42:5555` → true
///   - `emulator-5554` → false
///   - `R5CT60ZQR4M` → false
pub fn looks_wireless(s: &str) -> bool {
    // Quick filter: must contain ':' followed by digits, and the
    // pre-colon portion must look like an IPv4/IPv6 address or a host
    // name (we don't try to fully validate; the worst case is a false
    // positive labelling).
    let Some((host, port)) = s.rsplit_once(':') else {
        return false;
    };
    if !port.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    !host.is_empty() && (host.contains('.') || host.contains(':') || host == "localhost")
}

/// Conservative validator for ADB serials accepted over IPC/CLI. The
/// serial is passed as an argv element, not through a shell, but keeping
/// it narrow prevents path-like, whitespace, and control-character
/// values from reaching ADB or journal filenames.
pub fn valid_serial(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_state_parse_known() {
        assert_eq!(DeviceState::parse("device"), DeviceState::Device);
        assert_eq!(
            DeviceState::parse("unauthorized"),
            DeviceState::Unauthorized
        );
        assert_eq!(DeviceState::parse("offline"), DeviceState::Offline);
        assert_eq!(DeviceState::parse("recovery"), DeviceState::Recovery);
        assert_eq!(DeviceState::parse("bootloader"), DeviceState::Bootloader);
        assert_eq!(DeviceState::parse("sideload"), DeviceState::Sideload);
    }

    #[test]
    fn device_state_no_permissions_collapses_tail() {
        assert_eq!(
            DeviceState::parse("no permissions (verify udev rules)"),
            DeviceState::NoPermissions
        );
    }

    #[test]
    fn device_state_unknown_preserves_raw() {
        match DeviceState::parse("brand-new-state-2030") {
            DeviceState::Other(s) => assert_eq!(s, "brand-new-state-2030"),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn is_actionable_only_device() {
        assert!(DeviceState::Device.is_actionable());
        assert!(!DeviceState::Unauthorized.is_actionable());
        assert!(!DeviceState::Offline.is_actionable());
    }

    #[test]
    fn looks_wireless_classifies_correctly() {
        assert!(looks_wireless("192.168.1.42:5555"));
        assert!(looks_wireless("localhost:5555"));
        assert!(looks_wireless("device.local:5037"));
        assert!(!looks_wireless("emulator-5554"));
        assert!(!looks_wireless("R5CT60ZQR4M"));
        assert!(!looks_wireless("host:notnum"));
        assert!(!looks_wireless(""));
        assert!(!looks_wireless(":5555")); // no host
    }

    #[test]
    fn valid_serial_rejects_path_like_or_empty_values() {
        assert!(valid_serial("R5CT60ZQR4M"));
        assert!(valid_serial("emulator-5554"));
        assert!(valid_serial("192.168.1.42:5555"));
        assert!(valid_serial("device.local:5037"));
        assert!(!valid_serial(""));
        assert!(!valid_serial("../journal"));
        assert!(!valid_serial("serial with spaces"));
        assert!(!valid_serial("serial/with/slash"));
        assert!(!valid_serial(&"a".repeat(257)));
        assert!(!valid_serial("-e"));
        assert!(!valid_serial("--help"));
    }

    #[test]
    fn provenance_is_explicit_and_retired_after_disconnect() {
        fn device(serial: &str) -> Device {
            Device {
                serial: serial.to_string(),
                state: DeviceState::Device,
                model: None,
                product: None,
                device: None,
                build_fingerprint: None,
                transport_id: Some(99),
                connection_generation: 0,
                transport_kind: DeviceTransportKind::UnknownTcp,
                wireless: false,
            }
        }

        let endpoint = "imp44-test.local:38899";
        remember_wireless_transport(endpoint, DeviceTransportKind::TlsWifi);
        let mut first = vec![device(endpoint)];
        attach_transport_provenance(&mut first);
        assert_eq!(first[0].transport_kind, DeviceTransportKind::TlsWifi);
        assert!(first[0].wireless);

        attach_transport_provenance(&mut []);
        let mut reconnected = vec![device(endpoint)];
        attach_transport_provenance(&mut reconnected);
        assert_eq!(
            reconnected[0].transport_kind,
            DeviceTransportKind::UnknownTcp
        );

        let mut usb = vec![device("R5CT60ZQR4M")];
        attach_transport_provenance(&mut usb);
        assert_eq!(usb[0].transport_kind, DeviceTransportKind::Usb);
        assert!(!usb[0].wireless);
    }
}
