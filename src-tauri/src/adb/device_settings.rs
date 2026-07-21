//! Curated `adb shell settings get/put` editor (R-082).
//!
//! Android exposes a large, mostly-undocumented key/value store split across
//! the `system`, `secure`, and `global` namespaces. Rather than let the user
//! poke arbitrary keys (which can brick a device), Droidsmith ships a small
//! allow-list of well-understood, reversible settings — animation scales,
//! display timeout, font scale, and stay-awake — and validates every write
//! against that catalog before shelling out.

use serde::{Deserialize, Serialize};

use crate::adb::device::DeviceTarget;
use crate::adb::transport::{AdbTransport, TransportError};

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingNamespace {
    System,
    Secure,
    Global,
}

impl SettingNamespace {
    fn as_arg(self) -> &'static str {
        match self {
            SettingNamespace::System => "system",
            SettingNamespace::Secure => "secure",
            SettingNamespace::Global => "global",
        }
    }
}

/// How a value is validated and presented. Kept intentionally narrow so the
/// renderer can render an appropriate control and the backend can reject
/// out-of-range writes.
#[derive(specta::Type, Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingControl {
    /// Bounded decimal (e.g. animation scales 0.0–10.0).
    Float { min: f32, max: f32 },
    /// Bounded integer (e.g. screen-off timeout in milliseconds).
    Int { min: i64, max: i64 },
    /// A fixed set of allowed integer values with human labels.
    Choice { options: Vec<SettingChoice> },
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct SettingChoice {
    pub value: String,
    pub label: String,
}

/// A catalog entry. The type is public so the read/write helpers can hand one
/// back across the module boundary, but its fields stay private — callers must
/// go through [`validate_write`] to obtain one.
pub struct SettingSpec {
    id: &'static str,
    namespace: SettingNamespace,
    key: &'static str,
    control: ControlSpec,
}

enum ControlSpec {
    Float { min: f32, max: f32 },
    Int { min: i64, max: i64 },
    Choice(&'static [(&'static str, &'static str)]),
}

/// The allow-list. `id` is the stable renderer-facing identifier; `key` is the
/// on-device settings key. Every entry is reversible with a single `put`.
const CATALOG: &[SettingSpec] = &[
    SettingSpec {
        id: "window_animation_scale",
        namespace: SettingNamespace::Global,
        key: "window_animation_scale",
        control: ControlSpec::Float {
            min: 0.0,
            max: 10.0,
        },
    },
    SettingSpec {
        id: "transition_animation_scale",
        namespace: SettingNamespace::Global,
        key: "transition_animation_scale",
        control: ControlSpec::Float {
            min: 0.0,
            max: 10.0,
        },
    },
    SettingSpec {
        id: "animator_duration_scale",
        namespace: SettingNamespace::Global,
        key: "animator_duration_scale",
        control: ControlSpec::Float {
            min: 0.0,
            max: 10.0,
        },
    },
    SettingSpec {
        id: "screen_off_timeout",
        namespace: SettingNamespace::System,
        key: "screen_off_timeout",
        // 15 s … 30 min, in milliseconds.
        control: ControlSpec::Int {
            min: 15_000,
            max: 1_800_000,
        },
    },
    SettingSpec {
        id: "font_scale",
        namespace: SettingNamespace::System,
        key: "font_scale",
        control: ControlSpec::Float { min: 0.5, max: 2.0 },
    },
    SettingSpec {
        id: "stay_on_while_plugged_in",
        namespace: SettingNamespace::Global,
        key: "stay_on_while_plugged_in",
        // Bitmask of charging sources (AC=1, USB=2, Wireless=4). We expose the
        // common cases: off, and "on for any source" (7).
        control: ControlSpec::Choice(&[("0", "Off"), ("7", "On (any charger)")]),
    },
];

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct DeviceSetting {
    pub id: String,
    pub namespace: SettingNamespace,
    pub key: String,
    pub control: SettingControl,
    /// Current on-device value, or `None` if unset / unreadable.
    pub value: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct DeviceSettingChange {
    pub id: String,
    pub namespace: SettingNamespace,
    pub key: String,
    pub previous_value: Option<String>,
    pub new_value: String,
    /// The exact `adb shell settings put …` command that was run, for the audit
    /// log / preview.
    pub command: String,
}

fn find_spec(id: &str) -> Option<&'static SettingSpec> {
    CATALOG.iter().find(|spec| spec.id == id)
}

fn control_view(spec: &SettingSpec) -> SettingControl {
    match &spec.control {
        ControlSpec::Float { min, max } => SettingControl::Float {
            min: *min,
            max: *max,
        },
        ControlSpec::Int { min, max } => SettingControl::Int {
            min: *min,
            max: *max,
        },
        ControlSpec::Choice(options) => SettingControl::Choice {
            options: options
                .iter()
                .map(|(value, label)| SettingChoice {
                    value: value.to_string(),
                    label: label.to_string(),
                })
                .collect(),
        },
    }
}

/// Read every catalog setting's current value with `settings get`.
pub fn read_device_settings(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
) -> Result<Vec<DeviceSetting>, TransportError> {
    let mut out = Vec::with_capacity(CATALOG.len());
    for spec in CATALOG {
        let raw = transport
            .shell_target(
                target,
                &["settings", "get", spec.namespace.as_arg(), spec.key],
            )
            .unwrap_or_default();
        out.push(DeviceSetting {
            id: spec.id.to_string(),
            namespace: spec.namespace,
            key: spec.key.to_string(),
            control: control_view(spec),
            value: normalize_get(&raw),
        });
    }
    Ok(out)
}

/// `settings get` prints `null` (or nothing) for unset keys.
fn normalize_get(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Validate `value` against the spec identified by `id`. Returns the on-device
/// key/namespace on success so the caller can write it.
pub fn validate_write(id: &str, value: &str) -> Result<&'static SettingSpec, String> {
    let spec = find_spec(id).ok_or_else(|| format!("unknown setting {id:?}"))?;
    let value = value.trim();
    match &spec.control {
        ControlSpec::Float { min, max } => {
            let parsed: f32 = value
                .parse()
                .map_err(|_| format!("{value:?} is not a number"))?;
            if !parsed.is_finite() || parsed < *min || parsed > *max {
                return Err(format!("value must be between {min} and {max}"));
            }
        }
        ControlSpec::Int { min, max } => {
            let parsed: i64 = value
                .parse()
                .map_err(|_| format!("{value:?} is not an integer"))?;
            if parsed < *min || parsed > *max {
                return Err(format!("value must be between {min} and {max}"));
            }
        }
        ControlSpec::Choice(options) => {
            if !options.iter().any(|(candidate, _)| *candidate == value) {
                return Err(format!("{value:?} is not an allowed value"));
            }
        }
    }
    Ok(spec)
}

/// Build the argv for the `settings put` write. Split out for testability.
pub fn put_argv<'a>(spec: &'a SettingSpec, value: &'a str) -> [&'a str; 5] {
    ["settings", "put", spec.namespace.as_arg(), spec.key, value]
}

