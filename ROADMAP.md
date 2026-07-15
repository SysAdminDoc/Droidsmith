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

- [ ] P3 — **IMP-36** Russian i18n missing plural form `_few`
  Why: Russian has 3 plural forms (one, few, many). Keys like `deviceCount` only define `_one` and `_other`, producing grammatically incorrect output for counts 2-4.
  Where: `src/locales/ru.json`

## Research-Driven Additions

### P0

### P1

### P2

### P3

- [ ] P3 — Replace misleading documentation screenshots with deterministic native-state captures
  Why: Current “Apps” and “Mirror” screenshots show the browser-only desktop-required state rather than the workflows their captions claim.
  Evidence: `docs/screenshots/`, `README.md`; `scripts/check-rendered-routes.mjs`
  Touches: `scripts/check-rendered-routes.mjs`, `docs/screenshots/`, `README.md`
  Acceptance: A deterministic mocked-native capture task renders every major route at desktop and narrow widths, README captions match visible states, and a smoke assertion rejects desktop-required placeholders in published workflow screenshots.
  Complexity: S

- [ ] P3 — Desktop-polish plugin cluster: window-state, tray, and background notifications
  Why: The app forgets window size/position, has no tray presence while supervising scrcpy/logcat, and gives no OS-level feedback on device connect/disconnect or long-job completion.
  Evidence: `src-tauri/src/lib.rs:34` (dialog plugin only); Tauri v2 window-state / tray / notification plugin docs
  Touches: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`, `src/App.tsx`
  Acceptance: Window size/position persist across launches; a tray icon keeps supervision alive when the window is closed/minimized; connect/disconnect and long-operation completion raise OS notifications consistent with the toast model; each plugin is capability-scoped.
  Complexity: M

- [ ] P3 — Add a light theme toggle
  Why: Surfaces are dark-only Tailwind; a light option is expected desktop polish and an accessibility preference for bright environments.
  Evidence: no theme state in `src/App.tsx` or `src/lib/`; dark-first `tailwind.config.ts`
  Touches: `tailwind.config.ts`, `src/App.tsx`, route components, a persisted `droidsmith.theme` key, locales
  Acceptance: A persisted theme toggle switches dark/light (default dark) across every route without contrast regressions; the choice survives restart; ui:smoke covers both themes.
  Complexity: M

- [ ] P3 — Add an incremental-install option for large APKs
  Why: `adb install --incremental` starts large installs before all bytes transfer, a measurable speed win the current single-shot install cannot offer.
  Evidence: AOSP incremental-install doc; `src-tauri/src/adb/actions.rs::install_apk`
  Touches: `src-tauri/src/adb/actions.rs`, `src-tauri/src/commands.rs`, `src/lib/tauri.ts`, `src/routes/Apps.tsx`
  Acceptance: An opt-in incremental toggle is used when the device/tooling reports support and falls back cleanly to a normal install otherwise, with the chosen mode recorded in the operation record.
  Complexity: S
  Depends on: app-bundle install (P1)

- [ ] P3 — Read-only layout / view-hierarchy inspector
  Why: A one-click `uiautomator dump` + saved snapshot extends Droidsmith's existing inspection panels with a developer-grade, no-root workflow competitors expose.
  Evidence: AYA layout inspect; `src/routes/Devices.tsx` inspection surfaces
  Touches: `src-tauri/src/commands.rs`, `src-tauri/src/adb/parsers.rs`, `src/lib/tauri.ts`, `src/routes/Devices.tsx`
  Acceptance: Users capture the current UI hierarchy to a saved artifact, browse nodes read-only, and export it; malformed dumps surface visible parse errors rather than being dropped.
  Complexity: M

## Audit-Deferred Items

- [ ] P3 — Differentiate success vs failure tone for inline device-control messages
  Why: Screenshot, density, and pull results render as identical faint `text-xs` spans, so a "Saved" and a "Failed" look the same; these confirm real device mutations and should read as success/error states.
  Where: `src/routes/Devices.tsx`

- [ ] P3 — Localize the `Unknown` fallback in battery/storage/bytes formatters
  Why: `formatBattery`/`formatStorage`/`formatBytes` return a hard-coded English "Unknown" for null values, bypassing i18n.
  Where: `src/routes/Devices.tsx`

- [ ] P3 — Query device storage with an explicit block size
  Why: `device_info` storage parsing reads `df` output whose units depend on the busybox/toybox variant; passing `df -k` (as `list_remote_files` now does) makes the KB assumption deterministic.
  Where: `src-tauri/src/adb/device_info.rs`, `src-tauri/src/commands.rs`

- [ ] P3 — Evict terminated scrcpy sessions from the supervisor map
  Why: `reap_locked` only runs on `launch`, so exited/stopped sessions linger in the session `HashMap` until the next launch (self-healing but unbounded if the user never launches again).
  Where: `src-tauri/src/scrcpy.rs`

- [ ] P3 — Give non-selectable device states a clear label and guidance
  Why: Devices reported with a non-unit `{ other }` state render a capitalized `Other (…)` badge with no explanatory prompt, unlike the `unauthorized`/`no_permissions` paths.
  Where: `src/routes/Devices.tsx`

## Research-Driven Additions

### P0

### P1

### P2

- [ ] P2 — **IMP-49** Publish generated contribution schemas with migration checks
  Why: Packs, profiles, and quirks are versioned, but unknown YAML fields can be ignored and `CONTRIBUTING.md` promises a missing `packs/schema.json`.
  Evidence: pack/profile/quirk Rust DTOs and loaders, `CONTRIBUTING.md:100`; JSON Schema 2020-12.
  Touches: Rust DTO derives/annotations, schema generator/check script, generated pack/profile/quirk schemas, fixtures, `CONTRIBUTING.md`.
  Acceptance: DTOs reject unknown fields; deterministic JSON Schemas validate every shipped YAML and provide editor completion; local checks fail a breaking schema diff without a version bump, documented migration, and backward-compatibility fixture.
  Complexity: M

- [ ] P2 — **IMP-50** Migrate preferences to a versioned typed settings store
  Why: Language and per-device mirror presets are unversioned raw `localStorage` keys with no backup, corruption recovery, export, or migration contract.
  Evidence: `src/lib/i18n.ts`, `src/routes/Mirror.tsx:123-172`, `src/routes/mirrorPresets.ts`; Tauri Store guidance.
  Touches: settings DTO/store service, Tauri capability/permission config, i18n and Mirror consumers, migration/recovery tests.
  Acceptance: A versioned typed store performs an idempotent one-time import of current keys, backs up before migration, preserves valid settings across upgrades, quarantines corrupt data without blocking launch, and offers scoped export/reset without exposing arbitrary store paths.
  Complexity: M

- [ ] P2 — **IMP-51** Expand deterministic rendered-state and race regression coverage
  Why: Current route smoke misses Wireless, Mirror, Logcat, Fastboot, onboarding, non-English layouts, error/loading/empty states, target switches, route unmounts, accessibility zoom, and stale async responses.
  Evidence: `scripts/check-rendered-routes.mjs`, `src/**/*.test.ts*`, reproduced desktop/mobile fixtures; Android Studio detached-state pattern.
  Touches: `scripts/check-rendered-routes.mjs`, browser fixtures, route/component tests, `test-results/rendered-routes/` expectations.
  Acceptance: Every route is asserted at desktop and 390px, selected non-English locale, 200% zoom, empty/loading/error/multi-device states, disconnect/reconnect and mid-request target switch; stale completions cannot mutate the new workspace and destructive controls remain disabled when detached.
  Complexity: L

- [ ] P2 — **IMP-52** Implement independent side-by-side device workspaces
  Why: The README promises side-by-side device tabs, while route-local selection currently unmounts and loses state; comparable ADB/scrcpy tools retain independent sessions for multiple devices.
  Evidence: `README.md:57`, `src/App.tsx`, `src/routes/Mirror.tsx:114-128`; Escrcpy and DeviceFarmer STF.
  Touches: `src/App.tsx`, shared workspace/device store, all route selection state, scrcpy/operation supervisors, persistence and regression tests.
  Acceptance: Users open at least two device tabs with independent route/filter/Android-user/session state; the active target is always visible; disconnected tabs become timestamped read-only snapshots and reconnect safely; no broadcast mutation exists.
  Complexity: XL

### P3

- [ ] P3 — **IMP-53** Add a tested offline Windows installer variant
  Why: Repair/refurbishing environments can lack reliable internet, while the default WebView2 bootstrap path may need a download on a clean machine.
  Evidence: `src-tauri/tauri.conf.json` and release scripts; Tauri Windows installer/WebView2 distribution guidance.
  Touches: `src-tauri/tauri.conf.json`, Windows bundle configuration, release smoke/checksum scripts, installation documentation.
  Acceptance: Releases retain the normal installer and additionally produce a clearly labeled offline WebView2 variant with checksum; a clean network-disabled Windows VM installs, launches, locates or explains missing Platform Tools, and uninstalls successfully.
  Complexity: M

## Research-Driven Additions

### P1

### P2

- [ ] P2 — **IMP-59** Persist named structured Logcat queries
  Why: Logcat currently offers only ephemeral tag/level/text fields, while comparable tools make recurring crash and package investigations reusable.
  Evidence: `src/routes/Logcat.tsx`; Android Studio Logcat query history/favorites; ADB AppControl console favorites.
  Touches: Logcat parser/query DTOs, typed settings from **IMP-50**, `src/lib/tauri.ts`, `src/routes/Logcat.tsx`, query migration/parser/rendered tests and locales.
  Acceptance: Users can save, rename, duplicate, reorder, export/import, and delete versioned presets for the supported subset of package/process/tag/message/level/age plus negation and linear-time regex; unsupported syntax is rejected with field-level guidance; presets may be global or device-scoped by hashed identity; quick history is bounded; raw log lines are never persisted; built-in `crash` and `stacktrace` presets match documented Android Studio semantics in fixtures.
  Complexity: M
  Depends on: **IMP-50**
