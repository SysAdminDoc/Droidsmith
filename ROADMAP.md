# Droidsmith Roadmap

Single source of truth for what's planned and what's in flight. Completed items
are deleted from here and logged in [CHANGELOG.md](CHANGELOG.md). Blocked items
live in [Roadmap_Blocked.md](Roadmap_Blocked.md). Research context lives in
[RESEARCH.md](RESEARCH.md); do not duplicate that here - link instead.

## Conventions

- `[ ]` - not started
- `[~]` - in flight
- Priority tags: **P0** (must ship in v0.1) . **P1** (v0.1 desirable / v0.2 must) . **P2** (later milestones) . **P3** (cosmetic / nice-to-have)
- **R-NNN** are roadmap items; **IMP-NN** are hardening / improvement items

## Research-Driven Additions

From the 2026-08-08 RESEARCH.md pass. The dominant theme is **claims the codebase
makes that its own gates do not check**, plus upstream drift in platform-tools
and Android, privacy-safe diagnostics, and the widest remaining product gap
(fleet work is CLI-only). IDs continue from R-150 / IMP-129.

### P1

### P2

- [ ] R-145 P2 — Record per-entry "verified removable on" provenance in packs
  Why: the most-discussed unmet ask in the debloat space is knowing which ROM a removal was actually proven safe on, and the pack schema records provenance only at pack level.
  Evidence: UAD-NG issue #1164 (13 comments); `packs/schema.json` `PackEntry` has `id`, `removal`, `description`, `depends_on`, `needed_by`, `labels` — no verification record; pack-level `targets` and `provenance` exist but describe the whole document.
  Touches: `packs/schema.json`, `src-tauri/src/packs/mod.rs`, `src-tauri/src/bin/pack_lint.rs`, `src/routes/debloat/PackPreview.tsx`, `packs/*.yaml`
  Acceptance: an entry may carry verification records (build fingerprint prefix, Android level, outcome, date, source); the Debloat preview shows whether the connected device matches any record and says "not verified on this build" when it does not; `pack_lint` rejects a record missing a source; absence of records is rendered as unknown, never as safe.
  Complexity: M

- [ ] R-146 P2 — Import a user-supplied UAD-NG list
  Why: content depth is the weakest axis — 138 bundled pack entries and 11 quirk rules against a continuously-maintained upstream list — and the local-file import pattern already used for R-095 closes it without redistributing GPL-3.0 data.
  Evidence: `packs/*.yaml` total 138 entries (including the example pack); `quirks/*.yaml` total 11 rules; UAD-NG is GPL-3.0 and its data file is `resources/assets/uad_lists.json` (already cited by pinned commit in `quirks/samsung-oneui.yaml`); `packs/schema.json` `RemovalLevel` already mirrors UAD-NG's tiers and `depends_on`/`needed_by` mirror its dependency graph; `import_pack` in `src-tauri/src/commands/packs.rs:275` is the audited host-path grant model to reuse. Depends on the licensing-posture open question in RESEARCH.md.
  Note: this does **not** supersede blocked **R-036** (bundling the UAD-NG list with attribution), which stays blocked on redistribution permission. This item ships zero upstream data and is the same dependency-free substitution that resolved R-095's local-file half.
  Touches: `src-tauri/src/commands/packs.rs`, `src-tauri/src/packs/mod.rs`, `src/routes/debloat/PackPicker.tsx`
  Acceptance: a user-selected `uad_lists.json` converts locally to a schema-valid pack with tier mapping and attribution recording the source file's SHA-256 and licence; nothing from UAD-NG is committed to this repository; the resulting pack is badged as imported and removable; conversion failures name the offending record rather than dropping it.
  Complexity: M

- [ ] R-147 P2 — Expose the remaining scrcpy flags
  Why: display selection in particular is the difference between mirroring a device and mirroring the *right surface* on a device that has more than one.
  Evidence: `src-tauri/src/scrcpy.rs` builds `--flex-display`, `--keep-active`, `--new-display`, `--start-app`, vp8/vp9 and `--ignore-video-encoder-constraints`, but contains no `--display-id`, `--list-displays`, `--mouse=`, `--camera-torch`, `--camera-zoom`, `--capture-orientation`, `--no-clipboard-autosync` or `--push-target`; scrcpy `doc/video.md` and `doc/control.md`.
  Touches: `src-tauri/src/scrcpy.rs`, `src/routes/Mirror.tsx`, `src/routes/mirrorPresets.ts`
  Acceptance: displays are enumerated via `--list-displays` and selectable; mouse mode, camera torch/zoom, capture orientation, clipboard autosync and push target are exposed; each is gated on the probed binary advertising it, following the existing encoder-probe pattern, and stored in the per-device preset.
  Complexity: M

- [ ] IMP-125 P2 — Render a real-scale package inventory in the rendered-route gate
  Why: the gate exercises a handful of packages while real devices report 500-900, and `PackageTable` is not row-virtualized — so whether large inventories are usable is currently unmeasured in either direction.
  Evidence: `scripts/check-rendered-routes.mjs` mocks `list_packages` with a small fixture keyed on `com.example.app`; `src/routes/apps/PackageTable.tsx:405-409` uses `IntersectionObserver` for lazy metadata only, with no row windowing.
  Touches: `scripts/check-rendered-routes.mjs`, `src/routes/apps/PackageTable.tsx`, `release-policy.json`
  Acceptance: the gate renders a 1,000-package inventory, exercises filter/search/sort, and asserts a declared interaction budget; virtualization is introduced only if the measurement fails the budget, and the budget lives in `release-policy.json` alongside the bundle budget.
  Complexity: M

