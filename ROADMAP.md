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

Added 2026-07-20 from the RESEARCH.md pass (v0.9.1). Items are actionable and
verifiable in the current headless harness; device-verification-only ideas
(multi-device broadcast, in-GUI touch mapping, auto-update hosting) are tracked
in Roadmap_Blocked.md, not here — see the Rejected Ideas table in RESEARCH.md.

### P2

### P3

## Research-Driven Additions — 2026-07-21 (v0.9.4)

From the 2026-07-21 RESEARCH.md pass. The prior pass's frontier (wireless
reconnect, drift detection, scrcpy flag surface, bypass-low-target-sdk,
archive/unarchive, explain_failure wiring, ProcessManager force-stop, local pack
import) has all shipped through v0.9.4. These are the verified next-tier gaps;
device-only and already-shipped ideas are in the RESEARCH.md Rejected table.
IDs continue from R-096 / IMP-73.

### P2

- [ ] P2 — R-098 — Export current device debloat state as a shareable pack/selection
  Why: Symmetric to the v0.9.4 local pack import; UAD-ng `selection_export`
  parity. `save_profile`/`profile.rs` only persist GUI-authored profiles — there
  is no capture-from-device export, so users can't carry "what I did on this
  phone" to a fresh device after OTA/factory reset.
  Evidence: UAD-ng wiki/Usage (selection_export.txt); verified gap —
  `commands.rs::save_profile` takes a renderer-authored `Profile`, no
  `capture`/`from_device` in `src-tauri/src/profile.rs`.
  Touches: `src-tauri/src/commands.rs` (new `export_device_pack` command),
  `src-tauri/src/packs/mod.rs` (serialize a `Pack` from observed disabled/
  uninstalled/archived state), `src/routes/Debloat.tsx`, bindings, locales.
  Acceptance: on a selected device, one action writes a schema-valid pack YAML
  (round-trips through the existing `import_pack`/`packs::load`) capturing the
  current disabled/uninstalled/archived set; covered by a Rust unit test on the
  serialize path.
  Complexity: M

- [ ] P2 — IMP-74 — Unit tests for `process_tree.rs`
  Why: The only security-relevant module with no `#[cfg(test)]`; a regression in
  cross-platform child-containment/kill could let adb/fastboot/scrcpy children
  outlive an operation.
  Evidence: `src-tauri/src/process_tree.rs` (no test module; host_path/install/
  scrcpy/gnirehtet all have one).
  Touches: `src-tauri/src/process_tree.rs`.
  Acceptance: `cargo test` covers the PID-hierarchy parse and the kill/containment
  decision paths (including the empty/malformed-input branches) with no new
  clippy warnings.
  Complexity: S

