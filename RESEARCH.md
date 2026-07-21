# Research — Droidsmith

Date: 2026-07-20 — replaces all prior research (prior pass was 2026-07-18 / v0.5.3
and is now largely shipped).

## Executive Summary

Droidsmith v0.9.1 is a cross-platform, open-source Tauri 2 (Rust 2.11.2 core +
React/TS) ADB workstation with a broad live surface: device discovery/authorize,
wireless pairing, package management (install APK/APKS/XAPK/APKM, backup/export,
disable/enable/archive/uninstall, permission audit, reversible journal), curated
debloat (9 OEM packs), scrcpy launch/supervision/recording, gnirehtet reverse
tethering, file manager, network/process/layout inspectors, logcat with saved
queries, YAML automation profiles + headless CLI, read-only fastboot, a
shell/console with a read-only safety gate, diagnostics/host-doctor, and 5
locales (de/en/es/ru/zh). It remains the only OSS tool combining all of this in a
~10 MB binary. Since the last research pass most of the old top-10 shipped
(scrcpy exposure, gnirehtet, locales 2→5, packs 7→9, sub-panel extraction), so
this pass focuses on the **next** frontier.

The codebase is exceptionally clean post-audit: zero TODO/FIXME markers, no
stubs, 73 IPC commands all (bar one) wired to the UI. The remaining opportunities
are (1) closing the loudest unmet ADB-desktop complaints (wireless
reconnect, multi-device), (2) the one genuinely unsolved market gap (post-OTA
debloat drift), (3) exposing the large scrcpy 3.0–4.1 flag surface Droidsmith
launches but doesn't drive, and (4) tightening a handful of concrete internal
gaps a prior audit left.

Top opportunities in priority order:

