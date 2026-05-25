//! File-only crash log + native-error reporting for cases where the GUI
//! hasn't started yet (or has died).
//!
//! Two surfaces:
//! 1. `fatal_dialog(title, message)` — synchronous OS-native message box.
//!    Works without a Tauri runtime (used from `lib.rs::run` on startup
//!    failure).
//! 2. `install_panic_hook(log_dir)` — captures Rust panics into a rotating
//!    log file at `<log_dir>/crash.log`.
//!
//! No network. No PII. The user can always wipe the log folder from
//! Settings → Diagnostics (UI piece lands in Phase 7).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const CRASH_LOG: &str = "crash.log";
const MAX_LOG_SIZE: u64 = 1_048_576; // 1 MB
const MAX_LOG_BACKUPS: usize = 5;

/// Best-effort: the OS user's config directory, with the app subfolder.
///
/// Used by the panic hook _before_ a Tauri `AppHandle` exists. Once the
/// app is running we prefer `app.path().app_data_dir()`.
pub fn fallback_log_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let mut p = PathBuf::from(h);
                p.push(".config");
                p
            })
        })?;
    Some(base.join("Droidsmith"))
}

/// Show a native message box. Falls back to stderr if the OS dialog
/// can't be raised (headless CI, missing libs).
pub fn fatal_dialog(title: &str, message: &str) {
    eprintln!("FATAL: {title}\n{message}");
    show_native(title, message);
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)] // single FFI call to user32!MessageBoxW; pointers are
                      // locally-owned, NUL-terminated wide-string buffers
fn show_native(title: &str, message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
    // MB_OK | MB_ICONERROR
    const MB_OK_ICONERROR: u32 = 0x0000_0010;
    let title_w = to_wide(title);
    let msg_w = to_wide(message);
    // SAFETY: pointers come from `to_wide`, both buffers contain a final
    // NUL terminator. MessageBoxW is documented to be thread-safe and to
    // not retain the input pointers past the call.
    unsafe {
        ffi_win::MessageBoxW(
            std::ptr::null_mut(),
            msg_w.as_ptr(),
            title_w.as_ptr(),
            MB_OK_ICONERROR,
        );
    }
}

#[cfg(target_os = "macos")]
fn show_native(title: &str, message: &str) {
    let script = format!(
        "display alert \"{}\" message \"{}\" as critical",
        escape_applescript(title),
        escape_applescript(message),
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show_native(title: &str, message: &str) {
    // Try zenity → kdialog → notify-send.
    if std::process::Command::new("zenity")
        .args(["--error", "--title", title, "--text", message])
        .status()
        .ok()
        .is_some_and(|s| s.success())
    {
        return;
    }
    if std::process::Command::new("kdialog")
        .args(["--error", &format!("{title}\n\n{message}")])
        .status()
        .ok()
        .is_some_and(|s| s.success())
    {
        return;
    }
    let _ = std::process::Command::new("notify-send")
        .args(["-u", "critical", title, message])
        .status();
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)] // module exists solely to declare the user32!MessageBoxW
                      // FFI binding consumed by `show_native` above
mod ffi_win {
    #[link(name = "user32")]
    extern "system" {
        pub fn MessageBoxW(
            hwnd: *mut std::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            u_type: u32,
        ) -> i32;
    }
}

#[cfg(target_os = "macos")]
fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Hook Rust panics so they land in a rotating log file. Idempotent.
pub fn install_panic_hook(log_dir: PathBuf) {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_crash_line(&log_dir, info);
        original(info);
    }));
}

fn write_crash_line(dir: &Path, info: &std::panic::PanicHookInfo<'_>) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let log_path = dir.join(CRASH_LOG);

    rotate_if_needed(&log_path)?;

    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "?".to_string());
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_string());

    writeln!(f, "[{}] panic at {location}: {payload}", iso_now())?;
    Ok(())
}

fn rotate_if_needed(log_path: &Path) -> std::io::Result<()> {
    let Ok(meta) = log_path.metadata() else {
        return Ok(());
    };
    if meta.len() <= MAX_LOG_SIZE {
        return Ok(());
    }

    let oldest = log_path.with_extension(format!("log.{MAX_LOG_BACKUPS}"));
    let _ = fs::remove_file(&oldest);
    for i in (1..MAX_LOG_BACKUPS).rev() {
        let from = log_path.with_extension(format!("log.{i}"));
        let to = log_path.with_extension(format!("log.{}", i + 1));
        let _ = fs::rename(from, to);
    }
    fs::rename(log_path, log_path.with_extension("log.1"))?;
    Ok(())
}

fn iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}") // seconds-since-epoch; full ISO formatting deferred to R-051 logcat work
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_when_under_threshold_is_noop() {
        let tmp = std::env::temp_dir().join("droidsmith-rot-test-a");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let log = tmp.join(CRASH_LOG);
        fs::write(&log, b"tiny").unwrap();
        rotate_if_needed(&log).unwrap();
        assert!(log.exists());
        assert_eq!(fs::read(&log).unwrap(), b"tiny");
    }

    #[test]
    fn rotate_shifts_when_over_threshold() {
        let tmp = std::env::temp_dir().join("droidsmith-rot-test-b");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let log = tmp.join(CRASH_LOG);
        let big = vec![b'x'; (MAX_LOG_SIZE + 1) as usize];
        fs::write(&log, &big).unwrap();
        rotate_if_needed(&log).unwrap();
        assert!(!log.exists() || fs::read(&log).unwrap().is_empty());
        assert!(log.with_extension("log.1").exists());
    }
}
