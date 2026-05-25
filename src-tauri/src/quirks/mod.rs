//! Vendor-quirks engine.
//!
//! When a `pm` action fails on a vendor-locked ROM (HyperOS blocking
//! `pm disable`, BBK devices restricting adb, etc.) the raw failure
//! string is opaque. This module owns:
//!
//! - The [`Quirk`] data model: detection signature + human explanation
//!   + suggested mitigation.
//! - A loader that reads `quirks/*.yaml` files at startup.
//! - [`explain`] which matches a failed action against the loaded
//!   quirks and returns the best match (or None).
//!
//! Design tenet from RESEARCH_DEEPDIVE.md: "Don't lie to the user."
//! When an OEM blocks a `pm disable`, we surface the exact reason
//! instead of "Operation failed".

use std::path::Path;

use serde::{Deserialize, Serialize};

const MAX_QUIRKS_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path:?}: {source}")]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: serde_yml::Error,
    },
}

pub fn load_file(path: &Path) -> Result<Vec<Quirk>, QuirkError> {
    let text =
        crate::fs_util::read_to_string_limited(path, MAX_QUIRKS_BYTES).map_err(|source| {
            QuirkError::Read {
                path: path.to_path_buf(),
                source,
            }
        })?;
    serde_yml::from_str(&text).map_err(|source| QuirkError::Parse {
        path: path.to_path_buf(),
        source,
    })
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
        let quirks: Vec<Quirk> = serde_yml::from_str(yaml).unwrap();
        assert_eq!(quirks.len(), 1);
        assert_eq!(quirks[0].id, "hyperos-pm-disable-blocked");
        match quirks[0].mitigation.as_ref().unwrap() {
            Mitigation::TryAlternativeAction { suggest_kind, .. } => {
                assert_eq!(suggest_kind, "uninstall_for_user");
            }
            other => panic!("unexpected mitigation: {other:?}"),
        }
    }
}
