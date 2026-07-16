//! Vendor-quirks engine.
//!
//! When a `pm` action fails on a vendor-locked ROM (HyperOS blocking
//! `pm disable`, BBK devices restricting adb, etc.) the raw failure
//! string is opaque. This module owns:
//!
//! - The [`QuirkDocument`] file envelope and [`Quirk`] data model:
//!   schema version + detection signature + human explanation +
//!   suggested mitigation.
//! - A loader that reads `quirks/*.yaml` files at startup.
//! - [`explain`] which matches a failed action against the loaded
//!   quirks and returns the best match (or None).
//!
//! Design tenet from RESEARCH_DEEPDIVE.md: "Don't lie to the user."
//! When an OEM blocks a `pm disable`, we surface the exact reason
//! instead of "Operation failed".
//!
//! ```yaml
//! version: "1"
//! quirks:
//!   - id: hyperos-pm-disable-blocked
//!     title: "Xiaomi HyperOS blocks pm disable-user"
//!     matches:
//!       error_contains: ["DELETE_FAILED_INTERNAL_ERROR"]
//!       manufacturer: ["Xiaomi"]
//!       rom: ["hyperos"]
//!     explanation: "HyperOS blocks pm disable-user for many system apps."
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const QUIRK_SCHEMA_VERSION: &str = "1";

const MAX_QUIRKS_BYTES: u64 = 512 * 1024;
pub(crate) const QUIRK_SCHEMA_MIGRATION: &str =
    "wrap the quirk list as version: \"1\" plus quirks: [...], then validate it with the bundled quirk loader";

#[derive(schemars::JsonSchema, specta::Type, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuirkDocument {
    /// Bump on every breaking change; this build accepts only v1 files.
    #[schemars(extend("const" = QUIRK_SCHEMA_VERSION))]
    pub version: String,
    /// Ordered rules. The first matching quirk wins.
    #[serde(default)]
    pub quirks: Vec<Quirk>,
}

#[derive(specta::Type, Debug, Deserialize)]
#[serde(untagged)]
enum QuirkFile {
    Document(QuirkDocument),
    Legacy(Vec<Quirk>),
}

#[derive(schemars::JsonSchema, specta::Type, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quirk {
    /// Stable id for cross-referencing in bug reports.
    pub id: String,
    /// Human-readable title — first line shown to the user.
    pub title: String,
    /// The matcher. All conditions are AND-ed.
    #[serde(default)]
    pub matches: QuirkMatch,
    /// Body of the explanation. Markdown allowed; the UI renders
    /// inline.
    pub explanation: String,
    /// Optional suggested next step. The UI may render this as a
    /// one-click action button if the kind is recognised.
    #[serde(default)]
    pub mitigation: Option<Mitigation>,
}

#[derive(schemars::JsonSchema, specta::Type, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuirkMatch {
    /// Substrings to look for in the failing command's stderr or
    /// stdout. ANY-match within the field; ALL-match across fields.
    #[serde(default)]
    pub error_contains: Vec<String>,
    /// Manufacturer values that should be present (case-insensitive
    /// equality against `ro.product.manufacturer`).
    #[serde(default)]
    pub manufacturer: Vec<String>,
    /// ROM family hints — free-form, case-insensitive substring match
    /// against `ro.build.version.incremental` or known fingerprints.
    #[serde(default)]
    pub rom: Vec<String>,
    /// Package id patterns the failed action was applied to (exact
    /// match on package id).
    #[serde(default)]
    pub package_id: Vec<String>,
}

#[derive(schemars::JsonSchema, specta::Type, Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mitigation {
    /// Suggest re-running the action with a different `ActionKind`.
    /// Most common case: "pm disable failed → try pm uninstall --user 0".
    TryAlternativeAction {
        suggest_kind: String,
        rationale: String,
    },
    /// Surface a doc link or workaround the user has to perform off-app.
    Documentation { url: String, note: String },
    /// No automated fix; this quirk is informational only.
    None,
}

/// Context to feed [`explain`]. Fields are optional so a caller can
/// pass partial info — manufacturer / rom may not always be available
/// (e.g. during initial device handshake).
#[derive(Debug, Default)]
pub struct DeviceContext<'a> {
    pub manufacturer: Option<&'a str>,
    pub rom: Option<&'a str>,
    pub package_id: Option<&'a str>,
    pub raw_error: Option<&'a str>,
}

