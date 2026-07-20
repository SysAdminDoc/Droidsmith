# Research — Droidsmith

Date: 2026-07-18 — replaces all prior research.

## Executive Summary

Droidsmith v0.5.3 is a cross-platform, open-source Tauri 2 ADB management
workstation (Rust + React/TypeScript) with 9 shipped routes, 55+ IPC commands,
crash-consistent journals, 2 locales (en/ru), and comprehensive test/smoke
coverage. It occupies a unique position: no other open-source tool combines
debloating, screen mirroring, file management, logcat, profiles, and a headless
CLI in a single cross-platform ~10 MB binary. The codebase is clean (zero
TODO/FIXME markers), well-tested, and security-hardened with IPC isolation,
input validation, and atomic writes.

The competitive landscape is fragmented and weakening. UAD-ng (the primary
debloat competitor) is in a maintainer crisis and struggling with Iced GUI
rendering bugs. ADB AppControl remains Windows-only and closed-source. Escrcpy
and Aya use Electron (100+ MB). Droidsmith's Tauri 2 stack, MIT license, and
breadth-of-scope are genuine differentiators.

Top 10 opportunities in priority order:

1. **Expose scrcpy 4.x capabilities** — flex display, keep-active, VP8/VP9,
   new audio sources are shipped in scrcpy but invisible in the GUI.
2. **Auto-update mechanism** — no update path exists; users must manually
   download new releases.
3. **Post-OTA debloat drift detection** — the #1 recurring complaint across all
   ADB communities: disabled packages silently reappear after OTA updates.
4. **Additional locales** — only en/ru are shipped; the i18n architecture is
   ready for contributor-driven expansion.
5. **Expand debloat packs** — only 7 OEM packs ship; OnePlus, Vivo, Realme,
   Nothing, and Huawei are unrepresented.
6. **Device health dashboard** — battery cycle count, temperature, and detailed
   storage breakdowns are available via `dumpsys` but unsurfaced.
7. **Drag-and-drop APK install** — every commercial competitor supports this;
   Droidsmith requires a file picker dialog.
8. **Keyboard-to-touch mapping** — scrcpy's #1 feature request (55 comments on
   issue #712), QtScrcpy's most popular feature.
9. **WCAG 2.2 AA hardening** — forced-colors mode, ARIA grid for data tables,
   target size compliance.
10. **Curated debloat presets** — named profiles like "Privacy Max" or "Battery
    Saver" that select packages by tag.

## Product Map

### Core workflows
- Discover, authorize, inspect, health-check, and recover ADB-connected devices
- Manage packages: enumerate, install (APK/APKS/XAPK/APKM), backup/export,
  disable/enable/archive/uninstall, permission audit, journal undo
- Debloat with curated YAML packs (4-tier risk), dependency expansion, recovery
  baselines, and batch apply with verification
- Mirror/control via scrcpy: launch/supervision, presets, recording, codec
  negotiation
- Automate with declarative YAML profiles (GUI + headless CLI), portable
  recovery baselines

### User personas
- Privacy-conscious Android user (debloat, audit permissions)
- Power user / tinkerer (shell, fastboot, settings, multi-device)
- Developer (logcat, layout inspector, processes, bugreport)
- IT / fleet manager (profiles, baselines, headless CLI, batch ops)

### Platforms and distribution
- Windows (MSI/NSIS), macOS (DMG), Linux (AppImage/deb/rpm — blocked on build host)
- Single binary ~10 MB via Tauri vs Electron competitors at ~100+ MB
- MIT licensed, intentionally local-first, zero telemetry

### Key integrations
- ADB binary (shell-out with typed Rust parsers)
- scrcpy (PATH detection, capability probing, session supervision)
- Platform-tools policy (version pinning, SHA-256 verification)
- i18next (renderer i18n with CLDR plural rules)
- Tauri Specta (generated TypeScript IPC bindings)

## Competitive Landscape

