//! Device property queries via `adb shell getprop` and `dumpsys`.
//!
//! Used by the R-013 device dashboard to show model, Android version,
//! battery, storage, and network details for a selected device.

use serde::Serialize;

use crate::adb::device::DeviceTarget;
use crate::adb::transport::{AdbTransport, TransportError};

#[derive(specta::Type, Debug, Clone, Serialize)]
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
    /// R-079 health dashboard: per-partition breakdown (system/data/cache/...).
    pub storage_partitions: Vec<StoragePartition>,
    /// R-079 health dashboard: thermal-zone temperatures from the HAL.
    pub thermal_zones: Vec<ThermalZone>,
    pub wifi_ip: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct BatteryInfo {
    pub level: Option<u8>,
    pub status: Option<String>,
    pub temperature: Option<f32>,
    /// Battery health label decoded from the `health:` code (Good, Overheat, ...).
    pub health: Option<String>,
    /// Instantaneous voltage in millivolts.
    pub voltage_mv: Option<u32>,
    /// Cell chemistry, e.g. `Li-ion`.
    pub technology: Option<String>,
    /// Charge cycle count (Android 14+ exposes this via `dumpsys battery`).
    pub cycle_count: Option<u32>,
    /// Remaining charge in microampere-hours (`Charge counter:`).
    pub charge_counter_uah: Option<i64>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct StorageInfo {
    pub total_kb: Option<u64>,
    pub used_kb: Option<u64>,
    pub available_kb: Option<u64>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct StoragePartition {
    /// Mount point, e.g. `/data`, `/system`, `/cache`.
    pub mount: String,
    pub total_kb: Option<u64>,
    pub used_kb: Option<u64>,
    pub available_kb: Option<u64>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct ThermalZone {
    /// HAL zone name, e.g. `CPU`, `battery`, `skin`.
    pub name: String,
    pub temperature_c: f32,
    /// Throttling status label from `mStatus` (`None`, `Light`, `Severe`, ...).
    pub status: Option<String>,
}

pub fn get_device_info(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
) -> Result<DeviceInfo, TransportError> {
    let props = fetch_properties(transport, target)?;
    let battery = parse_battery(
        &transport
            .shell_target(target, &["dumpsys", "battery"])
            .unwrap_or_default(),
    );
    // `-k` pins 1K-block units so the KB interpretation is deterministic
    // regardless of the busybox/toybox default block size. One `df` call
    // yields every mount; we keep `/data` as the headline `storage` figure
    // and surface the curated partition set for the health dashboard.
    let df_output = transport
        .shell_target(target, &["df", "-k"])
        .unwrap_or_default();
    let storage_partitions = parse_df_partitions(&df_output);
    let storage = storage_partitions
        .iter()
        .find(|p| p.mount == "/data")
        .map(StoragePartition::to_storage_info)
        .or_else(|| {
            parse_df(
                &transport
                    .shell_target(target, &["df", "-k", "/data"])
                    .unwrap_or_default(),
            )
        });
    let thermal_zones = parse_thermal(
        &transport
            .shell_target(target, &["dumpsys", "thermalservice"])
            .unwrap_or_default(),
    );
    let wifi_ip = get_prop(&props, "dhcp.wlan0.ipaddress")
        .or_else(|| get_prop(&props, "wifi.interface.ip"))
        .filter(|ip| !ip.is_empty());

    Ok(DeviceInfo {
        serial: target.serial.clone(),
        model: get_prop(&props, "ro.product.model"),
        manufacturer: get_prop(&props, "ro.product.manufacturer"),
        android_version: get_prop(&props, "ro.build.version.release"),
        sdk_level: get_prop(&props, "ro.build.version.sdk"),
        build_fingerprint: get_prop(&props, "ro.build.fingerprint"),
        security_patch: get_prop(&props, "ro.build.version.security_patch"),
        hardware_serial: get_prop(&props, "ro.serialno"),
        battery,
        storage,
        storage_partitions,
        thermal_zones,
        wifi_ip,
    })
}

fn fetch_properties(
    transport: &dyn AdbTransport,
    target: &DeviceTarget,
) -> Result<Vec<(String, String)>, TransportError> {
    let stdout = transport.shell_target(target, &["getprop"])?;
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
    let mut health: Option<String> = None;
    let mut voltage_mv: Option<u32> = None;
    let mut technology: Option<String> = None;
    let mut cycle_count: Option<u32> = None;
    let mut charge_counter_uah: Option<i64> = None;

    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("level:") {
            level = rest.trim().parse().ok();
        } else if let Some(rest) = line.strip_prefix("status:") {
            status = Some(battery_status_label(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("health:") {
            health = Some(battery_health_label(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("voltage:") {
            voltage_mv = rest.trim().parse().ok();
        } else if let Some(rest) = line.strip_prefix("technology:") {
            let tech = rest.trim();
            technology = (!tech.is_empty()).then(|| tech.to_string());
        } else if let Some(rest) = line.strip_prefix("temperature:") {
            if let Ok(raw) = rest.trim().parse::<i32>() {
                temperature = Some(raw as f32 / 10.0);
            }
        } else if let Some(rest) = line
            .strip_prefix("Charge cycle count:")
            .or_else(|| line.strip_prefix("Cycle count:"))
        {
            cycle_count = rest.trim().parse().ok();
        } else if let Some(rest) = line.strip_prefix("Charge counter:") {
            charge_counter_uah = rest.trim().parse().ok();
        }
    }

    if level.is_none() && status.is_none() {
        return None;
    }

    Some(BatteryInfo {
        level,
        status,
        temperature,
        health,
        voltage_mv,
        technology,
        cycle_count,
        charge_counter_uah,
    })
}

fn battery_health_label(code: &str) -> String {
    match code {
        "1" => "Unknown".to_string(),
        "2" => "Good".to_string(),
        "3" => "Overheat".to_string(),
        "4" => "Dead".to_string(),
        "5" => "Over voltage".to_string(),
        "6" => "Failure".to_string(),
        "7" => "Cold".to_string(),
        other => format!("Health {other}"),
    }
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
    // `df -k /data` reports 1K-block units on every Android variant. Format:
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

impl StoragePartition {
    fn to_storage_info(&self) -> StorageInfo {
        StorageInfo {
            total_kb: self.total_kb,
            used_kb: self.used_kb,
            available_kb: self.available_kb,
        }
    }
}

/// Mounts surfaced in the health dashboard, in display order. `df -k` reports
/// many pseudo-filesystems; we keep the user-meaningful ones.
const PARTITION_MOUNTS: &[&str] = &["/system", "/vendor", "/data", "/cache"];

fn parse_df_partitions(stdout: &str) -> Vec<StoragePartition> {
    let mut found: Vec<StoragePartition> = Vec::new();
    for line in stdout.lines().skip(1) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        // Filesystem 1K-blocks Used Available Use% Mounted-on
        if tokens.len() < 6 {
            continue;
        }
        let mount = *tokens.last().unwrap();
        if !PARTITION_MOUNTS.contains(&mount) {
            continue;
        }
        if found.iter().any(|p| p.mount == mount) {
            continue;
        }
        let total_kb = tokens[1].parse().ok();
        let used_kb = tokens[2].parse().ok();
        let available_kb = tokens[3].parse().ok();
        if total_kb.is_none() && available_kb.is_none() {
            continue;
        }
        found.push(StoragePartition {
            mount: mount.to_string(),
            total_kb,
            used_kb,
            available_kb,
        });
    }
    // Present partitions in the curated order rather than df's discovery order.
    found.sort_by_key(|p| {
        PARTITION_MOUNTS
            .iter()
            .position(|m| *m == p.mount)
            .unwrap_or(usize::MAX)
    });
    found
}

fn parse_thermal(stdout: &str) -> Vec<ThermalZone> {
    let mut zones = Vec::new();
    // `dumpsys thermalservice` emits lines like:
    //   Temperature{mValue=30.0, mType=3, mName=CPU, mStatus=0}
    // We only ingest the current-HAL block; cached/pending readings repeat
    // names, so de-dupe on first sighting.
    for line in stdout.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Temperature{") else {
            continue;
        };
        let body = rest.trim_end_matches('}');
        let mut name: Option<String> = None;
        let mut value: Option<f32> = None;
        let mut status: Option<String> = None;
        for field in body.split(',') {
            let Some((key, val)) = field.trim().split_once('=') else {
                continue;
            };
            match key.trim() {
                "mName" => name = Some(val.trim().to_string()),
                "mValue" => value = val.trim().parse().ok(),
                "mStatus" => status = Some(thermal_status_label(val.trim())),
                _ => {}
            }
        }
        if let (Some(name), Some(temperature_c)) = (name, value) {
            if temperature_c.is_finite()
                && !name.is_empty()
                && !zones.iter().any(|z: &ThermalZone| z.name == name)
            {
                zones.push(ThermalZone {
                    name,
                    temperature_c,
                    status,
                });
            }
        }
    }
    zones
}

fn thermal_status_label(code: &str) -> String {
    match code {
        "0" => "None".to_string(),
        "1" => "Light".to_string(),
        "2" => "Moderate".to_string(),
        "3" => "Severe".to_string(),
        "4" => "Critical".to_string(),
        "5" => "Emergency".to_string(),
        "6" => "Shutdown".to_string(),
        other => format!("Status {other}"),
    }
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
        assert_eq!(
            get_prop(&props, "ro.product.model").as_deref(),
            Some("Pixel 8")
        );
        assert_eq!(
            get_prop(&props, "ro.build.version.sdk").as_deref(),
            Some("34")
        );
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
  voltage: 4301
  temperature: 295
  technology: Li-ion
  Charge counter: 3520000
  Charge cycle count: 145
";
        let info = parse_battery(stdout).unwrap();
        assert_eq!(info.level, Some(78));
        assert_eq!(info.status.as_deref(), Some("Charging"));
        assert_eq!(info.temperature, Some(29.5));
        assert_eq!(info.health.as_deref(), Some("Good"));
        assert_eq!(info.voltage_mv, Some(4301));
        assert_eq!(info.technology.as_deref(), Some("Li-ion"));
        assert_eq!(info.cycle_count, Some(145));
        assert_eq!(info.charge_counter_uah, Some(3_520_000));
    }

    #[test]
    fn parse_df_partitions_keeps_curated_mounts_in_order() {
        let stdout = "\
Filesystem     1K-blocks    Used Available Use% Mounted on
/dev/block/dm-9 113021876 87654320  25367556  78% /data
tmpfs             4096000       0   4096000   0% /dev
/dev/block/dm-0   2500000 2400000    100000  96% /system
/dev/block/dm-2    900000  120000    780000  14% /cache
";
        let parts = parse_df_partitions(stdout);
        let mounts: Vec<&str> = parts.iter().map(|p| p.mount.as_str()).collect();
        assert_eq!(mounts, vec!["/system", "/data", "/cache"]);
        assert_eq!(parts[0].total_kb, Some(2_500_000));
        assert_eq!(parts[1].available_kb, Some(25_367_556));
    }

    #[test]
    fn parse_thermal_extracts_named_zones() {
        let stdout = "\
Current temperatures from HAL:
	Temperature{mValue=30.0, mType=3, mName=CPU, mStatus=0}
	Temperature{mValue=28.5, mType=2, mName=battery, mStatus=1}
Cached temperatures:
	Temperature{mValue=99.0, mType=3, mName=CPU, mStatus=0}
";
        let zones = parse_thermal(stdout);
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].name, "CPU");
        assert_eq!(zones[0].temperature_c, 30.0);
        assert_eq!(zones[0].status.as_deref(), Some("None"));
        assert_eq!(zones[1].name, "battery");
        assert_eq!(zones[1].status.as_deref(), Some("Light"));
    }

    #[test]
    fn parse_thermal_returns_empty_on_garbage() {
        assert!(parse_thermal("no temperatures here").is_empty());
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
