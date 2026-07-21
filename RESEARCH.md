# Research — Droidsmith

Date: 2026-07-21 — replaces all prior research (previous pass was 2026-07-20 /
v0.9.1; the bulk of its top-10 has since shipped through v0.9.4).

## Executive Summary

Droidsmith v0.9.4 is a cross-platform, open-source Tauri 2 (Rust core +
React/TS) ADB workstation: device discovery/authorize, wireless pairing with
connection history + auto-reconnect, package install (APK/APKS/XAPK/APKM) with
`--bypass-low-target-sdk-block` retry and reversible archive/unarchive, backup/
export, permission audit, per-device journal undo, curated + **locally
importable** YAML debloat packs (v0.9.4), scrcpy launch/supervision/recording
with a broad 27-field flag surface (max-fps, crop, fullscreen, no-control,
orientation, screen-off, audio codec/source, **virtual/flex display**,
**keep-active**, camera mirroring, AV1/VP8/VP9), gnirehtet reverse tethering,
file/network/process(force-stop)/layout inspectors, logcat with saved queries,
YAML automation profiles + headless CLI, read-only fastboot, quirk-based
failure hints, diagnostics/host-doctor, and 5 locales. Still the only OSS tool
combining all of this in a ~10 MB binary.

The codebase is exceptionally mature: **zero TODO/FIXME markers, 88/88 IPC
commands wired to the UI** (the previously-orphaned `explain_failure` is now
called from Debloat.tsx:1802), tauri pinned to 2.11.2 (patches the
Origin-Confusion advisory GHSA-7gmj-67g7-phm9, fixed ≥2.11.1). Because the
prior pass's frontier largely shipped, the remaining opportunity has shifted
from "catch up on scrcpy/ADB parity" to **"become the local-first Android
inspection + lifecycle workstation"** — capabilities no scrcpy front-end offers
and that are fully verifiable in the headless smoke harness.

Top opportunities in priority order:

1. **APK Analyzer panel** (offline, no device) — manifest / permissions / dex
   counts / 64K-multidex / size breakdown / signing scheme. No OSS competitor
   in the set has it; 100% offline; builds on existing `apk_metadata.rs`.
2. **Export current device debloat state as a shareable pack/selection** —
   symmetric to the v0.9.4 local import; UAD-ng `selection_export` parity;
   headless.
3. **`process_tree.rs` unit tests** — the one security-relevant module still
   with no `#[cfg(test)]` (child-containment/kill).
4. **Dependency/security re-audit** — release-policy exceptions expire
   2026-10-15; `time` and transitive `reqwest`/`hyper` are in the tree.
5. **scrcpy `--display-ime-policy` + `--no-vd-destroy-content`** — the two
   remaining headless-testable flags that complete the virtual-display surface.
6. **Diagnostics: `adb server-status` mDNS + negotiated USB link speed** —
   richer host-doctor from platform-tools 36/37.
7. **ANR/tombstone/crash-log viewer** and **per-process meminfo/gfxinfo
   snapshot** — Studio-grade debugging no scrcpy front-end offers.

## Product Map

- **Core workflows:** discover→authorize→inspect/health→recover (USB +
  wireless); package lifecycle (enumerate, install multi-format, backup/export,
  disable/enable/archive/unarchive/uninstall, permissions, journal undo);
  debloat via 4-tier YAML packs (bundled + imported) with dependency expansion,
  recovery baselines, verified batch apply; scrcpy mirror/control/record;
  declarative YAML profiles (GUI + `droidsmith-cli`).
- **Personas:** privacy-conscious user; power user (shell/fastboot/tuning);
  developer (logcat/layout/process/bugreport); IT/fleet (profiles, baselines,
  headless CLI).
- **Platforms/distribution:** Windows (MSI/NSIS built locally, unsigned), macOS
  (DMG), Linux (AppImage/deb/rpm — blocked on build host). ~10 MB, MIT,
  local-first, zero telemetry.
- **Integrations:** ADB (typed Rust parsers over shell-out), scrcpy (PATH probe
  + supervised sessions), gnirehtet, platform-tools SHA-256 policy, i18next,
  tauri-specta generated bindings.

## Competitive Landscape

### Escrcpy — Apache-2.0, Electron/Vue (v2.11.1, 2026-05, 10.5k★, very active)
Ships input mapping, multi-device broadcast, batch screenshot/APK, draggable
control sidebar, and **AutoGLM natural-language control** (issue #614 requests
MCP). Learn: the batch/broadcast orchestration UX. Avoid: Electron footprint.

### QtScrcpy — Apache-2.0, Qt/C++ (v3.3.3, 2025-11, 30.6k★)
Game keymap scripts + **group control** (one input → all devices). Learn: the
group-broadcast model. Avoid: bundled scrcpy-fork maintenance.

