//! Canonical persistence identity for one device.
//!
//! Everything Droidsmith stores per device — the action journal, per-device
//! settings scopes, recovery-baseline ownership — was keyed on the ADB serial
//! alone. Serials are not unique in practice: clone and OEM firmware ships
//! duplicated values, and some devices report an empty one (scrcpy #1148,
//! #3537). Two such devices shared a single journal file, so an undo row
//! recorded against device A became offerable against device B.
//!
//! The identity therefore mixes the build fingerprint into the serial. The
//! fingerprint is not itself device-unique — two identical handsets on the same
//! build report the same string — but it is the strongest stable attribute
//! Droidsmith already holds for every device it is allowed to mutate, and every
//! mutation path proves it is present before touching the device
//! (`adb::validate_device_target` fails closed without one). Duplicate serials
//! across differing builds now separate; duplicate serials on identical builds
//! remain indistinguishable, which is stated here rather than papered over.
//!
//! Runtime *addressing* is a separate concern and was already correct:
//! `DeviceTarget::adb_selector` prefers `-t <transport_id>`. A transport id is
//! per-server-session, so it can address a device but can never key persistence.

use crate::adb::DeviceTarget;

/// Separates the serial from the fingerprint in the canonical form. `|` is
/// rejected by `adb::valid_serial`, so the first occurrence always ends the
/// serial even if a fingerprint ever carried one.
const IDENTITY_SEPARATOR: char = '|';

/// A device's identity for persistence purposes.
///
/// The fingerprint is optional so legacy stores — written before the
/// fingerprint was mixed in — stay addressable. An identity without one is a
/// *legacy* identity: it is readable, but no mutation path can produce it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    serial: String,
    fingerprint: Option<String>,
}

impl DeviceIdentity {
    /// Build an identity from a serial and an optionally-known build
    /// fingerprint. A blank or whitespace-only fingerprint is treated as
    /// unknown rather than as a distinct value, so a device never flips
    /// between two stores because a probe returned an empty string.
    pub fn new(serial: &str, build_fingerprint: Option<&str>) -> Self {
        Self {
            serial: serial.to_string(),
            fingerprint: build_fingerprint
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        }
    }

    pub fn from_target(target: &DeviceTarget) -> Self {
        Self::new(&target.serial, target.build_fingerprint.as_deref())
    }

    /// An identity for a store written before fingerprints were mixed in.
    /// Reserved for migration and upgrade-verification readers; production
    /// mutation paths always have a fingerprint.
    pub fn legacy_serial_only(serial: &str) -> Self {
        Self::new(serial, None)
    }

    /// Recover an identity from a canonical string. Round-trips
    /// [`DeviceIdentity::canonical`]; a string with no separator parses as a
    /// legacy serial-only identity, which is exactly what pre-fingerprint
    /// callers stored.
    pub fn parse(canonical: &str) -> Self {
        match canonical.split_once(IDENTITY_SEPARATOR) {
            Some((serial, fingerprint)) => Self::new(serial, Some(fingerprint)),
            None => Self::legacy_serial_only(canonical),
        }
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn fingerprint(&self) -> Option<&str> {
        self.fingerprint.as_deref()
    }

    /// Stable string form. Domains that need an opaque key hash this with
    /// their own domain separator; the renderer sends this exact string over
    /// IPC for the settings store. `src/lib/deviceIdentity.ts` mirrors this
    /// format and is pinned to the same literal by tests on both sides.
    pub fn canonical(&self) -> String {
        match &self.fingerprint {
            Some(fingerprint) => format!("{}{IDENTITY_SEPARATOR}{fingerprint}", self.serial),
            None => self.serial.clone(),
        }
    }

    /// The canonical form this device used before the fingerprint was mixed
    /// in, or `None` when this identity already *is* that form. Callers use it
    /// to adopt a pre-existing store in place instead of silently starting
    /// empty.
    pub fn legacy_canonical(&self) -> Option<String> {
        self.fingerprint.as_ref().map(|_| self.serial.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_form_is_pinned_and_mirrored_by_the_renderer_helper() {
        // `src/lib/deviceIdentity.test.ts` asserts this exact literal. A change
        // on either side without the other is a silent store split.
        let identity = DeviceIdentity::new(
            "R5CT60ZQR4M",
            Some("google/panther/panther:16/BP1A/1:user/release-keys"),
        );
        assert_eq!(
            identity.canonical(),
            "R5CT60ZQR4M|google/panther/panther:16/BP1A/1:user/release-keys"
        );
    }

    #[test]
    fn an_unknown_fingerprint_keeps_the_legacy_serial_only_form() {
        // Byte-identical to what pre-fingerprint builds stored, so legacy
        // journals and settings scopes stay addressable without a rewrite.
        for absent in [None, Some(""), Some("   ")] {
            let identity = DeviceIdentity::new("abc", absent);
            assert_eq!(identity.canonical(), "abc");
            assert_eq!(identity.fingerprint(), None);
            assert_eq!(identity.legacy_canonical(), None);
        }
    }

    #[test]
    fn duplicate_serials_on_different_builds_separate() {
        let first = DeviceIdentity::new("SHARED", Some("brand/a:16/A/1:user/release-keys"));
        let second = DeviceIdentity::new("SHARED", Some("brand/b:16/B/2:user/release-keys"));
        assert_ne!(first.canonical(), second.canonical());
        assert_eq!(first.legacy_canonical().as_deref(), Some("SHARED"));
        assert_eq!(second.legacy_canonical().as_deref(), Some("SHARED"));
    }

    #[test]
    fn parse_round_trips_both_forms() {
        for identity in [
            DeviceIdentity::new("abc", Some("brand/x:16/X/1:user/release-keys")),
            DeviceIdentity::legacy_serial_only("abc"),
            DeviceIdentity::legacy_serial_only(""),
        ] {
            assert_eq!(DeviceIdentity::parse(&identity.canonical()), identity);
        }
    }

    #[test]
    fn a_fingerprint_bearing_separator_still_ends_the_serial_at_the_first_one() {
        let parsed = DeviceIdentity::parse("abc|brand|weird");
        assert_eq!(parsed.serial(), "abc");
        assert_eq!(parsed.fingerprint(), Some("brand|weird"));
    }

    #[test]
    fn from_target_reads_the_verified_fingerprint() {
        let mut target = DeviceTarget {
            serial: "abc".into(),
            build_fingerprint: Some("brand/x:16/X/1:user/release-keys".into()),
            ..DeviceTarget::default()
        };
        assert_eq!(
            DeviceIdentity::from_target(&target).canonical(),
            "abc|brand/x:16/X/1:user/release-keys"
        );
        target.build_fingerprint = None;
        assert_eq!(DeviceIdentity::from_target(&target).canonical(), "abc");
    }
}
