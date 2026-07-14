//! Debloat-pack format, loader, and validator.
//!
//! A "pack" is a YAML file under `packs/` describing a set of packages
//! to safely disable / uninstall for a given OEM, ROM, or device class.
//! The schema is deliberately small so community contributions are
//! cheap:
//!
//! ```yaml
//! name: "Pixel — vanilla Android"
//! version: "1"
//! description: "Tested on Pixel 6/7/8 with stock Android 14."
//! targets:
//!   manufacturer: ["Google"]
//!   rom: ["aosp"]
//!   android_min: 12
//! packages:
//!   - id: com.android.bookmarkprovider
//!     removal: recommended
//!     description: "Legacy bookmark provider; replaced by Chrome data."
//!   - id: com.google.android.apps.docs
//!     removal: advanced
//!     description: "Google Drive integration. Removing breaks
//!       'Save to Drive' from Chrome and Gmail."
//!     depends_on: []
//!     needed_by: []
//! ```
//!
//! Removal levels mirror UAD-NG's curated set (we explicitly reuse
//! their data model so future imports — R-036 — line up):
//!
//!   - `recommended` — safe for most users
//!   - `advanced`    — known side effects, documented per entry
//!   - `expert`      — power-user only
//!   - `unsafe`      — likely to brick a critical function
//!
//! Validation is done with serde — invalid YAML → typed error → CLI
//! exit code != 0 in `droidsmith-pack-lint`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adb::packages::valid_package_name;

pub const PACK_SCHEMA_VERSION: &str = "1";