#[derive(Debug, thiserror::Error)]
pub enum QuirkError {
    #[error("could not read {path:?}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read quirks directory {path:?}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path:?}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[error("quirk file {path:?} failed validation: {reasons}")]
    Validate { path: PathBuf, reasons: String },
}

pub fn load_file(path: &Path) -> Result<Vec<Quirk>, QuirkError> {
    let text =
        crate::fs_util::read_to_string_limited(path, MAX_QUIRKS_BYTES).map_err(|source| {
            QuirkError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
    let file: QuirkFile = serde_yaml_ng::from_str(&text).map_err(|source| QuirkError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    match file {
        QuirkFile::Document(document) => {
            let issues = lint_document(&document);
            if !issues.is_empty() {
                return Err(QuirkError::Validate {
                    path: path.to_path_buf(),
                    reasons: issues.join("; "),
                });
            }
            Ok(document.quirks)
        }
        QuirkFile::Legacy(quirks) => Err(QuirkError::Validate {
            path: path.to_path_buf(),
            reasons: format!(
                "legacy quirk file contains {} unversioned entries; files without a schema version are no longer accepted (migration path: {QUIRK_SCHEMA_MIGRATION})",
                quirks.len()
            ),
        }),
    }
}

pub fn load_dir(path: &Path) -> Result<Vec<Quirk>, QuirkError> {
    let entries = std::fs::read_dir(path).map_err(|source| QuirkError::ReadDir {
        path: path.to_path_buf(),
        source,
    })?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| QuirkError::ReadDir {
            path: path.to_path_buf(),
            source,
        })?;
        let file_path = entry.path();
        if file_path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
            files.push(file_path);
        }
    }
    files.sort();

    let mut out = Vec::new();
    for file in files {
        out.extend(load_file(&file)?);
    }
    Ok(out)
}

pub fn lint_document(document: &QuirkDocument) -> Vec<String> {
    let mut issues = Vec::new();

    if document.version != QUIRK_SCHEMA_VERSION {
        issues.push(format!(
            "unsupported quirk schema version {:?} (supported: {:?}; migration path: {QUIRK_SCHEMA_MIGRATION})",
            document.version, QUIRK_SCHEMA_VERSION
        ));
    }
    if document.quirks.is_empty() {
        issues.push("quirk document has no entries".to_string());
    }

    for (i, quirk) in document.quirks.iter().enumerate() {
        let label = format!("quirk #{} {:?}", i + 1, quirk.id);
        if quirk.id.trim().is_empty() {
            issues.push(format!("{label}: id is empty"));
        }
        if quirk.title.trim().is_empty() {
            issues.push(format!("{label}: title is empty"));
        }
        if quirk.explanation.trim().is_empty() {
            issues.push(format!("{label}: explanation is empty"));
        }
        if quirk.matches.error_contains.is_empty()
            && quirk.matches.manufacturer.is_empty()
            && quirk.matches.rom.is_empty()
            && quirk.matches.package_id.is_empty()
        {
            issues.push(format!("{label}: matches has no constraints"));
        }

        lint_non_empty_values(
            &mut issues,
            &label,
            "matches.error_contains",
            &quirk.matches.error_contains,
        );
        lint_non_empty_values(
            &mut issues,
            &label,
            "matches.manufacturer",
            &quirk.matches.manufacturer,
        );
        lint_non_empty_values(&mut issues, &label, "matches.rom", &quirk.matches.rom);
        lint_non_empty_values(
            &mut issues,
            &label,
            "matches.package_id",
            &quirk.matches.package_id,
        );
        for package_id in &quirk.matches.package_id {
            if !crate::adb::packages::valid_package_name(package_id) {
                issues.push(format!(
                    "{label}: matches.package_id contains invalid id {package_id:?}"
                ));
            }
        }

        if let Some(mitigation) = &quirk.mitigation {
            match mitigation {
                Mitigation::TryAlternativeAction {
                    suggest_kind,
                    rationale,
                } => {
                    if suggest_kind.trim().is_empty() {
                        issues.push(format!("{label}: mitigation.suggest_kind is empty"));
                    }
                    if rationale.trim().is_empty() {
                        issues.push(format!("{label}: mitigation.rationale is empty"));
                    }
                }
                Mitigation::Documentation { url, note } => {
                    if url.trim().is_empty() {
                        issues.push(format!("{label}: mitigation.url is empty"));
                    }
                    if note.trim().is_empty() {
                        issues.push(format!("{label}: mitigation.note is empty"));
                    }
                }
                Mitigation::None => {}
            }
        }
    }

    issues
}

