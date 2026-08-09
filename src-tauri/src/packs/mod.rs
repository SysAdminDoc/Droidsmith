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
    /// Preferred operation for this package. Omitted entries retain the
    /// historical `disable` default; the planner re-checks the command on the
    /// selected device before producing a mutation plan.
    #[serde(default)]
    pub action: Option<PackAction>,
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
    /// Per-build evidence for the removal outcome. An empty list is explicit
    /// unknown evidence, never a claim that removal is safe.
    #[serde(default, alias = "verification_records", alias = "verified_on")]
    pub verification: Vec<PackVerification>,
}

#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct PackVerification {
    /// Case-insensitive prefix of `ro.build.fingerprint` on the tested build.
    #[serde(alias = "fingerprint_prefix", alias = "build_fingerprint")]
    pub build_fingerprint_prefix: String,
    /// Android SDK/API level observed during the verification.
    #[serde(alias = "api_level")]
    pub android_level: u32,
    /// Outcome observed when the package was tested on this build.
    pub outcome: PackVerificationOutcome,
    /// UTC verification date in `YYYY-MM-DD` form.
    pub date: String,
    /// Human-auditable source for the verification evidence.
    pub source: String,
}

#[derive(
    schemars::JsonSchema, specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PackVerificationOutcome {
    /// The package was removed or otherwise completed the tested operation.
    #[serde(
        alias = "success",
        alias = "passed",
        alias = "pass",
        alias = "verified"
    )]
    Removed,
    /// The tested operation did not establish a removable package state.
    #[serde(
        alias = "failure",
        alias = "failed",
        alias = "not_removed",
        alias = "not_verified"
    )]
    Failed,
}

impl PackVerificationOutcome {
    fn is_positive(self) -> bool {
        matches!(self, Self::Removed)
    }
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
#[serde(rename_all = "snake_case")]
pub enum PackAction {
    Suspend,
    #[default]
    Disable,
    Archive,
    UninstallForUser,
}

impl PackAction {
    pub fn action_kind(self) -> crate::adb::actions::ActionKind {
        match self {
            Self::Suspend => crate::adb::actions::ActionKind::Suspend,
            Self::Disable => crate::adb::actions::ActionKind::Disable,
            Self::Archive => crate::adb::actions::ActionKind::Archive,
            Self::UninstallForUser => crate::adb::actions::ActionKind::UninstallForUser,
        }
    }

    /// Lower numbers are safer. A renderer override may only move toward a
    /// safer action than the pack's preferred operation.
    pub fn safety_rank(self) -> u8 {
        match self {
            Self::Suspend => 0,
            Self::Disable => 1,
            Self::Archive => 2,
            Self::UninstallForUser => 3,
        }
    }

    pub fn is_no_riskier_than(self, preferred: Self) -> bool {
        self.safety_rank() <= preferred.safety_rank()
    }
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

impl RemovalLevel {
    /// Whether applying an entry requires an explicit operator acknowledgement
    /// on a headless command line. The GUI has a separate review checkbox;
    /// keeping this predicate in the pack domain prevents CLI callers from
    /// duplicating the tier spelling.
    pub fn is_unsafe(self) -> bool {
        self == Self::Unsafe
    }
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
    /// Installed packages whose PackageManager UID resolves to the
    /// `android.uid.system` app id for the selected Android user.
    pub system_uid_packages: HashSet<String>,
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
    /// Runtime safety tier after deterministic device evidence is applied.
    /// This never mutates the source pack or its schema-v1 removal tier.
    pub effective_removal: RemovalLevel,
    /// Action after applying the pack preference and any reviewed safer
    /// override. Unsupported actions are marked in `status` and never planned.
    pub resolved_action: PackAction,
    /// True when the entry was raised to Unsafe because it shares
    /// `android.uid.system` on the selected device/user.
    pub shared_system_uid: bool,
    /// Whether a positive per-entry verification record matches this device.
    /// Unknown is deliberately distinct from verified.
    pub verification: PackVerificationStatus,
}

#[derive(specta::Type, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackVerificationStatus {
    Verified,
    NotVerified,
    Unknown,
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
    /// True when the pack was imported from a user-supplied local file
    /// (stored under the app-data `packs/` directory) rather than bundled
    /// with the app. Imported packs can be removed from the picker.
    pub imported: bool,
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

/// A single malformed or duplicated file found while loading a pack directory.
/// The path is reduced to its file name so a CLI or renderer can report the
/// problem without disclosing the host's directory layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackDirectoryError {
    pub file: String,
    pub code: String,
    pub message: String,
}

/// Load every YAML pack in a directory, retaining per-file failures instead of
/// making one bad contribution hide the healthy packs. The loader is shared by
/// the GUI's resource model and the headless CLI so both surfaces resolve the
/// same schema and duplicate-id rules.
pub fn load_directory(
    directory: &Path,
) -> Result<(Vec<Pack>, Vec<PackDirectoryError>), std::io::Error> {
    if !directory.is_dir() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if file.starts_with('_')
            || !path
                .extension()
                .is_some_and(|extension| extension == "yaml" || extension == "yml")
        {
            continue;
        }
        match load(&path) {
            Ok(pack) => loaded.push(pack),
            Err(error) => errors.push(PackDirectoryError {
                file,
                code: pack_error_code(&error).to_string(),
                message: error.to_string(),
            }),
        }
    }

