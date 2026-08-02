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

## Research-Driven Additions — 2026-07-31 (v0.9.12)

From the 2026-07-31 RESEARCH.md pass. The prior pass's frontier (lockfile/version
contract, npm advisories, CI restoration, target-bound lifecycles, v0.5.3 upgrade
preservation, public project metadata, ADB transcript corpus, command-boundary
split) has all shipped. These items
are dominated by **external drift** — advisories and upstream deadlines that
postdate the prior pass — plus the content and pre-mutation-guardrail gaps that
the competitive failure evidence exposes. Device-only and off-mission ideas stay
in the RESEARCH.md Rejected table or Roadmap_Blocked.md. IDs continue from
R-119 / IMP-95.

### P0

### P2

- [ ] P2 — R-128: Add filter-based profile predicates as schema v3
  Why: profiles are ordered lists of concrete package actions, so they are effectively device-specific — which limits fleet `run` and the planned `--retry-from` to fleets of identical devices.
  Evidence: AppManager 4.1.0 (2026-06-29) ships filter-based profiles resolving predicates at run time with boolean expressions over `&`, `|`, and parentheses; Droidsmith already has versioned schemas with explicit review-and-migrate paths (`contribution-schema-policy.json`, profile v1→v2).
  Touches: `src-tauri/src/profile.rs`, `profiles/schema.json`, `src-tauri/src/bin/droidsmith_cli.rs`, `src/routes/Profiles.tsx`, `contribution-schema-policy.json`, `src/locales/*.json`, fixtures and tests.
  Acceptance: profile schema `"3"` supports predicates over attributes Droidsmith already enumerates (system vs user, enabled state, installer package, Android user, archived state) combined by a bounded, non-backtracking boolean grammar; v2 files migrate explicitly with review, exactly as v1→v2 does, and v2 remains loadable; the import diff resolves predicates against the live device and shows every matched package and planned command before apply; expression evaluation is total — an unresolvable attribute excludes the package and is reported, never silently matched.
  Complexity: L

### P3

- [ ] P3 — R-130: Report accurate per-app storage from PackageManager
  Why: accurate app sizes are a sponsor-gated feature in the closest commercial competitor and are available from a single documented command.
  Evidence: `pm get-package-storage-stats [--user] <PKG>` is present in AOSP `PackageManagerShellCommand.java`; ADB AppControl 1.8.6 gates "accurate app sizes (A8+)" behind its paid tier.
  Touches: `src-tauri/src/adb/packages.rs`, `src-tauri/src/apk_metadata.rs`, `src/routes/apps/PackageTable.tsx`, `src/locales/*.json`, fixtures.
  Acceptance: app/data/cache sizes are read per package on demand within the existing lazy, bounded, identity-cached metadata path with no full-inventory scan; the command is runtime-probed and the column reports `unavailable` rather than an estimate where unsupported; sizes never block row rendering.
  Complexity: S

- [ ] P3 — R-131: Resolve marketing device names from build properties
  Why: device lists show codenames, which makes multi-device selection error-prone — and picking the wrong device is the highest-consequence mistake in this app.
  Evidence: DeviceFarmer STF #644 requests exactly this and is unmet; Droidsmith already parses `ro.product.*` in `src-tauri/src/adb/device_info.rs`.
  Touches: new offline name map resource, `src-tauri/src/adb/device_info.rs`, `src/routes/devices/DeviceTable.tsx`, `src/routes/common.tsx` (`DevicePicker`), `tauri.conf.json` resources.
  Acceptance: a bundled offline map resolves codename to marketing name with a cited source and revision date; the raw codename and serial remain visible alongside it; an unmapped device shows the codename unchanged with no guessed name; the map ships as a versioned resource checked by `npm run bundle:check`.
  Complexity: S

