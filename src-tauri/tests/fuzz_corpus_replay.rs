//! Replay the checked-in libFuzzer seeds in the ordinary parser test lane.
//!
//! The scheduled fuzz job explores new inputs; this test keeps every seed
//! executable on stable builds so a parser refactor cannot silently invalidate
//! the corpus or move a known crash out of normal CI.

use std::path::{Path, PathBuf};

use droidsmith_lib::adb::{packages, parsers, transport, users, wireless};
use droidsmith_lib::{journal::JournalEntry, packs, profile, quirks};

fn corpus_files(name: &str) -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fuzz")
        .join("corpus")
        .join(name);
    let mut files = std::fs::read_dir(directory)
        .expect("fuzz corpus directory")
        .map(|entry| entry.expect("fuzz corpus entry").path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    files.sort();
    files
}

#[test]
fn replay_adb_text_corpus() {
    let files = corpus_files("adb_text");
    assert!(!files.is_empty(), "ADB corpus must keep at least one seed");
    for path in files {
        let bytes = std::fs::read(path).expect("ADB corpus seed");
        let text = String::from_utf8_lossy(&bytes);
        let _ = transport::parse_devices_long(&text);
        let _ = packages::parse_pm_list(&text, true);
        let _ = users::parse_pm_list_users(&text);
        let _ = parsers::parse_ls_output(&text);
        let _ = parsers::parse_fastboot_devices(&text);
        let _ = parsers::parse_ss_output(&text);
        let _ = parsers::parse_ps_output(&text);
        let _ = wireless::parse_mdns_services(&text);
    }
}

#[test]
fn replay_yaml_document_corpus() {
    let files = corpus_files("yaml_documents");
    assert!(!files.is_empty(), "YAML corpus must keep at least one seed");
    for path in files {
        let bytes = std::fs::read(path).expect("YAML corpus seed");
        let text = String::from_utf8_lossy(&bytes);
        if let Ok(pack) = serde_yaml_ng::from_str::<packs::Pack>(&text) {
            let _ = packs::lint(&pack);
        }
        if let Ok(profile) = serde_yaml_ng::from_str::<profile::Profile>(&text) {
            let _ = profile::lint(&profile);
        }
        if let Ok(quirks) = serde_yaml_ng::from_str::<quirks::QuirkDocument>(&text) {
            let _ = quirks::lint_document(&quirks);
        }
    }
}

#[test]
fn replay_journal_jsonl_corpus() {
    let files = corpus_files("journal_jsonl");
    assert!(
        !files.is_empty(),
        "journal corpus must keep at least one seed"
    );
    for path in files {
        let bytes = std::fs::read(path).expect("journal corpus seed");
        for line in bytes.split(|byte| *byte == b'\n') {
            let _ = serde_json::from_slice::<JournalEntry>(line);
        }
    }
}
