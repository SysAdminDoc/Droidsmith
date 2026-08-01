use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../platform-tools-policy.json");
    validate_platform_tools_policy();
    tauri_build::build();
}

fn validate_platform_tools_policy() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let path = manifest_dir.join("../platform-tools-policy.json");
    let bytes = fs::read(&path).unwrap_or_else(|error| {
        panic!("could not read {}: {error}", path.display());
    });
    let policy: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!("{} is not valid JSON: {error}", path.display());
    });
    assert_eq!(
        policy
            .get("schemaVersion")
            .and_then(serde_json::Value::as_u64),
        Some(1),
        "{} must use platform-tools policy schema 1",
        path.display()
    );
    for field in [
        "reviewedOn",
        "recommendedVersion",
        "warningBelowVersion",
        "sourceUrl",
        "rationale",
    ] {
        assert!(
            policy
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.trim().is_empty()),
            "{} field {field:?} must be a non-empty string",
            path.display()
        );
    }
    assert!(
        policy
            .get("knownBadRules")
            .and_then(serde_json::Value::as_array)
            .is_some(),
        "{} field \"knownBadRules\" must be an array",
        path.display()
    );
}
