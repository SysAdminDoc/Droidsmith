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

## Research-Driven Additions

### P0

### P1

### P2

### P3

## Research-Driven Additions

### P1

### P2

### P3

- [ ] P3 — **IMP-60** Add package-name Logcat filtering via PID→package resolution
  Why: IMP-59 shipped structured Logcat query presets over tag/message/PID/level/age with negation and a linear-time-safe regex subset, but package/process-*name* filtering was deferred: the stream now uses `-v threadtime` (timestamp + PID) which still carries no package or process name, and Android Studio resolves those from a live PID→package map the app does not yet build.
  Evidence: `src-tauri/src/commands.rs::stream_logcat` (threadtime, PID only); `src/routes/logcatQueries.ts` (pidFilter); Android Studio Logcat `package:`/`process:` semantics.
  Touches: a periodic `ps`/`pm` PID→package snapshot in the backend, `LogcatQuery` package/process fields, `src/routes/logcatQueries.ts` matching, `src/routes/Logcat.tsx`, locales, tests.
  Acceptance: A query can filter (and negate) by package or process name; the PID→package map refreshes on a bounded cadence without blocking the stream; lines whose PID is unmapped are surfaced rather than silently dropped; presets round-trip the new fields.
  Complexity: M
