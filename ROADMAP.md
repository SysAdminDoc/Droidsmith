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

- [ ] P2 — R-087 Post-OTA debloat-drift detection
  Why: the one debloat gap no competitor (UAD-ng, Canta, ADB AppControl) solves —
  packages silently reappear/re-enable after an OTA. Droidsmith already has
  recovery baselines; it lacks change detection to prompt a re-check.
  Evidence: UAD-ng FAQ/wiki (no auto re-apply); RESEARCH.md community signal #7.
  Touches: `src-tauri/src/commands.rs` (capture `ro.build.fingerprint` /
  `ro.build.version.incremental` into the recovery baseline), `src/routes/
  apps/RecoveryBaselinePanel.tsx` + `src/routes/Debloat.tsx` (flag when the
  current fingerprint differs from the baseline and offer a drift re-check).
  Acceptance: a baseline stores the build fingerprint; on reconnect a changed
  fingerprint surfaces a "device updated — review debloat drift" prompt that
  runs the existing drift diff; re-apply stays behind explicit confirmation
  (never silent). Unit test on the fingerprint-diff/drift logic + smoke flow.
  Complexity: L

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

- [ ] P3 — IMP-70 Tests for deviceStore.ts and lib/logcatQueries.ts
  Why: the device-lifecycle `useSyncExternalStore` machine and the logcat
  persistence layer (with storage fallback) are untested core logic.
  Evidence: no `src/lib/deviceStore.test.ts`; `src/lib/logcatQueries.ts` untested
  (the tested file is the sibling `src/routes/logcatQueries.ts`).
  Touches: `src/lib/deviceStore.test.ts`, `src/lib/logcatQueries.test.ts` (new).
  Acceptance: subscribe/notify/reconnect transitions and load/save-with-fallback
  are covered; `npm test` green.
  Complexity: S

- [ ] P3 — IMP-71 LayoutInspector filter-to-empty blank panel
  Why: capturing nodes then filtering to zero matches renders a blank panel;
  peer inspectors show a "no matching results" state.
  Evidence: `LayoutInspector.tsx:136` (empty only when `nodes.length === 0`) and
  `:141` (list only when `filtered.length > 0`) — no branch between.
  Touches: `src/routes/devices/LayoutInspector.tsx`.
  Acceptance: nodes present + zero filter matches shows a "no matching nodes"
  EmptyState; covered by a smoke assertion.
  Complexity: S

- [ ] P3 — R-091 NetworkInspector export/copy
  Why: Logcat and LayoutInspector offer export; the socket table does not.
  Evidence: `src/routes/devices/NetworkInspector.tsx` has no export/copy action.
  Touches: `src/routes/devices/NetworkInspector.tsx` (copy-to-clipboard /
  save-as-text of the current socket rows).
  Acceptance: the current (filtered) socket table can be copied or saved; smoke
  assertion on the action.
  Complexity: S

- [ ] P3 — R-094 gnirehtet discovery hint when not on PATH
  Why: `InternetSharing` returns `null` when gnirehtet is missing, so the feature
  is silently invisible with no way to learn it exists.
  Evidence: `src/routes/devices/InternetSharing.tsx:122` early-returns `null`.
  Touches: `src/routes/devices/InternetSharing.tsx` (render a locate-failure hint
  with install guidance, mirroring scrcpy/fastboot).
  Acceptance: with gnirehtet absent, the panel shows an "install gnirehtet to
  enable Share Internet" hint instead of nothing; smoke assertion.
  Complexity: S

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
