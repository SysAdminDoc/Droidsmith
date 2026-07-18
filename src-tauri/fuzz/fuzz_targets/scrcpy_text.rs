#![no_main]

use droidsmith_lib::scrcpy::{parse_tool_video_codecs, parse_version, parse_video_encoders};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = parse_version(&text);
    let _ = parse_tool_video_codecs(&text);
    let _ = parse_video_encoders(&text);
});
