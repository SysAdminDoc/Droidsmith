//! Enumerate installed packages on a device.
//!
//! Source of truth is `pm list packages` plus its flag combinations:
//!
//! - `pm list packages -e` → enabled packages
//! - `pm list packages -d` → disabled packages
//! - `pm list packages -3` → third-party only (no system apps)
//! - `pm list packages -s` → system only
//! - `pm list packages -f` → prefix each line with the APK path
//! - `pm list packages -U` → suffix each line with `uid:NNN`
//! - `pm list packages -i` → suffix with `installer=<pkg>`
//!
//! For v0.1 we do two passes: one `-e -f -U -i` and one `-d -f -U -i`,
//! then union them with `enabled: bool`. Labels and icons (F-NEW-09)
//! land later — for now the UI shows package names.

use crate::adb::transport::{AdbTransport, TransportError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppPackage {
    /// Application package id, e.g. `com.android.chrome`.
    pub package: String,
    /// True when found via `-e`, false when only `-d`.
    pub enabled: bool,
    /// Heuristic: an app is "system" if its APK is under `/system/`,
    /// `/product/`, `/vendor/`, `/apex/`, or `/system_ext/`. Mirrors
    /// the heuristic Android Settings itself uses.
    pub system: bool,
    /// Absolute APK path on the device, when `-f` produced one.
    pub apk_path: Option<String>,
    /// UID owning the package, when `-U` produced one.
    pub uid: Option<u32>,
    /// Installer source package id, when `-i` produced one. Used to
    /// surface "Installed from Play Store" vs ApkMirror / sideload.
    pub installer: Option<String>,
}

impl AppPackage {
    /// Convenience filter: matches Android's "Show system" toggle in
    /// the Settings → Apps screen. Kept ahead of the renderer-side
    /// filter UI so the rule lives next to the type.
    #[allow(dead_code)]
    pub fn is_user(&self) -> bool {
        !self.system
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageFilter {
    All,
    User,
    System,
    Enabled,
    Disabled,
}

/// Enumerate packages on `serial`, applying `filter` after the union.
pub fn list_packages(
    t: &dyn AdbTransport,
    serial: &str,
    filter: PackageFilter,
) -> Result<Vec<AppPackage>, TransportError> {
    let enabled_raw = t.shell(serial, &["pm", "list", "packages", "-e", "-f", "-U", "-i"])?;
    let disabled_raw = t.shell(serial, &["pm", "list", "packages", "-d", "-f", "-U", "-i"])?;

    let mut packages = parse_pm_list(&enabled_raw, true);
    for entry in parse_pm_list(&disabled_raw, false) {
        // A package can appear in both lists if `pm` is racing with an
        // enable/disable; the disabled row wins (matches `pm`'s own
        // ordering semantics — a freshly-disabled app reports disabled).
        if let Some(existing) = packages.iter_mut().find(|p| p.package == entry.package) {
            *existing = entry;
        } else {
            packages.push(entry);
        }
    }

    Ok(packages
        .into_iter()
        .filter(|p| match filter {
            PackageFilter::All => true,
            PackageFilter::User => !p.system,
            PackageFilter::System => p.system,
            PackageFilter::Enabled => p.enabled,
            PackageFilter::Disabled => !p.enabled,
        })
        .collect())
}

/// Parse the output of `pm list packages -f -U -i [-e|-d]`.
///
/// Each line is `package:<path>=<id> [uid:<n>] [installer=<pkg>]`. We
/// tolerate missing optional fields because `pm` on older Androids may
/// drop them.
///
/// Example real lines (Android 14, Pixel):
/// ```text
/// package:/system/priv-app/Chrome/Chrome.apk=com.android.chrome uid:10042 installer=com.android.vending
/// package:/data/app/~~aaa==/com.example.foo-bbb==/base.apk=com.example.foo uid:10412 installer=null
/// ```
pub fn parse_pm_list(stdout: &str, enabled: bool) -> Vec<AppPackage> {
    let mut out = Vec::with_capacity(64);
    for line in stdout.lines() {
        let line = line.trim_end();
        if let Some(pkg) = parse_pm_line(line, enabled) {
            out.push(pkg);
        }
    }
    out
}

fn parse_pm_line(line: &str, enabled: bool) -> Option<AppPackage> {
    let body = line.strip_prefix("package:")?;
    // body is `<apk_path>=<id> [uid:N] [installer=X]` OR just `<id>` if
    // `-f` wasn't requested (defensive).
    let mut tokens = body.split_whitespace();
    let head = tokens.next()?;

    let (apk_path, package) = if let Some((path, id)) = head.rsplit_once('=') {
        (Some(path.to_string()), id.to_string())
    } else {
        (None, head.to_string())
    };

    if package.is_empty() || !valid_package_name(&package) {
        return None;
    }

    let mut uid: Option<u32> = None;
    let mut installer: Option<String> = None;
    for tok in tokens {
        if let Some(rest) = tok.strip_prefix("uid:") {
            uid = rest.parse().ok();
        } else if let Some(rest) = tok.strip_prefix("installer=") {
            // adb prints the literal string "null" when no installer
            // is recorded; normalise to None.
            installer = match rest {
                "null" | "" => None,
                other => Some(other.to_string()),
            };
        }
    }

    let system = apk_path.as_deref().map(is_system_path).unwrap_or(false);

    Some(AppPackage {
        package,
        enabled,
        system,
        apk_path,
        uid,
        installer,
    })
}

fn is_system_path(p: &str) -> bool {
    p.starts_with("/system/")
        || p.starts_with("/product/")
        || p.starts_with("/vendor/")
        || p.starts_with("/apex/")
        || p.starts_with("/system_ext/")
}

/// Conservative validator for Android package identifiers. The platform
/// allows letters/digits/dot/underscore; this catches obvious junk
/// (empty, leading dot, etc.) without rejecting real packages.
pub(crate) fn valid_package_name(s: &str) -> bool {
    if s.is_empty() || s.starts_with('.') || s.ends_with('.') {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::transport::MockTransport;

    const ENABLED_FIXTURE: &str = "\
package:/system/priv-app/Chrome/Chrome.apk=com.android.chrome uid:10042 installer=com.android.vending
package:/data/app/~~aa==/com.example.foo-bb==/base.apk=com.example.foo uid:10412 installer=null
package:/product/app/YouTube/YouTube.apk=com.google.android.youtube uid:10100 installer=com.android.vending
";
    const DISABLED_FIXTURE: &str = "\
package:/system/app/FacebookStub/FacebookStub.apk=com.facebook.appmanager uid:10200 installer=null
";

    #[test]
    fn parses_a_known_line_fully() {
        let v = parse_pm_list(ENABLED_FIXTURE, true);
        assert_eq!(v.len(), 3);
        let chrome = &v[0];
        assert_eq!(chrome.package, "com.android.chrome");
        assert_eq!(
            chrome.apk_path.as_deref(),
            Some("/system/priv-app/Chrome/Chrome.apk")
        );
        assert_eq!(chrome.uid, Some(10042));
        assert_eq!(chrome.installer.as_deref(), Some("com.android.vending"));
        assert!(chrome.system);
        assert!(chrome.enabled);

        let foo = &v[1];
        assert_eq!(foo.installer, None); // "null" → None
        assert!(!foo.system); // /data/app/ → user
    }

    #[test]
    fn parses_disabled_lines() {
        let v = parse_pm_list(DISABLED_FIXTURE, false);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].package, "com.facebook.appmanager");
        assert!(!v[0].enabled);
    }

    #[test]
    fn skips_blank_lines_and_garbage() {
        let s = concat!(
            "\n",
            "package:not-a-package\n",
            "package:com.valid.id\n",
            "some-garbage-line\n",
        );
        let v = parse_pm_list(s, true);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].package, "com.valid.id");
    }

    #[test]
    fn list_packages_unions_enabled_and_disabled() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "-e", "-f", "-U", "-i"],
            Ok(ENABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "-d", "-f", "-U", "-i"],
            Ok(DISABLED_FIXTURE.to_string()),
        );

        let v = list_packages(&mock, "abc", PackageFilter::All).unwrap();
        assert_eq!(v.len(), 4);
        let enabled: Vec<_> = v.iter().filter(|p| p.enabled).collect();
        assert_eq!(enabled.len(), 3);
    }

    #[test]
    fn list_packages_user_filter_excludes_system() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "-e", "-f", "-U", "-i"],
            Ok(ENABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "-d", "-f", "-U", "-i"],
            Ok(DISABLED_FIXTURE.to_string()),
        );
        let v = list_packages(&mock, "abc", PackageFilter::User).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].package, "com.example.foo");
    }

    #[test]
    fn list_packages_disabled_filter() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "-e", "-f", "-U", "-i"],
            Ok(ENABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "-d", "-f", "-U", "-i"],
            Ok(DISABLED_FIXTURE.to_string()),
        );
        let v = list_packages(&mock, "abc", PackageFilter::Disabled).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].package, "com.facebook.appmanager");
    }

    #[test]
    fn is_system_path_classifies_partitions() {
        assert!(is_system_path("/system/app/Chrome.apk"));
        assert!(is_system_path("/product/app/X.apk"));
        assert!(is_system_path("/vendor/app/X.apk"));
        assert!(is_system_path("/apex/com.x/app/X.apk"));
        assert!(is_system_path("/system_ext/app/X.apk"));
        assert!(!is_system_path("/data/app/X.apk"));
        assert!(!is_system_path(""));
    }
}
