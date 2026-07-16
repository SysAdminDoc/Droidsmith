//! Debloat-pack format, loader, and validator.
//!
//! A "pack" is a YAML file under `packs/` describing a set of packages
//! to safely disable / uninstall for a given OEM, ROM, or device class.
//! The schema is deliberately small so community contributions are
//! cheap:
//!
//! ```yaml
//! id: "pixel-vanilla"
//! revision: 1
//! name: "Pixel — vanilla Android"
//! version: "1"
//! description: "Tested on Pixel 6/7/8 with stock Android 14."
//! targets:
//!   manufacturer: ["Google"]
//!   rom: ["aosp"]
//!   build_fingerprint: ["google/"]
//!   android_min: 12
//!   user_scope: owner
//! provenance:
//!   source: "https://github.com/SysAdminDoc/Droidsmith"
//!   license: "MIT"
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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adb::packages::valid_package_name;

pub const PACK_SCHEMA_VERSION: &str = "1";

const MAX_PACK_BYTES: u64 = 512 * 1024;
pub(crate) const PACK_SCHEMA_MIGRATION: &str =
    "convert the file to the v1 pack schema in src-tauri/src/packs/mod.rs, set version: \"1\", then run droidsmith-pack-lint";

#[derive(schemars::JsonSchema, specta::Type, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    /// Stable machine identifier; never derived from the display name.
    #[serde(default)]
    pub id: String,
    /// Monotonic content revision for audit records and cached assessments.
    #[serde(default)]
    pub revision: u32,
    /// Human-friendly title shown in the pack picker.
    pub name: String,
    /// Bump on every breaking change; the loader checks
    /// `version == "1"` for now and refuses to load future revs.
    #[schemars(extend("const" = PACK_SCHEMA_VERSION))]
    pub version: String,
    /// One-paragraph description shown under the title.
    pub description: String,
    /// Device and Android-user constraints assessed before the picker and
    /// revalidated immediately before a pack plan is created.
    #[serde(default)]
    pub targets: PackTargets,
    /// The packages this pack offers to remove.
    pub packages: Vec<PackEntry>,
    /// Free-form attribution / licence (e.g. "Adapted from UAD-NG, GPL-3.0").
    /// Optional, but pack-lint warns when missing for community packs.
    #[serde(default)]
    pub attribution: Option<String>,
    /// Structured source/license information retained in every operation plan.
    #[serde(default)]
    pub provenance: PackProvenance,
}

#[derive(schemars::JsonSchema, specta::Type, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackProvenance {
    pub source: String,
    pub license: String,
}

#[derive(schemars::JsonSchema, specta::Type, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackTargets {
    /// Manufacturer strings as reported by `ro.product.manufacturer`.
    #[serde(default)]
    pub manufacturer: Vec<String>,
    /// ROM family, e.g. ["oneui", "stock"]. Free-form, matched
    /// case-insensitively.
    #[serde(default)]
    pub rom: Vec<String>,
    /// Optional case-insensitive model substrings.
    #[serde(default)]
    pub model: Vec<String>,
    /// Optional case-insensitive build-fingerprint substrings.
    #[serde(default)]
    pub build_fingerprint: Vec<String>,
    /// Inclusive minimum Android API level (e.g. 30 for Android 11).
    #[serde(default)]
    pub android_min: Option<u32>,
    /// Inclusive maximum Android API level.
    #[serde(default)]
    pub android_max: Option<u32>,
    /// Explicit Android-user policy. Packs must never silently inherit user 0.
    #[serde(default)]
    pub user_scope: UserScope,
}

#[derive(
    schemars::JsonSchema,
    specta::Type,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum UserScope {
    #[default]
    Unspecified,
    Owner,
    Current,
    Any,
}

#[derive(schemars::JsonSchema, specta::Type, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum RemovalLevel {
    Recommended,
    Advanced,
    Expert,
    Unsafe,
}