### UAD-ng — GPL-3.0, Rust/iced (v1.2.0, 2026-01, 8.6k★)
**Portable package-state snapshot/restore** and **`selection_export.txt`**
(+ export uninstalled-with-descriptions), multi-user. Learn: the shareable
selection export and snapshot→apply-to-many — Droidsmith just shipped *import*
but not the symmetric *export*. Still has **no post-OTA re-application** (an
ecosystem-wide gap Droidsmith's drift detection partially addresses).

### Canta — Shizuku/on-device (v3.2.2, 2026-03)
**Tracks previously-uninstalled apps across reinstalls** and flags OEM
re-adds after OTA. Learn: the "these got resurrected by the last update"
reconciliation view. Avoid: on-device/Shizuku model.

### Aya — AGPL-3.0, Electron (v1.14.2, 2025-11, 5.3k★)
Live CPU/mem/**FPS** perf monitor + multi-session terminal. Learn: `dumpsys
gfxinfo/meminfo` polling for a per-process snapshot (Droidsmith has the process
inspector but no memory/CPU columns). Avoid: Electron, AGPL, device-side FPS
hook.

### ADB AppControl — closed, Windows/.NET
**Paywalls** a Debloat Wizard, richer Process Manager, dark theme, batch
install. Paywalled = undervalued OSS target: Droidsmith already gives debloat +
process force-stop + dark theme free; a guided debloat *wizard* is the delta.
Avoid: Windows-only, closed, telemetry.

### scrcpy — Apache-2.0, C/SDL (v4.0, 2026-05)
Every new flag is a potential GUI toggle. Droidsmith already exposes flex
display, keep-active, camera, AV1/VP8/VP9. Remaining unexposed: headless-safe
`--display-ime-policy`, `--no-vd-destroy-content`; device-dependent
`--camera-torch`/`--camera-zoom`, `--gamepad=uhid`, OTG control-only.

## Security, Privacy, and Reliability

- **Tauri Origin-Confusion (GHSA-7gmj-67g7-phm9, ≤2.11.0):** already mitigated —
  Cargo.lock pins `tauri` 2.11.2. No action beyond keeping the pin ≥2.11.1.
  (Verified: `src-tauri/Cargo.lock`.)
- **Dependency re-audit due:** release-policy exceptions expire **2026-10-15**.
  `time`, `reqwest`, `hyper`, `tokio` are present transitively; `rustls`/`webpki`
  are **not** in the tree, so RUSTSEC rustls-webpki advisories are N/A, but
  RUSTSEC-2026-0009 (`time` DoS) should be checked with `cargo audit`.
- **`process_tree.rs`** (child-containment/kill, security-relevant) is the sole
  module with **no `#[cfg(test)]`** — a regression could let adb/fastboot/scrcpy
  children outlive an operation. (host_path/install/scrcpy/gnirehtet all have
  test modules — a prior claim to the contrary was incorrect.)
- **Advanced Protection Mode (Android 16/17):** Google is wiring APM to block
  Developer Options and USB data signaling. Host-doctor should detect
  "present-but-unauthorized/APM-locked" and explain it rather than showing a
  generic auth error. (Needs live validation; no stable adb signal yet — stays
  in Roadmap_Blocked as R-092 remainder.)
- **platform-tools 37.0.1 (Jul 2026):** `openscreen` mDNS backend deleted
  (`ADB_MDNS_OPENSCREEN` now a no-op); new `libadbusb`; the `ADB_USB_LEGACY`/
  `ADB_LIBUSB` toggles Droidsmith already surfaces remain correct.

## Architecture Assessment

### Strengths
Thin IPC glue over domain modules; JSON-Lines journals (HGFS-safe); schema
versioning with migration paths; generated TS bindings; 30+ flow smoke harness;
zero stubs; 88/88 commands wired.

### Refactor candidates
- **`src/routes/Debloat.tsx`** is now 1,928 LOC with ~10 inline sub-components
  (`DebloatApplyReview`, `PackErrors`, `PackImportControl`, `PackCard`,
  `PackPicker`, `PackPreview`, `CompatibilityChecks`, `QueueApply*`, `QuirkHint`)
  — extract to `src/routes/debloat/` to match the completed IMP-67/IMP-72 splits.
- Secondary: `Apps.tsx` (1,686), `Logcat.tsx` (1,177), `Mirror.tsx` (1,158),
  `Profiles.tsx` (1,127), `Wireless.tsx` (1,056) — lower urgency; each already
  has self-contained panels.

### Test gaps
- `src-tauri/src/process_tree.rs` — no tests (security-relevant).
- Route-level React components (Apps/Debloat/Mirror/Logcat/etc.) have no
  component tests, but their extracted logic modules (`debloatPack`,
  `debloatQueue`, `mirrorPresets`, `appsJournal`, `logcatQueries`, `deviceStore`,
  `settings`) are covered — the logic/UI split keeps this acceptable.

### Data / feature gaps (verified)
- **No capture-from-device path:** `save_profile` (commands.rs) and
  `profile.rs` only persist GUI-authored profiles — there is no way to export
  the current device's debloat/package state as a shareable pack or selection
  (UAD-ng parity gap). Recovery baselines snapshot state for OTA drift review
  but are not a portable, re-appliable debloat artifact.
- **No APK static-analysis surface:** `apk_metadata.rs` exists (~30 KB) and is
  used during install, but there is no offline APK Analyzer route
  (manifest/permissions/dex/size).
- **No crash-artifact viewer:** Diagnostics captures bugreports but does not
  parse ANR/tombstone/`dumpsys dropbox` records into a browsable list.

## Rejected Ideas

| Idea | Source | Rejection reason |
|------|--------|-----------------|
| App archive / unarchive | platform research | Already shipped — `ActionKind::Archive` + `RequestUnarchive` in `adb/actions.rs`. |
| `--bypass-low-target-sdk-block` install retry | Android 15 research | Already shipped — `install.rs` + `Apps.tsx` override dialog. |
| Tauri Origin-Confusion mitigation | GHSA-7gmj-67g7-phm9 | Already patched — Cargo.lock pins tauri 2.11.2 (≥2.11.1). |
| ARIA `role="grid"` table conversion | internal a11y scan | Plain semantic `<table>` is the correct pattern for static tables; the baseline is deliberate and documented (IMP-73). Not a defect. |
| Multi-device broadcast / group control | Escrcpy, QtScrcpy | Overlaps blocked IMP-52 (needs ≥2 physical devices to verify). Track there. |
| Keyboard→touch / game keymap editor | Escrcpy #618, QtScrcpy | Vanilla scrcpy has no mapping API; parked as blocked R-081. Needs a device. |
| Camera torch/zoom, `--gamepad=uhid`, OTG control-only, floating control sidebar | scrcpy v4.0, #4793 | Device-dependent to verify; keep as thin best-effort scrcpy passthroughs, not owned code Droidsmith must device-test each release. |
| Remote ADB tunneling (`adb connect` from anywhere) | DeviceFarmer/STF | Contradicts local-first; widens the network/threat surface. |
| Browser/WebSocket remote view (ws-scrcpy) | ws-scrcpy | Contradicts the ~10 MB local-first binary; heavy decoder stack. |
| Live FPS/perf overlay needing a device-side hook | Aya | Contradicts zero-install; only the headless `dumpsys` snapshot half is filed (R-103). |
| Physical-device locate (flash bright screen) | STF | No portable ADB primitive without a device-side app; gimmicky. |
| Pure-Rust `adb_client` (drop shell-out) | droidtui | Reference `adb` binary handles OEM quirks; re-evaluate at `adb_client` maturity. |
| MCP server over `droidsmith-cli` | Escrcpy #614 | Interesting leapfrog and fits the CLI/YAML strength, but speculative + L effort; left Under Consideration, not roadmapped this pass. |
| SharedPreferences/SQLite inspector | Android Studio | Requires `run-as` (debuggable apps only) or root; device-dependent, narrow scope — Under Consideration. |

## Sources

### Competitors / ecosystem
- https://github.com/viarotel-org/escrcpy
- https://github.com/barry-ran/QtScrcpy
- https://github.com/liriliri/aya
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/wiki/Usage
- https://github.com/samolego/Canta
- https://github.com/DeviceFarmer/stf
- https://adbappcontrol.com/en/
- https://vysor.org/vysor-pro/
- https://developer.android.com/tools/apkanalyzer

### scrcpy / platform-tools / Android
- https://github.com/Genymobile/scrcpy/releases/tag/v4.0
- https://github.com/Genymobile/scrcpy/releases/tag/v3.3
- https://github.com/Genymobile/scrcpy/releases/tag/v3.2
- https://github.com/Genymobile/scrcpy/blob/master/doc/gamepad.md
- https://github.com/Genymobile/scrcpy/blob/master/doc/audio.md
- https://github.com/Genymobile/scrcpy/issues/6643
- https://github.com/Genymobile/scrcpy/issues/4793
- https://developer.android.com/tools/releases/platform-tools
- https://bayton.org/android/advisories/android-15-app-install/
- https://www.esper.io/blog/android-dessert-bites-16-app-archiving-857169
- https://support.google.com/android/answer/15341885
- https://www.androidauthority.com/android-advanced-protection-mode-developer-options-3679725/
- https://developer.android.com/studio/inspect/database
- https://developer.android.com/studio/inspect/task

### Security
- https://github.com/tauri-apps/tauri/security/advisories/GHSA-7gmj-67g7-phm9
- https://rustsec.org/advisories/
- https://blog.rust-lang.org/2026/03/21/cve-2026-33056

## Open Questions

1. **Export format for device→pack (R-098):** should the exported artifact be a
   full debloat *pack* (id/targets/packages, importable via the v0.9.4
   `import_pack`) or a lightweight *selection list* (package ids + action)? Pack
   format re-uses existing validation and round-trips through import; recommend
   pack, with an optional flat-list export for UAD-ng interop.
2. **APK Analyzer dex depth (R-097):** full method/class enumeration needs a dex
   parser (bytes → header counts is cheap; per-method requires more). Scope v1
   to header-level counts + 64K check + manifest/permissions/size, defer
   per-method trees unless a maintained pure-Rust dex crate is confirmed.
3. **APM detection signal:** still no stable adb property to distinguish
   "Advanced Protection disabled debugging" from an ordinary unauthorized
   device — R-092 remainder stays blocked until one is confirmed on a device.
