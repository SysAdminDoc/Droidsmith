use std::path::PathBuf;
use std::process::Command;
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
    /// Found at a platform-default Android Studio location.
    AndroidStudio,
    /// Found at a common Linux distro location.
    DistroPackage,
    /// Bundled with Droidsmith as a sidecar (resolved at runtime by the
    /// shell plugin, not by this fn).
    Bundled,
    /// Not found anywhere we know to look.
    NotFound,
}

/// Search for `adb` on this machine. Order:
/// 1. `$PATH`
/// 2. `$ANDROID_HOME/platform-tools/adb[.exe]`
/// 3. `$ANDROID_SDK_ROOT/platform-tools/adb[.exe]`
/// 4. Platform-default Android Studio install paths
/// 5. Common Linux distro packages
///
/// Returns the resolved path with its discovery source. The bundled sidecar
/// fallback is resolved separately via the Tauri shell plugin (see
/// `src-tauri/tauri.conf.json` `bundle.externalBin`).
pub fn locate_adb() -> AdbResolution {
    if let Ok(p) = which::which("adb") {
        let version = adb_version(&p);
        return AdbResolution {
            path: Some(p.display().to_string()),
            source: ResolveSource::Path,
            version,
        };
    }

    for (path, source) in candidate_paths() {
        if path.is_file() {
            let version = adb_version(&path);
            return AdbResolution {
                path: Some(path.display().to_string()),
                source,
                version,
            };
        }
    }

    AdbResolution {
        path: None,
        source: ResolveSource::NotFound,
        version: None,
    }
}

fn candidate_paths() -> Vec<(PathBuf, ResolveSource)> {
    let mut out = Vec::new();
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };

    // Env-var driven (highest priority after PATH)
    for env_var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(root) = std::env::var_os(env_var) {
            let mut p = PathBuf::from(root);
            p.push("platform-tools");
            p.push(exe);
            out.push((p, ResolveSource::AndroidHome));
        }
    }

    // Android Studio default install locations
    if let Some(home) = dirs_home() {
        if cfg!(windows) {
            out.push((
                home.join("AppData/Local/Android/Sdk/platform-tools/adb.exe"),
                ResolveSource::AndroidStudio,
            ));
        } else if cfg!(target_os = "macos") {
            out.push((
                home.join("Library/Android/sdk/platform-tools/adb"),
                ResolveSource::AndroidStudio,
            ));
        } else {
            out.push((
                home.join("Android/Sdk/platform-tools/adb"),
                ResolveSource::AndroidStudio,
            ));
        }
    }

    // Distro packages (Linux only)
    if cfg!(target_os = "linux") {
        for p in [
            "/usr/lib/android-sdk/platform-tools/adb",
            "/usr/local/bin/adb",
            "/opt/android-sdk/platform-tools/adb",
        ] {
            out.push((PathBuf::from(p), ResolveSource::DistroPackage));
        }
    }

    out
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Run `<adb> version` with a 2s timeout. Returns the first line of stdout
/// stripped, or None on any error.
fn adb_version(path: &std::path::Path) -> Option<String> {
    let output = Command::new(path)
        .arg("version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let result = wait_with_timeout(output, Duration::from_secs(2))?;
    if !result.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&result.stdout);
    stdout.lines().next().map(|s| s.trim().to_string())
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<std::process::Output> {
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().ok().flatten() {
            let mut stdout = Vec::new();
            if let Some(mut s) = child.stdout.take() {
                use std::io::Read;
                let _ = s.read_to_end(&mut stdout);
            }
            return Some(std::process::Output {
                status,
                stdout,
                stderr: Vec::new(),
            });
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_adb_returns_a_resolution_regardless_of_environment() {
        let r = locate_adb();
        match r.source {
            ResolveSource::NotFound => assert!(r.path.is_none()),
            _ => assert!(r.path.is_some()),
        }
    }

    #[test]
    fn candidate_paths_includes_android_home_when_set() {
        // Snapshot existing then mutate. This test is single-threaded by
        // default in cargo test; we don't restore because env state is
        // process-local and the test runner spawns one process per test
        // binary.
        std::env::set_var("ANDROID_HOME", "/tmp/fake-sdk");
        let candidates = candidate_paths();
        std::env::remove_var("ANDROID_HOME");

        assert!(candidates
            .iter()
            .any(|(p, src)| p.to_string_lossy().contains("/tmp/fake-sdk")
                && *src == ResolveSource::AndroidHome));
    }
}
