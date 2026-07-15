use droidsmith_lib::adb::packages::parse_pm_list;
use droidsmith_lib::adb::parsers::{
    parse_fastboot_devices, parse_ls_output, parse_ps_output, parse_ss_output,
};
use droidsmith_lib::adb::transport::parse_devices_long;
use droidsmith_lib::adb::users::parse_pm_list_users;
use droidsmith_lib::adb::wireless::parse_mdns_services;
use droidsmith_lib::{journal::JournalEntry, packs, profile, quirks};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};

fn stable_config() -> Config {
    Config {
        cases: 256,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(0xD20D_5A17),
        ..Config::default()
    }
}

proptest! {
    #![proptest_config(stable_config())]

    #[test]
    fn arbitrary_adb_text_is_panic_free_deterministic_and_row_bounded(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let text = String::from_utf8_lossy(&bytes);
        let line_budget = text.lines().count();

        let devices = parse_devices_long(&text).unwrap();
        let packages = parse_pm_list(&text, true);
        let users = parse_pm_list_users(&text);
        let files = parse_ls_output(&text);
        let fastboot = parse_fastboot_devices(&text);
        let sockets = parse_ss_output(&text);
        let processes = parse_ps_output(&text);
        let mdns = parse_mdns_services(&text).unwrap();

        for count in [
            devices.len(), packages.len(), users.len(), files.len(),
            fastboot.len(), sockets.len(), processes.len(), mdns.len(),
        ] {
            prop_assert!(count <= line_budget);
        }

        let encoded = serde_json::to_vec(&(
            &devices, &packages, &users, &files, &fastboot, &sockets,
            &processes, &mdns,
        )).unwrap();
        let repeated = serde_json::to_vec(&(
            parse_devices_long(&text).unwrap(),
            parse_pm_list(&text, true),
            parse_pm_list_users(&text),
            parse_ls_output(&text),
            parse_fastboot_devices(&text),
            parse_ss_output(&text),
            parse_ps_output(&text),
            parse_mdns_services(&text).unwrap(),
        )).unwrap();
        prop_assert_eq!(&encoded, &repeated);
        prop_assert!(encoded.len() <= bytes.len().saturating_mul(32)
            .saturating_add(line_budget.saturating_mul(512))
            .saturating_add(1024));
    }

    #[test]
    fn arbitrary_yaml_is_panic_free_and_lint_output_is_bounded(
        bytes in prop::collection::vec(any::<u8>(), 0..2048)
    ) {
        let text = String::from_utf8_lossy(&bytes);
        let mut diagnostics = Vec::new();

        if let Ok(value) = serde_yaml_ng::from_str::<packs::Pack>(&text) {
            let first = packs::lint(&value);
            prop_assert_eq!(&first, &packs::lint(&value));
            diagnostics.extend(first);
        }
        if let Ok(value) = serde_yaml_ng::from_str::<profile::Profile>(&text) {
            let first = profile::lint(&value);
            prop_assert_eq!(&first, &profile::lint(&value));
            diagnostics.extend(first);
        }
        if let Ok(value) = serde_yaml_ng::from_str::<quirks::QuirkDocument>(&text) {
            let first = quirks::lint_document(&value);
            prop_assert_eq!(&first, &quirks::lint_document(&value));
            diagnostics.extend(first);
        }

        let encoded = serde_json::to_vec(&diagnostics).unwrap();
        prop_assert!(encoded.len() <= 1024 * 1024);
    }

    #[test]
    fn arbitrary_jsonl_lines_are_independent_and_deterministic(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let text = String::from_utf8_lossy(&bytes);
        let mut accepted = 0usize;
        for line in text.lines() {
            let first = serde_json::from_str::<JournalEntry>(line)
                .ok()
                .and_then(|entry| serde_json::to_string(&entry).ok());
            let second = serde_json::from_str::<JournalEntry>(line)
                .ok()
                .and_then(|entry| serde_json::to_string(&entry).ok());
            prop_assert_eq!(first.is_some(), second.is_some());
            prop_assert_eq!(&first, &second);
            accepted += usize::from(first.is_some());
        }
        prop_assert!(accepted <= text.lines().count());
    }

    #[test]
    fn valid_package_rows_round_trip(
        package in "[a-z][a-z0-9]{0,7}(\\.[a-z][a-z0-9]{0,7}){1,3}",
        uid in 10_000u32..200_000u32,
    ) {
        let line = format!(
            "package:/data/app/{package}/base.apk={package} uid:{uid} installer=com.android.vending"
        );
        let parsed = parse_pm_list(&line, true);
        prop_assert_eq!(parsed.len(), 1);
        prop_assert_eq!(&parsed[0].package, &package);
        prop_assert_eq!(parsed[0].uid, Some(uid));
        prop_assert!(parsed[0].enabled);
        prop_assert!(!parsed[0].system);
    }
}