- [ ] P3 — IMP-110: Make onboarding resolve against live host and device state
  Why: the tour is a static five-step carousel that tells every user the same thing regardless of what is actually wrong, while the most-viewed content in the entire ADB corpus is failure diagnosis.
  Evidence: `src/routes/Onboarding.tsx` renders a fixed `STEPS` array and embeds `HostDoctor` below it, with no branching on host OS or live device state; Stack Overflow's ADB questions on unauthorized/offline/not-listed devices total well over 3M views.
  Touches: `src/routes/Onboarding.tsx`, `src/routes/HostDoctor.tsx`, `src-tauri/src/host_diagnostics.rs`, `src/locales/*.json`, `scripts/check-rendered-routes.mjs`.
  Acceptance: steps already satisfied by live evidence are marked resolved rather than presented as instructions; the driver step branches on host OS; the step matching the current failure is opened first; every claim is backed by a probe result and an unprobeable step is shown as informational rather than asserted; the smoke harness drives at least the all-clear, no-device, and unauthorized states.
  Complexity: M

- [ ] P3 — IMP-111: Add component-level tests for the highest-risk routes
  Why: all 22 frontend test files target pure-logic modules and `App.test.tsx` is the only `.tsx` test, so a single 4,379-line Playwright script is the sole regression gate for every route's UI.
  Evidence: `scripts/check-rendered-routes.mjs` is 4,379 lines; `src/routes/Apps.tsx` (1,933) and `src/routes/Mirror.tsx` (1,427) have no direct test; stale-response and device-switch races are the most repeated fix theme in git history.
  Touches: `src/routes/Apps.tsx`, `src/routes/Mirror.tsx`, `src/routes/Debloat.tsx` and sibling test files, `vitest.config.ts`, `package.json`.
  Acceptance: component tests cover the destructive-review, stale-completion, and device-switch paths for Apps, Debloat, and Mirror against mocked IPC; a test fails when an in-flight response resolves after a device change; the rendered-route smoke keeps its cross-route responsibility rather than absorbing these cases.
  Complexity: M

- [ ] P3 — IMP-112: Add a light theme toggle
  Why: a persisted light theme improves daylight readability, but the dark-theme-specific translucent overlays and fixed deep backgrounds must be converted rather than merely reversing the color ramp.
  Evidence: the computed rendered-contrast gate shipped in IMP-105 now resolves layered foreground/background colors for every route and overlay state; 42 current source files still contain `white/<opacity>`, `bg-white`, or `text-white` treatments tuned for dark surfaces.
  Touches: `tailwind.config.ts`, `src/index.css`, shared shell and route components, settings persistence, `scripts/check-rendered-routes.mjs`, `src/locales/*.json`.
  Acceptance: the toggle persists and switches every route without reload; theme-aware surface/text/border tokens replace dark-only overlays and fixed deep backgrounds; `npm run ui:smoke` runs the IMP-105 computed-contrast audit across all routes and overlay states in both themes with zero WCAG AA failures; forced-colors and reduced-motion behavior remains unchanged.
  Complexity: L

- [ ] P3 — R-132: Surface Android 17 app memory limits
  Why: Android 17 added a shell-reachable memory-limiter surface that no desktop ADB tool exposes, and it explains otherwise-inscrutable app kills.
  Evidence: Android 17 (API 37, released 2026-06-16) ships `am memory-limiter status | ignore <uid>|none|all | manual <pid> <limit>|max|none`, with kill attribution readable as `ApplicationExitInfo.getDescription() == "MemoryLimiter:AnonSwap"`; applies only to a device subset.
  Touches: `src-tauri/src/adb/device_info.rs`, `src-tauri/src/commands/devices.rs`, `src/routes/devices/ProcessManager.tsx`, `src/locales/*.json`, fixtures.
  Acceptance: `status` is read-only and shown only when the command is runtime-probed as present on an SDK 37+ device; the mutating `ignore`/`manual` forms either stay out of scope or enter the journaled reviewed-action path with a proven inverse, never a bare passthrough; absence reports `unsupported` rather than an error.
  Complexity: S
