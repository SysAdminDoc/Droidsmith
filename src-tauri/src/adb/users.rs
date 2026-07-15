//! Enumerate Android users so package workflows can target an explicit
//! `--user <id>` instead of silently assuming the owner (user 0).
//!
//! Sources:
//! - `pm list users` → the full user set with names and running state.
//! - `am get-current-user` → the foreground user id (Android 10+). On
//!   older platforms this command is missing; callers use the unambiguous
//!   `pm list users` "(current)" hint and otherwise fail closed.

use crate::adb::device::DeviceTarget;
use crate::adb::transport::{AdbTransport, TransportError};
use serde::{Deserialize, Serialize};

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidUser {
    /// Numeric user id passed to `pm --user`.
    pub id: u32,
    /// Human-readable name, e.g. `Owner`, `Work profile`.
    pub name: String,
    /// True when `pm list users` reports the user as running.
    pub running: bool,
    /// True for the foreground user (resolved via `am get-current-user`,
    /// falling back to the `pm list users` "(current)" marker).
    pub current: bool,
}

/// Enumerate users on `serial` and mark the foreground one.
pub fn list_users(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
) -> Result<Vec<AndroidUser>, TransportError> {
    let raw = t.shell_target(target, &["pm", "list", "users"])?;
    let mut users = parse_pm_list_users(&raw);

    if users.is_empty() {
        return Err(TransportError::Parse(format!(
            "Android user discovery returned no parseable users for {:?}",
            target.serial
        )));
    }

    // `am get-current-user` is the authoritative foreground user on
    // Android 10+. If it fails or is empty (older platforms), keep the
    // "(current)" hint parsed from `pm list users`.
    if let Ok(current_raw) = t.shell_target(target, &["am", "get-current-user"]) {
        if let Some(current) = current_raw
            .trim()
            .lines()
            .next()
            .and_then(|l| l.trim().parse::<u32>().ok())
        {
            if !users.iter().any(|user| user.id == current) {
                return Err(TransportError::Parse(format!(
                    "foreground Android user {current} is absent from pm list users"
                )));
            }
            for u in &mut users {
                u.current = u.id == current;
            }
        }
    }

    let current_count = users.iter().filter(|user| user.current).count();
    if current_count != 1 {
        return Err(TransportError::Parse(format!(
            "Android user discovery is ambiguous: expected one foreground user, found {current_count}"
        )));
    }
    Ok(users)
}

/// Parse `pm list users` output.
///
/// Real shape (Android 14):
/// ```text
/// Users:
///         UserInfo{0:Owner:c13} running
///         UserInfo{10:Work profile:1030} running
/// ```
/// Some builds append `(current)` after `running`. The flags after the
/// third colon are a hex bitmask we don't decode here.
pub fn parse_pm_list_users(stdout: &str) -> Vec<AndroidUser> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        let Some(start) = trimmed.find("UserInfo{") else {
            continue;
        };
        let after = &trimmed[start + "UserInfo{".len()..];
        let Some(close) = after.find('}') else {
            continue;
        };
        let inner = &after[..close]; // e.g. `10:Work profile:1030`
        let tail = &after[close + 1..]; // e.g. ` running (current)`

        // Split into id / name / flags. The name itself may contain
        // colons in exotic locales, so split from both ends: first colon
        // for the id, last colon for the flags.
        let Some((id_str, rest)) = inner.split_once(':') else {
            continue;
        };
        let name = match rest.rsplit_once(':') {
            Some((name, _flags)) => name,
            None => rest,
        };
        let Ok(id) = id_str.trim().parse::<u32>() else {
            continue;
        };

        let running = tail.contains("running");
        let current = tail.contains("(current)");
        out.push(AndroidUser {
            id,
            name: name.trim().to_string(),
            running,
            current,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::transport::MockTransport;

    const USERS_FIXTURE: &str = "\
Users:
        UserInfo{0:Owner:c13} running
        UserInfo{10:Work profile:1030} running
";

    fn target() -> DeviceTarget {
        DeviceTarget {
            serial: "abc".into(),
            transport_id: Some(1),
            connection_generation: 2,
            model: None,
            product: None,
            device: None,
            build_fingerprint: Some("build/test".into()),
            transport_kind: crate::adb::DeviceTransportKind::Usb,
            untrusted_transport_override: false,
        }
    }

    #[test]
    fn parses_multiple_users() {
        let users = parse_pm_list_users(USERS_FIXTURE);
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].id, 0);
        assert_eq!(users[0].name, "Owner");
        assert!(users[0].running);
        assert_eq!(users[1].id, 10);
        assert_eq!(users[1].name, "Work profile");
    }

    #[test]
    fn parses_current_marker() {
        let users = parse_pm_list_users("        UserInfo{10:Guest:410} running (current)\n");
        assert_eq!(users.len(), 1);
        assert!(users[0].current);
    }

    #[test]
    fn skips_garbage_lines() {
        let users = parse_pm_list_users("Users:\n  not a user info line\n");
        assert!(users.is_empty());
    }

    #[test]
    fn list_users_marks_foreground_from_am() {
        let mock = MockTransport::new();
        mock.expect_shell("abc", &["pm", "list", "users"], Ok(USERS_FIXTURE.into()));
        mock.expect_shell("abc", &["am", "get-current-user"], Ok("10\n".into()));
        let users = list_users(&mock, &target()).unwrap();
        assert!(!users[0].current, "user 0 is not foreground");
        assert!(users[1].current, "user 10 is foreground");
    }

    #[test]
    fn list_users_fails_closed_when_discovery_is_empty() {
        let mock = MockTransport::new();
        mock.expect_shell("abc", &["pm", "list", "users"], Ok(String::new()));
        mock.expect_shell("abc", &["am", "get-current-user"], Ok(String::new()));
        let error = list_users(&mock, &target()).unwrap_err();
        assert!(error.to_string().contains("returned no parseable users"));
    }
}