#[derive(Debug, Clone)]
pub struct DevicePackContext {
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub build_fingerprint: Option<String>,
    pub api_level: Option<u32>,
    pub user_id: u32,
    pub user_current: bool,
    pub installed_packages: HashSet<String>,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Compatible,
    Unknown,
    Mismatch,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct CompatibilityCheck {
    pub field: String,
    pub status: CompatibilityStatus,
    pub expected: Vec<String>,
    pub actual: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackEntryStatus {
    Ready,
    Missing,
    Unsupported,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PackEntryAssessment {
    pub id: String,
    pub status: PackEntryStatus,
    pub detail: Option<String>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PackAssessment {
    pub status: CompatibilityStatus,
    pub override_required: bool,
    pub checks: Vec<CompatibilityCheck>,
    pub entries: Vec<PackEntryAssessment>,
}

#[derive(specta::Type, Debug, Clone, Serialize)]
pub struct PackCandidate {
    pub pack: Pack,
    pub assessment: PackAssessment,
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

    if !valid_pack_id(&p.id) {
        issues.push(format!(
            "id {:?} must be lowercase kebab-case and 3-64 characters",
            p.id
        ));
    }
    if p.revision == 0 {
        issues.push("revision must be at least 1".to_string());
    }
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
    if p.provenance.source.trim().is_empty() {
        issues.push("provenance.source is empty".to_string());
    }
    if p.provenance.license.trim().is_empty() {
        issues.push("provenance.license is empty".to_string());
    }
    if p.targets.user_scope == UserScope::Unspecified {
        issues.push("targets.user_scope must be owner, current, or any".to_string());
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

    for entry in &p.packages {
        for dependency in &entry.depends_on {
            if !seen.contains(dependency.as_str()) {
                issues.push(format!(
                    "entry {:?}: depends_on references package {:?} outside this pack",
                    entry.id, dependency
                ));
            }
        }
    }
    if let Err(error) = expand_dependencies(p, p.packages.iter().map(|entry| entry.id.clone())) {
        issues.push(error);
    }

    issues
}

pub fn valid_pack_id(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

/// Compute the recursive `depends_on` closure in pack order. Cycles are
/// rejected by lint and again here so renderer input can never create a loop.
pub fn expand_dependencies(
    pack: &Pack,
    selected: impl IntoIterator<Item = String>,
) -> Result<HashSet<String>, String> {
    let entries: HashMap<&str, &PackEntry> = pack
        .packages
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    let mut expanded = HashSet::new();
    let mut visiting = HashSet::new();

    fn visit(
        id: &str,
        entries: &HashMap<&str, &PackEntry>,
        expanded: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
    ) -> Result<(), String> {
        let entry = entries
            .get(id)
            .ok_or_else(|| format!("selected package {id:?} is not in this pack"))?;
        if expanded.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.to_string()) {
            return Err(format!("dependency cycle includes package {id:?}"));
        }
        for dependency in &entry.depends_on {
            visit(dependency, entries, expanded, visiting)?;
        }
        visiting.remove(id);
        expanded.insert(id.to_string());
        Ok(())
    }

    for id in selected {
        visit(&id, &entries, &mut expanded, &mut visiting)?;
    }
    Ok(expanded)
}

pub fn assess(pack: &Pack, context: &DevicePackContext) -> PackAssessment {
    let mut checks = vec![pattern_check(
        "manufacturer",
        &pack.targets.manufacturer,
        context.manufacturer.as_deref(),
    )];
    checks.push(pattern_check(
        "model",
        &pack.targets.model,
        context.model.as_deref(),
    ));
    checks.push(pattern_check(
        "build_fingerprint",
        &pack.targets.build_fingerprint,
        context.build_fingerprint.as_deref(),
    ));

    let api_expected = match (pack.targets.android_min, pack.targets.android_max) {
        (Some(min), Some(max)) => vec![format!("{min}-{max}")],
        (Some(min), None) => vec![format!(">={min}")],
        (None, Some(max)) => vec![format!("<={max}")],
        (None, None) => vec!["any".to_string()],
    };
    let api_status = match context.api_level {
        Some(api)
            if pack.targets.android_min.is_some_and(|min| api < min)
                || pack.targets.android_max.is_some_and(|max| api > max) =>
        {
            CompatibilityStatus::Mismatch
        }
        Some(_) => CompatibilityStatus::Compatible,
        None if pack.targets.android_min.is_some() || pack.targets.android_max.is_some() => {
            CompatibilityStatus::Unknown
        }
        None => CompatibilityStatus::Compatible,
    };
    checks.push(CompatibilityCheck {
        field: "api_level".to_string(),
        status: api_status,
        expected: api_expected,
        actual: context.api_level.map(|api| api.to_string()),
    });

    let user_status = match pack.targets.user_scope {
        UserScope::Owner if context.user_id == 0 => CompatibilityStatus::Compatible,
        UserScope::Current if context.user_current => CompatibilityStatus::Compatible,
        UserScope::Any => CompatibilityStatus::Compatible,
        UserScope::Unspecified => CompatibilityStatus::Unknown,
        UserScope::Owner | UserScope::Current => CompatibilityStatus::Mismatch,
    };
    checks.push(CompatibilityCheck {
        field: "android_user".to_string(),
        status: user_status,
        expected: vec![format!("{:?}", pack.targets.user_scope).to_lowercase()],
        actual: Some(format!(
            "{}{}",
            context.user_id,
            if context.user_current {
                " (current)"
            } else {
                ""
            }
        )),
    });

    let status = if checks
        .iter()
        .any(|check| check.status == CompatibilityStatus::Mismatch)
    {
        CompatibilityStatus::Mismatch
    } else if checks
        .iter()
        .any(|check| check.status == CompatibilityStatus::Unknown)
    {
        CompatibilityStatus::Unknown
    } else {
        CompatibilityStatus::Compatible
    };

    let pack_ids: HashSet<&str> = pack
        .packages
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    let entries = pack
        .packages
        .iter()
        .map(|entry| {
            if !context.installed_packages.contains(&entry.id) {
                return PackEntryAssessment {
                    id: entry.id.clone(),
                    status: PackEntryStatus::Missing,
                    detail: Some(
                        "package is not installed for the selected Android user".to_string(),
                    ),
                };
            }
            let unavailable: Vec<&str> = entry
                .depends_on
                .iter()
                .filter(|dependency| {
                    !pack_ids.contains(dependency.as_str())
                        || !context.installed_packages.contains(dependency.as_str())
                })
                .map(String::as_str)
                .collect();
            if unavailable.is_empty() {
                PackEntryAssessment {
                    id: entry.id.clone(),
                    status: PackEntryStatus::Ready,
                    detail: None,
                }
            } else {
                PackEntryAssessment {
                    id: entry.id.clone(),
                    status: PackEntryStatus::Unsupported,
                    detail: Some(format!(
                        "required package(s) unavailable: {}",
                        unavailable.join(", ")
                    )),
                }
            }
        })
        .collect();

    PackAssessment {
        status,
        override_required: status != CompatibilityStatus::Compatible,
        checks,
        entries,
    }
}

fn pattern_check(field: &str, expected: &[String], actual: Option<&str>) -> CompatibilityCheck {
    let status = if expected.is_empty() {
        CompatibilityStatus::Compatible
    } else if let Some(actual) = actual {
        if expected
            .iter()
            .any(|pattern| actual.to_lowercase().contains(&pattern.to_lowercase()))
        {
            CompatibilityStatus::Compatible
        } else {
            CompatibilityStatus::Mismatch
        }
    } else {
        CompatibilityStatus::Unknown
    };
    CompatibilityCheck {
        field: field.to_string(),
        status,
        expected: if expected.is_empty() {
            vec!["any".to_string()]
        } else {
            expected.to_vec()
        },
        actual: actual.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
id: "pixel-vanilla"
revision: 1
name: "Pixel — vanilla Android"
version: "1"
description: "Tested on Pixel 6/7/8 with stock Android 14."
targets:
  manufacturer: ["Google"]
  rom: ["aosp"]
  build_fingerprint: ["google/"]
  android_min: 12
  user_scope: owner
provenance:
  source: "https://github.com/SysAdminDoc/Droidsmith"
  license: "MIT"
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
    fn rejects_unknown_pack_fields() {
        let unknown_root = GOOD.replace("name: \"Pixel", "unexpected: true\nname: \"Pixel");
        let error = serde_yaml_ng::from_str::<Pack>(&unknown_root)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"));

        let unknown_target = GOOD.replace(
            "  manufacturer: [\"Google\"]",
            "  manufacturer: [\"Google\"]\n  unexpected: true",
        );
        let error = serde_yaml_ng::from_str::<Pack>(&unknown_target)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"));
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

    #[test]
    fn expands_transitive_dependencies_and_rejects_cycles() {
        let mut pack: Pack = serde_yaml_ng::from_str(GOOD).unwrap();
        pack.packages[1].depends_on = vec![pack.packages[0].id.clone()];
        let expanded = expand_dependencies(&pack, vec![pack.packages[1].id.clone()]).unwrap();
        assert_eq!(expanded.len(), 2);
        assert!(expanded.contains(&pack.packages[0].id));

        pack.packages[0].depends_on = vec![pack.packages[1].id.clone()];
        assert!(lint(&pack)
            .iter()
            .any(|issue| issue.contains("dependency cycle")));
    }

    #[test]
    fn assesses_device_user_and_per_entry_support() {
        let mut pack: Pack = serde_yaml_ng::from_str(GOOD).unwrap();
        pack.packages[1].depends_on = vec![pack.packages[0].id.clone()];
        let context = DevicePackContext {
            manufacturer: Some("Samsung".into()),
            model: Some("SM-S928U".into()),
            build_fingerprint: Some("samsung/e3q/e3q:15/test".into()),
            api_level: Some(35),
            user_id: 10,
            user_current: true,
            installed_packages: HashSet::from([pack.packages[1].id.clone()]),
        };

        let assessment = assess(&pack, &context);
        assert_eq!(assessment.status, CompatibilityStatus::Mismatch);
        assert!(assessment.override_required);
        assert_eq!(assessment.entries[0].status, PackEntryStatus::Missing);
        assert_eq!(assessment.entries[1].status, PackEntryStatus::Unsupported);
        assert!(assessment.entries[1]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&pack.packages[0].id)));
    }
}
