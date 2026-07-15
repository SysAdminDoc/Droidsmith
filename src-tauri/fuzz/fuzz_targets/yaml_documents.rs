#![no_main]

use droidsmith_lib::{packs, profile, quirks};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(value) = serde_yaml_ng::from_str::<packs::Pack>(&text) {
        let _ = packs::lint(&value);
    }
    if let Ok(value) = serde_yaml_ng::from_str::<profile::Profile>(&text) {
        let _ = profile::lint(&value);
    }
    if let Ok(value) = serde_yaml_ng::from_str::<quirks::QuirkDocument>(&text) {
        let _ = quirks::lint_document(&value);
    }
});
