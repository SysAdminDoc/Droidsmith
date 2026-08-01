//! Known-vulnerable version policy for the host `scrcpy` binary.
//!
//! Droidsmith launches and supervises whatever `scrcpy` is on the host, and
//! CVE-2025-34449 (fixed in 3.3.4) is a *device-attacks-host* memory-safety bug
//! — the one direction that matters for a tool pointed at an untrusted device.
//! scrcpy publishes no GitHub security advisory for it and NVD does not index
//! it under the keyword `scrcpy`, so nothing automated will ever warn about it;
//! the floor is tracked in `scrcpy-policy.json` and asserted by the release
//! gate.
//!
//! The policy is advisory. It never blocks a launch, never blocks a newer
//! version, and reports [`ScrcpySecurityStatus::Unknown`] whenever the version
//! or the policy itself cannot be read, so a parsing problem can never be
//! mistaken for a clean bill of health.

use serde::{Deserialize, Serialize};

const POLICY_JSON: &str = include_str!("../../scrcpy-policy.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScrcpyPolicy {
    schema_version: u32,
    security_floor_version: String,
    advisories: Vec<ScrcpyAdvisory>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScrcpyAdvisory {
    id: String,
    below_version: String,
    source_url: String,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrcpySecurityStatus {
    /// The version could not be parsed, or the bundled policy is unusable.
    Unknown,
    /// At or above every known floor.
    Supported,
    /// Below at least one advisory floor.
    KnownVulnerable,
}

#[derive(specta::Type, Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScrcpySecurityAssessment {
    pub status: ScrcpySecurityStatus,
    /// Advisory identifiers the detected version is below, e.g. `CVE-2025-34449`.
    pub advisories: Vec<String>,
    /// First version carrying every fix in this policy.
    pub security_floor_version: String,
    pub source_url: Option<String>,
}

pub fn assess(version: &str) -> ScrcpySecurityAssessment {
    let Some(policy) = policy() else {
        // A malformed bundled policy must degrade to "no verdict", never to a
        // clean result and never to a panic on first mirror launch.
        return ScrcpySecurityAssessment {
            status: ScrcpySecurityStatus::Unknown,
            advisories: Vec::new(),
            security_floor_version: String::new(),
            source_url: None,
        };
    };
    let floor = policy.security_floor_version.clone();
    let Some(detected) = version_tuple(version) else {
        return ScrcpySecurityAssessment {
            status: ScrcpySecurityStatus::Unknown,
            advisories: Vec::new(),
            security_floor_version: floor,
            source_url: None,
        };
    };

    let mut matched = Vec::new();
    let mut source_url = None;
    for advisory in &policy.advisories {
        let Some(below) = version_tuple(&advisory.below_version) else {
            continue;
        };
        if detected < below {
            matched.push(advisory.id.clone());
            source_url.get_or_insert_with(|| advisory.source_url.clone());
        }
    }

    ScrcpySecurityAssessment {
        status: if matched.is_empty() {
            ScrcpySecurityStatus::Supported
        } else {
            ScrcpySecurityStatus::KnownVulnerable
        },
        advisories: matched,
        security_floor_version: floor,
        source_url,
    }
}

fn policy() -> Option<ScrcpyPolicy> {
    let policy: ScrcpyPolicy = serde_json::from_str(POLICY_JSON).ok()?;
    (policy.schema_version == 1).then_some(policy)
}

/// Parse `MAJOR[.MINOR[.PATCH]]`, ignoring any suffix such as `-rc1`.
fn version_tuple(value: &str) -> Option<(u32, u32, u32)> {
    let trimmed = value.trim().trim_start_matches('v');
    let core = trimmed
        .split(|c: char| c == '-' || c == '+' || c.is_whitespace())
        .next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_policy_parses_and_pins_the_cve_floor() {
        let policy = policy().expect("bundled scrcpy policy must parse");
        assert_eq!(policy.security_floor_version, "3.3.4");
        assert!(policy
            .advisories
            .iter()
            .any(|advisory| advisory.id == "CVE-2025-34449"));
    }

    #[test]
    fn versions_below_the_floor_are_flagged_with_the_cve() {
        for version in ["3.3.3", "3.0.0", "2.7", "1.25", "3.3"] {
            let assessment = assess(version);
            assert_eq!(
                assessment.status,
                ScrcpySecurityStatus::KnownVulnerable,
                "{version} should be flagged",
            );
            assert_eq!(assessment.advisories, vec!["CVE-2025-34449".to_string()]);
            assert!(assessment.source_url.is_some());
        }
    }

    #[test]
    fn the_fixed_release_and_newer_are_supported_and_never_blocked() {
        for version in ["3.3.4", "3.3.10", "3.4.0", "4.0", "4.1", "5.0.0"] {
            assert_eq!(
                assess(version).status,
                ScrcpySecurityStatus::Supported,
                "{version} must not be flagged",
            );
        }
    }

    #[test]
    fn unparseable_versions_report_unknown_rather_than_clean() {
        for version in ["", "   ", "unknown", "3.x", "a.b.c", "3.3.4.5"] {
            let assessment = assess(version);
            assert_eq!(
                assessment.status,
                ScrcpySecurityStatus::Unknown,
                "{version:?} must not claim a verdict",
            );
            assert!(assessment.advisories.is_empty());
        }
    }

    #[test]
    fn release_suffixes_are_tolerated() {
        assert_eq!(assess("v3.3.4").status, ScrcpySecurityStatus::Supported);
        assert_eq!(
            assess("3.3.3-rc1").status,
            ScrcpySecurityStatus::KnownVulnerable
        );
    }
}