1. **Wireless connection history + auto-reconnect on launch** — the single
   loudest recurring complaint across ADB tooling (scrcpy #721/#4866/#3003).
2. **Post-OTA debloat-drift detection** — build-fingerprint change → re-check the
   recovery baseline. No competitor solves this; strongest differentiation lever.
3. **Wire up (or remove) the orphaned `explain_failure` quirks-hint IPC** — built,
   registered, tested, but never called from any UI surface.
4. **Expand the Mirror scrcpy flag surface** — `--max-fps`, `--crop`,
   `--fullscreen`, `--no-control` (view-only), `--display-orientation`,
   `--screen-off-timeout`, `--audio-codec`, then new/flex virtual display,
   camera mirroring, VP8/VP9, rich audio sources.
5. **ProcessManager force-stop action** + inspector export (Network/Layout).
6. **`--bypass-low-target-sdk-block` install option + Advanced-Protection
   detection** — Android 14–16 will otherwise fail opaquely.
7. **process_tree.rs + deviceStore.ts test coverage** — untested
   security/lifecycle code.
8. **LayoutInspector filter-to-empty blank panel** and the smaller state/a11y
   fixes.

## Product Map

### Core workflows
- Discover → authorize → inspect/health-check → recover ADB devices (USB +
  wireless pairing).
- Package lifecycle: enumerate, install multi-format, backup/export, disable/
  enable/archive/uninstall, permission audit, per-device journal undo.
- Debloat via 4-tier YAML packs with dependency expansion, recovery baselines,
  and verified batch apply.
- Mirror/control via supervised scrcpy sessions (presets, recording, codecs).
- Automate via declarative YAML profiles (GUI + `droidsmith-cli`).

### User personas
Privacy-conscious user (debloat/permissions); power user (shell, fastboot,
tuning, multi-device); developer (logcat, layout inspector, processes,
bugreport); IT/fleet (profiles, baselines, headless CLI).

### Platforms and distribution
Windows (MSI/NSIS), macOS (DMG), Linux (AppImage/deb/rpm — blocked on build
host). ~10 MB Tauri binary, MIT, local-first, zero telemetry.

### Key integrations
ADB (typed Rust parsers over shell-out), scrcpy (PATH probe + session
supervision), gnirehtet, platform-tools policy (SHA-256 pinning), i18next,
tauri-specta generated bindings.

## Competitive Landscape

### Escrcpy — Apache-2.0, Electron/Vue
Ships **keyboard-to-touch mapping**, **multi-device single-window control with
input broadcast**, batch screenshot/APK install, reorderable control bar.
Learn: the in-GUI input-mapping layer (synthetic touch injection — not a scrcpy
flag) and multi-device broadcast UX. Avoid: Electron footprint, paid tier.

### QtScrcpy — Apache-2.0, Qt/C++
Ships **game keymap scripts** and **group control** (one input → many phones).
Learn: keymap-profile model and group broadcast. Avoid: bundled scrcpy fork
maintenance burden.

### UAD-ng — GPL-3.0, Rust/iced (v1.2.0, 2026-01, healthy)
**Remote-updatable community package list**, safety tiers
(Recommended/Advanced/Expert/Unsafe), **snapshots applied to many devices**,
`--user` multi-user targeting. Learn: remote list updates without an app
release; snapshot→apply-to-many. Avoid: iced Wayland rendering issues;
debloat-only scope. Note: **no post-OTA re-application** — an ecosystem-wide gap.

### Aya — AGPL-3.0, Electron
**Live CPU/mem/FPS perf overlay** + layout inspector + process/shell/logcat.
Learn: the perf overlay (Droidsmith already has the layout inspector). Avoid:
Electron, AGPL.

### ADB AppControl — closed, Windows, .NET
Paywalls a **tiered debloat wizard**, **cross-store app search deep-links**,
**virtual hardware-button overlay**, status-bar icon hiding, ADB key protection.
Paywalled features signal undervalued OSS targets. Avoid: Windows-only, closed,
telemetry.

### scrcpy — Apache-2.0, C/SDL3 (v4.1)
Every new CLI option is a Droidsmith GUI feature: v3.0 `--new-display`,
`--angle`, `--capture-orientation`, `--screen-off-timeout`; v3.2 audio-source
expansion + `--display-ime-policy`; v3.1/3.3 gamepad + UHID-on-virtual-display;
v4.0 flex display, `--keep-active`, camera torch/zoom; v4.1 VP8/VP9.

## Security, Privacy, and Reliability

- **Tauri CVE-2026-42184 resolved**: Cargo.lock pins `tauri` 2.11.2 (fix ≥2.11.1).
  `tauri-plugin-shell` and `tauri-plugin-updater` are **not** present (adb is
  spawned via `std::process`, not plugin-shell), so CVE-2025-31477 is N/A.
  Prior open-question #1 is closed.
- **Advanced Protection Mode (Android 16)** blocks sideloading and is expected to
  disable/restrict Developer Options (USB/wireless debugging). An ADB tool that
  silently "sees no device" under AP mode will generate support noise — detect
  and explain it (host-doctor/diagnostics).
- **Low-target-SDK install block** (A14 min targetSDK 23, A15 → 24): old APKs
  fail unless `adb install --bypass-low-target-sdk-block` is passed — surface as
  an install checkbox.
- **platform-tools 37.0.1 (Jul 2026)**: `openscreen` mDNS backend removed
  (`ADB_MDNS_OPENSCREEN` is now a no-op); new `ADB_USB_LEGACY=1` Windows fallback
  and macOS `ADB_LIBUSB=1` — surface as host-doctor troubleshooting toggles.
- **Untested security-relevant code**: `src-tauri/src/process_tree.rs`
  (cross-platform child-containment/kill) has no `#[cfg(test)]`; a regression
  could let adb/fastboot/scrcpy children outlive an operation.
- Core Rust deps (serde, zip, sha2, base64, uuid) carry zero RustSec advisories
  as of 2026-07-20. Release-policy exceptions expire 2026-10-15 — schedule a
  re-audit.

## Architecture Assessment

### Strengths
Thin IPC glue over domain modules; JSON-Lines journals (HGFS-safe, diffable);
schema versioning with migration paths; generated TS bindings; a 30+ flow smoke
harness. No stubs; 72/73 commands wired.

### Refactor candidates
- `src/routes/Devices.tsx` (1,633 LOC) still hosts several inline panels
  (`AdbHealthPanel` 545–676, `RecoveryDialog` 687–840, `DeviceTable` 891–1023,
  `DeviceDetail` 1053–1221, `DeviceHealthCards` 1221–1346) — extract to
  `src/routes/devices/` to match the completed IMP-67 split.
- `Debloat.tsx` (1,642) and `Apps.tsx` (1,610) have self-contained extractable
  panels (`DebloatApplyReview`, `PackPicker`, `PackPreview`, `ActionOverlay`).
- **Orphaned IPC**: `explain_failure` (lib.rs:137, commands.rs:4515) is
  registered, bound, and unit-tested but has **no frontend caller** — the quirks
  failure-hint feature was never surfaced. Wire it into operation-failure UI or
  remove it.

### Test gaps
- `src-tauri/src/process_tree.rs` — no tests (security-relevant).
- `src/lib/deviceStore.ts` (device-lifecycle `useSyncExternalStore` machine) and
  `src/lib/logcatQueries.ts` (persistence with storage fallback) — no test files.

### UI/UX gaps (verified by file:line)
- `LayoutInspector.tsx` (136/141): filter-to-empty renders a blank panel — no
  "no matching nodes" state (NetworkInspector/ProcessManager both have one).
- `ProcessManager.tsx`: read-only — no force-stop/kill action a user expects.
- `NetworkInspector.tsx`: no export/copy of the socket table (Logcat/Layout have
  export).
- `InternetSharing.tsx` (122): returns `null` when gnirehtet isn't on PATH — the
  feature is silently invisible with no install hint (scrcpy/fastboot surface a
  locate-failure message).
- Table semantics split: only `PackageTable.tsx` uses the ARIA grid pattern; 9
  other tables use plain HTML — standardize or document the baseline.

## Rejected Ideas

| Idea | Source | Rejection reason |
|------|--------|-----------------|
| Keyboard-to-touch mapping via a scrcpy flag | Escrcpy/QtScrcpy | Vanilla scrcpy has no mapping API; already parked as blocked R-081. The in-GUI synthetic-touch path needs a physical device to verify — keep in Roadmap_Blocked. |
| Multi-device group/broadcast control (new) | scrcpy #4122/#400 | Overlaps blocked IMP-52 (multi-device workspaces); acceptance needs ≥2 physical devices. Track under IMP-52, don't duplicate. |
| tauri-plugin-updater as "blocked on code signing" | R-075 | The Ed25519 updater key is NOT code signing (satisfies the no-signing rule); but the manifest still needs a real GitHub Release + endpoint. Stays blocked on release infra, not signing — see note in Roadmap_Blocked. |
| Redistribute UAD-ng list directly | R-036 | Still needs upstream redistribution permission. A generic "import remote pack by URL + SHA pin" avoids the dependency and is actionable (see R-096). |
| On-device companion / perf agent | Aya | Contradicts zero-install desktop philosophy; Aya's FPS overlay needs a device-side hook. |
| Electron migration / WebUSB web build | Aya, ya-webadb | ~10 MB Tauri binary is a core differentiator; WebUSB is Chromium-only. |
| Pure-Rust ADB (drop shell-out) | adb_client | Reference adb binary handles OEM quirks; re-evaluate at adb_client v4. |

## Sources

### Competitors / ecosystem
- https://github.com/viarotel-org/escrcpy
- https://github.com/barry-ran/QtScrcpy
- https://github.com/liriliri/aya
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/wiki/FAQ
- https://github.com/samolego/Canta
- https://www.adbappcontrol.com/en/
- https://adbappcontrol.com/en/extended/
- https://github.com/kil0bit-kb/scrcpy-gui

### scrcpy releases / community demand
- https://github.com/Genymobile/scrcpy/releases/tag/v3.0
- https://github.com/Genymobile/scrcpy/releases/tag/v3.2
- https://github.com/Genymobile/scrcpy/releases/tag/v4.0
- https://github.com/Genymobile/scrcpy/releases/tag/v4.1
- https://github.com/Genymobile/scrcpy/issues/721
- https://github.com/Genymobile/scrcpy/issues/4866
- https://github.com/Genymobile/scrcpy/issues/4122
- https://github.com/Genymobile/scrcpy/issues/3137
- https://github.com/Genymobile/scrcpy/issues/5564

### Platform / standards
- https://developer.android.com/tools/releases/platform-tools
- https://developer.android.com/about/versions/16/behavior-changes-all
- https://www.androidauthority.com/android-advanced-protection-mode-developer-options-3679725/
- https://xdaforums.com/t/platform-tools-adb-install-bypass-low-target-sdk-block.4703676/
- https://v2.tauri.app/plugin/updater/

### Security
- https://github.com/tauri-apps/tauri/security/advisories
- https://rustsec.org/

## Open Questions

1. **Post-OTA re-apply reliability**: OTAs rewrite `/system` and can flip package
   states unpredictably per OEM. Is drift *detection* (fingerprint diff + baseline
   re-check) the safe scope, deferring *auto* re-apply behind explicit user
   confirmation? (Recommend: detect + prompt, never silent re-apply.)
2. **Auto-update hosting**: R-075 needs a real GitHub Release + a stable manifest
   endpoint and an Ed25519 keypair (not a code-signing cert). Who owns key
   storage/rotation and cuts the first tagged release?
3. **SECURITY.md contact**: still `security@droidsmith.invalid` — needs a real
   channel or GitHub private vulnerability reporting enabled before any release.