    let mut id_counts = HashMap::<String, usize>::new();
    for pack in &loaded {
        *id_counts.entry(pack.id.clone()).or_default() += 1;
    }
    loaded.retain(|pack| {
        if id_counts.get(&pack.id).copied().unwrap_or_default() > 1 {
            errors.push(PackDirectoryError {
                file: format!("{}.yaml", pack.id),
                code: "pack_duplicate_id".to_string(),
                message: format!(
                    "stable pack id {:?} is declared by more than one runtime pack",
                    pack.id
                ),
            });
            false
        } else {
            true
        }
    });
    loaded.sort_by(|left, right| left.id.cmp(&right.id));
    errors.sort_by(|left, right| left.file.cmp(&right.file));
    Ok((loaded, errors))
}

fn pack_error_code(error: &PackError) -> &'static str {
    match error {
        PackError::Read { .. } => "pack_read",
        PackError::Parse { .. } => "pack_parse",
        PackError::Validate { .. } => "pack_validate",
    }
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
        for verification in &entry.verification {
            if verification.build_fingerprint_prefix.trim().is_empty() {
                issues.push(format!(
                    "entry {:?}: verification.build_fingerprint_prefix is empty",
                    entry.id
                ));
            }
            if verification.android_level == 0 {
                issues.push(format!(
                    "entry {:?}: verification.android_level must be greater than 0",
                    entry.id
                ));
            }
            if !valid_verification_date(&verification.date) {
                issues.push(format!(
                    "entry {:?}: verification.date must be YYYY-MM-DD",
                    entry.id
                ));
            }
            if verification.source.trim().is_empty() {
                issues.push(format!(
                    "entry {:?}: verification.source is empty",
                    entry.id
                ));
            }
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

/// What the source device did to a package, used to tier an exported entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovedKind {
    /// Installed but disabled (`pm disable`).
    Disabled,
    /// Archived (APK removed, user data kept — Android 15+).
    Archived,
    /// Uninstalled for the selected user (`pm uninstall --user`).
    Uninstalled,
}

/// One package captured from a device's current debloat state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedPackage {
    pub id: String,
    pub kind: RemovedKind,
}

/// Device metadata retained on an exported pack so it round-trips through the
/// importer and assesses correctly against the originating hardware.
#[derive(Debug, Clone, Default)]
pub struct DeviceExportContext {
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub build_fingerprint: Option<String>,
    pub api_level: Option<u32>,
    pub user_id: u32,
    /// Absolute capture date (`YYYY-MM-DD`); the caller stamps it.
    pub date: String,
}

/// Serialize an exported pack to schema-v1 YAML. The result round-trips through
/// [`load`] (parse + lint) — see the `exported_pack_round_trips` test.
pub fn to_yaml(pack: &Pack) -> Result<String, serde_yaml_ng::Error> {
    serde_yaml_ng::to_string(pack)
}

