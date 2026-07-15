#![no_main]

use droidsmith_lib::adb::packages::parse_pm_list;
use droidsmith_lib::adb::parsers::{
    parse_fastboot_devices, parse_ls_output, parse_ps_output, parse_ss_output,
};
use droidsmith_lib::adb::transport::parse_devices_long;
use droidsmith_lib::adb::users::parse_pm_list_users;
use droidsmith_lib::adb::wireless::parse_mdns_services;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = parse_devices_long(&text);
    let _ = parse_pm_list(&text, true);
    let _ = parse_pm_list_users(&text);
    let _ = parse_ls_output(&text);
    let _ = parse_fastboot_devices(&text);
    let _ = parse_ss_output(&text);
    let _ = parse_ps_output(&text);
    let _ = parse_mdns_services(&text);
});
