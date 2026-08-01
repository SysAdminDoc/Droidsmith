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

- [ ] P2 — R-119: Add resumable fleet execution from a prior report
  Why: Commercial fleets and OSS multi-device tools emphasize batch continuity; Droidsmith emits stable fleet JSON but cannot safely rerun only failed/skipped targets after interruption.
  Evidence: `src-tauri/src/bin/droidsmith_cli.rs`; DeviceFarmer STF; Escrcpy multi-device workflows; commercial fleet/session reporting.
  Touches: `src-tauri/src/bin/droidsmith_cli.rs`, `src-tauri/src/profile.rs`, `src-tauri/src/journal/`, CLI fixtures and smoke tests.
  Acceptance: `run --retry-from <report.json>` validates report schema, profile hash, action set, device identity, Android user, and current transport before selecting only failed/skipped devices; dry-run is required before apply when inputs drift; completed actions are never replayed implicitly; JSON records lineage to the source report and uses stable exit codes for mixed outcomes.
  Complexity: L

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

- [ ] P2 — R-123: Add a reversible action tier below disable and uninstall
  Why: the whole documented failure mode is users reaching for an irreversible action because no safer rung is offered; Android exposes several fully reversible package states that no desktop GUI surfaces.
  Evidence: `pm suspend`/`unsuspend`, `pm suspend-quarantine`, `pm unstop`, `pm hide-notifications`, `pm set-distracting-restriction` are all present in AOSP `PackageManagerShellCommand.java`; Canta #148 ("allow disabling apps", 14 reactions) is the top request of the leading mobile debloater; grep confirms none of `appops`, `suspend`, or `standby` appears anywhere in `src-tauri/src`.
  Touches: `src-tauri/src/adb/actions.rs` (`ActionKind`), `src-tauri/src/adb/packages.rs`, `src-tauri/src/journal/`, `src/routes/Apps.tsx`, `src/routes/apps/PackageTable.tsx`, `src/locales/*.json`, `packs/schema.json`, tests.
  Acceptance: suspend/unsuspend join the journaled action set with a proven inverse and post-state verification; the review screen ranks available actions by reversibility and defaults to the least destructive one that achieves the user's intent; every new subcommand is runtime-probed per device (parse `pm help`) and hidden rather than broken when absent; pack schema v1 remains valid and unchanged.
  Complexity: M

- [ ] P2 — R-124: Grow the vendor quirk corpus from the documented failure record
  Why: the quirks engine — schema, loader, `explain_failure`, and UI hint surface — is fully built and tested but ships exactly one rule, so a headline differentiator is inert.
  Evidence: `quirks/hyperos.yaml` is the only rule file (1 rule); UAD-NG's tracker documents specific, reproducible hazards with package names and ROMs: `com.android.overlay.circletosearch` soft-bricks HyperOS 2.0 (#1150), `com.android.phone` removal breaks SIM detection (#1168), Parental Controls removal broke Google One VPN on Pixel (#1096), `com.samsung.android.timezone.data_R` and `com.android.uwb.resources` bootloops (#1358/#1353/#1302), Samsung SM-A202F and A40 bootloops (#1311/#1295).
  Touches: `quirks/*.yaml`, `src-tauri/src/quirks/`, `src-tauri/src/commands/packs.rs`, `src/routes/debloat/QuirkHint.tsx`, `contribution-schema-policy.json`, tests.
  Acceptance: quirk rules exist for each vendor that ships a pack, each citing a public source URL and the ROM/build it was observed on; rules match on package plus manufacturer/ROM rather than error text alone where the hazard is pre-emptive; the debloat review surfaces a hazard before apply, not only after a failure; every rule validates against schema `"1"` with no schema change; each rule states its evidence basis so an unobserved combination reports `unknown` rather than implying verification. Note: this adds *rules describing publicly reported behaviour with attribution*, which is distinct from redistributing UAD-NG's curated list — that remains blocked as R-036.
  Complexity: M

