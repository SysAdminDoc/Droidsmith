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

### P2

- [ ] P2 — R-081: Keyboard-to-touch mapping GUI for scrcpy
  Why: scrcpy's most-requested feature (issue #712, 55 comments). QtScrcpy's
  most popular feature. Escrcpy ships keyboard mapping scripts. Users want to
  map keyboard keys to touch locations for gaming and accessibility.
  Evidence: scrcpy #712 (55 comments); QtScrcpy README; Escrcpy keyboard docs
  Touches: `src-tauri/src/scrcpy.rs` (pass `--keyboard=uhid` or mapping file),
  `src/routes/Mirror.tsx` (mapping editor UI), `src-tauri/src/settings.rs`
  (persist per-device mappings), locale files
  Acceptance: Mirror route offers a visual mapping editor where users place
  virtual buttons on a device screenshot and bind them to keyboard keys. The
  mapping persists per-device and is passed to scrcpy on launch. At minimum,
  tap-at-coordinates bindings work; swipe/joystick are stretch goals.
  Complexity: L

- [ ] P2 — R-082: ADB system settings editor
  Why: UAD-ng #1226 (6 comments) requests GUI for changing system settings over
  ADB. Common use cases: disable animations (speed up device), change display
  DPI, toggle developer options. ADB AppControl paywalls DPI changes.
  Evidence: UAD-ng #1226; ADB AppControl features; adb-enhanced scenarios
  Touches: `src-tauri/src/adb/` (new `settings.rs` module for `adb shell
  settings get/put`), new route or Devices sub-panel, locale files
  Acceptance: Settings panel shows a curated list of safe system/secure/global
  settings (animation scales, display density, stay-on-while-charging, USB
  debugging timeout). Each shows current value and allows editing with a
  preview of the ADB command. Changes are journaled and undoable.
  Complexity: M

- [ ] P2 — IMP-62: ARIA grid pattern for package data table
  Why: The Apps package table contains interactive elements (checkboxes, action
  buttons) but uses basic `<table>` semantics. The W3C ARIA grid pattern
  (role="grid" with arrow-key cell navigation) is the correct pattern for
  tables with actionable cells, enabling keyboard-only users to navigate 500+
  package rows efficiently.
  Evidence: W3C ARIA APG grid pattern; WCAG 2.1.1 (keyboard); UAD-ng #605
  Touches: `src/routes/Apps.tsx` (table refactor to grid role),
  `src/routes/common.tsx` (TableHeaderCell/TableCell role attributes)
  Acceptance: Arrow keys navigate between cells in the package table. Enter/Space
  activates the focused cell's control. Page Up/Down scroll. `aria-rowcount` and
  `aria-sort` are present. NVDA can read column headers and cell content.
  Complexity: M

- [ ] P2 — IMP-63: Curated debloat presets (Privacy Max, Battery Saver, etc.)
  Why: Users face decision fatigue choosing individual packages. UAD-ng #583
  requests a verified "Safe" list. Named presets like "Privacy Max" (remove all
  telemetry/analytics), "Battery Saver" (disable background services), "Minimal
  Google" (keep only essential Google apps) reduce the burden.
  Evidence: UAD-ng #583 (20 comments); Canta tag-based filtering; ADB AppControl
  Debloat Wizard
  Touches: `packs/*.yaml` (add `tags` field per entry), `src-tauri/src/packs/`
  (preset definition, tag-based selection), `src/routes/Debloat.tsx` (preset
  picker UI), locale files
  Acceptance: Debloat route offers at least 3 named presets. Selecting a preset
  pre-checks the matching packages. User can review and modify before applying.
  Presets compose with the existing pack/tier system.
  Complexity: M

- [ ] P2 — IMP-67: Extract Devices.tsx and Apps.tsx into sub-route modules
  Why: `Devices.tsx` (2,975 LOC) and `Apps.tsx` (3,063 LOC) each contain 4-6
  independent sub-panels. Extracting to `src/routes/devices/*.tsx` and
  `src/routes/apps/*.tsx` improves navigability without changing behavior.
  Evidence: LOC analysis; sub-panel inventory in architecture assessment
  Touches: `src/routes/Devices.tsx` → `src/routes/devices/`,
  `src/routes/Apps.tsx` → `src/routes/apps/`
  Acceptance: Same rendered output and smoke test results. Each sub-panel is a
  separate file. No new dependencies.
  Complexity: M

### P3

- [ ] P3 — R-084: Gnirehtet reverse tethering integration
  Why: Gnirehtet (7,779 stars, same dev as scrcpy) shares PC internet with
  Android via USB. Escrcpy integrates this. Useful when device has no Wi-Fi
  or in restricted networks.
  Evidence: Gnirehtet GitHub; Escrcpy feature list
  Touches: `src-tauri/src/` (new `gnirehtet.rs` module for binary detection
  and session supervision, similar to `scrcpy.rs`), `src/routes/Devices.tsx`
  or new route, locale files
  Acceptance: If Gnirehtet is on PATH, Devices shows a "Share Internet" toggle.
  Clicking it starts/stops the reverse tethering session with status feedback.
  Complexity: M

- [ ] P3 — R-085: RTL language architecture
  Why: Arabic (ar), Hebrew (he), and Persian (fa) are RTL languages. The i18n
  system supports `dir` metadata per locale, but the CSS uses physical
  properties (`margin-left`, `padding-right`) throughout, which break under
  RTL. CSS logical properties (`margin-inline-start`, `padding-inline-end`)
  are the correct foundation.
  Evidence: i18next RTL docs; MDN CSS logical properties
  Touches: all `src/routes/*.tsx` (replace physical with logical CSS
  properties), `tailwind.config.ts` (logical property utilities),
  `src/lib/i18n.ts` (direction propagation)
  Acceptance: Adding an RTL locale (e.g., ar) correctly mirrors the layout.
  No physical margin/padding properties remain in route files.
  Complexity: L
