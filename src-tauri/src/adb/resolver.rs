//! Find the `adb` binary on this machine and probe its version.
//!
//! The resolver is intentionally I/O-light and side-effect-free apart from
//! the version probe (a single child process with a hard 2s wall clock).
//! It is cached behind a `OnceLock` so a `heartbeat` storm doesn't spawn
//! a fresh `adb version` every call — `invalidate_cache()` exists for the
//! Settings → Diagnostics "rescan" button (UI piece lands later).
//!
//! Module boundary: this file owns _only_ resolution and probing. Actual
//! ADB operations (device listing, shell, install/uninstall) land with
//! R-011 in a `transport::` submodule; the trait will accept a resolved
//! `AdbResolution` so we can stub it for tests without spawning real
//! children.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;

/// Result of resolving the `adb` binary on this machine.
#[derive(Debug, Serialize, Clone)]
pub struct AdbResolution {
    pub path: Option<String>,
    pub source: ResolveSource,
    pub version: Option<String>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // `Bundled` is reserved for R-010 once the sidecar lands
pub enum ResolveSource {
    /// Found on the user's `PATH`.
    Path,
    /// Found via the `ANDROID_HOME` or `ANDROID_SDK_ROOT` env var.
    AndroidHome,
    /// Found at a platform-default Android Studio install location.
    AndroidStudio,
    /// Found at a Homebrew prefix on macOS.
    Homebrew,
    /// Found at a common Linux distro install location.
    DistroPackage,
    /// Bundled with Droidsmith as a sidecar (resolved at runtime by the
    /// shell plugin, not by this fn).
    Bundled,
    /// Not found anywhere we know to look.
    NotFound,
}

/// Inputs to `candidate_paths`. Pulling these out of `locate_adb` lets us
/// unit-test the path-building rules without mutating process-global env
/// state — which previously broke `cargo test` parallelism.
#[derive(Debug, Default, Clone)]
pub struct ResolverEnv {
    pub android_home: Option<PathBuf>,
    pub android_sdk_root: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

impl ResolverEnv {
    /// Snapshot the real environment.
    pub fn from_os() -> Self {
        Self {
            android_home: read_env_path("ANDROID_HOME"),
            android_sdk_root: read_env_path("ANDROID_SDK_ROOT"),
            home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from),
        }
    }
}

/// Read an env var, returning `None` if unset _or_ empty (Windows happily
/// returns `Some("")` for `set X=`).
fn read_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).and_then(|s| {
        if s.is_empty() {
            None
        } else {
            Some(PathBuf::from(s))
        }
    })
}

static CACHE: OnceLock<AdbResolution> = OnceLock::new();

/// Search for `adb` on this machine. Order:
/// 1. `$PATH` (most specific to the user)
/// 2. `$ANDROID_HOME/platform-tools/adb[.exe]`
/// 3. `$ANDROID_SDK_ROOT/platform-tools/adb[.exe]`
/// 4. Platform-default Android Studio install paths
/// 5. Homebrew prefix on macOS
/// 6. Common Linux distro packages
///
/// The bundled sidecar is resolved separately via the Tauri shell plugin
/// (`bundle.externalBin` in `tauri.conf.json`) — lands with R-010.
///
/// Result is cached for the process lifetime. Call `invalidate_cache` to
/// force a rescan (e.g. after the user installed Android Studio while the
/// app was running).
pub fn locate_adb() -> AdbResolution {
    CACHE
        .get_or_init(|| resolve(&ResolverEnv::from_os()))
        .clone()
}

// Note: tests bypass the cache and call `resolve` directly, so they're
// process-local and parallel-safe. A future Settings → Diagnostics
// "rescan" button will need a thread-safe invalidate; defer until that UI
// exists rather than shipping a stub.

/// Resolve in terms of an explicit `ResolverEnv`. Pure, no global state.
/// Public for tests and for any future caller (e.g. CLI) that wants to
/// supply its own env without touching `OnceLock`.
pub fn resolve(env: &ResolverEnv) -> AdbResolution {
    // 1. PATH
    if let Ok(p) = which::which("adb") {
        return finalize(p, ResolveSource::Path);
    }

    // 2..6. candidate paths
    for (path, source) in candidate_paths(env) {
        if path.is_file() {
            return finalize(path, source);
        }
    }

    AdbResolution {
        path: None,
        source: ResolveSource::NotFound,
        version: None,
    }
}

fn finalize(path: PathBuf, source: ResolveSource) -> AdbResolution {
    let version = probe_version(&path);
    AdbResolution {
        path: Some(path.display().to_string()),
        source,
        version,
    }
}

