# Droidsmith Roadmap

Single source of truth for what's planned and what's in flight. Completed items
are deleted from here and logged in [CHANGELOG.md](CHANGELOG.md). Blocked items
live in [Roadmap_Blocked.md](Roadmap_Blocked.md). Research context lives in
[RESEARCH_REPORT.md](RESEARCH_REPORT.md); do not duplicate that here - link
instead.

## Conventions

- `[ ]` - not started
- `[~]` - in flight
- Priority tags: **P0** (must ship in v0.1) . **P1** (v0.1 desirable / v0.2 must) . **P2** (later milestones) . **P3** (cosmetic / nice-to-have)
- **R-NNN** are roadmap items; **IMP-NN** are hardening / improvement items

## Remaining

### P3

- [ ] P3 — Persist gnirehtet reverse-tethering across navigation
  Why: the "Share Internet" toggle now stops its supervised session when the
  selected device changes or the Devices route unmounts, so tethering does not
  survive navigating away (e.g. to install something that needs the shared
  connection). Safe default, but persistence would be friendlier.
  Where: `src/routes/devices/InternetSharing.tsx` (drop the cleanup stop),
  `src-tauri/src/gnirehtet.rs` + `src-tauri/src/commands.rs` (add a
  list-sessions-by-serial command so a remount can re-attach to a running
  session instead of showing "start" and spawning a duplicate).

## Research-Driven Additions

Added 2026-07-20 from the RESEARCH.md pass (v0.9.1). Items are actionable and
verifiable in the current headless harness; device-verification-only ideas
(multi-device broadcast, in-GUI touch mapping, auto-update hosting) are tracked
in Roadmap_Blocked.md, not here — see the Rejected Ideas table in RESEARCH.md.

### P2

### P3

- [ ] P3 — R-089 scrcpy v3–v4 capability surface + validation-heavy flags
  Why: net-new scrcpy capabilities plus the validation-heavier flags deferred
  from R-088 (which shipped the stable toggles + `--max-fps`).
  Evidence: scrcpy v3.0 `--new-display`, v3.2 audio-source expansion, v4.0 flex
  display + camera torch/zoom, v4.1 VP8/VP9. Deferred from R-088: `--crop`,
  `--display-orientation`, `--screen-off-timeout`, `--audio-codec`.
  Touches: `src-tauri/src/scrcpy.rs` (capability probe + args + validation),
  `src/routes/Mirror.tsx` + `src/routes/mirrorPresets.ts` (virtual-display,
  camera, VP8/VP9 fallback, audio-source picker, crop/orientation/audio-codec
  inputs), degrade gracefully on scrcpy <4.0.
  Acceptance: capabilities gate on detected scrcpy version; crop/orientation/
  timeout/audio-codec validated and persisted; args asserted in unit tests;
  unsupported flags hidden on older scrcpy.
  Complexity: M

- [ ] P3 — R-093 Host-doctor USB/mDNS backend troubleshooting toggles
  Why: platform-tools 37.0.1 changed USB/mDNS backends; device-detection issues
  now hinge on env toggles users can't discover.
  Evidence: 37.0.1 removed `openscreen` mDNS backend; added `ADB_USB_LEGACY=1`
  (Windows) and macOS `ADB_LIBUSB=1`.
  Touches: `src/routes/HostDoctor.tsx`, `src-tauri/src/` adb-invocation env
  handling (opt-in `ADB_USB_LEGACY` / mDNS backend note + relaunch adb server).
  Acceptance: host-doctor surfaces a USB-backend toggle that sets the env for the
  adb server and reports the active mDNS backend; unit test on env assembly.
  Complexity: S

- [ ] P3 — R-095 Import remote debloat pack by URL with SHA-256 pin
  Why: static bundled YAML can't gain OEM/package coverage without an app
  release; a generic pinned-URL import gives UAD-ng-style freshness without the
  blocked R-036 redistribution dependency.
  Evidence: UAD-ng ships a remote-updatable community list; Droidsmith packs are
  static (`packs/*.yaml`).
  Touches: `src-tauri/src/packs/` (fetch + SHA-256 verify + schema-validate a
  remote pack), `src/routes/Debloat.tsx` (add-remote-pack UX).
  Acceptance: a user can add a pack from a URL that is SHA-256-pinned and
  schema-validated before use; malformed/unpinned sources are rejected; unit
  test on verify + validate.
  Complexity: M

- [ ] P3 — IMP-72 Extract Devices.tsx inline panels
  Why: Devices.tsx (1,633 LOC) still hosts several independent panels inline,
  unlike the completed IMP-67 device sub-panel split.
  Evidence: `AdbHealthPanel` (545–676), `RecoveryDialog` (687–840), `DeviceTable`
  (891–1023), `DeviceDetail` (1053–1221), `DeviceHealthCards` (1221–1346).
  Touches: new files under `src/routes/devices/`, `src/routes/Devices.tsx`.
  Acceptance: panels move to `src/routes/devices/` with no behavior change;
  typecheck/lint/tests/ui:smoke green.
  Complexity: M

- [ ] P3 — IMP-73 Standardize data-table ARIA semantics
  Why: only `PackageTable.tsx` uses the ARIA grid pattern; 9 other tables use
  plain HTML, an inconsistent a11y baseline.
  Evidence: `PackageTable.tsx:230,259` (`role="row"`/`aria-rowindex`) vs plain
  `<table>` in NetworkInspector/ProcessManager/Debloat/Devices/Fastboot/Profiles/
  Wireless/JournalPanel/RecoveryBaselinePanel.
  Touches: the listed table components (or a shared table primitive in
  `src/routes/common.tsx`).
  Acceptance: interactive/sortable tables share one documented pattern
  (grid roles + `aria-sort` where sortable); no visual regressions.
  Complexity: M