fn lint_non_empty_values(issues: &mut Vec<String>, label: &str, field: &str, values: &[String]) {
    for value in values {
        if value.trim().is_empty() {
            issues.push(format!("{label}: {field} contains an empty value"));
        }
    }
}

/// Search loaded quirks for the best match. Returns the first quirk
/// whose `matches` are all satisfied by `ctx`. Ordering matters: more
/// specific quirks should appear first in the file. Returns `None` if
/// nothing matches — callers should fall back to the raw error string.
pub fn explain<'a>(quirks: &'a [Quirk], ctx: &DeviceContext<'_>) -> Option<&'a Quirk> {
    quirks.iter().find(|q| matches_one(q, ctx))
}

fn matches_one(q: &Quirk, ctx: &DeviceContext<'_>) -> bool {
    let m = &q.matches;

    if !m.error_contains.is_empty() {
        let Some(err) = ctx.raw_error else {
            return false;
        };
        if !m.error_contains.iter().any(|needle| err.contains(needle)) {
            return false;
        }
    }
    if !m.manufacturer.is_empty() {
        let Some(mfr) = ctx.manufacturer else {
            return false;
        };
        if !m
            .manufacturer
            .iter()
            .any(|expected| expected.eq_ignore_ascii_case(mfr))
        {
            return false;
        }
    }
    if !m.rom.is_empty() {
        let Some(rom) = ctx.rom else {
            return false;
        };
        let rom_lc = rom.to_ascii_lowercase();
        if !m
            .rom
            .iter()
            .any(|expected| rom_lc.contains(&expected.to_ascii_lowercase()))
        {
            return false;
        }
    }
    if !m.package_id.is_empty() {
        let Some(pkg) = ctx.package_id else {
            return false;
        };
        if !m.package_id.iter().any(|expected| expected == pkg) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hyperos_quirk() -> Quirk {
        Quirk {
            id: "hyperos-pm-disable-blocked".into(),
            title: "Xiaomi HyperOS blocks pm disable".into(),
            matches: QuirkMatch {
                error_contains: vec!["DELETE_FAILED_INTERNAL_ERROR".into(), "not allowed".into()],
                manufacturer: vec!["Xiaomi".into(), "Redmi".into()],
                rom: vec!["hyperos".into()],
                package_id: vec![],
            },
            explanation: "HyperOS hardens system apps so `pm disable` no longer applies.".into(),
            mitigation: Some(Mitigation::TryAlternativeAction {
                suggest_kind: "uninstall_for_user".into(),
                rationale: "`pm uninstall --user 0` succeeds where `pm disable` is blocked. Data stays in /system; user-side state is removed.".into(),
            }),
        }
    }

    #[test]
    fn matches_when_all_fields_align() {
        let q = hyperos_quirk();
        let ctx = DeviceContext {
            manufacturer: Some("Xiaomi"),
            rom: Some("HyperOS 1.0 v2.0.0.0"),
            package_id: Some("com.miui.cleanmaster"),
            raw_error: Some("Failure [DELETE_FAILED_INTERNAL_ERROR]"),
        };
        assert!(matches_one(&q, &ctx));
    }

    #[test]
    fn fails_when_manufacturer_differs() {
        let q = hyperos_quirk();
        let ctx = DeviceContext {
            manufacturer: Some("Google"),
            rom: Some("HyperOS-flavoured custom rom"),
            package_id: None,
            raw_error: Some("Failure [DELETE_FAILED_INTERNAL_ERROR]"),
        };
        assert!(!matches_one(&q, &ctx));
    }

    #[test]
    fn fails_when_error_text_missing() {
        let q = hyperos_quirk();
        let ctx = DeviceContext {
            manufacturer: Some("Xiaomi"),
            rom: Some("HyperOS 1.0"),
            package_id: None,
            raw_error: Some("some unrelated error"),
        };
        assert!(!matches_one(&q, &ctx));
    }

    #[test]
    fn explain_returns_first_matching() {
        let quirks = vec![hyperos_quirk()];
        let ctx = DeviceContext {
            manufacturer: Some("Xiaomi"),
            rom: Some("HyperOS"),
            package_id: None,
            raw_error: Some("not allowed: shell uid is not allowed"),
        };
        let got = explain(&quirks, &ctx);
        assert!(got.is_some());
        assert_eq!(got.unwrap().id, "hyperos-pm-disable-blocked");
    }

    #[test]
    fn explain_returns_none_when_no_match() {
        let quirks = vec![hyperos_quirk()];
        let ctx = DeviceContext {
            manufacturer: Some("Samsung"),
            rom: Some("OneUI 6.1"),
            package_id: None,
            raw_error: Some("not allowed"),
        };
        assert!(explain(&quirks, &ctx).is_none());
    }

    #[test]
    fn rom_match_is_case_insensitive_substring() {
        let q = hyperos_quirk();
        let ctx = DeviceContext {
            manufacturer: Some("Xiaomi"),
            rom: Some("MIUI 14 - HyperOS Preview"),
            package_id: None,
            raw_error: Some("DELETE_FAILED_INTERNAL_ERROR"),
        };
        assert!(matches_one(&q, &ctx));
    }

    #[test]
    fn empty_matcher_matches_anything() {
        let q = Quirk {
            id: "wildcard".into(),
            title: "always".into(),
            matches: QuirkMatch::default(),
            explanation: "always".into(),
            mitigation: None,
        };
        let ctx = DeviceContext::default();
        assert!(matches_one(&q, &ctx));
    }

    #[test]
    fn yaml_round_trip() {
        let yaml = r#"
version: "1"
quirks:
  - id: hyperos-pm-disable-blocked
    title: "Xiaomi HyperOS blocks pm disable"
    matches:
      error_contains: ["DELETE_FAILED_INTERNAL_ERROR"]
      manufacturer: ["Xiaomi"]
      rom: ["hyperos"]
    explanation: "Use uninstall instead."
    mitigation:
      kind: try_alternative_action
      suggest_kind: uninstall_for_user
      rationale: "pm uninstall --user 0 works where pm disable doesn't"
"#;
        let document: QuirkDocument = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(lint_document(&document).is_empty());
        assert_eq!(document.version, "1");
        assert_eq!(document.quirks.len(), 1);
        assert_eq!(document.quirks[0].id, "hyperos-pm-disable-blocked");
        match document.quirks[0].mitigation.as_ref().unwrap() {
            Mitigation::TryAlternativeAction { suggest_kind, .. } => {
                assert_eq!(suggest_kind, "uninstall_for_user");
            }
            other => panic!("unexpected mitigation: {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_quirk_fields() {
        let yaml = r#"
version: "1"
quirks:
  - id: strict
    title: "Strict"
    matches:
      error_contains: ["failure"]
      unexpected: true
    explanation: "Reject unknown matcher fields."
"#;
        let error = serde_yaml_ng::from_str::<QuirkDocument>(yaml)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown field"));
    }

    #[test]
    fn load_dir_reads_yaml_files_in_stable_order() {
        let dir = std::env::temp_dir().join("droidsmith-quirks-dir-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("readme.txt"), "ignored").unwrap();
        std::fs::write(
            dir.join("b.yaml"),
            r#"
version: "1"
quirks:
  - id: second
    title: "Second"
    matches:
      error_contains: ["second"]
    explanation: "Second quirk"
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("a.yaml"),
            r#"
version: "1"
quirks:
  - id: first
    title: "First"
    matches:
      error_contains: ["first"]
    explanation: "First quirk"
"#,
        )
        .unwrap();

        let loaded = load_dir(&dir).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "first");
        assert_eq!(loaded[1].id, "second");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_file_rejects_legacy_sequence_with_migration_path() {
        let dir = std::env::temp_dir().join("droidsmith-quirks-legacy-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("legacy.yaml");
        std::fs::write(
            &path,
            r#"
- id: legacy
  title: "Legacy"
  matches:
    error_contains: ["legacy"]
  explanation: "Legacy shape"
"#,
        )
        .unwrap();

        let err = load_file(&path).unwrap_err().to_string();
        assert!(err.contains("legacy quirk file"));
        assert!(err.contains("migration path"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lint_document_names_unsupported_version_migration_path() {
        let document = QuirkDocument {
            version: "2".into(),
            quirks: vec![hyperos_quirk()],
        };
        let issues = lint_document(&document);
        assert!(issues
            .iter()
            .any(|i| i.contains("unsupported quirk schema version")));
        assert!(issues.iter().any(|i| i.contains("migration path")));
    }

    #[test]
    fn lint_document_rejects_unconstrained_rules() {
        let document = QuirkDocument {
            version: "1".into(),
            quirks: vec![Quirk {
                id: "wildcard".into(),
                title: "always".into(),
                matches: QuirkMatch::default(),
                explanation: "always".into(),
                mitigation: None,
            }],
        };
        let issues = lint_document(&document);
        assert!(issues
            .iter()
            .any(|i| i.contains("matches has no constraints")));
    }
}
