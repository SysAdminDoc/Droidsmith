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

const ANDROID_UID_USER_RANGE: u32 = 100_000;
const ANDROID_SYSTEM_APP_ID: u32 = 1_000;

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
pub struct PackageSubcommandCapability {
    pub supported: bool,
    pub reason: String,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageActionCapabilities {
    pub suspend: PackageSubcommandCapability,
    pub unsuspend: PackageSubcommandCapability,
    /// `pm get-package-storage-stats`. Probed the same way as the mutating
    /// subcommands because OEMs drop it just as freely; API level is not an
    /// authority.
    pub storage_stats: PackageSubcommandCapability,
}

/// Per-package storage as PackageManager reports it, in bytes.
///
/// Read from `pm get-package-storage-stats`, which is the documented AOSP
/// surface for this and the only way to get a real number rather than an
/// estimate from the APK size. Absent on devices that do not advertise the
/// subcommand, which is why the whole struct is optional upstream: reporting
/// "unavailable" is correct, guessing is not.
#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PackageStorageStats {
    /// Installed app code — the APK and its extracted artifacts.
    pub code_bytes: u64,
    pub data_bytes: u64,
    pub cache_bytes: u64,
}

/// Parse `pm get-package-storage-stats` output.
///
/// The command prints `<field>: <n> bytes (<human>)` lines; the human-readable
/// suffix is deliberately ignored, since it is rounded. Missing fields default
/// to zero rather than failing the whole read, but a response with none of the
/// expected fields returns `None` — an empty struct would be indistinguishable
/// from a package that genuinely occupies nothing.
pub fn parse_package_storage_stats(stdout: &str) -> Option<PackageStorageStats> {
    let mut stats = PackageStorageStats {
        code_bytes: 0,
        data_bytes: 0,
        cache_bytes: 0,
    };
    let mut seen = false;
    for line in stdout.lines() {
        let Some((field, rest)) = line.trim().split_once(':') else {
            continue;
        };
        // `<n> bytes (…)` — take the leading integer and ignore the rest.
        let Some(value) = rest
            .split_whitespace()
            .next()
            .and_then(|token| token.parse::<u64>().ok())
        else {
            continue;
        };
        match field.trim() {
            "code" => {
                stats.code_bytes = value;
                seen = true;
            }
            "data" => {
                stats.data_bytes = value;
                seen = true;
            }
            "cache" => {
                stats.cache_bytes = value;
                seen = true;
            }
            _ => {}
        }
    }
    seen.then_some(stats)
}