pub fn command_preview(spec: &SettingSpec, value: &str) -> String {
    format!(
        "adb shell settings put {} {} {}",
        spec.namespace.as_arg(),
        spec.key,
        value
    )
}

pub fn spec_namespace(spec: &SettingSpec) -> SettingNamespace {
    spec.namespace
}

pub fn spec_key(spec: &SettingSpec) -> &'static str {
    spec.key
}

pub fn spec_id(spec: &SettingSpec) -> &'static str {
    spec.id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_get_treats_null_and_blank_as_unset() {
        assert_eq!(normalize_get("null"), None);
        assert_eq!(normalize_get("  \n"), None);
        assert_eq!(normalize_get("1.0"), Some("1.0".to_string()));
    }

    #[test]
    fn validate_write_enforces_float_range() {
        assert!(validate_write("window_animation_scale", "0.5").is_ok());
        assert!(validate_write("window_animation_scale", "10").is_ok());
        assert!(validate_write("window_animation_scale", "11").is_err());
        assert!(validate_write("window_animation_scale", "-1").is_err());
        assert!(validate_write("window_animation_scale", "fast").is_err());
    }

    #[test]
    fn validate_write_enforces_int_range() {
        assert!(validate_write("screen_off_timeout", "30000").is_ok());
        assert!(validate_write("screen_off_timeout", "1000").is_err());
        assert!(validate_write("screen_off_timeout", "2000000").is_err());
    }

    #[test]
    fn validate_write_enforces_choice_membership() {
        assert!(validate_write("stay_on_while_plugged_in", "0").is_ok());
        assert!(validate_write("stay_on_while_plugged_in", "7").is_ok());
        assert!(validate_write("stay_on_while_plugged_in", "3").is_err());
    }

    #[test]
    fn validate_write_rejects_unknown_setting() {
        assert!(validate_write("rm_rf", "1").is_err());
    }

    #[test]
    fn put_argv_and_preview_are_consistent() {
        let spec = validate_write("font_scale", "1.15").unwrap();
        assert_eq!(
            put_argv(spec, "1.15"),
            ["settings", "put", "system", "font_scale", "1.15"]
        );
        assert_eq!(
            command_preview(spec, "1.15"),
            "adb shell settings put system font_scale 1.15"
        );
    }
}