- [ ] P2 — R-125: Consume the structured ADB device-tracking channel
  Why: the app regex-parses `adb devices -l`, which cannot express connection states or link speed that AOSP already publishes as structured data — and this retires the premise of the blocked R-101 note.
  Evidence: AOSP `services.cpp` exposes `host:track-devices-proto-binary` / `-proto-text`; `proto/adb_host.proto` defines `Device{serial, state, bus_address, product, model, device, connection_type, negotiated_speed, max_speed, transport_id}` and a `ConnectionState` including `DETACHED`, `RESCUE`, `NOPERMISSION`; Roadmap_Blocked.md R-101 correctly notes `server-status` has no USB-speed field — the speed lives on the per-device message instead. Needs live validation: the service names are verified from AOSP source, but the host `adb` CLI surface for reaching them at the supported versions is not — probe before building.
  Touches: `src-tauri/src/adb/transport.rs`, `src-tauri/src/adb/parsers.rs`, `src-tauri/src/adb/device.rs`, `src-tauri/src/commands/devices.rs`, `src/lib/deviceStore.ts`, `src/routes/Devices.tsx`, fixtures, `Roadmap_Blocked.md`.
  Acceptance: the text-proto path is used when the host `adb` supports it and falls back to the existing text parser otherwise, decided by runtime probe rather than a version assumption and with no new protobuf dependency; `DETACHED`/`NOPERMISSION`/`RESCUE` become distinct, explained states instead of collapsing into "unauthorized" or "offline"; `negotiated_speed`/`max_speed` are surfaced only when the device message actually carries them; fixtures cover both paths and a malformed proto response degrades to the text parser rather than failing. On success, close out the R-101 USB-link-speed remainder in Roadmap_Blocked.md.
  Complexity: L

- [ ] P2 — R-126: Add a guided pre-OTA restore and post-OTA re-apply round trip
  Why: debloated devices break on OTA updates, and the accepted community workflow — restore everything, update, re-debloat — is entirely manual today.
  Evidence: XDA guidance ("Do not install OTA update on a debloated phone, as you will face boot loops") and recovery threads; Droidsmith already has read-only OTA drift review, portable recovery baselines, and profiles, so only the orchestration is missing.
  Touches: `src-tauri/src/recovery_baseline.rs`, `src-tauri/src/upgrade.rs`, `src-tauri/src/profile.rs`, `src/routes/Apps.tsx`, `src/routes/apps/RecoveryBaselinePanel.tsx`, `src-tauri/src/bin/droidsmith_cli.rs`, `src/locales/*.json`, tests.
  Acceptance: a reviewed pre-OTA plan restores every recoverable package to its baseline state and reports exactly which packages cannot be restored before the user updates; after an OTA the drift review pairs with a re-apply plan derived from the same baseline, requires a dry-run diff, and never replays actions against packages whose post-OTA state already matches; the CLI exposes the same plan/apply pair with stable exit codes; irreversible packages are named explicitly at both ends.
  Complexity: L

- [ ] P2 — R-128: Add filter-based profile predicates as schema v3
  Why: profiles are ordered lists of concrete package actions, so they are effectively device-specific — which limits fleet `run` and the planned `--retry-from` to fleets of identical devices.
  Evidence: AppManager 4.1.0 (2026-06-29) ships filter-based profiles resolving predicates at run time with boolean expressions over `&`, `|`, and parentheses; Droidsmith already has versioned schemas with explicit review-and-migrate paths (`contribution-schema-policy.json`, profile v1→v2).
  Touches: `src-tauri/src/profile.rs`, `profiles/schema.json`, `src-tauri/src/bin/droidsmith_cli.rs`, `src/routes/Profiles.tsx`, `contribution-schema-policy.json`, `src/locales/*.json`, fixtures and tests.
  Acceptance: profile schema `"3"` supports predicates over attributes Droidsmith already enumerates (system vs user, enabled state, installer package, Android user, archived state) combined by a bounded, non-backtracking boolean grammar; v2 files migrate explicitly with review, exactly as v1→v2 does, and v2 remains loadable; the import diff resolves predicates against the live device and shows every matched package and planned command before apply; expression evaluation is total — an unresolvable attribute excludes the package and is reported, never silently matched.
  Complexity: L

- [ ] P2 — R-129: Render a local fleet report from existing run output
  Why: the CLI emits stable per-device JSON that nothing renders, and reporting is precisely what commercial fleet tools paywall.
  Evidence: `droidsmith-cli run --all-devices --json` emits a `devices[]` array with `outcome: ran | error | skipped`; AirDroid Business gates Alerts and Reports behind its Standard tier; DeviceFarmer STF's value is operational status across a pool.
  Touches: `src-tauri/src/bin/droidsmith_cli.rs`, `src/routes/Profiles.tsx`, new renderer module under `src/routes/`, `src/locales/*.json`, fixtures.
  Acceptance: a saved fleet report JSON can be opened read-only and rendered as a per-device outcome summary with failure reasons, skip causes, and per-action detail; rendering performs no device access and no network access; hashed device identity is shown rather than raw serials, matching the redaction rules already used by recovery baselines; the renderer rejects unknown schema versions with migration guidance; it composes with R-119's `--retry-from` rather than duplicating it.
  Complexity: M

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