/// Read one package's storage, or `None` when the device does not support the
/// command or did not answer with usable fields.
///
/// Deliberately never falls back to an APK-size estimate: an estimate that
/// looks like a measurement is worse than an honest gap.
pub fn package_storage_stats(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    user_id: u32,
    package: &str,
) -> Option<PackageStorageStats> {
    if !valid_package_name(package) {
        return None;
    }
    let user = user_id.to_string();
    let output = t
        .shell_target(
            target,
            &["pm", "get-package-storage-stats", "--user", &user, package],
        )
        .ok()?;
    parse_package_storage_stats(&output)
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

/// Can `pm uninstall --user N` be undone for this package?
///
/// This is the question users need answered *before* the irreversible step,
/// not after. `pm install-existing` only works when PackageManager still holds
/// the APK on a read-only partition; for a package that only ever lived in
/// `/data/app`, uninstall-for-user is final.
#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstallRecoverability {
    /// PackageManager reports the package as a system package for this user,
    /// so the platform APK survives the uninstall and `install-existing`
    /// restores it.
    Recoverable,
    /// The package is installed for this user and is not a system package.
    /// Uninstalling removes the only copy of the APK.
    NotRecoverable,
    /// The device did not give an answer this can be derived from. Never
    /// presented as recoverable.
    Unknown,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallRecoveryEvidence {
    pub verdict: UninstallRecoverability,
    /// Stable code naming the evidence the verdict rests on, so the UI can
    /// explain *why* rather than only *what*.
    pub reason_code: String,
    /// APK path PackageManager reported, when it reported one. Retained for
    /// support bundles: it is the single most useful datum when a verdict is
    /// disputed.
    pub apk_path: Option<String>,
}

impl UninstallRecoveryEvidence {
    fn new(verdict: UninstallRecoverability, reason_code: &str, apk_path: Option<String>) -> Self {
        Self {
            verdict,
            reason_code: reason_code.to_string(),
            apk_path,
        }
    }

    pub fn unknown(reason_code: &str) -> Self {
        Self::new(UninstallRecoverability::Unknown, reason_code, None)
    }
}

/// Prove — before the mutation — whether uninstalling `package` for `user_id`
/// can be undone.
///
/// The authority is `pm list packages -s`, which filters on
/// `ApplicationInfo.FLAG_SYSTEM`. That flag matters more than the APK path:
/// an *updated* system app reports a `/data/app` path while still carrying the
/// flag, and uninstalling it for a user falls back to the platform APK rather
/// than removing it. Classifying on the path alone — which is all
/// [`AppPackage::system`] can do — would call those packages unrecoverable and
/// scare users away from a reversible action.
///
/// Every failure mode returns `Unknown`. A probe that cannot answer must not
/// produce a verdict, because the cost of a wrong `Recoverable` is a package
/// the user can never get back.
pub fn assess_uninstall_recovery(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    user_id: u32,
    package: &str,
) -> UninstallRecoveryEvidence {
    if !valid_package_name(package) {
        return UninstallRecoveryEvidence::unknown("invalid_package_name");
    }
    let user = user_id.to_string();

    let installed = match t.shell_target(
        target,
        &["pm", "list", "packages", "--user", &user, "-f", package],
    ) {
        Ok(raw) => parse_pm_list(&raw, true)
            .into_iter()
            .find(|entry| entry.package == package),
        Err(_) => return UninstallRecoveryEvidence::unknown("probe_failed"),
    };
    let Some(installed) = installed else {
        // Nothing to uninstall for this user, so there is no verdict to give.
        return UninstallRecoveryEvidence::unknown("package_not_installed_for_user");
    };
    let apk_path = installed.apk_path.clone();

    let system = match t.shell_target(
        target,
        &[
            "pm", "list", "packages", "--user", &user, "-s", "-f", package,
        ],
    ) {
        Ok(raw) => parse_pm_list(&raw, true)
            .into_iter()
            .any(|entry| entry.package == package),
        Err(_) => {
            return UninstallRecoveryEvidence::new(
                UninstallRecoverability::Unknown,
                "system_flag_probe_failed",
                apk_path,
            )
        }
    };

    if system {
        return UninstallRecoveryEvidence::new(
            UninstallRecoverability::Recoverable,
            "platform_apk_retained",
            apk_path,
        );
    }
    // Not flagged system. If the APK also sits on a read-only partition the
    // two signals disagree, which is not a state this can adjudicate — some
    // OEM builds stage packages in unusual places. Say so instead of guessing.
    if apk_path.as_deref().is_some_and(is_system_path) {
        return UninstallRecoveryEvidence::new(
            UninstallRecoverability::Unknown,
            "system_flag_conflicts_with_apk_path",
            apk_path,
        );
    }
    UninstallRecoveryEvidence::new(
        UninstallRecoverability::NotRecoverable,
        "only_copy_is_user_installed",
        apk_path,
    )
}

impl AppPackage {
    /// Convenience filter: matches Android's "Show system" toggle in
    /// the Settings → Apps screen. Kept ahead of the renderer-side
    /// filter UI so the rule lives next to the type.
    #[allow(dead_code)]
    pub fn is_user(&self) -> bool {
        !self.system
    }

    /// True when PackageManager reports the well-known `android.uid.system`
    /// app id for this Android user. Secondary users offset Linux UIDs by
    /// `PER_USER_RANGE`, so compare the app-id portion rather than only UID
    /// 1000. A missing UID is unknown and must never be classified as safe or
    /// unsafe from this signal alone.
    pub fn uses_android_system_uid(&self) -> bool {
        self.uid
            .is_some_and(|uid| uid % ANDROID_UID_USER_RANGE == ANDROID_SYSTEM_APP_ID)
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
/// Run one `pm list packages` filter pass, preferring the enriched flag set
/// (`-U` UID, `-i` installer) but retrying with the core flags when a device
/// rejects them. `-U` only exists on Android 9+ (and some OEM builds trim
/// `-i`), and an unknown flag makes `pm` exit non-zero, which would otherwise
/// abort the whole enumeration. Losing uid/installer on older devices is an
/// acceptable degradation; a genuine failure of the reduced query propagates.
fn list_packages_raw(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    user: &str,
    filter_flag: &str,
) -> Result<String, TransportError> {
    match t.shell_target(
        target,
        &[
            "pm",
            "list",
            "packages",
            "--user",
            user,
            filter_flag,
            "-f",
            "-U",
            "-i",
        ],
    ) {
        Ok(raw) => Ok(raw),
        Err(TransportError::Exit { .. }) => t.shell_target(
            target,
            &["pm", "list", "packages", "--user", user, filter_flag, "-f"],
        ),
        Err(other) => Err(other),
    }
}

pub fn list_packages_with_capability(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
    filter: PackageFilter,
    user_id: u32,
) -> Result<PackageListing, TransportError> {
    let user = user_id.to_string();
    let archive = archive_capability(t, target);
    let enabled_raw = list_packages_raw(t, target, &user, "-e")?;
    let disabled_raw = list_packages_raw(t, target, &user, "-d")?;

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
    // The `-u` (also-uninstalled) pass and the archive probe only enrich the
    // list with archived/retained remnants. If a device or vendor `pm` rejects
    // them, degrade to the core enabled/disabled list rather than failing the
    // whole enumeration.
    let candidates = match list_packages_raw(t, target, &user, "-u") {
        Ok(known_raw) => {
            let mut candidates = parse_pm_list(&known_raw, false);
            candidates.retain(|candidate| {
                !packages
                    .iter()
                    .any(|installed| installed.package == candidate.package)
            });
            candidates
        }
        Err(TransportError::Exit { .. }) => Vec::new(),
        Err(other) => return Err(other),
    };
    let archived = if archive.supported && !candidates.is_empty() {
        archived_package_names(
            t,
            target,
            user_id,
            &candidates
                .iter()
                .map(|candidate| candidate.package.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default()
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

/// Probe package-manager action support from the device's own command surface.
/// OEMs routinely backport or remove individual `pm` subcommands, so API level
/// is not an authority for these actions.
pub fn package_action_capabilities(
    t: &dyn AdbTransport,
    target: &DeviceTarget,
) -> PackageActionCapabilities {
    match t.shell_target(target, &["pm", "help"]) {
        Ok(help) => parse_package_action_capabilities(&help),
        Err(error) => {
            let reason = format!("could not inspect package-manager commands: {error}");
            PackageActionCapabilities {
                suspend: PackageSubcommandCapability {
                    supported: false,
                    reason: reason.clone(),
                },
                unsuspend: PackageSubcommandCapability {
                    supported: false,
                    reason: reason.clone(),
                },
                storage_stats: PackageSubcommandCapability {
                    supported: false,
                    reason,
                },
            }
        }
    }
}

pub fn parse_package_action_capabilities(help: &str) -> PackageActionCapabilities {
    let has = |command: &str| {
        help.lines()
            .map(str::trim_start)
            .filter(|line| !line.starts_with('#'))
            .any(|line| line.split_whitespace().next() == Some(command))
    };
    let capability = |command: &str| {
        let supported = has(command);
        PackageSubcommandCapability {
            supported,
            reason: if supported {
                format!("pm {command} is advertised by this device")
            } else {
                format!("pm {command} is not advertised by this device")
            },
        }
    };
    PackageActionCapabilities {
        suspend: capability("suspend"),
        unsuspend: capability("unsuspend"),
        storage_stats: capability("get-package-storage-stats"),
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
    if s.is_empty() || s.starts_with('.') || s.starts_with('-') || s.ends_with('.') {
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
    fn pm_help_capabilities_require_each_advertised_subcommand() {
        let capabilities = parse_package_action_capabilities(
            "Package manager commands:\n  suspend [--user USER_ID] PACKAGE\n  list packages\n",
        );
        assert!(capabilities.suspend.supported);
        assert!(!capabilities.unsuspend.supported);
        assert!(capabilities.unsuspend.reason.contains("not advertised"));

        let both = parse_package_action_capabilities(
            "  suspend [--user USER_ID] PACKAGE\n  unsuspend [--user USER_ID] PACKAGE\n",
        );
        assert!(both.suspend.supported);
        assert!(both.unsuspend.supported);
        // Storage stats are probed the same way, and absent by default.
        assert!(!both.storage_stats.supported);
        let with_stats = parse_package_action_capabilities(
            "  get-package-storage-stats [--user <USER_ID>] <PACKAGE>\n",
        );
        assert!(with_stats.storage_stats.supported);
    }

    #[test]
    fn storage_stats_parse_real_output_and_refuse_to_guess() {
        // Verbatim from a Samsung SM-S938B on SDK 36.
        let stats = parse_package_storage_stats(
            "code: 3584 bytes (3.50 Kb)\n\
             data: 6068736 bytes (5.79 Mb)\n\
             cache: 439808 bytes (429.50 Kb)\n\
             apk: 0 bytes\n\
             lib: 0 bytes\n",
        )
        .expect("real output parses");
        // The rounded human-readable suffix is ignored; the byte count is the
        // value, and 5.79 Mb would not round-trip to 6068736.
        assert_eq!(stats.code_bytes, 3584);
        assert_eq!(stats.data_bytes, 6_068_736);
        assert_eq!(stats.cache_bytes, 439_808);

        // A genuinely empty package still reports its fields, so zeroes are a
        // measurement...
        let zeroed =
            parse_package_storage_stats("code: 0 bytes\ndata: 0 bytes\ncache: 0 bytes\n").unwrap();
        assert_eq!(zeroed.code_bytes, 0);

        // ...but a device that answered with none of them is unavailable, not
        // zero. Reporting 0 B for "we could not ask" is the exact failure this
        // guards against.
        for unusable in [
            "",
            "Unknown command: get-package-storage-stats",
            "Exception occurred while executing 'get-package-storage-stats'",
            "code: not-a-number bytes",
        ] {
            assert!(
                parse_package_storage_stats(unusable).is_none(),
                "{unusable:?} must not parse as a measurement"
            );
        }

        // Partial output is honoured for the fields that are present.
        let partial = parse_package_storage_stats("cache: 12 bytes (12.00 B)\n").unwrap();
        assert_eq!(partial.cache_bytes, 12);
        assert_eq!(partial.code_bytes, 0);
    }

    #[test]
    fn failed_pm_help_probe_hides_optional_actions() {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &["pm", "help"],
            Err(TransportError::Parse("vendor pm failed".into())),
        );
        let capabilities = package_action_capabilities(&mock, &target());
        assert!(!capabilities.suspend.supported);
        assert!(!capabilities.unsuspend.supported);
        assert!(capabilities.suspend.reason.contains("could not inspect"));
    }

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
    fn system_uid_detection_handles_owner_and_secondary_android_users() {
        let mut package = AppPackage {
            package: "com.example.system".to_string(),
            enabled: true,
            system: true,
            apk_path: Some("/system/app/System/System.apk".to_string()),
            uid: Some(1_000),
            installer: None,
            archived: false,
            retained: false,
        };
        assert!(package.uses_android_system_uid());

        package.uid = Some(101_000);
        assert!(package.uses_android_system_uid());

        package.uid = Some(10_042);
        assert!(!package.uses_android_system_uid());
        package.uid = None;
        assert!(!package.uses_android_system_uid());
    }

    #[test]
    fn package_names_cannot_be_reinterpreted_as_options() {
        assert!(!valid_package_name("--user"));
        assert!(!valid_package_name("-rf"));
        assert!(valid_package_name("com.vendor.feature-name"));
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
    fn list_packages_falls_back_when_enriched_flags_are_rejected() {
        // A device on Android < 9 rejects `-U`; enumeration must degrade to the
        // core `-f` flags instead of failing outright.
        let mock = MockTransport::new();
        let reject = || {
            Err(TransportError::Exit {
                code: 255,
                stderr: "Error: Unknown option: -U".to_string(),
            })
        };
        for filter in ["-e", "-d", "-u"] {
            mock.expect_shell(
                "abc",
                &[
                    "pm", "list", "packages", "--user", "0", filter, "-f", "-U", "-i",
                ],
                reject(),
            );
        }
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "--user", "0", "-e", "-f"],
            Ok(ENABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "--user", "0", "-d", "-f"],
            Ok(DISABLED_FIXTURE.to_string()),
        );
        mock.expect_shell(
            "abc",
            &["pm", "list", "packages", "--user", "0", "-u", "-f"],
            Ok(String::new()),
        );

        let v = list_packages(&mock, &target(), PackageFilter::All, 0).unwrap();
        assert_eq!(v.len(), 4);
        assert_eq!(v.iter().filter(|p| p.enabled).count(), 3);
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

    fn recovery_mock(
        listing: Result<String, TransportError>,
        system_listing: Option<Result<String, TransportError>>,
    ) -> MockTransport {
        let mock = MockTransport::new();
        mock.expect_shell(
            "abc",
            &[
                "pm",
                "list",
                "packages",
                "--user",
                "0",
                "-f",
                "com.example.app",
            ],
            listing,
        );
        if let Some(system_listing) = system_listing {
            mock.expect_shell(
                "abc",
                &[
                    "pm",
                    "list",
                    "packages",
                    "--user",
                    "0",
                    "-s",
                    "-f",
                    "com.example.app",
                ],
                system_listing,
            );
        }
        mock
    }

    #[test]
    fn an_updated_system_app_is_recoverable_despite_its_data_apk_path() {
        // The trap R-122 exists to close: an updated system app reports a
        // /data/app path, so path-based classification calls it a user app and
        // would warn that uninstalling is final. FLAG_SYSTEM says otherwise,
        // and install-existing does restore it.
        let mock = recovery_mock(
            Ok("package:/data/app/~~a==/com.example.app-b==/base.apk=com.example.app\n".into()),
            Some(Ok(
                "package:/data/app/~~a==/com.example.app-b==/base.apk=com.example.app\n".into(),
            )),
        );
        let evidence = assess_uninstall_recovery(&mock, &target(), 0, "com.example.app");
        assert_eq!(evidence.verdict, UninstallRecoverability::Recoverable);
        assert_eq!(evidence.reason_code, "platform_apk_retained");
        assert!(evidence
            .apk_path
            .as_deref()
            .unwrap()
            .starts_with("/data/app"));
    }

    #[test]
    fn a_user_installed_package_is_reported_as_not_recoverable() {
        let mock = recovery_mock(
            Ok("package:/data/app/~~a==/com.example.app-b==/base.apk=com.example.app\n".into()),
            Some(Ok(String::new())),
        );
        let evidence = assess_uninstall_recovery(&mock, &target(), 0, "com.example.app");
        assert_eq!(evidence.verdict, UninstallRecoverability::NotRecoverable);
        assert_eq!(evidence.reason_code, "only_copy_is_user_installed");
    }

    #[test]
    fn every_unanswerable_probe_reports_unknown_rather_than_recoverable() {
        let cases: Vec<(&str, MockTransport)> = vec![
            (
                "probe_failed",
                recovery_mock(Err(TransportError::Parse("adb died".into())), None),
            ),
            (
                "package_not_installed_for_user",
                recovery_mock(Ok(String::new()), None),
            ),
            (
                "system_flag_probe_failed",
                recovery_mock(
                    Ok("package:/system/app/A/A.apk=com.example.app\n".into()),
                    Some(Err(TransportError::Parse("adb died".into()))),
                ),
            ),
            (
                // A read-only APK path with no FLAG_SYSTEM is contradictory
                // evidence, not a licence to guess either way.
                "system_flag_conflicts_with_apk_path",
                recovery_mock(
                    Ok("package:/system/app/A/A.apk=com.example.app\n".into()),
                    Some(Ok(String::new())),
                ),
            ),
        ];
        for (expected, mock) in cases {
            let evidence = assess_uninstall_recovery(&mock, &target(), 0, "com.example.app");
            assert_eq!(
                evidence.verdict,
                UninstallRecoverability::Unknown,
                "{expected} must not produce a verdict"
            );
            assert_eq!(evidence.reason_code, expected);
        }

        let rejected = assess_uninstall_recovery(&MockTransport::new(), &target(), 0, ".bad");
        assert_eq!(rejected.verdict, UninstallRecoverability::Unknown);
        assert_eq!(rejected.reason_code, "invalid_package_name");
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
