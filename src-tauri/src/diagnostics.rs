//! File-only crash log + native error reporting for cases where the GUI
//! hasn't started yet (or has died).
//!
//! Two surfaces:
//! 1. [`fatal_dialog`] — synchronous OS-native message box. Works without
//!    a Tauri runtime; used from [`crate::run`] on startup failure.
//! 2. [`install_panic_hook`] — captures Rust panics into a rotating log
//!    file at `<log_dir>/crash.log`.
//!
//! Plus a small public helper, [`log_fatal`], that writes a single record
//! without needing a panic to happen — used by `lib.rs` when
//! `tauri::Builder::run` returns `Err`, so the user-visible "a crash log
//! was written" message is actually truthful.
//!
//! No network. No PII. The user can always wipe the log folder from
//! Settings → Diagnostics (UI piece lands in Phase 7).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const CRASH_LOG: &str = "crash.log";
const MAX_LOG_SIZE: u64 = 1_048_576; // 1 MB
const MAX_LOG_BACKUPS: usize = 5;

/// Resolve a log directory we can write to.
///
/// Tries, in order: `$APPDATA/Droidsmith`, `$XDG_CONFIG_HOME/Droidsmith`,
/// `$HOME/.config/Droidsmith`, `<temp>/Droidsmith`. The final fallback
/// guarantees the panic hook installs even on minimal containers without
/// a HOME — better to write somewhere ephemeral than silently lose
/// every crash.
pub fn fallback_log_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("APPDATA").map(PathBuf::from) {
        return p.join("Droidsmith");
    }
    if let Some(p) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        return p.join("Droidsmith");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("Droidsmith");
    }
    std::env::temp_dir().join("Droidsmith")
}

/// Show a native message box. Falls back to stderr if the OS dialog
/// can't be raised (headless CI, missing libs).
pub fn fatal_dialog(title: &str, message: &str) {
    eprintln!("FATAL: {title}\n{message}");
    show_native(title, message);
}

/// Append a structured fatal-error record to `<dir>/crash.log` and rotate
/// if the file is past the size cap. Errors are silently swallowed because
/// we only call this from already-failing code paths — surfacing a second
/// failure to the user is worse than the original.
pub fn log_fatal(dir: &Path, source: &str, message: &str) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let log_path = dir.join(CRASH_LOG);
    let _ = rotate_if_needed(&log_path);

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(f, "[{}] fatal {source}: {message}", iso_now());
    }
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
    // SAFETY: both buffers were just allocated, each ends in a NUL
    // terminator (added by `to_wide`), and MessageBoxW is documented to
    // not retain the input pointers past the call. The null HWND is
    // explicitly permitted by the API.
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
    // Try zenity → kdialog → notify-send. Each is silenced into a noop
    // if the binary doesn't exist.
    if try_run(&["zenity", "--error", "--title", title, "--text", message]) {
        return;
    }
    if try_run(&["kdialog", "--error", &format!("{title}\n\n{message}")]) {
        return;
    }
    let _ = try_run(&["notify-send", "-u", "critical", title, message]);
}

#[cfg(all(unix, not(target_os = "macos")))]
fn try_run(argv: &[&str]) -> bool {
    let (cmd, rest) = match argv.split_first() {
        Some(x) => x,
        None => return false,
    };
    std::process::Command::new(cmd)
        .args(rest)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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
    // AppleScript string literals: backslash, double-quote, and control
    // chars (newline, tab) need escapes. Anything else passes through.
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Hook Rust panics so they land in a rotating log file. Idempotent in
/// the sense that each call chains the prior hook, so calling multiple
/// times records each panic once per installed hook (avoid that).
pub fn install_panic_hook(log_dir: PathBuf) {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_panic_record(&log_dir, info);
        original(info);
    }));
}

fn write_panic_record(dir: &Path, info: &std::panic::PanicHookInfo<'_>) -> std::io::Result<()> {
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
        return Ok(()); // doesn't exist yet — nothing to rotate
    };
    if meta.len() <= MAX_LOG_SIZE {
        return Ok(());
    }

    let oldest = log_path.with_extension(format!("log.{MAX_LOG_BACKUPS}"));
    let _ = fs::remove_file(&oldest);
    for i in (1..MAX_LOG_BACKUPS).rev() {
        let from = log_path.with_extension(format!("log.{i}"));
        let to = log_path.with_extension(format!("log.{}", i + 1));
        if from.exists() {
            // Surface rotation failures to stderr so a wedged file lock
            // doesn't silently swallow every subsequent crash record.
            if let Err(e) = fs::rename(&from, &to) {
                eprintln!("[droidsmith] crash log rotate failed ({from:?} -> {to:?}): {e}");
            }
        }
    }
    fs::rename(log_path, log_path.with_extension("log.1"))?;
    Ok(())
}

/// Wall-clock UTC stamp for a crash-log line. Delegates to the shared
/// `crate::time` formatter so journals and crash logs match exactly.
fn iso_now() -> String {
    crate::time::iso_utc_now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_under_threshold_is_noop() {
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

    #[test]
    fn log_fatal_creates_file_and_appends() {
        let tmp = std::env::temp_dir().join("droidsmith-fatal-test");
        let _ = fs::remove_dir_all(&tmp);
        log_fatal(&tmp, "init", "boom");
        let log = tmp.join(CRASH_LOG);
        let body = fs::read_to_string(&log).unwrap();
        assert!(body.contains("fatal init: boom"));
        log_fatal(&tmp, "init", "second");
        let body = fs::read_to_string(&log).unwrap();
        assert!(body.contains("boom"));
        assert!(body.contains("second"));
    }

    #[test]
    fn fallback_log_dir_returns_something() {
        // It never returns None now — must always return a path.
        let p = fallback_log_dir();
        assert!(!p.as_os_str().is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_escape_covers_special_chars() {
        assert_eq!(escape_applescript("a\\b"), "a\\\\b");
        assert_eq!(escape_applescript("a\"b"), "a\\\"b");
        assert_eq!(escape_applescript("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_applescript("tab\there"), "tab\\there");
    }
}
