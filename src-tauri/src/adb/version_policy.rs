//! Shared Android Platform Tools compatibility policy.
//!
//! Runtime assessment and development archive pins are sourced from the same
//! repository-level JSON document. The release gate validates its dates,
//! downloads, fetch-script usage, and README summary.

use std::cmp::Ordering;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const POLICY_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../platform-tools-policy.json"
));

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformToolsStatus {
    Supported,
    Warn,
    Blocked,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlatformToolsAssessment {
    pub status: PlatformToolsStatus,
    pub rationale: String,
    pub recommended_version: String,
    pub warning_below_version: String,
    pub policy_reviewed_on: String,
    pub source_url: String,
}

impl Default for PlatformToolsAssessment {
    fn default() -> Self {
        assess(None)
    }
}

#[derive(specta::Type, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformToolsPolicy {
    schema_version: u32,
    reviewed_on: String,
    recommended_version: String,
    warning_below_version: String,
    source_url: String,
    rationale: String,
    known_bad_rules: Vec<KnownBadRule>,
}

#[derive(specta::Type, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnownBadRule {
    version: String,
    status: PlatformToolsStatus,
    rationale: String,
    source_url: String,
}

fn policy() -> &'static PlatformToolsPolicy {
    static POLICY: OnceLock<PlatformToolsPolicy> = OnceLock::new();
    POLICY.get_or_init(|| {
        let parsed: PlatformToolsPolicy = serde_json::from_str(POLICY_JSON)
            .expect("platform-tools-policy.json must match the Rust policy schema");
        assert_eq!(
            parsed.schema_version, 1,
            "unsupported platform-tools policy schema"
        );
        parsed
    })
}

pub fn assess(version: Option<&str>) -> PlatformToolsAssessment {
    let policy = policy();
    let Some(version) = version.filter(|value| !value.trim().is_empty()) else {
        return assessment(
            policy,
            PlatformToolsStatus::Warn,
            format!(
                "Platform Tools did not report a release version; compatibility is unverified. Version {} is recommended.",
                policy.recommended_version
            ),
            None,
        );
    };

    if let Some(rule) = policy
        .known_bad_rules
        .iter()
        .find(|rule| versions_equal(version, &rule.version))
    {
        return assessment(
            policy,
            rule.status,
            rule.rationale.clone(),
            Some(rule.source_url.clone()),
        );
    }

    match compare_versions(version, &policy.warning_below_version) {
        Some(Ordering::Less) => assessment(
            policy,
            PlatformToolsStatus::Warn,
            format!(
                "Platform Tools {version} predates the {} reliability floor; upgrade to {}. Existing operations remain available unless a known-bad rule applies.",
                policy.warning_below_version, policy.recommended_version
            ),
            None,
        ),
        Some(_) => assessment(
            policy,
            PlatformToolsStatus::Supported,
            if compare_versions(version, &policy.recommended_version)
                .is_some_and(|ordering| ordering != Ordering::Less)
            {
                policy.rationale.clone()
            } else {
                format!(
                    "Platform Tools {version} meets the {} reliability floor; {} remains recommended for the current mDNS backend and discovery behavior.",
                    policy.warning_below_version, policy.recommended_version
                )
            },
            None,
        ),
        None => assessment(
            policy,
            PlatformToolsStatus::Warn,
            format!(
                "Platform Tools reported an unrecognized version ({version}); compatibility was not blocked. Version {} is recommended.",
                policy.recommended_version
            ),
            None,
        ),
    }
}

pub fn is_recommended(version: &str) -> bool {
    compare_versions(version, &policy().recommended_version)
        .is_some_and(|ordering| ordering != Ordering::Less)
}

fn assessment(
    policy: &PlatformToolsPolicy,
    status: PlatformToolsStatus,
    rationale: String,
    source_url: Option<String>,
) -> PlatformToolsAssessment {
    PlatformToolsAssessment {
        status,
        rationale,
        recommended_version: policy.recommended_version.clone(),
        warning_below_version: policy.warning_below_version.clone(),
        policy_reviewed_on: policy.reviewed_on.clone(),
        source_url: source_url.unwrap_or_else(|| policy.source_url.clone()),
    }
}

fn versions_equal(left: &str, right: &str) -> bool {
    match (version_tuple(left), version_tuple(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    Some(version_tuple(left)?.cmp(&version_tuple(right)?))
}

fn version_tuple(version: &str) -> Option<(u32, u32, u32)> {
    let core = version.trim().trim_start_matches(['v', 'V']);
    let mut parts = core.split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_supports_floor_and_every_newer_recognized_version() {
        assert_eq!(
            assess(Some("36.0.2")).status,
            PlatformToolsStatus::Supported
        );
        assert_eq!(
            assess(Some("37.0.0-13729486")).status,
            PlatformToolsStatus::Supported
        );
        assert_eq!(
            assess(Some("99.4.1")).status,
            PlatformToolsStatus::Supported
        );
    }

    #[test]
    fn policy_warns_for_old_or_unrecognized_versions_without_blocking() {
        assert_eq!(assess(Some("35.0.2")).status, PlatformToolsStatus::Warn);
        assert_eq!(
            assess(Some("future-channel")).status,
            PlatformToolsStatus::Warn
        );
        assert_eq!(assess(None).status, PlatformToolsStatus::Warn);
    }

    #[test]
    fn policy_blocks_only_an_explicit_known_bad_release() {
        let assessment = assess(Some("36.0.1-123456"));
        assert_eq!(assessment.status, PlatformToolsStatus::Blocked);
        assert!(assessment
            .rationale
            .contains("never advanced beyond Canary"));
    }

    #[test]
    fn recommendation_uses_full_semantic_version_ordering() {
        assert!(!is_recommended("36.9.9"));
        assert!(is_recommended("37.0.0"));
        assert!(is_recommended("38.0.0"));
    }
}
