use std::path::PathBuf;

pub fn locate_adb() -> Option<PathBuf> {
    if let Ok(p) = which::which("adb") {
        return Some(p);
    }
    for candidate in candidate_paths() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();

    if let Some(home) = dirs_home() {
        out.push(home.join("AppData/Local/Android/Sdk/platform-tools/adb.exe"));
        out.push(home.join("Library/Android/sdk/platform-tools/adb"));
        out.push(home.join("Android/Sdk/platform-tools/adb"));
    }

    out.push(PathBuf::from("/usr/lib/android-sdk/platform-tools/adb"));
    out.push(PathBuf::from("/usr/local/bin/adb"));
    out.push(PathBuf::from("/opt/android-sdk/platform-tools/adb"));

    out
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
