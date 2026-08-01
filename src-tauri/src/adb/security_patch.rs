//! Wireless-debugging risk classification from the device security patch level.
//!
//! CVE-2026-0073 (AOSP bulletin 2026-05-01, rated Critical) is a mutual-auth
//! bypass in `adbd`: `adbd_tls_verify_cert` treated the `-1` "different key
//! types" return of `EVP_PKEY_cmp` as truthy, so a client certificate of the
//! wrong key type against a stored host key authorised the connection. It is
//! reachable by an adjacent attacker once wireless debugging is exposed, needs
//! no user interaction, and yields code execution as the shell user.
//!
//! Droidsmith's own pairing flow plants a host key in the device trust store,
//! so it must not offer a wireless handoff for an unpatched device without
//! saying so first. This module owns the classification only; the decision to
//! warn lives in the renderer.
//!
//! The classification never guesses. A patch string that is absent, malformed,
//! or outside the advisory's stated Android range reports
//! [`WirelessDebuggingRisk::Unknown`] rather than implying either verdict.

use serde::Serialize;

/// First Android security patch level carrying the CVE-2026-0073 fix.
pub const WIRELESS_AUTH_BYPASS_PATCH_FLOOR: &str = "2026-05-01";

/// Lowest SDK level the AOSP bulletin lists as affected (Android 14).
const AFFECTED_SDK_MIN: u32 = 34;

/// Highest SDK level the AOSP bulletin lists as affected (Android 16).
const AFFECTED_SDK_MAX: u32 = 36;

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WirelessDebuggingRisk {
    /// The patch level is missing, malformed, or the build sits outside the
    /// advisory's stated Android range. Neither verdict is claimed.
    Unknown,
    /// Android 14-16 build whose patch level predates the fix.
    AuthBypassUnpatched,
    /// Patch level is at or after the fix.
    Patched,
}

/// Classify a device from its `ro.build.version.security_patch` and
/// `ro.build.version.sdk` values.
pub fn classify_wireless_debugging_risk(
    security_patch: Option<&str>,
    sdk_level: Option<&str>,
) -> WirelessDebuggingRisk {
    let Some(patch) = security_patch.and_then(parse_patch_date) else {
        return WirelessDebuggingRisk::Unknown;
    };
    let floor = parse_patch_date(WIRELESS_AUTH_BYPASS_PATCH_FLOOR)
        .expect("the compiled-in patch floor is a valid YYYY-MM-DD date");
    if patch >= floor {
        return WirelessDebuggingRisk::Patched;
    }
    // Below the floor, but only Android 14-16 are listed as affected. An older
    // or newer build reports Unknown instead of a fabricated verdict.
    match sdk_level.and_then(|value| value.trim().parse::<u32>().ok()) {
        Some(sdk) if (AFFECTED_SDK_MIN..=AFFECTED_SDK_MAX).contains(&sdk) => {
            WirelessDebuggingRisk::AuthBypassUnpatched
        }
        _ => WirelessDebuggingRisk::Unknown,
    }
}

/// Parse a strict `YYYY-MM-DD` patch level into a comparable tuple.
///
/// OEMs ship malformed values here (empty strings, `unknown`, bare years), so
/// anything that is not an exact, in-range calendar date is rejected.
fn parse_patch_date(value: &str) -> Option<(u32, u32, u32)> {
    let trimmed = value.trim();
    let mut parts = trimmed.split('-');
    let year = parts.next()?;
    let month = parts.next()?;
    let day = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    let year: u32 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_levels_at_or_after_the_floor_are_patched() {
        for patch in ["2026-05-01", "2026-05-05", "2026-06-01", "2027-01-01"] {
            assert_eq!(
                classify_wireless_debugging_risk(Some(patch), Some("35")),
                WirelessDebuggingRisk::Patched,
                "{patch} should be patched",
            );
        }
    }

    #[test]
    fn affected_android_versions_below_the_floor_are_unpatched() {
        for sdk in ["34", "35", "36"] {
            assert_eq!(
                classify_wireless_debugging_risk(Some("2026-04-01"), Some(sdk)),
                WirelessDebuggingRisk::AuthBypassUnpatched,
                "sdk {sdk} should be flagged",
            );
        }
        assert_eq!(
            classify_wireless_debugging_risk(Some("2025-12-05"), Some("34")),
            WirelessDebuggingRisk::AuthBypassUnpatched,
        );
    }

    #[test]
    fn builds_outside_the_advisory_range_report_unknown() {
        // Android 13 and Android 17 are not listed as affected; an old patch
        // level on them must not be reported as this CVE.
        for sdk in ["33", "30", "37"] {
            assert_eq!(
                classify_wireless_debugging_risk(Some("2026-04-01"), Some(sdk)),
                WirelessDebuggingRisk::Unknown,
                "sdk {sdk} is outside the bulletin range",
            );
        }
    }

    #[test]
    fn absent_or_malformed_values_never_fabricate_a_verdict() {
        for patch in [
            None,
            Some(""),
            Some("   "),
            Some("unknown"),
            Some("2026"),
            Some("2026-05"),
            Some("2026-5-1"),
            Some("2026-13-01"),
            Some("2026-05-32"),
            Some("2026-05-01-1"),
            Some("not-a-date"),
        ] {
            assert_eq!(
                classify_wireless_debugging_risk(patch, Some("35")),
                WirelessDebuggingRisk::Unknown,
                "{patch:?} must be unknown",
            );
        }
        // A valid pre-floor patch with an unusable SDK is still unknown.
        assert_eq!(
            classify_wireless_debugging_risk(Some("2026-04-01"), None),
            WirelessDebuggingRisk::Unknown,
        );
        assert_eq!(
            classify_wireless_debugging_risk(Some("2026-04-01"), Some("banana")),
            WirelessDebuggingRisk::Unknown,
        );
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        assert_eq!(
            classify_wireless_debugging_risk(Some(" 2026-06-01 "), Some(" 35 ")),
            WirelessDebuggingRisk::Patched,
        );
    }
}