/// Build a schema-valid [`Pack`] capturing a device's currently disabled,
/// archived, and uninstalled packages (R-098). Errors when there is nothing to
/// export. The produced pack lints clean so it re-imports via `import_pack`.
pub fn from_device_state(
    removed: &[RemovedPackage],
    context: &DeviceExportContext,
) -> Result<Pack, String> {
    let mut seen = HashSet::new();
    let mut packages = Vec::new();
    for entry in removed {
        if !valid_package_name(&entry.id) || !seen.insert(entry.id.clone()) {
            continue;
        }
        let (removal, description) = match entry.kind {
            RemovedKind::Disabled => (
                RemovalLevel::Recommended,
                "Disabled on the source device.".to_string(),
            ),
            RemovedKind::Archived => (
                RemovalLevel::Advanced,
                "Archived on the source device (APK removed, user data kept).".to_string(),
            ),
            RemovedKind::Uninstalled => (
                RemovalLevel::Recommended,
                "Uninstalled for the selected user on the source device.".to_string(),
            ),
        };
        let verification = match (context.build_fingerprint.as_deref(), context.api_level) {
            (Some(build_fingerprint), Some(android_level))
                if !build_fingerprint.trim().is_empty()
                    && android_level > 0
                    && valid_verification_date(&context.date) =>
            {
                vec![PackVerification {
                    build_fingerprint_prefix: build_fingerprint.to_string(),
                    android_level,
                    outcome: PackVerificationOutcome::Removed,
                    date: context.date.clone(),
                    source: "Droidsmith device export".to_string(),
                }]
            }
            _ => Vec::new(),
        };
        packages.push(PackEntry {
            id: entry.id.clone(),
            removal,
            action: None,
            description,
            depends_on: Vec::new(),
            needed_by: Vec::new(),
            labels: vec!["device-export".to_string()],
            verification,
        });
    }
    if packages.is_empty() {
        return Err(
            "no disabled, archived, or uninstalled packages to export from this device".to_string(),
        );
    }

    let device_label = [context.manufacturer.as_deref(), context.model.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let device_label = if device_label.trim().is_empty() {
        "this device".to_string()
    } else {
        device_label
    };
    let user_scope = if context.user_id == 0 {
        UserScope::Owner
    } else {
        UserScope::Current
    };

    let pack = Pack {
        id: sanitize_pack_id(&context.manufacturer, &context.model),
        revision: 1,
        name: format!("{device_label} — captured debloat"),
        version: PACK_SCHEMA_VERSION.to_string(),
        description: format!(
            "Captured from the current device state on {date}: {count} package(s) disabled, archived, or uninstalled. Review the removal tiers before applying to another device.",
            date = context.date,
            count = packages.len()
        ),
        targets: PackTargets {
            manufacturer: context.manufacturer.clone().into_iter().collect(),
            rom: Vec::new(),
            model: context.model.clone().into_iter().collect(),
            build_fingerprint: Vec::new(),
            android_min: context.api_level,
            android_max: None,
            user_scope,
        },
        packages,
        attribution: Some("Captured from device state by Droidsmith".to_string()),
        provenance: PackProvenance {
            source: "Droidsmith device export".to_string(),
            license: "unspecified".to_string(),
        },
    };
    debug_assert!(lint(&pack).is_empty(), "exported pack must lint clean");
    Ok(pack)
}

/// Derive a valid kebab-case pack id from device metadata, always ending in
/// `-export` and falling back to `device-export` when nothing usable remains.
fn sanitize_pack_id(manufacturer: &Option<String>, model: &Option<String>) -> String {
    let raw = [manufacturer.as_deref(), model.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("-");
    let mut slug = String::new();
    let mut last_hyphen = true; // suppress a leading hyphen
    for character in raw.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_hyphen = false;
        } else if !last_hyphen {
            slug.push('-');
            last_hyphen = true;
        }
    }
    let stem = slug.trim_matches('-');
    // Reserve room for the `-export` suffix within the 64-char id ceiling.
    let stem: String = stem.chars().take(48).collect();
    let stem = stem.trim_matches('-');
    let candidate = if stem.is_empty() {
        "device-export".to_string()
    } else {
        format!("{stem}-export")
    };
    if valid_pack_id(&candidate) {
        candidate
    } else {
        "device-export".to_string()
    }
}