const MAX_PACK_BYTES: u64 = 512 * 1024;
const PACK_SCHEMA_MIGRATION: &str =
    "convert the file to the v1 pack schema in src-tauri/src/packs/mod.rs, set version: \"1\", then run droidsmith-pack-lint";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    /// Human-friendly title shown in the pack picker.
    pub name: String,
    /// Bump on every breaking change; the loader checks
    /// `version == "1"` for now and refuses to load future revs.
    pub version: String,
    /// One-paragraph description shown under the title.
    pub description: String,
    /// Optional device targeting hints. None of these are enforced at
    /// load time — the wizard uses them to default-select compatible
    /// packs based on the connected device.
    #[serde(default)]
    pub targets: PackTargets,
    /// The packages this pack offers to remove.
    pub packages: Vec<PackEntry>,
    /// Free-form attribution / licence (e.g. "Adapted from UAD-NG, GPL-3.0").
    /// Optional, but pack-lint warns when missing for community packs.
    #[serde(default)]
    pub attribution: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackTargets {
    /// Manufacturer strings as reported by `ro.product.manufacturer`.
    #[serde(default)]
    pub manufacturer: Vec<String>,
    /// ROM family, e.g. ["oneui", "stock"]. Free-form, matched
    /// case-insensitively.
    #[serde(default)]
    pub rom: Vec<String>,
    /// Inclusive minimum Android API level (e.g. 30 for Android 11).
    #[serde(default)]
    pub android_min: Option<u32>,
    /// Inclusive maximum Android API level.
    #[serde(default)]
    pub android_max: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackEntry {
    /// Android package identifier.
    pub id: String,
    /// Severity tier; matches UAD-NG semantics.
    pub removal: RemovalLevel,
    /// What the package does, in user-facing language. Pack-lint
    /// requires this — no anonymous entries.
    pub description: String,
    /// Optional list of package IDs whose removal forces this one off
    /// too. Surfaced in the diff preview.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Optional list of package IDs that need this one to stay enabled.
    /// Warned about during preview.
    #[serde(default)]
    pub needed_by: Vec<String>,
    /// Free-form tags for search/grouping ("ads", "telemetry", "bloat",
    /// "vendor-locked").
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemovalLevel {
    Recommended,
    Advanced,
    Expert,
    Unsafe,
}

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[error("pack {path} failed validation: {reasons}")]
    Validate { path: PathBuf, reasons: String },
}

pub fn load(path: &Path) -> Result<Pack, PackError> {
    let text = crate::fs_util::read_to_string_limited(path, MAX_PACK_BYTES).map_err(|source| {
        PackError::Read {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let pack: Pack = serde_yaml_ng::from_str(&text).map_err(|source| PackError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let issues = lint(&pack);
    if !issues.is_empty() {
        return Err(PackError::Validate {
            path: path.to_path_buf(),
            reasons: issues.join("; "),
        });
    }
    Ok(pack)
}

/// Validation rules applied at load time AND surfaced by the
/// `droidsmith-pack-lint` binary. Returns a list of human-readable
/// reasons; empty means clean.
pub fn lint(p: &Pack) -> Vec<String> {
    let mut issues = Vec::new();

    if p.name.trim().is_empty() {
        issues.push("name is empty".to_string());
    }
    if p.version != PACK_SCHEMA_VERSION {
        issues.push(format!(
            "unsupported pack version {:?} (supported: {:?}; migration path: {PACK_SCHEMA_MIGRATION})",
            p.version, PACK_SCHEMA_VERSION
        ));
    }
    if p.description.trim().is_empty() {
        issues.push(
            "description is empty (community packs need a one-paragraph rationale)".to_string(),
        );
    }
    if p.packages.is_empty() {
        issues.push("pack has no entries".to_string());
    }

    if let (Some(lo), Some(hi)) = (p.targets.android_min, p.targets.android_max) {
        if lo > hi {
            issues.push(format!(
                "targets.android_min ({lo}) > targets.android_max ({hi})"
            ));
        }
    }

    let mut seen = HashSet::new();
    for entry in &p.packages {
        if !valid_package_name(&entry.id) {
            issues.push(format!(
                "entry {:?}: not a valid Android package id",
                entry.id
            ));
        }
        if !seen.insert(entry.id.as_str()) {
            issues.push(format!("entry {:?}: duplicate id", entry.id));
        }
        if entry.description.trim().is_empty() {
            issues.push(format!(
                "entry {:?}: description is empty (community guideline requires user-facing rationale)",
                entry.id
            ));
        }
        for dep in &entry.depends_on {
            if !valid_package_name(dep) {
                issues.push(format!(
                    "entry {:?}: depends_on contains invalid id {:?}",
                    entry.id, dep
                ));
            }
        }
        for need in &entry.needed_by {
            if !valid_package_name(need) {
                issues.push(format!(
                    "entry {:?}: needed_by contains invalid id {:?}",
                    entry.id, need
                ));
            }
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
name: "Pixel — vanilla Android"
version: "1"
description: "Tested on Pixel 6/7/8 with stock Android 14."
targets:
  manufacturer: ["Google"]
  rom: ["aosp"]
  android_min: 12
packages:
  - id: com.android.bookmarkprovider
    removal: recommended
    description: "Legacy bookmark provider; replaced by Chrome data."
  - id: com.google.android.apps.docs
    removal: advanced
    description: "Google Drive integration. Removing breaks 'Save to Drive' from Chrome and Gmail."
    depends_on: []
    needed_by: []
    labels: ["productivity"]
"#;

    /// Bundle contract: every pack shipped in the repo's `packs/`
    /// directory must load and lint cleanly. A corrupt or invalid bundled
    /// pack fails this test rather than silently disappearing at runtime.
    #[test]
    fn all_bundled_packs_load_cleanly() {
        let packs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs");
        if !packs_dir.is_dir() {
            return; // repo layout without bundled packs (e.g. vendored crate)
        }
        let mut checked = 0;
        for entry in std::fs::read_dir(&packs_dir).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
            {
                load(&path).unwrap_or_else(|e| {
                    panic!("bundled pack {} failed the contract: {e}", path.display())
                });
                checked += 1;
            }
        }
        assert!(checked > 0, "expected at least one bundled pack");
    }

    #[test]
    fn parses_a_well_formed_pack() {
        let p: Pack = serde_yaml_ng::from_str(GOOD).unwrap();
        assert_eq!(p.version, "1");
        assert_eq!(p.packages.len(), 2);
        assert_eq!(p.packages[0].removal, RemovalLevel::Recommended);
        assert_eq!(p.packages[1].labels, vec!["productivity"]);
        assert!(lint(&p).is_empty());
    }

    #[test]
    fn rejects_unsupported_version() {
        let bad = GOOD.replace("version: \"1\"", "version: \"2\"");
        let p: Pack = serde_yaml_ng::from_str(&bad).unwrap();
        let issues = lint(&p);
        assert!(issues
            .iter()
            .any(|i| i.contains("unsupported pack version")));
        assert!(issues.iter().any(|i| i.contains("migration path")));
    }

    #[test]
    fn flags_duplicate_ids() {
        let bad = GOOD.to_string()
            + r#"  - id: com.google.android.apps.docs
    removal: expert
    description: "duplicate row"
"#;
        let p: Pack = serde_yaml_ng::from_str(&bad).unwrap();
        let issues = lint(&p);
        assert!(issues.iter().any(|i| i.contains("duplicate id")));
    }

    #[test]
    fn flags_invalid_package_id() {
        let bad = r#"
name: "x"
version: "1"
description: "x"
packages:
  - id: ".bad"
    removal: recommended
    description: "leading dot"
"#;
        let p: Pack = serde_yaml_ng::from_str(bad).unwrap();
        let issues = lint(&p);
        assert!(issues
            .iter()
            .any(|i| i.contains("not a valid Android package id")));
    }

    #[test]
    fn flags_empty_entry_description() {
        let bad = r#"
name: "x"
version: "1"
description: "x"
packages:
  - id: com.x.y
    removal: recommended
    description: ""
"#;
        let p: Pack = serde_yaml_ng::from_str(bad).unwrap();
        let issues = lint(&p);
        assert!(issues.iter().any(|i| i.contains("description is empty")));
    }

    #[test]
    fn flags_inverted_android_min_max() {
        let bad = r#"
name: "x"
version: "1"
description: "x"
targets:
  android_min: 34
  android_max: 24
packages:
  - id: com.x.y
    removal: recommended
    description: "ok"
"#;
        let p: Pack = serde_yaml_ng::from_str(bad).unwrap();
        let issues = lint(&p);
        assert!(issues.iter().any(|i| i.contains("android_min")));
    }

    #[test]
    fn load_round_trips_through_a_tempfile() {
        let dir = std::env::temp_dir().join("droidsmith-pack-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("good.yaml");
        std::fs::write(&path, GOOD).unwrap();
        let p = load(&path).unwrap();
        assert_eq!(p.packages.len(), 2);
    }
}
