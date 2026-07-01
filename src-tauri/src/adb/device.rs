//! Device value types returned from the [`crate::adb::transport`] layer.

use serde::Serialize;

/// A single connected device, as seen by `adb devices -l`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Optional `transport_id:`. Stable for the life of the adb-server
    /// session.
    pub transport_id: Option<u32>,
    /// True if the device serial parses as a `host:port` (wireless)
    /// rather than a hardware serial.
    pub wireless: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
}