pub fn valid_pack_id(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

/// Recursion ceiling for [`expand_dependencies`]. An imported pack is
/// attacker-supplied data (512 KB of YAML fits a chain thousands of links
/// deep); without a cap a crafted `depends_on` chain overflows the stack and
/// aborts the process. No legitimate pack nests anywhere near this deep.
const MAX_DEPENDENCY_DEPTH: usize = 64;

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
        depth: usize,
        entries: &HashMap<&str, &PackEntry>,
        expanded: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
    ) -> Result<(), String> {
        if depth >= MAX_DEPENDENCY_DEPTH {
            return Err(format!(
                "dependency chain at package {id:?} exceeds the {MAX_DEPENDENCY_DEPTH}-level depth cap"
            ));
        }
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
            visit(dependency, depth + 1, entries, expanded, visiting)?;
        }
        visiting.remove(id);
        expanded.insert(id.to_string());
        Ok(())
    }

    for id in selected {
        visit(&id, 0, &entries, &mut expanded, &mut visiting)?;
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
            let shared_system_uid = context.system_uid_packages.contains(&entry.id);
            let effective_removal = if shared_system_uid {
                RemovalLevel::Unsafe
            } else {
                entry.removal
            };
            if !context.installed_packages.contains(&entry.id) {
                return PackEntryAssessment {
                    id: entry.id.clone(),
                    status: PackEntryStatus::Missing,
                    detail: Some(
                        "package is not installed for the selected Android user".to_string(),
                    ),
                    effective_removal,
                    resolved_action: entry.action.unwrap_or_default(),
                    shared_system_uid,
                    verification: assess_verification(entry, context),
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
                    effective_removal,
                    resolved_action: entry.action.unwrap_or_default(),
                    shared_system_uid,
                    verification: assess_verification(entry, context),
                }
            } else {
                PackEntryAssessment {
                    id: entry.id.clone(),
                    status: PackEntryStatus::Unsupported,
                    detail: Some(format!(
                        "required package(s) unavailable: {}",
                        unavailable.join(", ")
                    )),
                    effective_removal,
                    resolved_action: entry.action.unwrap_or_default(),
                    shared_system_uid,
                    verification: assess_verification(entry, context),
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

fn assess_verification(entry: &PackEntry, context: &DevicePackContext) -> PackVerificationStatus {
    if entry.verification.is_empty() {
        return PackVerificationStatus::Unknown;
    }
    let (Some(build_fingerprint), Some(android_level)) =
        (context.build_fingerprint.as_deref(), context.api_level)
    else {
        return PackVerificationStatus::Unknown;
    };
    let build_fingerprint = build_fingerprint.to_ascii_lowercase();
    if entry.verification.iter().any(|record| {
        record.outcome.is_positive()
            && record.android_level == android_level
            && build_fingerprint.starts_with(&record.build_fingerprint_prefix.to_ascii_lowercase())
    }) {
        PackVerificationStatus::Verified
    } else {
        PackVerificationStatus::NotVerified
    }
}

fn valid_verification_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return false;
    }
    let month = value[5..7].parse::<u8>().ok();
    let day = value[8..10].parse::<u8>().ok();
    matches!((month, day), (Some(1..=12), Some(1..=31)))
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
    fn pack_actions_are_schema_checked_and_default_to_disable() {
        let defaulted: Pack = serde_yaml_ng::from_str(GOOD).unwrap();
        assert_eq!(defaulted.packages[0].action, None);

        let suspended = GOOD.replace(
            "    removal: recommended\n",
            "    removal: recommended\n    action: suspend\n",
        );
        let parsed: Pack = serde_yaml_ng::from_str(&suspended).unwrap();
        assert_eq!(parsed.packages[0].action, Some(PackAction::Suspend));

        let unknown = suspended.replace("action: suspend", "action: erase");
        let error = serde_yaml_ng::from_str::<Pack>(&unknown)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unknown variant"),
            "unexpected error: {error}"
        );
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
    fn verification_requires_a_source_and_valid_metadata() {
        let mut pack: Pack = serde_yaml_ng::from_str(GOOD).unwrap();
        pack.packages[0].verification = vec![PackVerification {
            build_fingerprint_prefix: "google/panther".into(),
            android_level: 35,
            outcome: PackVerificationOutcome::Removed,
            date: "2026-08-09".into(),
            source: String::new(),
        }];
        let issues = lint(&pack);
        assert!(issues
            .iter()
            .any(|issue| issue.contains("verification.source is empty")));
    }

    #[test]
    fn per_entry_verification_is_exactly_scoped_to_build_and_android_level() {
        let mut pack: Pack = serde_yaml_ng::from_str(GOOD).unwrap();
        pack.packages[0].verification = vec![PackVerification {
            build_fingerprint_prefix: "google/panther".into(),
            android_level: 35,
            outcome: PackVerificationOutcome::Removed,
            date: "2026-08-09".into(),
            source: "test fixture".into(),
        }];
        let context = DevicePackContext {
            manufacturer: Some("Google".into()),
            model: Some("Pixel 7".into()),
            build_fingerprint: Some("google/panther/panther:15/test".into()),
            api_level: Some(35),
            user_id: 0,
            user_current: true,
            installed_packages: HashSet::from([pack.packages[0].id.clone()]),
            system_uid_packages: HashSet::new(),
        };
        let assessment = assess(&pack, &context);
        assert_eq!(
            assessment.entries[0].verification,
            PackVerificationStatus::Verified
        );

        let mut different_build = context.clone();
        different_build.build_fingerprint = Some("google/cheetah/cheetah:15/test".into());
        assert_eq!(
            assess(&pack, &different_build).entries[0].verification,
            PackVerificationStatus::NotVerified
        );

        pack.packages[0].verification.clear();
        assert_eq!(
            assess(&pack, &context).entries[0].verification,
            PackVerificationStatus::Unknown
        );
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
    fn deep_dependency_chains_hit_the_depth_cap_instead_of_the_stack() {
        // A crafted imported pack can nest `depends_on` links thousands deep;
        // unbounded recursion would abort the process via stack overflow.
        let mut pack: Pack = serde_yaml_ng::from_str(GOOD).unwrap();
        let template = pack.packages[0].clone();
        pack.packages = (0..(MAX_DEPENDENCY_DEPTH * 4))
            .map(|index| {
                let mut entry = template.clone();
                entry.id = format!("com.chain.p{index}");
                entry.depends_on = vec![format!("com.chain.p{}", index + 1)];
                entry
            })
            .collect();
        pack.packages.last_mut().unwrap().depends_on.clear();

        let error = expand_dependencies(&pack, vec!["com.chain.p0".to_string()]).unwrap_err();
        assert!(error.contains("depth cap"), "unexpected error: {error}");

        // A chain safely below the cap still expands fully.
        let selected = vec![format!("com.chain.p{}", pack.packages.len() - 8)];
        let expanded = expand_dependencies(&pack, selected).unwrap();
        assert_eq!(expanded.len(), 8);
    }

    #[test]
    fn exported_pack_round_trips_through_load() {
        let removed = vec![
            RemovedPackage {
                id: "com.example.bloat".into(),
                kind: RemovedKind::Disabled,
            },
            RemovedPackage {
                id: "com.example.archived".into(),
                kind: RemovedKind::Archived,
            },
            RemovedPackage {
                id: "com.example.gone".into(),
                kind: RemovedKind::Uninstalled,
            },
            // Duplicate and invalid ids are dropped, not surfaced as lint errors.
            RemovedPackage {
                id: "com.example.bloat".into(),
                kind: RemovedKind::Disabled,
            },
            RemovedPackage {
                id: ".invalid".into(),
                kind: RemovedKind::Disabled,
            },
        ];
        let context = DeviceExportContext {
            manufacturer: Some("Google".into()),
            model: Some("Pixel 8".into()),
            build_fingerprint: Some("google/panther/panther:14/AP2A.240705.004".into()),
            api_level: Some(34),
            user_id: 0,
            date: "2026-07-21".into(),
        };
        let pack = from_device_state(&removed, &context).unwrap();
        assert_eq!(pack.id, "google-pixel-8-export");
        assert_eq!(pack.packages.len(), 3);
        assert_eq!(pack.targets.user_scope, UserScope::Owner);
        assert_eq!(pack.packages[0].verification.len(), 1);
        assert_eq!(
            pack.packages[0].verification[0].build_fingerprint_prefix,
            "google/panther/panther:14/AP2A.240705.004"
        );
        assert!(lint(&pack).is_empty(), "{:?}", lint(&pack));

        // The serialized YAML must parse and lint cleanly via the import path.
        let yaml = to_yaml(&pack).unwrap();
        let dir = std::env::temp_dir().join(format!("droidsmith-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("export.yaml");
        std::fs::write(&path, yaml).unwrap();
        let loaded = load(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(loaded.id, pack.id);
        assert_eq!(loaded.packages.len(), 3);
        assert_eq!(loaded.packages[1].removal, RemovalLevel::Advanced);
    }

    #[test]
    fn export_rejects_empty_and_sanitizes_missing_metadata() {
        assert!(from_device_state(
            &[],
            &DeviceExportContext {
                date: "2026-07-21".into(),
                ..Default::default()
            }
        )
        .is_err());

        let pack = from_device_state(
            &[RemovedPackage {
                id: "com.x.y".into(),
                kind: RemovedKind::Disabled,
            }],
            &DeviceExportContext {
                user_id: 10,
                date: "2026-07-21".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(pack.id, "device-export");
        assert_eq!(pack.targets.user_scope, UserScope::Current);
        assert!(lint(&pack).is_empty());
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
            system_uid_packages: HashSet::from([pack.packages[1].id.clone()]),
        };

        let assessment = assess(&pack, &context);
        assert_eq!(assessment.status, CompatibilityStatus::Mismatch);
        assert!(assessment.override_required);
        assert_eq!(assessment.entries[0].status, PackEntryStatus::Missing);
        assert_eq!(assessment.entries[1].status, PackEntryStatus::Unsupported);
        assert_eq!(
            assessment.entries[1].effective_removal,
            RemovalLevel::Unsafe
        );
        assert!(assessment.entries[1].shared_system_uid);
        assert!(assessment.entries[1]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&pack.packages[0].id)));
    }
}
