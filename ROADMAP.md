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

- [ ] P1 - Reconcile docs and README claims with shipped behavior
  Why: `docs/DEVELOPMENT.md` still says every nav item is a stub, while README claims side-by-side device tabs and plugins that are not implemented.
  Evidence: `docs/DEVELOPMENT.md`, `README.md`, `src/App.tsx`, `Roadmap_Blocked.md`.
  Touches: `README.md`, `docs/DEVELOPMENT.md`, `RESEARCH_REPORT.md`, screenshots if UI claims change.
  Acceptance: Public docs describe the current live routes, blocked sidecars/plugins are named as planned, and no setup text points users at obsolete stub-route status.
  Complexity: S

- [ ] P1 - Add a local production-bundle smoke test
  Why: Tauri resource, sidecar, and frontend build drift can pass unit tests while installed builds miss data files or binaries.
  Evidence: `src-tauri/tauri.conf.json`, `Roadmap_Blocked.md` R-006/R-010, UAD-NG privacy/update notes, scrcpy official-source warning.
  Touches: package scripts, `scripts/`, `src-tauri/tauri.conf.json`, local release checklist.
  Acceptance: A local command builds the frontend, verifies bundle metadata/resources, checks third-party license entries for adb/fastboot/scrcpy/data packs, and fails on missing artifacts.
  Complexity: M

- [ ] P2 - Add persisted language selection and translation contribution path
  Why: English/Russian parity exists, but language is auto-detected only and there is no contributor workflow comparable to AppManager Weblate or Canta Crowdin.
  Evidence: `src/lib/i18n.ts`, `src/lib/i18n.test.ts`, AppManager translations, Canta translations.
  Touches: `src/lib/i18n.ts`, settings/shell UI, `src/locales/*.json`, README contributor docs.
  Acceptance: User can switch language in-app, choice persists, fallback is tested, and contributor docs explain locale keys and parity tests.
  Complexity: M

- [ ] P2 - Add route-level accessibility and visual regression checks
  Why: The app has many dense tables, overlays, dialogs, and status panels; current tests mostly cover nav/i18n structure, not keyboard flow or clipping.
  Evidence: `src/App.test.tsx`, `src/lib/i18n.test.ts`, Apps action overlay, command palette, Onboarding, Android Studio dense-tooling benchmarks.
  Touches: Playwright or Vitest browser setup, route components, screenshots.
  Acceptance: Automated checks cover sidebar navigation, command palette, action overlay, Debloat queue states, Apps table, and mobile/narrow widths with no keyboard trap or text overflow.
  Complexity: M

- [ ] P2 - Prepare plugin boundaries without shipping a marketplace
  Why: Plugin and marketplace work is deferred, but pack/quirk/profile schemas need stable extension seams before third-party OEM modules accumulate.
  Evidence: `Roadmap_Blocked.md` R-062/R-063, `src-tauri/src/packs/mod.rs`, `src-tauri/src/quirks/mod.rs`, README plugin claim.
  Touches: `src-tauri/src/packs/`, `src-tauri/src/quirks/`, `src-tauri/src/profile.rs`, schema docs.
  Acceptance: Pack, quirk, and profile schemas have versioned compatibility rules, lint errors name migration paths, and README stops implying a shipped plugin system.
  Complexity: L