### UAD-ng — 8,535 stars, Rust/Iced, GPL-3.0
**Does well:** Community-curated debloat lists with per-package safety ratings
and descriptions; snapshot/restore; multi-user targeting.
**Learn from:** Auto-fetch of package definitions at launch (no app update
needed for list changes); "disable by default" safety philosophy (issue #1426).
**Avoid:** Iced GUI framework (Wayland text rendering completely broken per
issue #884, wgpu crashes); narrow debloat-only scope; maintainer crisis
(discussion #731 — actively seeking Rust developers).

### Escrcpy — 10,495 stars, Electron/Vue, Apache-2.0
**Does well:** Keyboard mapping for touch/joystick/swipe; multi-device batch
broadcast; Gnirehtet reverse tethering integration; inset mirror window.
**Learn from:** Keyboard mapping UX patterns; batch screenshot across devices.
**Avoid:** Electron footprint; freemium paid tier; AI assistant gimmick.

### Aya — 5,280 stars, Electron/TypeScript, AGPL-3.0
**Does well:** Real-time CPU/memory/FPS monitoring; multi-session shell;
Kotlin on-device companion for deeper interaction.
**Learn from:** Performance monitoring dashboard; clipboard sync.
**Avoid:** Electron; AGPL license; Chinese-only community.

### ADB AppControl — closed-source, Windows-only, .NET
**Does well:** Debloat wizard with risk scanning; permission manager (paid);
display DPI/resolution tweaks; command favorites.
**Learn from:** Feature completeness that defines the market baseline.
**Avoid:** Windows-only; closed source; paywall; anonymous telemetry.

### scrcpy — 145,904 stars, C/SDL3, Apache-2.0
**Does well:** v4.0 flex display (resizable virtual displays); keep-active;
VP8/VP9 (v4.1); expanded audio sources; 35ms latency.
**Learn from:** Every new CLI option is a GUI feature Droidsmith should expose.
**Avoid:** CLI-only (Droidsmith fills this exact gap).

### ADBKit — 326 stars, Wails v2/Go/React, MIT
**Does well:** Binary Manager (auto-detect/download/version-track ADB +
fastboot + scrcpy); setup wizard; A/B slot management.
**Learn from:** Binary lifecycle management UX; the setup wizard concept.
**Avoid:** Code quality concerns; 126+ ESLint errors.

### ScrcpyGUI — 1,163 stars, Tauri v2/React, MIT
**Does well:** Same stack as Droidsmith; auto-update checker for scrcpy;
HID keyboard/mouse mode toggle; 5 theme variants.
**Learn from:** Tauri v2 + React integration patterns for scrcpy management.
**Avoid:** Windows-only despite cross-platform stack; mirror-only scope.

### Canta — 5,291 stars, Kotlin/Shizuku, LGPL-3.0
**Does well:** Consumes UAD-ng debloat list; no-PC on-device debloating.
**Learn from:** Validates that UAD-ng list is a reusable data source.
**Avoid:** Shizuku dependency; Samsung/MIUI bugs; reinstall failures.

## Security, Privacy, and Reliability

### Tauri CVEs to verify
- **CVE-2026-42184** (Medium 6.1): Origin confusion on Windows/Android. Fixed
  in tauri >=2.11.1. Verify resolved version in Cargo.lock.
- **CVE-2025-31477** (Critical 9.3): plugin-shell `open` RCE. Fixed in
  plugin-shell >=2.2.1. Droidsmith uses scoped capabilities (IMP-02) but
  verify the resolved plugin version.

### Platform-tools 37.0.1 (July 2026 canary)
- Deleted `openscreen` mDNS backend — health checks should stop referencing.
- New `ADB_USB_LEGACY=1` env var for Windows fallback — surface in host doctor.
- `kill-server` now reports blame (process holding port) — improve recovery UX.
- VP8/VP9 encoder support in scrcpy 4.1 for older/low-end devices.

### Dependency health
- All core Rust deps (serde, zip, sha2, base64, uuid) have zero RustSec
  advisories as of 2026-07-18.
- `proptest` pinned at 1.8.0 for MSRV — check if newer is compatible.
- Release-policy exceptions expire 2026-10-15 — schedule re-audit.

### Debloat safety
- UAD-ng issues #1400 (Xiaomi bootloop from "Recommended") and #1311 (Samsung)
  demonstrate per-OEM-ROM ratings are unreliable. Droidsmith already defaults to
  disable-first and requires unsafe-tier acknowledgement — correct mitigation.
- Post-OTA drift detection is missing: packages silently re-enabled after OTA.

### Android 16 changes
- `--bypass-low-target-sdk-block` flag for installing apps below SDK floor.
- Advanced Protection Mode may restrict Developer Options on future builds.
- OEM bootloader unlock restrictions tightening (Samsung removed toggle on
  OneUI 8; OnePlus requires permission request; Xiaomi 30-day wait).

## Architecture Assessment

### Strengths
- Clean command/domain/transport separation with thin IPC glue.
- JSON-Lines journal is HGFS-safe, diffable, zero-dependency.
- Schema versioning (packs v1, quirks v1, profiles v2) with migration paths.
- Generated TypeScript bindings via Tauri Specta eliminate type drift.
- Comprehensive smoke harness (30+ flows, 200% zoom, i18n sweep).

### Refactor candidates
- `src/routes/Devices.tsx` (2,975 LOC) and `src/routes/Apps.tsx` (3,063 LOC)
  contain multiple independent sub-panels. Extract into
  `src/routes/devices/*.tsx` and `src/routes/apps/*.tsx` subdirectories.
- Three empty duplicate "Research-Driven Additions" sections in ROADMAP.md.

### Test gaps
- No CLI smoke test for `droidsmith-cli` (parse → plan → dry-run with mock).
- Rendered-route smoke doesn't cover Settings export/import round-trip or
  Profiles workspace end-to-end.
- No fuzz target for scrcpy help-text/version parser (`src-tauri/src/scrcpy.rs`).

### Documentation drift
- `CONTRIBUTING.md` references R-030, R-034, R-070 as "coming" — shipped.
- `LICENSE-THIRD-PARTY.md` references R-010, R-040, R-036 as planned — stale.
- `docs/DEVELOPMENT.md` references R-010 four times as pending — stale.

## Rejected Ideas

| Idea | Source | Rejection reason |
|------|--------|-----------------|
| Pure-Rust ADB (replace shell-out) | adb_client v3.2.2 | Reference `adb` binary handles all OEM quirks; re-evaluate at adb_client v4. |
| On-device companion agent | Aya, Shizuku | Droidsmith's value is zero-install desktop management; Shizuku owns device-side. |
| Web version via WebUSB | ya-webadb/Tango | WebUSB is Chromium-only (<76% browsers); Firefox/Safari oppose. Not viable. |
| AI assistant | Escrcpy AutoGLM | Gimmicky; external LLM dependency contradicts local-first philosophy. |
| Electron migration | Aya, Escrcpy | ~10 MB Tauri binary is a core differentiator over ~100 MB Electron. |
| SQLite for journals | Internal analysis | JSON-Lines: HGFS-safe, diffable, zero-dep. SQLite adds complexity for no benefit. |
| Flatpak packaging | UAD-ng #1423 | Flatpak sandbox restricts USB access, making ADB unreliable. AppImage is correct. |
| Real-time network sniffing | ADBloat | Requires root or on-device VPN; out of scope for rootless desktop tool. |
| Root/bootloader/flashing | PixelFlasher | Incompatible with read-only fastboot scope and safety philosophy. |
| Cloud fleet / remote ADB | DeviceFarmer, Vysor | Expands trust boundary; solves different multi-operator problem. |
| Crowd-sourced debloat scores | Community signal | Quality variance too high; UAD-ng governance issues validate curated approach. |
| Immediate React 19/Vite 8/TW4 | Ecosystem | No current advisory or capability gap justifies the migration risk. |
| Full Android Studio Logcat grammar | AS reference | Named bounded presets cover workflow without IDE-scale parser. |

## Sources

### Competitors
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation
- https://github.com/liriliri/aya
- https://github.com/viarotel-org/escrcpy
- https://github.com/Drenzzz/ADBKit
- https://github.com/kil0bit-kb/scrcpy-gui
- https://github.com/samolego/Canta
- https://github.com/yume-chan/ya-webadb
- https://adbappcontrol.com/en/

### Mirror/control
- https://github.com/Genymobile/scrcpy
- https://github.com/Genymobile/scrcpy/releases/tag/v4.0
- https://github.com/Genymobile/scrcpy/releases/tag/v4.1
- https://github.com/barry-ran/QtScrcpy
- https://github.com/Genymobile/gnirehtet

### Community signal
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/discussions/731
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/583
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1400
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1426
- https://github.com/Genymobile/scrcpy/issues/712
- https://www.androidauthority.com/android-sideloading-24-hours-adb-3650540/
- https://www.androidauthority.com/android-wireless-adb-auto-reconnect-3624945/

### Platform / standards
- https://developer.android.com/about/versions/16/features
- https://developer.android.com/about/versions/16/behavior-changes-all
- https://developer.android.com/tools/releases/platform-tools
- https://v2.tauri.app/plugin/
- https://v2.tauri.app/plugin/updater/
- https://github.com/tauri-apps/tauri/security/advisories
- https://github.com/cocool97/adb_client

### Ecosystem
- https://github.com/RikkaApps/Shizuku
- https://github.com/timschneeb/awesome-shizuku
- https://github.com/mzlogin/awesome-adb
- https://github.com/nmeum/android-tools

### UX / accessibility
- https://www.w3.org/TR/WCAG22/
- https://www.w3.org/WAI/ARIA/apg/patterns/grid/
- https://www.smashingmagazine.com/2022/06/guide-windows-high-contrast-mode/
- https://docs.syncthing.net/intro/gui.html

### Security
- https://github.com/tauri-apps/tauri/security/advisories/GHSA-7gmj-67g7-phm9
- https://github.com/tauri-apps/tauri/security/advisories/GHSA-c9pr-q8gx-3mgp
- https://rustsec.org/

## Open Questions

1. **Tauri version pinning**: Verify the resolved Tauri core is >=2.11.1
   (CVE-2026-42184) and plugin-shell >=2.2.1 (CVE-2025-31477). If not, this
   becomes a P0 item.
2. **UAD-ng list format stability**: If R-036 redistribution permission is
   granted, does the UAD-ng JSON schema have a stability guarantee, or will
   Droidsmith need a format adapter?
3. **scrcpy v4.x minimum**: Should capability probing require scrcpy >=4.0 for
   flex display, or degrade gracefully for v3.x users? (Recommend: degrade.)
4. **Vulnerability reporting channel**: `SECURITY.md` still names
   `security@droidsmith.invalid`. Needs a real contact or GitHub private
   vulnerability reporting enabled.
5. **Auto-update signing key**: The tauri-plugin-updater requires an Ed25519
   keypair. Where should the private key be stored, and who manages rotation?