- [ ] R-148 P2 — Publish the unreleased versions
  Why: twelve releases exist only in source; the newest downloadable artifact is v0.5.3 from 2026-07-17, so no user can obtain any work from the last three weeks.
  Evidence: `gh release list` returns only `v0.5.3` (2026-07-17) and `v0.1.0`; manifests are at 0.9.17; `README.md:106` already discloses the gap. Depends on the bundle-capable-host open question in RESEARCH.md.
  Note: this is the **Windows unsigned** artifact only, which project policy permits. It does not supersede the blocked "unsigned multi-platform distribution" entry (Linux bundles, still blocked on no Linux host), **R-006/R-010** (signing, notarization, `externalBin`) or **R-110** (minisign-signed provenance) — those blockers are unchanged.
  Touches: release process, `packaging/`, `CHANGELOG.md`
  Acceptance: an unsigned Windows MSI and NSIS installer are built, `release:check` including `release:smoke` passes against the real bundle, the artifacts are attached to a `v0.9.x` GitHub release with generated notes, `SHA256SUMS` is published alongside, and the packaging manifests carry the real installer hashes.
  Complexity: M

- [ ] R-149 P2 — Expose the headless CLI as an MCP server
  Why: agent-driven device workflows are where this space is moving, and Droidsmith can serve them over stdio with no HTTP client, no telemetry and no new network surface — the one differentiator its architecture makes cheap and its competitors' architectures make expensive.
  Evidence: `src-tauri/src/bin/droidsmith_cli.rs` already emits stable `--json` for every operation with documented exit codes; escrcpy 3.0.8 ships an MCP-protocol assistant; `callstack/agent-device` (3.9k stars) and Android Studio Otter 3's agent tooling target the same workflows.
  Touches: `src-tauri/src/bin/` (new binary), `src-tauri/src/fleet_report.rs`, `README.md`
  Acceptance: a `droidsmith-mcp` stdio server exposes read-only tools (list devices, list packages, plan profile, inspect baseline, read fleet report) plus explicitly-flagged mutating tools; every mutating tool refuses without the same confirmation the GUI requires; the server makes no network connection and holds no state the CLI does not already own.
  Complexity: L

- [ ] R-151 P2 — Add historical app-exit and ANR diagnostics to Process Manager
  Why: Process Manager shows the live `ps` snapshot and Android 17 memory-limit status, but it cannot explain why an app previously died — the diagnosis users currently obtain from an opaque `dumpsys` command.
  Evidence: `src/routes/devices/ProcessManager.tsx` has no exit-history query; `src-tauri/src/adb/device_info.rs` and `src-tauri/src/commands/devices.rs` expose only `am memory-limiter status`; Android's `ApplicationExitInfo` defines stable crash/ANR/low-memory/package-state reasons and `dumpsys` is the supported bounded system-service inspection path.
  Touches: `src-tauri/src/adb/`, `src-tauri/src/commands/devices.rs`, `src/routes/devices/ProcessManager.tsx`, `src/locales/*.json`, transcript fixtures, rendered-route smoke
  Acceptance: a selected package can request a bounded, read-only `dumpsys activity exit-info` history; parsed rows show timestamp, user, process, reason, status, RSS/PSS where present, and an explicit unknown state for OEM formats; traces and raw dumps are not captured automatically; package/user selection and target lifecycle guards match the existing process query.
  Complexity: M

### P3

- [ ] IMP-127 P3 — Grey out file operations the device will refuse
  Why: the file manager currently lets a user confirm a mutation that cannot succeed, and the permission bits needed to know better are already parsed.
  Evidence: `src-tauri/src/adb/parsers.rs` parses `ls -la` permission columns; `src/routes/devices/FileManager.tsx` enables actions unconditionally; ADB Explorer v1.0.26070 ships permission-based action gating.
  Touches: `src/routes/devices/FileManager.tsx`, `src-tauri/src/remote_files.rs`
  Acceptance: push, rename, delete and mkdir are disabled with a reason when the parsed permissions or the protected-path list forbid them; an unparseable permission string leaves the action enabled rather than guessing.
  Complexity: M

- [ ] R-150 P3 — Ship an Android TV / Fire TV debloat pack
  Why: TV boxes are an underserved surface with the same problem and no maintained desktop tooling, and the vendor pack framework already covers Fire OS.
  Evidence: `packs/amazon-fireos.yaml` exists with 12 entries; `seun-novodev/android-tv-debloat-toolkit` (549 stars) is the closest thing to a maintained list.
  Touches: `packs/`, `quirks/`, `src-tauri/tauri.conf.json`
  Acceptance: a pack targeting Android TV / Google TV builds ships with `targets` distinguishing them from handsets, every entry carries a description and provenance, and `pack_lint` passes; the pack is not offered on devices whose characteristics do not match.
  Complexity: M

- [ ] IMP-128 P3 — Split `Apps.tsx`
  Why: it is the largest file in the frontend at 2,023 lines despite six components already extracted, and the initial bundle sits at 84% of its declared budget.
  Evidence: `src/routes/Apps.tsx` 2,023 lines against `src/routes/apps/` already holding `PackageTable`, `FilterControls`, `InstallPanels`, `JournalPanel`, `PermissionsPanel`, `RecoveryBaselinePanel`; `dist/assets/index-*.js` 380 KB against `release-policy.json` `initialJavaScriptBudgetBytes` 450000; the `commands.rs` split behind `command_registry.rs` is the precedent.
  Touches: `src/routes/Apps.tsx`, `src/routes/apps/`
  Acceptance: `Apps.tsx` becomes orchestration only, with export review, OTA restore/re-apply and the backup panels extracted; behaviour is unchanged as proven by the existing `Apps.test.tsx` and the rendered-route gate; the initial bundle does not grow.
  Complexity: M