/// Pure path-building. No I/O. Order matches the doc comment on
/// `locate_adb` minus the `$PATH` step which is handled separately.
fn candidate_paths(env: &ResolverEnv) -> Vec<(PathBuf, ResolveSource)> {
    let mut out = Vec::new();
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };

    for root in [env.android_home.as_ref(), env.android_sdk_root.as_ref()]
        .into_iter()
        .flatten()
    {
        out.push((
            root.join("platform-tools").join(exe),
            ResolveSource::AndroidHome,
        ));
    }

    if let Some(home) = env.home.as_ref() {
        // Android Studio default install locations
        let studio_subpath = if cfg!(windows) {
            "AppData/Local/Android/Sdk/platform-tools/adb.exe"
        } else if cfg!(target_os = "macos") {
            "Library/Android/sdk/platform-tools/adb"
        } else {
            "Android/Sdk/platform-tools/adb"
        };
        out.push((home.join(studio_subpath), ResolveSource::AndroidStudio));
    }

    // macOS Homebrew (both Intel and Apple Silicon prefixes)
    if cfg!(target_os = "macos") {
        out.push((
            PathBuf::from("/opt/homebrew/bin/adb"),
            ResolveSource::Homebrew,
        ));
        out.push((PathBuf::from("/usr/local/bin/adb"), ResolveSource::Homebrew));
    }

    // Distro packages (Linux only)
    if cfg!(target_os = "linux") {
        for p in [
            "/usr/lib/android-sdk/platform-tools/adb",
            "/usr/local/bin/adb",
            "/usr/bin/adb",
            "/opt/android-sdk/platform-tools/adb",
        ] {
            out.push((PathBuf::from(p), ResolveSource::DistroPackage));
        }
    }

    out
}

/// Run `<adb> version` with a 2s wall-clock timeout. Returns the first
/// line of stdout trimmed, or `None` on any failure.
///
/// **Concurrency model:** we read stdout on a worker thread while the main
/// thread polls `try_wait`. Reading and waiting are not interleaved — so
/// a verbose `adb version` cannot deadlock the child by filling the OS
/// pipe buffer before we collect it.
fn probe_version(path: &Path) -> Option<String> {
    let mut child = Command::new(path)
        .arg("version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::with_capacity(256);
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(2);
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break None,
        }
    };

    let buf = reader.join().unwrap_or_default();
    let status_ok = exit_status.map(|s| s.success()).unwrap_or(false);
    if !status_ok {
        return None;
    }
    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_returns_a_resolution_regardless_of_environment() {
        let env = ResolverEnv::default();
        let r = resolve(&env);
        match r.source {
            ResolveSource::NotFound => assert!(r.path.is_none()),
            _ => assert!(r.path.is_some()),
        }
    }

    #[test]
    fn candidate_paths_honours_android_home() {
        let env = ResolverEnv {
            android_home: Some(PathBuf::from("/fake/sdk")),
            ..ResolverEnv::default()
        };
        let cs = candidate_paths(&env);
        assert!(cs.iter().any(|(p, src)| {
            p.to_string_lossy()
                .replace('\\', "/")
                .contains("/fake/sdk/platform-tools/")
                && *src == ResolveSource::AndroidHome
        }));
    }

    #[test]
    fn candidate_paths_honours_android_sdk_root() {
        let env = ResolverEnv {
            android_sdk_root: Some(PathBuf::from("/sdk-root")),
            ..ResolverEnv::default()
        };
        let cs = candidate_paths(&env);
        assert!(cs.iter().any(|(p, src)| {
            p.to_string_lossy()
                .replace('\\', "/")
                .contains("/sdk-root/platform-tools/")
                && *src == ResolveSource::AndroidHome
        }));
    }

    #[test]
    fn candidate_paths_skips_empty_env_vars() {
        // `read_env_path` returns None for empty strings; if it ever
        // changed to return Some(""), we'd push "/platform-tools/adb"
        // here. This test guards the public contract.
        let env = ResolverEnv {
            android_home: None,
            android_sdk_root: None,
            home: None,
        };
        let cs = candidate_paths(&env);
        for (p, _) in &cs {
            let s = p.to_string_lossy();
            assert!(
                !s.starts_with("/platform-tools"),
                "expected no bare /platform-tools entry, got {s}"
            );
        }
    }

    #[test]
    fn read_env_path_returns_none_for_empty() {
        // SAFETY: this test mutates process-global env. We restore
        // immediately. cargo test runs tests in parallel within a
        // binary, but only ONE test touches DROIDSMITH_TEST_ENV — so
        // there's no race.
        std::env::set_var("DROIDSMITH_TEST_ENV", "");
        assert!(read_env_path("DROIDSMITH_TEST_ENV").is_none());
        std::env::remove_var("DROIDSMITH_TEST_ENV");

        std::env::set_var("DROIDSMITH_TEST_ENV", "/x");
        assert_eq!(
            read_env_path("DROIDSMITH_TEST_ENV"),
            Some(PathBuf::from("/x"))
        );
        std::env::remove_var("DROIDSMITH_TEST_ENV");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_candidates_include_homebrew_prefixes() {
        let env = ResolverEnv::default();
        let cs = candidate_paths(&env);
        let has_apple_silicon = cs.iter().any(|(p, src)| {
            p == &PathBuf::from("/opt/homebrew/bin/adb") && *src == ResolveSource::Homebrew
        });
        let has_intel = cs.iter().any(|(p, src)| {
            p == &PathBuf::from("/usr/local/bin/adb") && *src == ResolveSource::Homebrew
        });
        assert!(has_apple_silicon, "Homebrew apple-silicon path missing");
        assert!(has_intel, "Homebrew intel path missing");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_candidates_include_distro_paths() {
        let env = ResolverEnv::default();
        let cs = candidate_paths(&env);
        assert!(cs
            .iter()
            .any(|(p, src)| p == &PathBuf::from("/usr/bin/adb")
                && *src == ResolveSource::DistroPackage));
    }
}
