//! Tiny time helpers. Centralised so the diagnostics, journal, and CLI
//! all stamp things the same way (RFC3339-ish UTC).
//!
//! Hand-rolled instead of pulling `chrono` / `time` — the formatter is
//! cheap, no allocator hot-paths to worry about, and the dep surface
//! shrinks by one major crate.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `YYYY-MM-DDTHH:MM:SSZ` in UTC, from seconds since the Unix epoch.
/// Verified against epoch, 2024-02-29 (leap year), and 2000-03-01
/// (day after the century leap day).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn format_utc_rfc3339(epoch_secs: u64) -> String {
    let secs_in_day: u64 = 86_400;
    let total_days = (epoch_secs / secs_in_day) as i64;
    let time_of_day = epoch_secs % secs_in_day;
    let hour = (time_of_day / 3600) as u32;
    let minute = ((time_of_day % 3600) / 60) as u32;
    let second = (time_of_day % 60) as u32;

    // Howard Hinnant's day-to-Y/M/D algorithm (public domain).
    let z = total_days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// `format_utc_rfc3339(now)` shorthand. Returns the current UTC stamp.
pub fn iso_utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    format_utc_rfc3339(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_anchor() {
        assert_eq!(format_utc_rfc3339(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn leap_year_anchor() {
        // 2024-02-29T12:34:56Z
        assert_eq!(format_utc_rfc3339(1_709_210_096), "2024-02-29T12:34:56Z");
    }

    #[test]
    fn day_after_century_leap() {
        // 2000-03-01T00:00:00Z — the day after Feb 29 2000.
        assert_eq!(format_utc_rfc3339(951_868_800), "2000-03-01T00:00:00Z");
    }

    #[test]
    fn iso_utc_now_is_non_empty_and_z_suffixed() {
        let s = iso_utc_now();
        assert!(s.ends_with('Z'));
        assert!(s.len() >= 20);
    }
}
