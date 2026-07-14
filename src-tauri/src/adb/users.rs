//! Enumerate Android users so package workflows can target an explicit
//! `--user <id>` instead of silently assuming the owner (user 0).
//!
//! Sources:
//! - `pm list users` → the full user set with names and running state.
//! - `am get-current-user` → the foreground user id (Android 10+). On
//!   older platforms this command is missing; callers fall back to the
//!   `pm list users` "(current)" hint or user 0.

use crate::adb::transport::{AdbTransport, TransportError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub fn list_users(t: &dyn AdbTransport, serial: &str) -> Result<Vec<AndroidUser>, TransportError> {
    let raw = t.shell(serial, &["pm", "list", "users"])?;
    let mut users = parse_pm_list_users(&raw);

    // `am get-current-user` is the authoritative foreground user on
    // Android 10+. If it fails or is empty (older platforms), keep the
    // "(current)" hint parsed from `pm list users`.
    if let Ok(current_raw) = t.shell(serial, &["am", "get-current-user"]) {
        if let Some(current) = current_raw
            .trim()
            .lines()
            .next()
            .and_then(|l| l.trim().parse::<u32>().ok())
        {
            for u in &mut users {
                u.current = u.id == current;
            }
        }
    }

    // Guarantee at least the owner so the UI always has a target.
    if users.is_empty() {
        users.push(AndroidUser {
            id: 0,
            name: "Owner".to_string(),
            running: true,
            current: true,
        });
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
        let users = list_users(&mock, "abc").unwrap();
        assert!(!users[0].current, "user 0 is not foreground");
        assert!(users[1].current, "user 10 is foreground");
    }

    #[test]
    fn list_users_defaults_to_owner_when_empty() {
        let mock = MockTransport::new();
        mock.expect_shell("abc", &["pm", "list", "users"], Ok(String::new()));
        mock.expect_shell("abc", &["am", "get-current-user"], Ok(String::new()));
        let users = list_users(&mock, "abc").unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].id, 0);
    }
}
