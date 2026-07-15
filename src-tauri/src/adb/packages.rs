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
//! then union them with `enabled: bool`. Labels and icons are deliberately
//! fetched through the separate lazy metadata command so this hot path never
//! pulls every installed APK.

use crate::adb::device::DeviceTarget;
use crate::adb::transport::{AdbTransport, TransportError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// True when Android 15+ has removed the APK/cache while retaining user
    /// data and installer metadata for a later unarchive request.
    pub archived: bool,
    /// True when PackageManager still retains this package's user data for the
    /// selected Android user (`pm list packages -u`) but it is neither
    /// installed nor archived — an uninstalled-with-data remnant whose leftover
    /// data can be fully purged.
    pub retained: bool,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageArchiveCapability {
    pub supported: bool,
    pub api_level: Option<u32>,
    pub reason: String,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageListing {
    pub packages: Vec<AppPackage>,
    pub archive: PackageArchiveCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackagePresence {
    Installed { enabled: bool, system: bool },
    Archived,
    Retained { system: bool },
    Missing,
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

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageFilter {
    All,
    User,
    System,
    Enabled,
    Disabled,
    Archived,
    Retained,
}

/// Enumerate packages on `serial` for Android user `user_id`, applying
/// `filter` after the union. Passing the explicit `--user` keeps the
/// listed set consistent with the user that destructive actions target.
pub fn list_packages_with_capability(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    filter: PackageFilter,
    user_id: u32,
) -> Result<PackageListing, TransportError> {
    let user = user_id.to_string();
    let archive = archive_capability(t, target);
    let enabled_raw = t.shell_target(
        target,
        &[
            "pm", "list", "packages", "--user", &user, "-e", "-f", "-U", "-i",
        ],
    )?;
    let disabled_raw = t.shell_target(
        target,
        &[
            "pm", "list", "packages", "--user", &user, "-d", "-f", "-U", "-i",
        ],
    )?;

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

    // Packages PackageManager still tracks for this user but that aren't in the
    // enabled/disabled set are either Android 15+ archived apps (APK removed,
    // data + installer retained for unarchive) or uninstalled-with-data
    // remnants. `-u` works on every Android version, so this pass runs
    // unconditionally; archive probing only runs where archiving exists.
    let known_raw = t.shell_target(
        target,
        &[
            "pm", "list", "packages", "--user", &user, "-u", "-f", "-U", "-i",
        ],
    )?;
    let mut candidates = parse_pm_list(&known_raw, false);
    candidates.retain(|candidate| {
        !packages
            .iter()
            .any(|installed| installed.package == candidate.package)
    });
    let archived = if archive.supported {
        archived_package_names(
            t,
            target,
            user_id,
            &candidates
                .iter()
                .map(|candidate| candidate.package.clone())
                .collect::<Vec<_>>(),
        )?
    } else {
        HashSet::new()
    };
    for mut candidate in candidates {
        candidate.enabled = false;
        if archived.contains(&candidate.package) {
            candidate.archived = true;
        } else {
            candidate.retained = true;
        }
        packages.push(candidate);
    }

    let packages = packages
        .into_iter()
        .filter(|p| match filter {
            PackageFilter::All => true,
            PackageFilter::User => !p.system,
            PackageFilter::System => p.system,
            PackageFilter::Enabled => p.enabled && !p.archived && !p.retained,
            PackageFilter::Disabled => !p.enabled && !p.archived && !p.retained,
            PackageFilter::Archived => p.archived,
            PackageFilter::Retained => p.retained,
        })
        .collect();
    Ok(PackageListing { packages, archive })
}

fn archived_package_names(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    user_id: u32,
    packages: &[String],
) -> Result<HashSet<String>, TransportError> {
    const PROBE_BATCH_SIZE: usize = 128;
    const PROBE_SCRIPT: &str = "user=\"$1\"; shift; for package do if pm get-archived-package-metadata --user \"$user\" \"$package\" >/dev/null 2>&1; then printf '%s\\n' \"$package\"; fi; done";

    let user = user_id.to_string();
    let requested = packages.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut archived = HashSet::new();
    for batch in packages.chunks(PROBE_BATCH_SIZE) {
        let mut args = vec![
            "sh".to_string(),
            "-c".to_string(),
            PROBE_SCRIPT.to_string(),
            "droidsmith".to_string(),
            user.clone(),
        ];
        args.extend(batch.iter().cloned());
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        for package in t.shell_target(target, &refs)?.lines() {
            let package = package.trim();
            if requested.contains(package) && valid_package_name(package) {
                archived.insert(package.to_string());
            }
        }
    }
    Ok(archived)
}

pub fn list_packages(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    filter: PackageFilter,
    user_id: u32,
) -> Result<Vec<AppPackage>, TransportError> {
    Ok(list_packages_with_capability(t, target, filter, user_id)?.packages)
}

pub fn archive_capability(t: &dyn AdbTransport, target: &DeviceTarget) -> PackageArchiveCapability {
    let api_level = t
        .shell_target(target, &["getprop", "ro.build.version.sdk"])
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok());
    match api_level {
        Some(api) if api >= 35 => PackageArchiveCapability {
            supported: true,
            api_level: Some(api),
            reason: "Android 15+ package archiving is available".to_string(),
        },
        Some(api) => PackageArchiveCapability {
            supported: false,
            api_level: Some(api),
            reason: format!(
                "package archiving requires Android 15 (API 35); device reports API {api}"
            ),
        },
        None => PackageArchiveCapability {
            supported: false,
            api_level: None,
            reason: "could not determine the Android API level; package archiving is unavailable"
                .to_string(),
        },
    }
}

pub fn is_package_archived(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    user_id: u32,
    package: &str,
) -> Result<bool, TransportError> {
    if !valid_package_name(package) {
        return Err(TransportError::Parse(format!(
            "invalid package id {package:?}"
        )));
    }
    let user = user_id.to_string();
    // Discard the metadata payload (which may contain encoded icons) on the
    // device and use only the command status as the archive-state predicate.
    // User and package remain positional argv values, never interpolated.
    match t.shell_target(
        target,
        &[
            "sh",
            "-c",
            "pm get-archived-package-metadata --user \"$1\" \"$2\" >/dev/null",
            "droidsmith",
            &user,
            package,
        ],
    ) {
        Ok(_) => Ok(true),
        Err(TransportError::Exit { .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Inspect one package for one Android user, including packages removed for
/// that user but retained by PackageManager (`pm list packages -u`). System
/// provenance is derived from the reported APK path and therefore fails
/// closed when an OEM omits `-f` output.
pub fn inspect_package_presence(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    user_id: u32,
    package: &str,
) -> Result<PackagePresence, TransportError> {
    let user = user_id.to_string();
    for (enabled, state_flag) in [(false, "-d"), (true, "-e")] {
        let raw = t.shell_target(
            target,
            &[
                "pm", "list", "packages", "--user", &user, state_flag, "-f", package,
            ],
        )?;
        if let Some(entry) = parse_pm_list(&raw, enabled)
            .into_iter()
            .find(|entry| entry.package == package)
        {
            return Ok(PackagePresence::Installed {
                enabled,
                system: entry.system,
            });
        }
    }

    if archive_capability(t, target).supported && is_package_archived(t, target, user_id, package)?
    {
        return Ok(PackagePresence::Archived);
    }

    let retained = t.shell_target(
        target,
        &[
            "pm", "list", "packages", "--user", &user, "-u", "-f", package,
        ],
    )?;
    Ok(parse_pm_list(&retained, false)
        .into_iter()
        .find(|entry| entry.package == package)
        .map(|entry| PackagePresence::Retained {
            system: entry.system,
        })
        .unwrap_or(PackagePresence::Missing))
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
        archived: false,
        retained: false,
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
pub fn valid_package_name(s: &str) -> bool {
    if s.is_empty() || s.starts_with('.') || s.ends_with('.') {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::transport::MockTransport;

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
            "package:.leading-dot\n",
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
            &[
                "pm", "list", "packages", "--user", "0", "-e", "-f", "-U", "-i",
            ],
            Ok(ENABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-d", "-f", "-U", "-i",
            ],
            Ok(DISABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-u", "-f", "-U", "-i",
            ],
            Ok(String::new()),
        );

        let v = list_packages(&mock, &target(), PackageFilter::All, 0).unwrap();
        assert_eq!(v.len(), 4);
        let enabled: Vec<_> = v.iter().filter(|p| p.enabled).collect();
        assert_eq!(enabled.len(), 3);
    }

    #[test]
    fn android_15_listing_distinguishes_archived_from_retained_data() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &["getprop", "ro.build.version.sdk"],
            Ok("35\n".to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-e", "-f", "-U", "-i",
            ],
            Ok("package:/data/app/base.apk=com.example.installed\n".to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-d", "-f", "-U", "-i",
            ],
            Ok(String::new()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-u", "-f", "-U", "-i",
            ],
            Ok("package:com.example.installed\npackage:com.example.archived installer=com.android.vending\npackage:com.example.retained\n".to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "sh",
                "-c",
                "user=\"$1\"; shift; for package do if pm get-archived-package-metadata --user \"$user\" \"$package\" >/dev/null 2>&1; then printf '%s\\n' \"$package\"; fi; done",
                "droidsmith",
                "0",
                "com.example.archived",
                "com.example.retained",
            ],
            Ok("com.example.archived\n".to_string()),
        );

        let listing =
            list_packages_with_capability(&mock, &target(), PackageFilter::All, 0).unwrap();
        assert!(listing.archive.supported);
        assert_eq!(listing.packages.len(), 3);
        assert!(listing
            .packages
            .iter()
            .any(|package| package.package == "com.example.archived"
                && package.archived
                && !package.retained));
        assert!(listing
            .packages
            .iter()
            .any(|package| package.package == "com.example.retained"
                && package.retained
                && !package.archived
                && !package.enabled));
    }

    #[test]
    fn retained_filter_surfaces_only_uninstalled_with_data_packages() {
        let mock = MockTransport::new();
        // API < 35: archiving unsupported, so every non-installed `-u` remnant
        // is surfaced as retained-data rather than probed for archive state.
        mock.expect_shell(
            "abc",
            &["getprop", "ro.build.version.sdk"],
            Ok("34\n".to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-e", "-f", "-U", "-i",
            ],
            Ok("package:/data/app/base.apk=com.example.installed\n".to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-d", "-f", "-U", "-i",
            ],
            Ok(String::new()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-u", "-f", "-U", "-i",
            ],
            Ok("package:com.example.installed\npackage:com.example.ghost\n".to_string()),
        );

        let listing =
            list_packages_with_capability(&mock, &target(), PackageFilter::Retained, 0).unwrap();
        assert!(!listing.archive.supported);
        assert_eq!(listing.packages.len(), 1);
        let ghost = &listing.packages[0];
        assert_eq!(ghost.package, "com.example.ghost");
        assert!(ghost.retained);
        assert!(!ghost.archived);
        assert!(!ghost.enabled);
    }

    #[test]
    fn list_packages_user_filter_excludes_system() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-e", "-f", "-U", "-i",
            ],
            Ok(ENABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-d", "-f", "-U", "-i",
            ],
            Ok(DISABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-u", "-f", "-U", "-i",
            ],
            Ok(String::new()),
        );
        let v = list_packages(&mock, &target(), PackageFilter::User, 0).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].package, "com.example.foo");
    }

    #[test]
    fn list_packages_disabled_filter() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-e", "-f", "-U", "-i",
            ],
            Ok(ENABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-d", "-f", "-U", "-i",
            ],
            Ok(DISABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm", "list", "packages", "--user", "0", "-u", "-f", "-U", "-i",
            ],
            Ok(String::new()),
        );
        let v = list_packages(&mock, &target(), PackageFilter::Disabled, 0).unwrap();
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

    #[test]
    fn package_presence_distinguishes_retained_system_from_missing() {
        let mock = MockTransport::new();
        for flag in ["-d", "-e"] {
            mock.expect_shell(
                "abc",
                &[
                    "pm",
                    "list",
                    "packages",
                    "--user",
                    "10",
                    flag,
                    "-f",
                    "com.system.old",
                ],
                Ok(String::new()),
            );
        }
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "10",
                "-u",
                "-f",
                "com.system.old",
            ],
            Ok("package:/system/app/Old/Old.apk=com.system.old\n".into()),
        );

        assert_eq!(
            inspect_package_presence(&mock, &target(), 10, "com.system.old").unwrap(),
            PackagePresence::Retained { system: true }
        );
    }

    #[test]
    fn package_presence_preserves_installed_provenance_and_enabled_state() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-d",
                "-f",
                "com.example.foo",
            ],
            Ok(String::new()),
        );
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-e",
                "-f",
                "com.example.foo",
            ],
            Ok("package:/data/app/com.example.foo/base.apk=com.example.foo\n".into()),
        );

        assert_eq!(
            inspect_package_presence(&mock, &target(), 0, "com.example.foo").unwrap(),
            PackagePresence::Installed {
                enabled: true,
                system: false,
            }
        );
    }
}