- [ ] P2 — R-099 — Dependency + security re-audit (release-policy exceptions expire 2026-10-15)
  Why: Scheduled re-audit is due; `time`, `reqwest`, `hyper`, `tokio` are in the
  tree transitively and advisories accrue over time.
  Evidence: rustsec.org/advisories (RUSTSEC-2026-0009 `time` DoS); prior RESEARCH
  noted the 2026-10-15 exception expiry; `rustls`/`webpki` are absent so
  rustls-webpki advisories are N/A (verified against `src-tauri/Cargo.lock`).
  Touches: `src-tauri/Cargo.lock`, `release-policy.json`, `deny.toml`.
  Acceptance: `cargo audit` (and the repo's `security:audit:rust`) run clean or
  with justified, dated exceptions; expiry dates refreshed; no advisory left
  unreviewed.
  Complexity: S

### P3

- [ ] P3 — R-100 — scrcpy `--display-ime-policy` and `--no-vd-destroy-content` flags
  Why: The two remaining headless-testable scrcpy flags that complete the
  already-shipped virtual-display surface (IME placement on the virtual display;
  preserve virtual-display content when the window closes).
  Evidence: scrcpy v3.2 (`--display-ime-policy`) / v3.1 (`--no-vd-destroy-content`)
  release notes; `src-tauri/src/scrcpy.rs` `LaunchScrcpyRequest` (27 fields) does
  not include them.
  Touches: `src-tauri/src/scrcpy.rs` (request fields + version-gated arg emission
  + capability flags), `src/routes/Mirror.tsx`, bindings, locales.
  Acceptance: version-gated arg emission unit-tested exactly like
  `emits_new_display_and_audio_source_when_supported`; Mirror exposes the toggles
  only when scrcpy supports them.
  Complexity: S

- [ ] P3 — R-101 — Surface `adb server-status` mDNS state + negotiated USB link speed
  Why: platform-tools 36.0.0+ `adb server-status` reports mDNS status and USB
  SuperSpeed/SuperSpeed+ negotiated link speed — richer host-doctor/diagnostics
  than "device connected", and useful for diagnosing flaky-USB reports.
  Evidence: developer.android.com/tools/releases/platform-tools (v36.0.0
  server-status, v37.0.1 mDNS backend change).
  Touches: `src-tauri/src/host_diagnostics.rs` (parse `adb server-status`),
  `src/routes/HostDoctor.tsx` / `DiagnosticsCenter.tsx`, bindings, locales.
  Acceptance: host-doctor shows mDNS enabled/disabled and the negotiated USB
  link speed when the installed adb supports `server-status`, with a graceful
  fallback on older adb; parser covered by a Rust unit test over sample output.
  Complexity: M

- [ ] P3 — R-102 — Crash-artifact viewer (ANR / tombstone / dropbox)
  Why: No scrcpy front-end surfaces crash artifacts; complements the existing
  Diagnostics/bugreport flow with a read-only browsable list a developer expects.
  Evidence: developer.android.com/studio/inspect (App Inspection); gap — Droidsmith
  captures bugreports but does not parse ANR/tombstone/`dumpsys dropbox` records.
  Touches: new parser in `src-tauri/src/adb/parsers.rs` or a `crash_artifacts.rs`
  module, a command in `commands.rs`, a route under `src/routes/devices/`,
  bindings, locales.
  Acceptance: lists dropbox crash/ANR entries (tag, timestamp, process) parsed
  from `adb shell dumpsys dropbox --print`, with malformed rows shown as visible
  `parse_error` entries (matching the existing parser convention); parser
  unit-tested against sampled output. Live device needed only for end-to-end
  confirmation; the parse layer is headless-testable.
  Complexity: M

- [ ] P3 — R-103 — Per-process memory/CPU snapshot in Process Manager
  Why: Aya parity minus the device-side FPS hook; `dumpsys meminfo`/`gfxinfo`
  give a headless-parseable per-process snapshot the existing process inspector
  lacks (no memory/CPU columns today).
  Evidence: aya.liriliri.io (perf monitor); `src/routes/devices/ProcessManager.tsx`
  is a read-only process list with no resource columns.
  Touches: `src-tauri/src/process_tree.rs` or a new `dumpsys` parser,
  `src/routes/devices/ProcessManager.tsx`, bindings, locales.
  Acceptance: Process Manager shows PSS/RSS (and, where available, CPU%) per
  process from a `dumpsys meminfo`/`top -n 1` snapshot; parser unit-tested; live
  graphs are explicitly out of scope (they need a device).
  Complexity: M

- [ ] P3 — IMP-75 — Extract `Debloat.tsx` into `src/routes/debloat/`
  Why: 1,928 LOC with ~10 inline sub-components; matches the completed IMP-67
  (Devices) / IMP-72 refactor pattern; improves maintainability and testability.
  Evidence: `src/routes/Debloat.tsx` (DebloatApplyReview, PackErrors,
  PackImportControl, PackCard, PackPicker, PackPreview, CompatibilityChecks,
  QueueApply*, QuirkHint).
  Touches: `src/routes/Debloat.tsx` → new `src/routes/debloat/` modules.
  Acceptance: each panel lives in its own file under `src/routes/debloat/`;
  `Debloat.tsx` becomes a thin composition root; typecheck/lint/tests and
  `ui:smoke` stay green with no behavior change.
  Complexity: M


