#![no_main]

use droidsmith_lib::journal::JournalEntry;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for line in data.split(|byte| *byte == b'\n') {
        let _ = serde_json::from_slice::<JournalEntry>(line);
    }
});
