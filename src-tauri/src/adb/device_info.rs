//! Device property queries via `adb shell getprop` and `dumpsys`.
//!
//! Used by the R-013 device dashboard to show model, Android version,
//! battery, storage, and network details for a selected device.

use serde::Serialize;

use crate::adb::transport::{AdbTransport, TransportError};

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub serial: String,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub android_version: Option<String>,
    pub sdk_level: Option<String>,
    pub build_fingerprint: Option<String>,
    pub security_patch: Option<String>,
    pub hardware_serial: Option<String>,
    pub battery: Option<BatteryInfo>,
    pub storage: Option<StorageInfo>,
    pub wifi_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatteryInfo {
    pub level: Option<u8>,
    pub status: Option<String>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageInfo {
    pub total_kb: Option<u64>,
    pub used_kb: Option<u64>,
    pub available_kb: Option<u64>,
}

pub fn get_device_info(
    transport: &dyn AdbTransport,
    serial: &str,
) -> Result<DeviceInfo, TransportError> {
    let props = fetch_properties(transport, serial)?;
    let battery = parse_battery(&transport.shell(serial, &["dumpsys", "battery"]).unwrap_or_default());
    let storage = parse_df(&transport.shell(serial, &["df", "/data"]).unwrap_or_default());
    let wifi_ip = get_prop(&props, "dhcp.wlan0.ipaddress")
        .or_else(|| get_prop(&props, "wifi.interface.ip"))
        .filter(|ip| !ip.is_empty());

    Ok(DeviceInfo {
        serial: serial.to_string(),
        model: get_prop(&props, "ro.product.model"),
        manufacturer: get_prop(&props, "ro.product.manufacturer"),
        android_version: get_prop(&props, "ro.build.version.release"),
        sdk_level: get_prop(&props, "ro.build.version.sdk"),
        build_fingerprint: get_prop(&props, "ro.build.fingerprint"),
        security_patch: get_prop(&props, "ro.build.version.security_patch"),
        hardware_serial: get_prop(&props, "ro.serialno"),
        battery,
        storage,
        wifi_ip,
    })
}

fn fetch_properties(
    transport: &dyn AdbTransport,
    serial: &str,
) -> Result<Vec<(String, String)>, TransportError> {
    let stdout = transport.shell(serial, &["getprop"])?;
    Ok(parse_getprop(&stdout))
}

fn parse_getprop(stdout: &str) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(128);
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('[') {
            continue;
        }
        // Format: [key]: [value]
        let Some((key_part, val_part)) = line.split_once("]: [") else {
            continue;
        };
        let key = key_part.trim_start_matches('[');
        let val = val_part.trim_end_matches(']');
        out.push((key.to_string(), val.to_string()));
    }
    out
}

fn get_prop(props: &[(String, String)], key: &str) -> Option<String> {
    props
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

fn parse_battery(stdout: &str) -> Option<BatteryInfo> {
    if stdout.is_empty() {
        return None;
    }
    let mut level: Option<u8> = None;
    let mut status: Option<String> = None;
    let mut temperature: Option<f32> = None;

    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("level:") {
            level = rest.trim().parse().ok();
        } else if let Some(rest) = line.strip_prefix("status:") {
            status = Some(battery_status_label(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("temperature:") {
            if let Ok(raw) = rest.trim().parse::<u32>() {
                temperature = Some(raw as f32 / 10.0);
            }
        }
    }

    if level.is_none() && status.is_none() {
        return None;
    }

    Some(BatteryInfo {
        level,
        status,
        temperature,
    })
}

fn battery_status_label(code: &str) -> String {
    match code {
        "1" => "Unknown".to_string(),
        "2" => "Charging".to_string(),
        "3" => "Discharging".to_string(),
        "4" => "Not charging".to_string(),
        "5" => "Full".to_string(),
        other => format!("Status {other}"),
    }
}

fn parse_df(stdout: &str) -> Option<StorageInfo> {
    // `df /data` output varies by Android version. Common format:
    //   Filesystem  1K-blocks  Used  Available  Use%  Mounted on
    //   /dev/...    123456     78900  44556      64%   /data
    for line in stdout.lines().skip(1) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() >= 4 {
            let total_kb = tokens[1].parse().ok();
            let used_kb = tokens[2].parse().ok();
            let available_kb = tokens[3].parse().ok();
            if total_kb.is_some() || available_kb.is_some() {
                return Some(StorageInfo {
                    total_kb,
                    used_kb,
                    available_kb,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_getprop_extracts_key_value_pairs() {
        let stdout = "\
[ro.product.model]: [Pixel 8]
[ro.product.manufacturer]: [Google]
[ro.build.version.release]: [14]
[ro.build.version.sdk]: [34]
garbage line
";
        let props = parse_getprop(stdout);
        assert_eq!(props.len(), 4);
        assert_eq!(get_prop(&props, "ro.product.model").as_deref(), Some("Pixel 8"));
        assert_eq!(get_prop(&props, "ro.build.version.sdk").as_deref(), Some("34"));
        assert_eq!(get_prop(&props, "missing"), None);
    }

    #[test]
    fn parse_battery_extracts_level_and_status() {
        let stdout = "\
Current Battery Service state:
  AC powered: false
  USB powered: true
  status: 2
  health: 2
  present: true
  level: 78
  temperature: 295
";
        let info = parse_battery(stdout).unwrap();
        assert_eq!(info.level, Some(78));
        assert_eq!(info.status.as_deref(), Some("Charging"));
        assert_eq!(info.temperature, Some(29.5));
    }

    #[test]
    fn parse_battery_returns_none_on_empty() {
        assert!(parse_battery("").is_none());
    }

    #[test]
    fn parse_df_extracts_data_partition() {
        let stdout = "\
Filesystem     1K-blocks    Used Available Use% Mounted on
/dev/block/dm-9 113021876 87654320  25367556  78% /data
";
        let info = parse_df(stdout).unwrap();
        assert_eq!(info.total_kb, Some(113021876));
        assert_eq!(info.used_kb, Some(87654320));
        assert_eq!(info.available_kb, Some(25367556));
    }

    #[test]
    fn parse_df_returns_none_on_garbage() {
        assert!(parse_df("nothing useful").is_none());
    }
}
