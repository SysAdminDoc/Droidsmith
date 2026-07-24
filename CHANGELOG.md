# Changelog

All notable changes to Droidsmith. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) with Droidsmith's
own R-NNN / IMP-NN reference scheme so entries cross-link cleanly into
[ROADMAP.md](ROADMAP.md). Items move here from the roadmap when shipped.

The project follows [semver](https://semver.org/) from v0.1.0 onward; until
then versions stay at `0.0.x` and the changelog focuses on roadmap-item
completion.

## [Unreleased]

Working batches live here. Sections collapse into a versioned release on
each milestone tag.

### Added

- **R-117 — scrcpy control-only mode.** Mirror now offers a version-gated
  (scrcpy 3.2+) "No window" toggle that launches scrcpy with `--no-window` to
  control and/or record a device without opening a mirror window. The flag is
  hidden on older scrcpy and the setting persists in the per-device preset.
- **R-114 — Offline APK version diff.** The APK Analyzer can now compare the
  analyzed APK against a second local APK (chosen via a one-shot grant) and
  reports the deltas that matter for an update review: added/removed
  permissions, component-count changes, min/target/compile SDK changes, signing
  scheme changes, signer-certificate additions/removals, file-size change, and a
  package-id mismatch warning. The comparison is computed entirely locally and
  nothing is uploaded. (Diff logic lives in the frontend as a pure, fixture
  -tested function since both parsed reports are already client-side.)
- **R-111 — scrcpy app launcher.** When the detected scrcpy supports it
  (3.0+), Mirror offers an optional "Launch app on connect" control that starts
  a chosen app via `--start-app`, optionally into a `--new-display` virtual
  display. The picker autocompletes from the device's package inventory
  (loaded on demand) and the selection persists in the per-device mirror
  preset. The flag is validated as a package name and the control is hidden on
  older scrcpy.

## [0.9.11] - 2026-07-24

Fleet CLI, archive/debloat trust signals, packaging manifests, and async
hardening.

### Added

- **R-115 — winget + Scoop manifest generator.** `npm run packaging:generate`
  renders a winget singleton manifest (`packaging/winget/`) and a Scoop manifest
  (`packaging/scoop/`) from the repo version and Tauri bundle metadata, with
  placeholder installer URLs/hashes until a tagged release provides real ones.
  `npm run packaging:check` schema-validates the rendered manifests and asserts
  they track `package.json`. Public winget/Scoop submission stays tracked in
  Roadmap_Blocked.md pending a tagged GitHub release.
- **R-112 — Running services in the Debloat review.** The debloat safety review
  now probes each selected package (bounded to the first 16) for live services
  via `dumpsys` and surfaces which apps are running right now, so the reviewer
  knows a disable won't stop already-running services until reboot or
  force-stop. Handles loading, populated, none, truncated, and error states.
- **R-109 — Archive reversibility warning.** The package-action review now
  checks each package's installer-of-record before archiving. Android 15 can
  only restore an archived app through an installer that handles the unarchive
  intent, so packages that were sideloaded (no/`shell`/package-installer
  installer), whose installer has since been removed, or that carry an
  unverified third-party installer are flagged with a distinct warning instead
  of being presented as cleanly reversible. Only a Play Store installer is
  treated as reversible.
- **R-108 — CLI fleet mode.** `droidsmith-cli run`, `baseline-export`, and
  `baseline-inspect` now accept `--all-devices` to fan the operation over every
  connected, authorized device instead of a single `--device SERIAL`. Each
  device is planned/applied independently; `--json` emits a `devices[]` array
  with one `outcome: ran | error | skipped` entry per device.
  Unauthorized/offline devices and unauthenticated TCP transports (without
  `--allow-unsafe-transport`) are skipped rather than aborting the fleet, and
  the exit code is `1` if any device was skipped or failed.
  `baseline-export --all-devices --output <dir>` writes one `<serial>.json`
  baseline per device.

### Fixed

- **Audit — Debloat apply queue keeps running after leaving the route.**
  Navigating away mid-apply now stops the queue: unmounting bumps the queue
  generation and flags cancellation, so `runQueue` returns after the current
  already-journaled action instead of silently applying the remaining packages
  in the background with no visible progress or cancel control.
- **IMP-85 / audit — user-discovery races and dead code.** `loadUsers` in Apps
  and Debloat now carries a last-write request guard so interleaved
  `callListUsers` responses from rapid target switches can no longer clobber the
  current device's user selection, and the unreachable "Android user discovery
  returned no users" throw (a hardcoded English literal the backend contract
  already prevents) was removed.

## [0.9.10] - 2026-07-22

Deep engineering / UX audit pass: ~40 verified fixes across backend process
supervision, renderer async flows, localization, accessibility, and theming.

### Fixed

- Closing the app now terminates tracked scrcpy and gnirehtet sessions;
  previously the detached gnirehtet relay outlived the app, kept the device
  reverse-tethered, and made every relaunch fail with `RelayFailed` (address
  in use) with no in-app recovery.
- The per-device action journal takes an OS advisory file lock for each
  journal cycle, closing the GUI-vs-CLI race that could mint duplicate action
  ids, rewrite a live `Pending` intent as `Interrupted`, or truncate the file
  mid-append.
- Finished scrcpy recordings evicted by a concurrent status/stop/launch reap
  are retained in a bounded unclaimed-recording map, so the reveal/open grant
  can no longer be lost to a poll race.
- Post-exit pipe-reader joins in process capture and operation stages are
  bounded; a child that leaked its pipe write-ends to a detached descendant
  can no longer hang a completed operation forever and defeat cancellation.
  Stage stdin is capped at 64 KiB to prevent a pipe deadlock, and pack
  dependency expansion is depth-capped so a crafted imported pack cannot
  overflow the stack.
- `launch_scrcpy` and `start_gnirehtet` no longer freeze IPC dispatch for up
  to ~23 s on a capability-cache miss; their blocking work runs through
  `spawn_blocking_operation` like the sibling commands.
- Wrapped `df -k` rows no longer parse with transposed storage figures, and
  `getprop` values legitimately ending in `]` keep their closing bracket.
- Apps and Debloat key their effects on the scalar device fingerprint, so
  unrelated device plug/unplug events no longer refire user discovery, wipe
  package/metadata caches, resubscribe the drag-drop listener per keystroke,
  or reset the Debloat wizard mid-selection. Loaders carry request
  generations so slow responses cannot land under the wrong filter; batch
  applies, recovery baselines, and APK exports bail on mid-flight device
  switches; dropping an APK during a running install no longer starts a
  second concurrent install; and a superseded Debloat queue stops instead of
  resurrecting a stale done screen (retry failures are surfaced too).
- Device detail clicks are generation-guarded (last click wins), a null
  transport id no longer marks every null-transport row selected, a failed
  gnirehtet stop reports its error instead of losing it to the status poll,
  and a failed settings initialization retries instead of caching the
  rejection for the whole session.
- The command palette stops Escape from cascading into the underlying modal,
  scrolls the active option into view during arrow-key navigation, and gets
  a visible focus ring (as does the console prompt). Error/progress panels in
  Mirror and Bugreport announce via live regions, Wireless drops its
  double-announcing nested live wrapper, logcat reorder buttons name the
  query they move, and the Profiles workspace switcher replaces its partial
  ARIA tabs pattern with `aria-pressed` buttons.
- The non-embedded data-controls section regains its vertical padding in the
  Diagnostics Center.

### Changed

- Host Doctor finding summaries, remediation steps, and privacy notes are
  localized for all 17 finding codes in all five locales (previously titles
  only). Raw backend enums (wireless service kind, fastboot mode, host
  platform, profile package states), tuning choice labels, built-in logcat
  query names, and renderer fallback strings now render through locale keys
  with raw-value fallback. Russian gains ~35 missing translations with
  correct CLDR plurals, progress ellipses are normalized across locales, and
  byte/count formatting is locale-aware everywhere via one shared helper.
- Dialog and menu surfaces share a `surface-dialog` token at a uniform
  `rounded-lg`, Console/Logcat terminals share `surface-terminal`,
  divider-role borders are normalized to `border-white/10`, and the dead
  `darkMode: "class"` Tailwind config is removed.

## [0.9.9] - 2026-07-22

### Changed

- Reworked the application shell and all eleven primary workspaces around a
  flatter, denser visual system: larger readable type, compact route headers,
  hairline section separators, quieter status treatment, consistent control
  grouping, and a narrower navigation rail replace the previous wall of
  outlined cards and pills.
- Apps now uses underline filters, a compact installed-package table, and a
  per-row overflow menu for secondary commands; Logcat moves advanced filters
  behind disclosure and gives live output a larger, more legible terminal;
  Profiles, Debloat, Console, Tuning, and the remaining tools use route-specific
  workspace layouts instead of repeating the same card stack.
- Rendered-route coverage now captures every menu page and checks all eleven at
  desktop and narrow widths, in addition to the existing end-to-end workflows.

### Fixed

- Profiles constraints no longer collide or clip at desktop or mobile widths,
  Tuning command previews wrap safely on narrow screens, and compact shell
  actions no longer overflow the reduced navigation rail.

## [0.9.8] - 2026-07-22

Production audit: command and host-file trust boundaries, durable recovery,
bounded process/APK handling, async UI correctness, accessibility, and startup
performance.

### Security

- Diagnostic-looking Console commands that can clear Logcat buffers, write
  device-side logs, resize buffers, or invoke mutating `dumpsys` service verbs
  now require the same reviewed and journaled execution path as other device
  mutations.
- Reveal and Open With authorization is now registered only after Droidsmith
  successfully produces a regular artifact, then revalidates its canonical
  identity before launching the OS. Merely choosing a save destination—and
  later symlink retargeting—can no longer authorize an unrelated host file.

### Fixed

- File Manager mutations now preserve legal leading and trailing spaces in
  Android path components instead of silently redirecting the operation to a
  different sibling path.
- Action-journal replay now rebuilds undo links solely from durable inverse
  records, rejects exhausted IDs, skips oversized corrupt rows with bounded
  memory, repairs partial tails by scanning backward, and migrates legacy
  filenames to bounded collision-resistant per-device names that work on
  Windows reserved-name edge cases.
- Diagnostics, Logcat, and Layout Inspector exports now commit through sibling
  staging files instead of truncating their destination in place; failed writes
  leave the prior artifact intact. Suggested save names also reject Windows
  device names and invalid filename characters before opening the native dialog.
- Subprocess supervisors now surface pipe-read, worker-panic, and process-tree
  termination failures instead of returning partial output as success. Captured
  operations cap live IPC output with an omission notice, and rapidly exiting
  Logcat children stop after five reconnect attempts instead of retrying forever.
- APK Analyzer now snapshots and hashes the selected package once, then feeds
  the same bounded immutable bytes to ZIP parsing, signing-block inspection,
  and official signature verification. It also keeps only the forty largest
  entry rows during scanning, bounds control-laden display values, and reports
  Android `versionCodeMajor`-based long version codes correctly.
- Console commands now preserve quoted and escaped argument boundaries, report
  malformed input in every shipped language, cap retained scrollback, follow
  streaming output only while the user remains near the bottom, and discard
  mutation results from a superseded device target. Command Palette empty
  results retain correct combobox state, and Tuning/APK Analyzer now have
  visible navigation icons.
- Mirror capability and launch responses can no longer overwrite a newer
  device selection; a late scrcpy session is stopped rather than left detached.
  Paused and stopped Logcat views now continue expiring age-filtered rows.

### Performance

- Non-English locale resources now load on demand with an embedded-English
  startup fallback and actionable load failures. This reduced the measured
  initial renderer JavaScript from about 693 KB to 358 KB, and the release gate
  now enforces a 450,000-byte ceiling.

## [0.9.7] - 2026-07-21

Roadmap drain: trusted APK signature verification, bounded local Perfetto
captures, deterministic layout accessibility audits, renderer reliability,
portable settings, and route-level loading performance.

### Added

- **R-107 — Official APK signature verification.** The offline Analyzer now
  discovers a compatible Android SDK `apksigner` 0.9+ without making Java a
  requirement for its existing static report. Bounded official verification
  distinguishes verified, rejected, and Not verified outcomes; successful
  reports expose verified schemes, signer/source-stamp counts and identities,
  SHA-256 certificate fingerprints, subject/issuer, validity, and optional
  proof-of-rotation lineage with carried capabilities. PEM metadata is accepted
  only when its calculated certificate digest matches the value printed by
  `apksigner`; valid, tampered, multi-signer, and rotated fixtures plus a live
  Build Tools 37.0.0 probe cover the workflow.
- **R-106 — Bounded Perfetto system traces.** Supported Android 10+ devices now
  expose fixed UI-rendering, app-startup, and system-health presets with their
  sources, duration, ring buffer, and 64 MB file ceiling shown before capture.
  Privacy-gated traces use cancellable config-stdin capture, atomic local
  commit, one-shot destinations, remote cleanup on every exit path, and local
  Reveal/Open With actions without upload or an embedded viewer.
- **R-105 — Deterministic Layout Inspector accessibility audit.** UIAutomator
  captures now flag clickable nodes without labels, duplicate non-empty
  resource IDs, and density-aware touch targets smaller than 48dp. Findings
  link to exact tree nodes and raw attributes; local JSON/text exports carry a
  privacy warning and explicitly exclude color-contrast evaluation.
- **IMP-83 — Route-level renderer chunks.** All eleven workspaces now load as
  dynamic modules with localized loading and keyboard-recoverable failure
  states. Focus/hover and delayed adjacent-route preloads stay speculative, and
  the release gate now verifies Vite's manifest plus a 700,000-byte initial
  JavaScript ceiling (down from the prior roughly 934 KB entry).
- **IMP-79 — Reversible display controls.** Density and night-mode changes now
  capture the current user-bound raw/effective state before mutation, verify
  the resulting state, and persist the exact inverse in the action journal.
  Device Controls exposes an immediate Restore action, while Activity can
  safely undo the change after restart without guessing a default value.
- **IMP-78 — Renderer crash recovery.** Added a localized top-level error
  boundary with reload, backend-owned diagnostics-folder reveal, and a bounded
  redacted summary that remains manually selectable if clipboard access fails.
  A failure inside the recovery UI itself degrades to dependency-free static
  guidance instead of another blank window.
- **R-104 — Portable settings round-trip.** Expanded versioned settings exports
  to include language, mirror presets, Logcat query libraries, wireless history,
  and auto-reconnect. Imports validate before mutation, show a redacted
  merge/replace preview, write atomically, preserve machine-local device
  fingerprints, and create a restorable pre-import backup.

### Security

- **IMP-77 — Bounded subprocess capture.** Consolidated short-lived ADB,
  fastboot, host-diagnostic, and scrcpy probes behind a shared 4 MiB-per-stream
  collector. Output overflow now terminates and reaps the complete child tree
  with a typed error; streaming supervisors share the same bounded-tail logic.

### Fixed

- **IMP-82 — Accessible status and locale contracts.** Static state panels no
  longer announce themselves as live updates, while operation results opt into
  polite or assertive announcements. Process force-stop review is now a
  focus-trapped, Escape-dismissible modal, and APK/Wireless numbers and dates
  follow the selected locale across mobile and 200% reflow coverage.
- **IMP-81 — Public README contract.** Removed links to local-only documentation,
  embedded the supported source-build and unsigned-distribution guidance, and
  now distinguish the `0.9.6` source tree from the older published `v0.5.3`
  artifacts.
- **IMP-80 — Target-bound async lifecycle.** Centralized immutable device
  fingerprints, operation generations, stale-result guards, and cancellation
  registration in one renderer primitive. Apps permissions, File Manager,
  gnirehtet, Logcat, bugreport capture, and the device watcher now cancel or
  ignore work from superseded targets; the backend also closes the
  cancel-before-registration race without spawning an orphan process.
- **IMP-76 — Locale persistence contract.** Unified the renderer, Rust settings,
  and isolation allowlists behind a release-checked five-locale contract. Every
  shipped locale now survives restart, while a failed save remains visible
  without undoing the user's in-session language change.

## [0.9.6] - 2026-07-21

Roadmap drain: the Debloat workspace is now split into focused workflow panels
with the route retained as the orchestration and composition root.

### Security

- **Isolation policy completion.** Added explicit fail-closed classifications
  and payload validation for 19 registered commands covering APK analysis,
  local pack import/export, dropped packages, wireless history, device settings,
  gnirehtet control, and related read paths. Added regression coverage for the
  new path-grant purposes and mutation boundaries.
- Updated transitive `brace-expansion` lockfile entries to patched releases
  (`1.1.16` / `5.0.7`) after the release gate detected the current denial-of-
  service advisory; `npm audit --audit-level=moderate` is clean.

### Refactored

- **IMP-75 — Debloat workflow split.** Extracted apply review, pack preview,
  compatibility checks, queue progress/results, queue rows, quirk guidance, and
  queue helpers into focused `src/routes/debloat/` modules. The interaction and
  safety behavior is unchanged, including unsafe-tier acknowledgement, recovery
  baseline export, cancel-after-current, retry, and quirk explanation flows.

## [0.9.5] - 2026-07-21

Inspection + lifecycle expansion: offline APK Analyzer, device-state pack
export, richer Mirror and Process Manager, a dependency re-audit, and the
start of the Debloat.tsx split.

### Security

- **R-099 — Dependency re-audit.** Re-ran `cargo audit --deny warnings` clean
  over 527 crates; every ignored advisory remains an unfixable transitive
  (Tauri GTK3 / unic / build-time macro), and none affects a direct dependency
  (RUSTSEC-2026-0009 `time` does not apply to the pinned version). Refreshed the
  `release-policy.json` exception review window to 2027-01-15.

### Added

- **R-103 — Per-process CPU in Process Manager.** The process table now shows a
  sortable `%CPU` column parsed from the existing `ps -o %CPU` snapshot
  (`ProcessInfo.cpu_percent`), alongside the RSS memory column. OEM `ps` builds
  that omit the column show `—` rather than a fabricated value. Live graphs
  remain out of scope (they need a device-side sampler).
- **R-100 — scrcpy virtual-display flags.** Mirror gains version-gated
  `--display-ime-policy` (soft-keyboard placement on a virtual display, scrcpy
  3.2+) and `--no-vd-destroy-content` (keep virtual-display apps alive after the
  window closes, scrcpy 3.1+) controls, persisted in the per-device mirror
  preset and shown only when the detected scrcpy build supports them.
- **R-098 — Export device debloat state as a shareable pack.** A new "Export
  device state" control in the Debloat picker captures the selected device's
  currently disabled, archived, and uninstalled packages and writes them to a
  schema-valid pack YAML through a native save grant. The exported file
  round-trips through the v0.9.4 pack importer, so "what I removed on this
  phone" can be re-applied to another device after an OTA or factory reset.
  New `export_device_pack` IPC command, `pack_export_save` host-path purpose,
  and `packs::from_device_state` serializer.
- **R-097 — Offline APK Analyzer.** A new sidebar route statically inspects a
  local `.apk`/`.apks` chosen through the audited host-path grant — no device
  required. Reports package id, version code/name, min/target/compile SDK,
  requested permissions, activity/service/receiver/provider counts, DEX file /
  defined-class / method-reference totals with a multidex (64K) flag, the
  detected signing scheme (v1 JAR + v2/v3/v3.1 from the APK Signing Block), the
  file SHA-256, and the largest ZIP entries. New backend module
  `apk_analysis.rs` (reusing the AXML/resource-table parser from
  `apk_metadata.rs`), `analyze_apk` IPC command, and `apk_analyze_open`
  host-path purpose.

## [0.9.4] - 2026-07-21

Local debloat-pack import — the network-free half of R-095.

### Added

- **Import debloat packs from a local YAML file.** The Debloat picker gains an
  "Import a pack" control that opens a backend-owned native file dialog through
  the audited one-shot host-path grant model, optionally verifies a
  caller-supplied SHA-256 pin, schema-validates and lints the bytes with the
  same loader as bundled packs, rejects ids that shadow a bundled pack, and
  stores the file under the app-data `packs/` directory so it appears in the
  picker on the next load. Imported packs carry an "Imported" badge and a
  remove control. New IPC: `import_pack`, `remove_imported_pack`; new host-path
  purpose `pack_import_open`; `PackCandidate` now reports an `imported` flag.
  This ships the dependency-free alternative called out in R-095 with no
  outbound network capability — remote-URL fetching stays in
  [Roadmap_Blocked.md](Roadmap_Blocked.md) pending a maintainer network-posture
  decision.

## [0.9.3] - 2026-07-21

Roadmap drain to empty: the Devices.tsx god-file split, persistent gnirehtet
reverse-tethering, and the version-gated scrcpy capability surface (virtual
display, audio-source picker, camera mirroring).

### Added

- **R-089 scrcpy capability surface (virtual display, camera, audio sources).**
  Mirror now exposes a version-gated virtual/secondary display
  (`--new-display=<w>x<h>[/<dpi>]`, scrcpy 3.0+), an audio-source picker
  (`--audio-source`; expanded `mic-*`/`voice-call`/`playback` gated on 3.2+), and
  **camera mirroring** (`--video-source=camera` with `--camera-facing`/
  `--camera-size`, gated on scrcpy 2.7+). Camera mode is a video-source change,
  so display-only flags (crop, orientation, new/flex display, touch overlay,
  turn-screen-off) are suppressed. All values are validated (digits/`x`/`/`
  only, so no argument metacharacters reach the device transport), persist in
  mirror presets, and are asserted in the scrcpy arg-construction unit tests;
  unsupported controls are hidden on older scrcpy.

### Changed

- **Persist gnirehtet reverse-tethering across navigation.** The "Share Internet"
  panel no longer stops the supervised session when the Devices route unmounts;
  instead, on (re)mount it re-attaches to a session already running for the
  device via the new `find_gnirehtet_session` IPC. Tethering now survives
  navigating away (e.g. to install something over the shared connection) and is
  only torn down when the user clicks Stop.

### Refactored

- **IMP-72 Devices.tsx god-file split.** Extracted the inline device panels and
  helpers from `Devices.tsx` (1,633 → 370 LOC) into focused `src/routes/devices/`
  modules — `icons`, `DeviceHeaderActions`, `AdbHealthPanel`, `RecoveryDialog`,
  `DeviceTable` (toolbar/table/skeleton/state helpers), `DeviceDetail`
  (+ health cards), and `AuthorizePrompt` — matching the IMP-67 sub-panel split.
  No behavior change (typecheck/lint/tests/ui:smoke green).

## [0.9.2] - 2026-07-20

Roadmap drain: wireless reconnect history, post-OTA drift detection, quirk
failure hints, ProcessManager force-stop, an expanded Mirror scrcpy flag
surface, Host Doctor backend guidance, inspector/UX fixes, and new test
coverage.

### Added

- **R-093 USB/mDNS backend troubleshooting guidance.** When Host Doctor finds no
  connected device, it now surfaces the platform-specific USB/mDNS backend
  toggles introduced in platform-tools 37.0.1 (`ADB_USB_LEGACY` on Windows,
  `ADB_LIBUSB` on macOS) and reports any already set, so the "device not
  detected" fix is discoverable. Unit-tested.
- **R-087 Post-OTA debloat-drift detection.** Droidsmith now records each
  device's build fingerprint and, when it changes between sessions (an OTA
  update), surfaces a dismissible notice on the Apps route prompting the user to
  review their debloat recovery baseline — packages disabled/removed before the
  update may have returned. New IPC `observe_device_fingerprint`; the existing
  recovery-baseline review already renders the drift diff and confirmed re-apply.
- **R-088 / R-089 Expanded Mirror scrcpy flags.** Mirror now exposes
  `--max-fps`, `--fullscreen`, `--always-on-top`, `--no-control` (view-only)
  (R-088), plus the validation-heavier `--crop`, `--display-orientation`,
  `--screen-off-timeout`, and `--audio-codec` (R-089). All are selectable,
  persist in per-device mirror presets, validated (crop/orientation/timeout/
  codec reject malformed input, including shell metacharacters in crop), and
  asserted in the scrcpy arg-construction unit tests. (The remaining
  version-gated capability surface — virtual/new display, camera mirroring,
  audio-source picker — stays under R-089.)
- **R-090 ProcessManager force-stop.** Process rows that resolve to an app
  package now offer a confirmed **Force-stop** action (`am force-stop --user 0`)
  routed through the audited action planner/journal. Native binaries, kernel
  threads, and daemons are correctly excluded.
- **IMP-68 Vendor-quirk failure hints.** The previously orphaned
  `explain_failure` IPC is now wired into the Debloat results: a failed row
  offers a "Why did this fail?" action that matches the raw error and the
  device's manufacturer/build fingerprint against the bundled quirk rules and
  shows the explanation plus any suggested workaround.
- **R-086 Wireless connection history + reconnect.** Successful wireless
  connects are now recorded (host:port, last-connected time, optional label) in
  the backend settings store, bounded to the 32 most recent and deduplicated by
  endpoint. The Wireless route shows a history panel with one-click **Reconnect**
  and **Forget**, plus an opt-in "reconnect known devices on launch" toggle that
  attempts each saved endpoint once when the route first loads. New IPC:
  `list_wireless_history`, `forget_wireless_endpoint`, `set_wireless_auto_reconnect`.

## [0.9.1] - 2026-07-20

Deep audit pass — correctness, security, UX, performance, and maintainability
fixes across primary and secondary surfaces.

### Security

- The console shell gate classified commands by their head token only, but adb
  joins argv and runs it through the device `sh -c`, so a read-only head
  (`getprop`, `cat`, …) followed by a token carrying `; | & $ \` ( ) < >` — for
  example the console splitting `getprop; pm uninstall x` on whitespace —
  executed a hidden mutation while bypassing the reviewed/journaled executor.
  `classify_shell` now treats any shell control metacharacter as non-read-only,
  so `shell_run` rejects it and `plan_shell_action` routes it through review.

### Fixed

- Reverse-tethering (`Share Internet`) sessions were orphaned when the selected
  device changed or the route unmounted — the toggle remounts per device, so a
  running gnirehtet session lost its only control surface and a later remount
  would spawn a duplicate that fails on the busy relay port. The session is now
  stopped in the effect cleanup (and a "not tracked" stop of an already-reaped
  session is ignored).
- The permissions panel is not remounted when the inspected package changes, so
  a slow list/set request for the previous package could overwrite the current
  one; added a generation guard that drops stale resolutions.
- The Tuning editor rendered raw developer tokens (`invalid`, `nan`, `0.5–2`) as
  its inline validation message; these are now translated (all five locales).
- The Profiles ordered-action list and dry-run diff showed the raw action enum
  (`uninstall_for_user`) while the selector showed the translated label; both
  now use the shared label, and the actionable "ready" diff rows use an active
  tone instead of reading more muted than the "already matches" no-ops.
- File-operation status ("complete") no longer lingers after browsing to a
  different directory.

### Performance

- The installed-package table filtered and re-rendered every interactive row
  (each mounting an IntersectionObserver) on every search keystroke — real jank
  on devices with hundreds of packages. Filtering now uses a deferred search
  value and memoized collections so the heavy render is lower-priority.

### Refactored

- Extracted the duplicated `CapturedTail` process-output capture/redaction
  helper (≈90 lines) shared by the scrcpy and gnirehtet supervisors into a
  single `captured_tail` module with its own tests.
- Inlined the redundant Logcat `cancelWithRegistrationRetry` no-op wrapper (its
  registration-retry now lives in `callCancelOperation`).
- Normalized the Rust backend with rustfmt (accumulated drift across several
  modules) so `cargo fmt --check` passes.

## [0.9.0] - 2026-07-20

Reverse-tethering + maintainability batch: a gnirehtet "Share Internet" toggle,
the completed Devices/Apps god-file split, and an RTL logical-property
foundation across every route.

### Added

- Gnirehtet reverse-tethering ("Share Internet") toggle in the Devices controls
  surface (R-084). When the `gnirehtet` binary is on PATH, a per-device toggle
  starts/stops a supervised `gnirehtet run <serial>` session that shares the
  PC's internet with the device over USB; stopping restores the device's
  default network. Session start/poll/stop is supervised in
  `src-tauri/src/gnirehtet.rs`, mirroring the scrcpy lifecycle, with a
  duplicate-serial guard and classified exit reasons. The toggle stays hidden
  when the binary is absent.

### Refactored

- Laid the RTL layout foundation (R-085). Every route and shared component now
  uses CSS logical properties instead of physical ones — `ml/mr → ms/me`,
  `pl/pr → ps/pe`, `left/right → start/end`, `text-left/right → text-start/end`,
  and `border-l/r → border-s/e` (including logical border colors) — so a future
  RTL locale mirrors the layout automatically. Direction already propagates to
  `<html dir>` from each locale's `dir` metadata (`src/lib/i18n.ts`); Tailwind
  3.4's built-in logical utilities need no config change. No physical
  margin/padding directional utilities remain in the route files.
- Completed the Devices/Apps god-file split (IMP-67). `Devices.tsx`
  (2,695 LOC → 1,633) now delegates its device-controls surface to
  `src/routes/devices/DeviceControls.tsx` and its remote file browser to
  `src/routes/devices/FileManager.tsx`, joining the earlier
  Network/Layout/Process inspector extractions. `Apps.tsx` (3,063 LOC → 1,591)
  split its independent sub-panels into `src/routes/apps/`: `PackageTable`
  (ARIA grid + roving-tabindex), `JournalPanel`, `PermissionsPanel`,
  `RecoveryBaselinePanel`, `FilterControls` (filter chips + batch action bar),
  and `InstallPanels` (install status/override + backup status), with shared
  route-state types collected in `src/routes/apps/types.ts`. Behavior, rendered
  output, and the `ui:smoke` flows are unchanged.

## [0.8.0] - 2026-07-20

Device health + tuning batch: a battery/storage/thermal dashboard, a new
Tuning route for safe system-settings edits, curated debloat presets, an
accessibility pass (forced-colors + ARIA grid), a repaired end-to-end
`ui:smoke` gate, and the start of the Devices god-file split.

### Fixed

- Repaired the `ui:smoke` desktop route flow, which had been red since before
  the v0.7.0 finalize and never completed end-to-end. Four latent breaks: the
  debloat-complete assertion matched a hyphen where the locale renders an
  em-dash; the Settings language round-trip re-queried the selector by its
  English label after switching to Russian; the Tauri mock lacked the
  `settings_export` host-path purpose plus the webview/event-plugin globals
  (`metadata`, `__TAURI_EVENT_PLUGIN_INTERNALS__`) the Apps drag-drop listener
  needs; and the mock's `unregisterListener` shared the `callbacks` map with
  `transformCallback`, so unlisten was evicting the live `watch_devices`
  channel and starving device-snapshot delivery (IMP-69).

### Accessibility

- Windows High Contrast / forced-colors support (WCAG 2.2 AA). Focus indicators
  now fall back to a system-colored `outline` in forced-colors mode (where
  Tailwind's box-shadow focus rings are stripped), container/control borders
  pin to `ButtonBorder` so cards, inputs, and menus keep visible edges, native
  checkboxes/radios track the system `Highlight`, and progress fills stay
  legible. Native checkboxes/radios are also floored to a 20px box so the 24px
  target-spacing exception (2.5.8) is met alongside their label gap (IMP-61).
- The Apps package table is now a W3C ARIA grid: `role="grid"` with
  `aria-rowcount`/`aria-colcount`/`aria-multiselectable`, `role="row"` +
  `aria-rowindex`/`aria-selected` rows, and `columnheader`/`gridcell` cells.
  Roving-tabindex keyboard navigation moves a single tab stop with the arrow
  keys, Home/End (Ctrl for grid corners), and PageUp/PageDown; Enter/Space hands
  focus to the focused cell's control and Escape returns to the cell. Verified
  end-to-end in `ui:smoke` (IMP-62).

### Added

- Device health dashboard: the Devices detail panel now surfaces battery
  health (health status, charge cycle count, capacity in mAh, voltage,
  technology), per-partition storage usage (system/vendor/data/cache with
  used/free bars), and thermal-zone temperatures with throttling-status
  badges. Backed by new `dumpsys battery`/`df -k`/`dumpsys thermalservice`
  parsers in `device_info.rs` (R-079).
- Tuning route: a new sidebar pane that edits a curated allow-list of safe
  Android system settings over ADB (animation scales, screen-off timeout, font
  scale, stay-awake-while-charging). Each control shows the current value, an
  editable field/selector with range or choice validation, and a live preview
  of the `adb shell settings put …` command. Writes go through the privileged
  transport boundary, are validated against the catalog before execution, and
  land in a session change log with one-click revert. Backed by a new
  `adb/device_settings.rs` module and `list_device_settings`/
  `put_device_setting` commands, with de/es/ru/zh locales (R-082).
- Curated debloat presets: the Debloat pack preview offers named presets
  (Privacy Max, Bloatware Sweep, Minimal Google, Carrier Cleanup) that
  pre-check the ready packages whose labels match the preset's theme, using the
  existing per-entry `labels`. Only presets with at least one match are shown,
  each with its match count, and the selection remains fully reviewable/editable
  before applying (IMP-63).

## [0.7.0] - 2026-07-18

Design-system unification, locale expansion, and OEM pack coverage batch.

### Added

- German (de), Spanish (es), and Simplified Chinese (zh) locale support
  with full 1163-key parity. The i18n test suite now validates all 5
  shipped locales via `it.each` (R-077).
- OnePlus OxygenOS, Realme UI, and Nothing OS debloat packs (33 entries
  total) covering previously unrepresented OEMs. All pass
  `droidsmith-pack-lint` and the bundled-pack contract test (R-078).
- Background service inspector: `list_running_services` command parses
  `dumpsys activity services` to extract running ServiceRecord components
  per package (R-086).
- Settings export/reset smoke coverage in `ui:smoke` (IMP-68).

### Changed

- Extracted `FieldSelect` and `FieldTextArea` shared primitives into
  `common.tsx`, replacing 10 raw `<select>` and 5 raw `<textarea>` elements
  across 8 route files. Eliminates per-route style drift (bg, height,
  hover, transition inconsistencies).
- Replaced the hand-rolled `TransportChip` in Devices with the shared
  `TransportBadge` from common.tsx (-25 LOC).
- Replaced the one-off permission grant/deny toggle with the standard
  `Button` component using secondary/danger variants.
- Converged all route content panes from a mix of max-w-5xl/6xl/7xl/[88rem]
  to a consistent max-w-7xl (80rem).
- Replaced two arbitrary `shadow-[rgba(...)]` utilities with the theme
  `shadow-panel` and `shadow-glow` tokens.

### Fixed

- Diagnostics save dialog cancellation now shows a "Save cancelled."
  status message instead of leaving the UI in a silent state.
- Console hint text now explains that arrow-key history recall skips
  failed commands.

## [0.6.0] - 2026-07-18

Research-driven feature batch from the 2026-07-18 competitive analysis
(25+ competitors, 38+ community signals, Android 16 / scrcpy 4.1 /
platform-tools 37.0.1 updates reviewed).

### Added

- Mirror route gained scrcpy 4.x toggles: flex display (resizable virtual
  display) and keep-active (prevent device sleep). Both are capability-gated
  and hidden when the installed scrcpy is pre-4.0. VP8/VP9 codecs were
  already wired end-to-end (R-074).
- Recovery baseline inspection detects post-OTA debloat drift: packages
  whose applied action was reverted (e.g. disabled then re-enabled by a
  system update) show a "Drifted" status with a danger badge and a re-apply
  plan, instead of the confusing "Already matches" they showed before (R-076).
- Drag-and-drop APK installation: dropping an .apk/.apks/.xapk/.apkm file
  onto the Apps route triggers the full install flow with validation, preview,
  and journal recording. A new `grant_dropped_path` backend command validates
  and issues a one-shot path grant for the dropped file (R-080).
- Safe device disconnect: a Disconnect button in the Devices detail panel
  sends `adb disconnect` for wireless devices and advises safe-to-unplug for
  USB. The command never issues destructive ADB operations (R-083).

### Testing

- Added CLI smoke tests for `droidsmith-cli` argument parsing and the
  `migrate-v1` subcommand against a v1 profile fixture (IMP-64).
- Added a `scrcpy_text` fuzz target covering version, help-text, and encoder
  listing parsers; promoted those parsers to `pub(crate)` visibility (IMP-66).
- Stale documentation references in CONTRIBUTING.md, LICENSE-THIRD-PARTY.md,
  and docs/DEVELOPMENT.md updated to reflect shipped and blocked state (IMP-65).

### Changed

- Auto-update (R-075) moved to Roadmap_Blocked.md: requires an Ed25519
  keypair and release-manifest hosting that depend on R-006.

## [0.5.3] - 2026-07-17

### Fixed

- Package enumeration no longer aborts with "package enumeration failed" on
  devices whose `pm` rejects the enrichment flags. `pm list packages` was
  always invoked with `-U` (package UID, Android 9+) and `-i` (installer);
  a device that rejects an unknown flag exits non-zero and previously failed
  the entire Apps list. Each pass now retries with the core `-f` flags when the
  enriched set is rejected (losing only uid/installer on older devices), and
  the archived/retained enrichment passes degrade instead of failing the list.

## [0.5.2] - 2026-07-17

Engineering, security, and product-quality audit pass. No feature changes;
correctness, accessibility, and design-consistency hardening throughout.

### Fixed

- Backend error messages now render their text instead of `[object Object]`.
  Tauri command rejections arrive as plain `{ code, message }` objects, so the
  app-wide `String(error)` fallback showed a useless placeholder for every
  `CommandError`; a shared `errorMessage()` helper unwraps them and all 59 call
  sites route through it.
- Added the missing `common.unknown` and `apps.selectAll/selectPackage` locale
  keys, so battery/storage/file-size fallbacks and package-checkbox screen-reader
  labels no longer display raw i18n keys. A regression test now fails if any
  static `t("literal")` key is unresolvable.
- Aligned the renderer `regexError` and backend `validate_linear_regex` logcat
  guards: both are now escape- and character-class-aware, so patterns like
  `foo\)*` or `[)]+` are accepted (or rejected) identically instead of one
  side blocking a save the other allowed.
- Hardened ADB parsing: the scrcpy codec probe no longer panics when help text
  slices mid-UTF-8-character, and `ls` rows whose owner/group is a month
  abbreviation (e.g. `May`) no longer shift the size/name columns silently.
- Guarded export writes (`save_logcat_export`, `save_layout_export`) against a
  final-component symlink swap, matching `save_diagnostics`.
- Guarded stale-completion races: journal single/batch undo no longer refresh a
  switched device's state, and mirror session-status polling and record-launch
  no longer clobber or mis-target a freshly-selected device.
- Fastboot now clears a selection a rescan no longer lists and surfaces partial
  getvar failures instead of implying a complete table.
- Manual wireless pair/connect rejects malformed hosts before the ADB round-trip;
  settings reset normalizes the browser locale to a supported language.
- The logcat reconnect budget resets after a healthy run, so transient spawn
  failures spread across a long session no longer terminate the stream.

### Accessibility

- Async result messages (screenshot, display control, layout/logcat export,
  mirror preset, fastboot scan) are announced via `role="status"`/`"alert"`.
- The package filter is now a keyboard-navigable radiogroup (roving tabindex +
  Arrow/Home/End), the command palette's dialog role sits on its labelled
  surface, `aria-expanded` reflects real results, and the diagnostics focus
  trap no longer jumps focus behind a nested dialog.

### Changed

- Cached compiled logcat filter regexes and stopped refiltering the buffer on
  every render.
- Normalized off-theme `rose-*`/`slate-*` colors to the `red-*`/`anvil-*`
  tokens, fixed a broken `forge-400` focus ring, and removed dead scaffolding.

## [0.5.1] - 2026-07-17

### Fixed

- Device discovery now falls back to a one-shot ADB enumeration when the GUI's
  live watcher channel is rejected or reports a transient scan error, keeping
  authorized phones usable instead of leaving the Devices workspace blocked.
- Added a headless production-path regression that forces the watcher to fail
  persistently and verifies the fallback still renders the connected device.

## [0.5.0] - 2026-07-17

Brings the Devices workspace into close parity with the premium desktop
reference while retaining the native window frame and existing operational
workflows.

### Changed

- Reordered the Devices workspace around a compact selector and ADB-ready band,
  an eight-column lifecycle-health strip, selected-device details, and a calmer
  connected-device table with precise row selection.
- Moved ADB recovery into the header overflow, shortened the primary refresh
  action, and removed duplicate runtime/status treatments from the shell.
- Replaced the sidebar utility stack with compact Settings, Help, and About
  controls; language and persistence controls now live in Settings, while About
  exposes version/runtime context and the diagnostics center.
- Regenerated the README screenshots after desktop, mobile, command-palette,
  and Russian 200% reflow verification.

## [0.4.0] - 2026-07-17

Rebuilds the desktop visual system around readable typography, compact
navigation, quiet status language, and denser task-first layouts.

### Changed

- Reworked shared headers, navigation, surfaces, tables, buttons, inputs, status
  treatments, and spacing to remove roadmap chrome, excessive card outlines,
  tiny labels, and decorative pills across every shipped route.
- Moved device selection directly below the Devices header, automatically opens
  the only authorized device, presents ADB health as a divider-led summary, and
  keeps device controls ahead of host diagnostics.
- Simplified Mirror option grouping, reset page/sidebar/content scroll when
  changing workspaces, shortened route copy in both locales, and regenerated the
  README screenshots from the verified mocked-native state.
- Added the image-generated premium UI reference used to guide the implemented
  shell and Devices layout.

## [0.3.0] - 2026-07-16

Adds an opt-in incremental single-APK install mode with a clean fallback, and
corrects a stale roadmap blocker (app-bundle install was already shipped).

### Added

- The Apps installer gained an opt-in "Incremental" toggle that starts a large
  single-APK install before all bytes transfer (`adb install --incremental`).
  When the device or platform-tools lack Incremental FS support, it falls back
  cleanly to a normal install rather than failing, and the chosen mode
  (incremental, normal, or incremental-fell-back-to-normal) is both surfaced in
  the result and recorded in the install operation audit (schema v3). A genuine
  package failure — a signature or downgrade rejection — is reported as-is
  instead of being retried over a normal push. (App-bundle install for
  `.apks`/`.xapk`/`.apkm` archives was already shipped via atomic PackageInstaller
  sessions; the prior blocked note referencing a single-shot install path was
  stale.)

## [0.2.0] - 2026-07-16

Adds the versioned typed settings store, saveable structured Logcat query
presets (with package/process-name filtering), and a read-only layout
inspector; expands deterministic rendered-route coverage and fixes Russian
plurals and the documentation screenshots.

### Fixed

- The README "Package Workflow" and "Mirror Workflow" screenshots now show the
  actual populated workflows instead of the browser-only "desktop shell
  required" placeholder. Screenshots are regenerated deterministically from the
  mocked-native smoke state via `npm run docs:screenshots`, and the smoke run now
  asserts the documented routes never render the desktop-required placeholder and
  survive a narrow (390px) viewport.
- Russian plural output is now grammatically correct for counts 2–4 (IMP-36).
  Every pluralized string now defines the full CLDR set (`_one`/`_few`/`_many`/
  `_other`) in both locales — e.g. "2 устройства" (few) instead of the previous
  "2 устройств" (many). English `_few`/`_many` mirror `_other`, so key parity
  between the two locales is preserved.
- Inline device-control results (screenshot, density, force-dark, file pull) now
  render success in green and failure in red instead of an identical faint line,
  so a "Saved" and a "Failed" are visually distinct.
- Devices in a connected-but-unmanageable state (offline, bootloader, recovery,
  sideload, or an unrecognized `Other(...)` state) now get a dedicated guidance
  panel explaining what the state means and how to recover, instead of only a
  bare capitalized badge with no prompt.
- The battery/storage/file-size formatters on the Devices route now use the
  localized `common.unknown` string instead of a hard-coded English "Unknown".
- The scrcpy session supervisor now evicts terminated sessions on every status
  and stop call (not only on the next launch), so the session map stays bounded
  when a renderer polls one session but never launches another. The queried
  session is preserved so its final state is still observed.
- Device storage now queries `df -k /data`, pinning 1K-block units so the
  reported total/used/available are correct regardless of the busybox/toybox
  default block size.
- mDNS endpoint parsing now handles bracketed IPv6 (`[fe80::1]:5555`) correctly
  and rejects ambiguous bare IPv6 literals instead of mis-reading a trailing
  address group as the port.

### Changed

- Extracted the duplicated inline `DevicePicker` from Apps and Debloat into a
  single shared `common.tsx` component (IMP-37), removing the copy and its
  now-unused imports.

### Added

- Logcat query presets can now filter (and negate) by package or process name
  (IMP-60). While such a filter is active, Droidsmith polls `ps` on a bounded
  4-second cadence to resolve each line's PID to its process name (the base
  package plus an optional `:component` suffix); the snapshot never blocks the
  log stream, and a line whose PID is not yet mapped is surfaced rather than
  dropped, so a stale process table can't hide output. The new fields round-trip
  through the versioned store, isolation policy, and JSON import/export.
- Added a read-only layout / view-hierarchy inspector to the device dashboard.
  A one-click "Capture hierarchy" runs `uiautomator dump` (printed to `/dev/tty`,
  so no device-side file is written), parses the result into a depth-indented,
  searchable, read-only node tree (class, resource-id, text, content-desc,
  bounds, clickable), and exports the raw XML through a one-shot save grant.
  Malformed dumps surface visible parse-error rows instead of being dropped. The
  quote-aware parser needs no XML dependency and is covered by Rust fixtures; the
  capture/export commands are IPC-isolation-scoped and exercised by the smoke run.
- Expanded the deterministic rendered-route smoke coverage (IMP-51). Every route
  is now swept under the Russian locale at a 200%-zoom reflow viewport, and a new
  resilience flow exercises the Apps data route's loading skeleton, empty-state
  panel, and error panel (with recovery), plus a stale-completion / mid-request
  target-switch race: a package listing held in flight is superseded by a device
  hotplug, and the detached workspace hides its destructive controls and never
  lets the stale response mutate it or raise a console error. The smoke harness
  gained gate/release, forced-failure, and empty-listing mock controls to drive
  these states deterministically.
- Logcat gained saveable, versioned query presets (IMP-59). Presets filter on
  tag, message, PID, minimum level, and max age with per-field negation and an
  optional linear-time-safe regex subset (backreferences, lookaround, and
  group-quantifier backtracking are rejected with field-level guidance, enforced
  identically in the renderer and the Rust store). Presets can be saved, renamed,
  duplicated, reordered, deleted, and imported/exported as JSON; they live in the
  global scope or a per-device scope keyed by hashed identity, and only the query
  definition is persisted — never captured log lines. A bounded "recently
  applied" history and read-only built-in `Crashes & ANRs` / `Stack traces`
  presets (approximating Android Studio's `is:crash` / `is:stacktrace`) ship with
  fixture coverage. The logcat stream moved from `-v brief` to `-v threadtime` so
  timestamps and PIDs are available to the filters. Package/process-*name*
  filtering is tracked separately (IMP-60) since it needs a PID→package map.
- Migrated language and per-device mirror presets from unversioned raw
  `localStorage` keys to a versioned, backend-owned typed settings store
  (IMP-50). On first launch the renderer performs a bounded, isolation-validated
  one-time import of the legacy keys; the backend writes a redacted `v0` backup
  before migrating, quarantines corrupt data without blocking launch, and only
  clears the legacy renderer keys once the durable import succeeds. A new
  Diagnostics "Settings data" card offers scoped export (through a backend-issued
  save grant — the internal settings path never crosses IPC) and scoped reset for
  language, mirror presets, or all. The store, migration, quarantine, and
  export/reset paths are covered by Rust and renderer tests plus IPC isolation
  policy.
- Added deterministic JSON Schema 2020-12 contracts for pack, profile, and
  quirk contributions (IMP-49). Their Rust DTOs now reject unknown YAML fields;
  the release gate checks generated-schema fingerprints, migration notes, and a
  real profile-v1 compatibility fixture before accepting schema changes.
- Debloat pack preview now filters entries by a live search over package id,
  description, and labels (not just codename), and the final safety review
  requires an explicit acknowledgement checkbox before any unsafe-tier package
  can be applied — the confirm button stays disabled until the risk is accepted.
  Risk tiers and per-entry descriptions were already rendered; this closes the
  search and high-risk-acknowledgement gaps. The rendered-route smoke now asserts
  the unsafe confirm button is gated (2026-07-15).
- Added a "Show in folder" action that reveals produced artifacts (package
  exports, recovery baselines, screenshots, pulled files, scrcpy recordings) in
  the OS file manager. Implemented natively (no new plugin dependency): the
  backend records every save-dialog destination it issues this session and a
  gated `reveal_in_folder` command only honors those paths, so a renderer can
  never drive an open of an arbitrary location. The revealable registry is
  bounded and deduplicated; the platform reveal argv (explorer /select, open -R,
  xdg-open) is unit-tested, and the IPC isolation policy validates the path
  argument (2026-07-15).
- Surfaced uninstalled-but-data-retained packages as a dedicated "Data-only"
  Apps filter. `pm list packages -u` now runs on every Android version; remnants
  present in that listing but neither installed nor archived are labeled
  "Uninstalled · data retained" and offer a single guarded, journaled full-purge
  action (irreversible). Batch enable/archive eligibility excludes these
  remnants, and backend fixtures cover the API<35 retained path and the
  Android 15 archived-vs-retained split (2026-07-15).
- Added multi-select batch package actions with one reviewed, journaled plan.
  Users select two or more packages and apply a single reversible action
  (enable/disable/archive/request-unarchive) at once; the backend binds every
  item to one immutable target/user/kind, issues a shared `batch_id`, verifies
  each package's before-state is a reversible starting point, and continues past
  per-package device failures instead of aborting the rest. The journal renders
  the batch as a unit with a single grouped "Undo batch" control that proves
  reversibility for the whole remaining set before the first inverse runs.
  Backend fixtures cover mixed/duplicate rejection and partial-failure isolation
  (2026-07-15).
- Added Android 15 package archiving as a canonical, user-scoped Apps action.
  Droidsmith now capability-gates archive/unarchive, distinguishes archived
  packages from ordinary retained-data removals, previews exact `pm` commands,
  verifies asynchronous state transitions, journals both directions, and only
  offers undo after a proven round trip. Backend fixtures and the headless
  rendered-route smoke cover archive, listing, and Activity undo (2026-07-15).
- Added guarded file-manager push, mkdir, same-directory rename, and delete
  workflows with exact source/target previews, native one-shot push grants,
  canonical non-interpolated argv, explicit confirmation, durable outcomes,
  post-state verification, permission-denied recovery, and refreshed listings.
  Spaces and non-ASCII device names are covered by backend, isolation, and
  rendered-route tests (2026-07-15).
- Added a native Profiles workspace and schema v2 for repeatable package
  workflows. Users can author ordered canonical actions, bind optional
  device/SDK and owner/current/explicit-user constraints, atomically export
  YAML, and import through a one-shot file grant for a read-only live state and
  exact-command diff. Profile v1 requires an explicit reviewed migration; the
  CLI adds `migrate-v1`, JSON devices/run results, live dry-run state, and
  stable 0/1/2/3 exit semantics (2026-07-15).
- Generated the complete 55-command TypeScript IPC contract and nested DTO graph
  from Rust through a Rust-1.81-compatible Tauri Specta v2 toolchain. Runtime
  registration and generation now share one command list, renderer calls use
  generated wrappers, and the release gate byte-compares deterministic output
  so command or DTO drift fails before shipping (2026-07-15).
- Restored APK-derived Apps labels and raster icons through lazy viewport
  enrichment. Initial package listing performs no APK pulls; requested rows use
  a three-pull backend limit, APK size/timestamp cache invalidation, bounded
  manifest/resource/icon parsing, and drop-guarded temporary cleanup, with
  package-name fallbacks for unsupported vendor resources (2026-07-15).
- Added scrcpy capability negotiation and actionable Mirror failures. Droidsmith
  now caches the scrcpy version and device encoder inventory against the exact
  binary identity, exposes only mutually supported codecs/encoders, preserves a
  bounded stderr tail and classified exit reason, and guides recovery from
  option, disconnect, encoder, permission, and ADB failures (2026-07-15).
- Added a mandatory final debloat safety review before queue planning or device
  mutation. It summarizes the exact package count, names every selected
  unsafe-tier package, describes journal/undo coverage, traps and restores
  keyboard focus, and remains scrollable on constrained displays
  (2026-07-15).
- Added evidence-gated wireless ADB failure guidance. Pair/connect failures now
  probe bounded ADB mDNS health and count likely active tunnel interfaces,
  distinguish VPN-route from local-name resolution interference, and present
  specific remediation with copyable diagnostics that exclude endpoints,
  adapter names, serials, network addresses, and VPN product names
  (2026-07-15).
- Routed optional scrcpy session recordings through a purpose-scoped, one-shot
  native `.mp4`/`.mkv` save grant. Mirror presets no longer accept or retain
  renderer-authored host paths, and the isolation policy rejects the legacy
  `record_path` payload before it can reach Rust (2026-07-15).
- Added privacy-gated Android bugreport capture in Diagnostics. A dedicated
  warning names likely `dumpsys`, `dumpstate`, Logcat, screenshot, account,
  network, app, and device-state contents before a one-shot native ZIP save.
  Captures bind to an immutable target, enforce a 15-minute timeout and 1 GiB
  live budget, clean partial output on failure/cancellation, validate only ZIP
  structure, and emit a redacted sidecar with absolute time, hashed target/build
  identity, tool versions, size, and SHA-256. Reports are never inspected for
  redaction, opened, attached, or uploaded (IMP-58, 2026-07-15).
- Added one dated Platform Tools compatibility policy for runtime assessment,
  official archive pins, fetch scripts, diagnostics, and release validation.
  Droidsmith recommends 37.0.0, warns below the 36.0.2 reliability floor,
  blocks only the documented 36.0.1 Canary defect, never blocks unrecognized
  newer versions, and surfaces the selected ADB path/version/status with policy
  rationale and source (IMP-57, 2026-07-15).
- Added a non-elevated, read-only host connection doctor to Onboarding,
  Devices, and Diagnostics. It reports missing/unrunnable/versioned ADB,
  anonymized unauthorized/offline/no-permissions states, bounded Windows USB
  and ADB-interface evidence, Linux group/udev prerequisites, and redacted
  server overrides/version conflicts with official remediation links. It never
  installs drivers, edits rules, restarts ADB, or records device/key material
  (IMP-56, 2026-07-15).
- Replaced size-based legacy ADB backup claims with an evidence-based package
  export contract. Base/split APK ZIP export is now the default and includes a
  versioned manifest with per-artifact hashes and hashed device/build identity.
  The Advanced legacy-data path preflights target SDK, debuggable, and
  `allowBackup` evidence, blocks known exclusions, strictly validates an
  uncompressed `.ab` TAR before packaging, and explicitly avoids claims of
  completeness or restorability (IMP-55, 2026-07-15).
- Added verified per-user system-app recovery. Uninstall captures exact Android
  user, system provenance, and enabled state before mutation, then offers
  Activity undo only when `pm list packages -u` proves the preinstalled APK was
  retained. Undo runs `install-existing`, restores the prior enabled state, and
  verifies it; `/data/app`, missing-retention, and unsupported OEM paths remain
  explicitly irreversible (IMP-54, 2026-07-15).
- Added portable, versioned pre-change recovery baselines for Apps, Debloat, and
  the headless CLI. Exports atomically record a hashed device identity, build
  fingerprint, Android user, pack revision, package state, requested actions,
  and only truthful undo plans without raw serials or package metadata. Import
  is read-only until users review OTA/build drift and stable per-package skip
  reasons, then explicitly apply canonical journaled recovery actions (IMP-45,
  2026-07-15).

- Added cancellable APK/APKS/XAPK/APKM installation from the Apps route. Split
  archives are path- and size-bounded, staged through atomic Android
  `install-create`/`install-write`/`install-commit` sessions, and guarded by
  cleanup that abandons every uncommitted session. Known downgrade,
  low-target-SDK, split, ABI, storage, signature, and policy failures now show
  cause/remedy guidance; `-d` and `--bypass-low-target-sdk-block` retries require
  a separate confirmation and are durably recorded without storing host paths.
- Added a local-only Diagnostics center that previews and saves bounded,
  redacted JSON support bundles containing app/tool/OS versions, ADB health,
  anonymized devices, recent failed operations, and crash excerpts. Raw device
  serials, network addresses, pairing codes, secret-like values, and host paths
  are excluded by backend tests; an explicit wipe removes disposable crash and
  host-operation logs while preserving device recovery journals.

### Changed

- Reworked keyboard and assistive-technology behavior across the shell: route
  changes now move and announce focus, the command palette follows the
  combobox/listbox active-option pattern, device tables retain native semantics,
  applying actions remain modal, and Logcat batches live announcements. Muted
  text meets WCAG AA on elevated surfaces, while document language/direction and
  date/number formatting follow the selected locale (IMP-48, 2026-07-15).
- Added fixed-seed property coverage for ADB/OEM parsers, YAML schemas, and
  JSONL journals; a feature-gated fake Android tool now proves exact process
  I/O, bounded backpressure, target drift, disk failures, and descendant-tree
  cancellation. External Android tools run in hidden isolated process groups,
  and seeded optional fuzz targets cover all three untrusted formats (IMP-47).
- Made `npm run release:check` the authoritative local release gate. It now
  composes frontend and Rust formatting/lint/type/test checks, headless route
  smoke, npm/RustSec audits, Cargo license/source/ban and exact-duplicate
  policy, typed validation of all pack/quirk/profile YAML, version/resource
  parity, and production bundle smoke. Temporary audit and duplicate exceptions
  require an owner, rationale, and absolute expiry (IMP-46).
- Reduced Debloat queue package-list work from two full listings per package to
  one baseline listing per batch. Successful rows use the backend's targeted,
  journaled before/after state probes; rows lacking an after-state share at most
  one final recovery listing, and unknown verification is surfaced explicitly
  in English and Russian (IMP-34).

### Security

- Classified every live ADB target as USB, paired TLS Wi-Fi, explicit legacy
  TCP, or unknown TCP from backend provenance rather than its address. Legacy
  and unknown transports now fail closed at privileged Rust boundaries until
  the selected connection receives an explicit warning acknowledgement; the
  acknowledgement resets on selection/provenance changes and is persisted in
  mutation/install audit records (IMP-44).
- Replaced renderer-authored host paths with backend-owned native dialogs and
  expiring, purpose-scoped, one-shot grants for diagnostics and Logcat exports,
  backups, screenshots, pulls, pushes, package installs, and APK extraction.
  Removed renderer dialog permission and the JavaScript dialog dependency;
  install overrides receive a fresh single-use retry grant. Direct use, reuse,
  wrong-purpose use, expiration, and read/write intent are covered by tests.
- Routed host-side pulls, backups, screenshots, and APK extraction through a
  shared sibling-staging boundary. Outputs now preserve an existing destination
  until format/size validation, SHA-256 calculation, disk flush, and atomic
  replacement succeed; failed or cancelled operations remove their partials,
  and the renderer receives the canonical final path, size, and hash (IMP-43).
- Enabled Tauri's isolation pattern with a dependency-free IPC policy that
  classifies every registered Rust command and rejects malformed, unexpected,
  traversal-bearing, or unclassified sensitive payloads before they reach the
  backend. Removed all renderer inline styles, tightened production CSP to
  self-hosted scripts/styles with no unsafe inline/eval source, and added a
  policy regression gate. Rust host/device path validation now independently
  rejects traversal and control characters (IMP-42).
- Bound every device-scoped IPC and CLI operation to an immutable target
  carrying ADB transport ID, connection generation, model, and build
  fingerprint (IMP-39). Targets are revalidated immediately before execution,
  duplicate serials use `adb -t`, stale/reconnected targets fail closed, and
  package mutations recheck the explicit Android user instead of falling back
  to owner user 0.
- Hardened argument-injection boundaries in ADB/scrcpy IPC. Wireless hosts,
  device (remote) paths for `pull`/`push`/`ls`/APK-extract, and scrcpy recording
  paths are now rejected when they begin with `-`, so a renderer-supplied value
  can no longer reach `adb`/`scrcpy` as an option flag instead of a positional
  argument. Remote paths must also be absolute and non-empty. Directory listings
  now request `df -k` for deterministic free-space units.
- Remediated all RustSec advisories: replaced the unsound/unmaintained
  `serde_yml`/`libyml` YAML stack with the maintained `serde_yaml_ng`, and
  updated transitive `quick-xml` (0.39.4 → 0.41.0, via `plist` 1.10) and
  `anyhow` (1.0.102 → 1.0.103) to clear RUSTSEC-2025-0067/0068,
  RUSTSEC-2026-0190, and the high-severity RUSTSEC-2026-0194/0195.
- Added a local Rust dependency gate: `npm run security:audit` now runs
  `cargo audit --deny warnings` alongside `npm audit`. Unfixable transitive
  exceptions (Tauri's Linux GTK3 webview stack) are narrowly documented in
  `src-tauri/.cargo/audit.toml`.
- Hardened IPC trust boundary: `push_file`, `pull_file`, `install_apk`,
  `extract_apk`, and `take_screenshot` now require absolute local paths,
  preventing arbitrary host filesystem access from a compromised renderer.
- Added package name validation to `backup_package`, `list_permissions`, and
  `set_permission`. Added permission identifier validation to `set_permission`.
- Added fastboot variable key validation to `fastboot_getvar`; replaced the
  unbounded `Command::output()` retry with a timeout-guarded call.
- Removed the user-controllable `quirks_path` option from `explain_failure`;
  the command now exclusively loads bundled quirks from the resource directory.
- `valid_serial` now rejects leading-hyphen values to prevent ADB flag confusion.

### Accessibility

- Localized the entire Devices control surface (virtual remote, screenshot,
  display tuning, process manager, file manager, network inspector) — roughly 80
  previously hard-coded English strings now flow through i18n with English and
  Russian parity (IMP-33). Process-table sort headers are now real keyboard-
  focusable buttons with `aria-sort` instead of click-only `<th>` cells.
- Modal dialogs (onboarding tour, command palette, action-confirmation) now trap
  keyboard focus, move focus into the dialog on open, restore focus to the
  trigger on close, and close on Escape (IMP-31). The onboarding overlay gained
  proper `role="dialog"`/`aria-modal`/`aria-labelledby` semantics and a working
  document-level Escape handler. A shared `useFocusTrap` hook backs all three.
- Permission toggles now expose a descriptive `aria-label` ("Grant/Revoke
  <permission>") and `aria-pressed` state, so screen-reader users know which
  permission each Granted/Denied control affects (IMP-35).

### Added

- Added one cancellable app-wide device lifecycle watcher shared by Devices,
  Apps, Debloat, Mirror, Console, and Logcat. Hot-plug snapshots preserve
  immutable connection generations, while ADB health reports client/server,
  USB and mDNS backends, discovery status, and Android 17 Wi-Fi 2.0 capability.
  The Devices route now provides a localized, focus-trapped recovery review for
  `adb kill-server`, `adb start-server`, and `adb reconnect offline`, with live
  progress, cancellation, copyable diagnostics, and crash-consistent host
  operation records.
- Moved install, push/pull, APK extraction, package backup, and read-only shell
  subprocesses onto a shared background runner with structured output/progress
  channels and operation IDs. Cancellation now kills and reaps the actual child;
  Console, Apps backup, and Devices file pulls expose live status and cancel
  controls. Logcat now uses one cancellable incremental stream with a 2,000-line
  ring buffer, UTF-8-safe chunks, reconnect markers, local filtering, native
  export, and route-unmount guards instead of replacing a 200-line snapshot
  every two seconds.
- Routed every current device mutation through the crash-consistent audited
  executor. Package/profile/debloat actions, permission grant/revoke, virtual
  controls, display settings, and mutating Console commands now record an
  immutable target/user, canonical argv, incident ID, confirmation source,
  before/after state, outcome, and bounded/redacted output. Unknown Console
  commands fail into an explicit review dialog; read-only shell calls remain
  direct, permission changes are undoable through the same executor, and CLI
  dry-runs still stop before state capture or journal creation.
- Enforced debloat-pack compatibility and provenance end to end (IMP-41).
  Every pack now has a stable ID, monotonic revision, source/license record,
  OEM/model/build/API/user constraints, recursive dependency expansion, and
  per-package ready/missing/unsupported assessment. The picker shows live
  checks before selection; unknown or mismatched devices require an explicit
  override recorded in the crash-consistent journal. Underscore templates are
  linted in source but excluded from runtime resources and listings.
- Made GUI, undo, and CLI package-action journals crash-consistent (IMP-40):
  each operation syncs a write-ahead intent before touching the device, appends
  a terminal success/failure, repairs truncated JSONL tails, and recovers
  interrupted operations as visible “outcome unknown” records. Interrupted
  undos remain conservatively locked so a restart cannot double-apply them.
- Debloat now surfaces bundled packs that fail to load instead of silently
  skipping them. `list_packs` returns healthy packs plus a per-file error list
  with stable codes (`pack_read`/`pack_parse`/`pack_validate`) and copyable
  messages; the Debloat pack picker shows a warning panel for broken files while
  still listing the healthy packs. A `cargo test` bundle-contract test loads
  every shipped pack and fails if any is corrupt.
- Single-instance enforcement (`tauri-plugin-single-instance`): a second launch
  focuses the existing window and exits instead of spawning a rival process, so
  two Droidsmith windows can no longer race over the same `adb` server.
- Explicit Android user targeting for every package workflow. Package listing,
  planning, apply, undo, debloat sweeps, and CLI profiles now carry an explicit
  `pm --user <id>` (previously destructive actions silently hard-coded user 0
  while claiming "current user"). A new `list_users` command enumerates users
  and resolves the foreground user via `am get-current-user`; Apps and Debloat
  show a user selector when a device has more than one user, defaulting to the
  foreground user. `enable`/`clear`/`force-stop` now pass `--user` too, so a
  work-profile (user 10) action can no longer mutate the owner. Journal undo
  targets the exact user the original action changed. Profile YAML gains an
  optional per-action `user:` field (defaults to 0).

### Fixed

- The console now clears its scrollback and command-recall history when you
  switch devices, so output and up-arrow recall no longer mix commands from a
  previous device's shell.
- Localized the last hard-coded Devices tooltip and gave network-inspector rows
  a stable composite key instead of a bare array index.
- Fixed the network inspector showing a bogus `state = "0"` for stateless
  (UDP / netstat-style) sockets — the parser fell back to the numeric recv-q
  column when no TCP state was present; it now reports `UNCONN`.
- Made the crash-log panic hook idempotent (installs once) so a panic is never
  recorded multiple times if diagnostics setup runs more than once.
- Fixed cross-device state bleed in the Devices route. The process list, file
  listing, and network sockets are now reset when you switch devices (the
  controls panel is keyed by serial), so device A's data can no longer be shown
  — or acted on — while device B is selected.
- Mirror now resets its tracked session when you switch devices, so a running
  session for one device no longer blocks launching on another.
- Process manager, file manager, network inspector, and the permissions panel
  now surface the actual error when a device query fails instead of silently
  falling back to an empty state (common on devices that restrict `ss`/`ps` or
  reject a `pm grant`). Fastboot variable queries likewise show a clear message
  when the query runs but returns nothing.
- Fixed fastboot variable values not displaying after the getvar backend fix —
  the frontend still expected a `key: value` line and dropped the now-cleaned
  value; it now uses the returned value directly.
- Logcat now tracks and clears its poll timer on stop/unmount and resets the
  fetch-error badge when tailing stops or restarts, preventing a stale error
  indicator and a lingering post-stop poll.
- Fixed a journal race (IMP-30): two concurrent `apply_action` (or `journal_undo`)
  calls on the same device each opened an independent journal, derived the same
  next id, and could append duplicate ids with stale undo state. A per-device
  process lock now serializes the open→record cycle (and holds across an undo's
  reversibility check + inverse ADB call so an entry cannot be double-undone). A
  16-thread test asserts unique ids.
- Backups now warn when `adb backup` produced an APK-only, header-only `.ab`.
  Apps targeting Android 12 (API 31) or newer opt out of the deprecated
  `adb backup`, yielding a ~header-sized artifact that previously showed as a
  green "saved" success. The backend now classifies non-empty artifacts at or
  below 512 bytes as header-only, and the UI shows an explicit "contains no app
  data / APK-only" warning distinct from the empty-artifact case.
- Fixed screenshot capture and remote file pull, which always failed because the
  renderer passed relative host paths that the backend (correctly) rejects. Both
  now obtain their destination from the native save dialog, so the renderer never
  dictates an arbitrary host path. The screenshot device-side temp file is now
  unique per capture and always removed — even when the pull fails — so a partial
  capture never leaks onto `/sdcard`.
- Fixed `fastboot getvar` returning an empty value. fastboot writes successful
  variable values to stderr while exiting 0, but the shared runner returned
  stdout only and the "retry on error" path never triggered (success was never
  an error). Introduced a typed `ProcessOutput` (stdout + stderr + exit code +
  timeout state) captured in one execution; `getvar` now parses the value from
  stderr (stdout fallback) without a blind retry, preserves both streams on
  failure, and reports timeouts explicitly. Fake-fastboot tests cover success,
  error, and timeout.
- Fixed Vite `envPrefix` literal asterisk — `TAURI_ENV_*` was treated as a
  literal string, not a glob. Changed to `TAURI_ENV_` for correct prefix matching.
- Fixed `index.html` body using `bg-zinc-950` instead of the app's actual
  `bg-[#08090d]` theme color, preventing a flash of wrong background on load.
  Changed `color-scheme` meta from `dark light` to `dark` (dark-only app).
- Fixed Logcat polling stale closure — `fetchLogcat`'s recursive `setTimeout`
  now reads `selectedSerial` and `tagFilter` from refs instead of capturing
  them in the closure, so changes take effect without restarting the tail.
  Logcat fetch errors are now surfaced as a visible badge instead of swallowed.
- Fixed Console history key collision — entries now use a monotonic counter
  instead of `Date.now()` timestamps that could collide on rapid submissions.
- Fixed `loadDevices` in all six device-dependent routes (Apps, Debloat, Mirror,
  Console, Logcat, Fastboot) — `selectedSerial` in the `useCallback` dependency
  array caused unnecessary device re-scans on every serial change.
- Fixed `Card` component `className` override — `className || "p-4"` lost the
  default padding when any truthy className was passed. Changed to `??`.
- Fixed `is_iso_date` parser false-positive — now requires `YYYY-MM-DD` pattern
  instead of matching any 8+ char string containing a hyphen.
- `valid_package_name` now accepts hyphens, matching real OEM package names.
- Finished scrcpy sessions are now removed from the global session map during
  reap, preventing unbounded memory growth over long app sessions.

### Added

- Added `Ctrl+K` / `Cmd+K` global keyboard shortcut for the command palette.
- Added keyboard accessibility to device table rows: `role="button"`,
  `tabIndex`, `onKeyDown` (Enter/Space), focus-visible ring, and a title tooltip
  on non-actionable rows explaining authorization is required.
- Added `Escape` key handler on the onboarding modal overlay.
- Added `logcat.fetchFailed` i18n key (EN + RU) for the new error badge.

### Changed

- Replaced five identical local `Th`/`Td` component definitions with shared
  `TableHeaderCell`/`TableCell` imports from `common.tsx`, removing ~33 lines
  of duplicated code across Devices, Apps, Debloat, Wireless, and Fastboot.
- Removed unnecessary `staticlib` from `Cargo.toml` crate-type.
- Removed dead `stacked` prop from `ShellActions`.
- Deleted dead `placeholders.tsx`.
- Renamed `Apps.test.tsx` → `appsJournal.test.ts` to match its actual content.

- Added explicit v1 compatibility boundaries for pack, quirk, and profile
  schemas, including migration-path validation errors and a versioned bundled
  HyperOS quirk document.
- Added `npm run ui:smoke`, a Playwright-rendered route gate with mocked Tauri
  IPC for sidebar navigation, command palette focus, Apps overlays, Debloat
  queue states, and desktop/mobile overflow checks.
- Added a persisted shell language selector with validated EN/RU storage,
  fallback coverage, and contributor guidance for locale parity tests.
- Added `npm run release:smoke`, a local production-bundle smoke gate that
  builds the frontend/Tauri bundle, checks bundled resources, validates
  third-party notices, and fails on missing platform artifacts.
- Added supervised scrcpy sessions with per-device Mirror presets, keyboard
  mode, screen-off, stay-awake, touch display, recording, status polling, and
  stop controls.
- Added transcript-backed parsers for remote file listings, process snapshots,
  network sockets, and fastboot devices with visible degraded rows when OEM
  output cannot be parsed cleanly.
- Added path-safe package backups with a required save destination, modern
  Android `adb backup` limitations, artifact path/size reporting, raw ADB
  output, and empty-backup warnings.
- Added debloat queue progress rows with current-package visibility,
  cancel-after-current, retry-failed, journal ID reporting, and before/after
  disable verification for each selected package.
- Added the Apps per-device action journal with reversible disable/enable undo,
  visible irreversible/already-undone states, refreshed package state after
  undo, and renderer coverage for journal row status rules.
- Added a local `security:audit` gate and patched the Vite / transitive
  `js-yaml` npm advisories so `npm audit --audit-level=moderate` is clean.
- Bundled `packs/` and `quirks/` as Tauri resources, added a local
  resource-contract gate, and made `explain_failure` load bundled quirks when
  callers do not pass a manual path.
- **R-070** i18n now initializes in the renderer and wires English/Russian
  translations through the shell, navigation, command palette, onboarding, and
  shipped route surfaces, with locale parity coverage in Vitest.
- **R-020 / F-NEW-09** Apps route package rows now show APK-derived labels
  and displayable PNG/WebP/JPEG icons from a pure-Rust APK metadata parser,
  with package-name fallbacks when vendor APK resources cannot be resolved.

### Docs

- Reconciled README, development notes, and research summary with the shipped
  route surface, current sidecar/signing blockers, and deferred plugin work.
- Consolidated planning docs: active items remain in `ROADMAP.md`, shipped
  roadmap history is summarized in `COMPLETED.md`, and research context is
  summarized in `RESEARCH_REPORT.md` with the previous research files archived
  under `docs/archive/research/`.

## [0.1.0] — 2026-05-25

First tagged release. Pre-built Windows installer + portable `.exe`
attached to the GitHub release. Includes everything in the Phase 1+
end-to-end slice below plus the Phase 0 scaffold (R-001..R-002, IMP-01..IMP-07).

### 2026-05-25 — Phase 1+ end-to-end slice

The thin slice from device detection through action queue + journal,
plus the pack / quirks framework and the headless CLI. 67 Rust tests
and 5 frontend tests, all gates green.

**ADB domain layer (R-011, R-012):**

- `adb/` module: `resolver`, `transport`, `device`, `packages`, `actions`.
- `AdbTransport` trait + `ShellTransport` (real) + `MockTransport` (tests).
- `parse_devices_long` handles daemon-startup chatter, no-permissions
  multi-word state, wireless serial classification.
- `list_devices` Tauri command + live Devices route in the renderer
  with refresh button, empty/error/no-tauri states.

**Package enumeration (R-020, R-021):**

- Two-pass `pm list packages` union (-e/-d, both with -f/-U/-i).
- `AppPackage{package, enabled, system, apk_path, uid, installer}`.
- `PackageFilter::{All, User, System, Enabled, Disabled}` applied
  after the union.
- System detection by APK partition prefix
  (`/system|/product|/vendor|/apex|/system_ext`).
- `list_packages(serial, filter)` Tauri command.

**Action layer + undo journal (R-022, R-026.5):**

- `actions::{plan, apply}` with `ActionKind::{Disable, Enable,
  UninstallForUser, ClearData, ForceStop}`. Two-step preview→apply.
- In-band `pm` "Failure …" / "Error: …" markers recognised; lifted to
  `TransportError::Exit`.
- `journal::Journal` writes JSON-Lines per device under
  `<app_data_dir>/journal/<serial>.jsonl` (HGFS-safe, no SQLite).
- `record_undo` links both sides (`undone_by` / `undoes`).
- Filename-safe serial scrubbing for wireless devices.
- Corrupt-line tolerance: malformed JSONL rows are logged and skipped.
- Tauri commands: `plan_action`, `apply_action`, `journal_list`,
  `journal_undo`.

**Pack framework (R-030, R-031):**

- `packs::{Pack, PackTargets, PackEntry, RemovalLevel}` with UAD-NG-aligned
  semantics so R-036 imports line up cleanly.
- `packs::lint` validates name/description/version, package id
  validity, duplicate detection, depends_on/needed_by id validity,
  android_min ≤ android_max.
- `droidsmith-pack-lint` binary — exit 0 clean, 1 issues, 2 usage.
- `packs/_example.yaml` seeds the contributor copy.
- New dep: `serde_yml 0.0.12`.

**Vendor-quirks engine (R-034):**

- `quirks::{Quirk, QuirkMatch, Mitigation, DeviceContext}` with
  AND-across-fields / OR-within-field substring matching.
- Case-insensitive ROM substring matching so "MIUI 14 — HyperOS Preview"
  matches `rom: ["hyperos"]`.
- `explain_failure` Tauri command.
- `quirks/hyperos.yaml` seed rule: the documented Xiaomi
  `pm disable-user` block, with the `pm uninstall --user 0` workaround
  as a `try_alternative_action` mitigation.

**Headless CLI + profiles (R-060, R-061):**

- `droidsmith-cli` binary with `devices` and `run <profile.yaml> --device
  <serial> [--dry-run|--apply]` subcommands. Hand-rolled argv parser; no
  CLI-framework dependency. Exit codes 0/1/2/3.
- `profile::{Profile, ProfileDeviceMatch, ProfileAction}` YAML schema,
  serial-agnostic (serial bound at run time via `requests_for`).
- Profile lint mirrors the pack lint shape.

**Shared time module:**

- `crate::time::{format_utc_rfc3339, iso_utc_now}` extracted so the
  CLI bin, journal layer, and diagnostics module all stamp identically.
  Hand-rolled Howard Hinnant date algorithm — no `chrono`/`time` dep.

**Sidecar fetch (R-010 scaffolding):**

- `scripts/fetch-platform-tools.ps1` (Windows) and `.sh` (POSIX) —
  download, SHA-256 verify (placeholders pinned), extract, stage as
  per-target-triple sidecars under `src-tauri/binaries/`.
- AdbWinApi DLL handling on Windows.
- Binaries are NOT committed; `src-tauri/binaries/` is gitignored.
- Final `bundle.externalBin` wiring deferred to R-006.

**Per-pane placeholder routes (IMP-05):**

- `src/lib/tauri.ts` exposes typed wrappers around `invoke()` plus
  `inTauri()` for graceful "running in plain Vite" fallback.
- `src/routes/Devices.tsx` is fully live — calls `list_devices`,
  renders empty/error/no-adb states, refresh button.
- `src/routes/placeholders.tsx` has dedicated components for Apps,
  Debloat, Mirror, Console, Logcat, Fastboot. Each lists planned
  behaviour + the Rust commands already exposed (READY / TODO badges).
- `src/routes/common.tsx` factors `PaneHeader`, `PlaceholderBody`, `Card`.
- App.tsx is now a thin shell + router; sidebar carries a heartbeat
  summary at the foot.

**POSIX dev-mirror (IMP-07):**

- `scripts/dev-mirror.sh` — rsync-based, sentinel-guarded,
  `--watch`/`--reverse`/`--force`/`--dest` flags. Defaults to
  `~/.droidsmith-mirror`. Watch mode polls at 1s using `find -printf`
  for a coarse mtime fingerprint — dependency-free.

### Added

- **R-003** GitHub Actions CI matrix (`.github/workflows/ci.yml`) — Rust on Ubuntu/Windows/macOS running `cargo fmt --check`, `cargo check`, `cargo clippy --all-targets -D warnings`, `cargo test --all-targets`, plus a separate frontend job (typecheck, lint, prettier, vitest).
- **R-004** Lint enforcement: `[lints.clippy] all = warn`, `[lints.rust] unsafe_code = deny` with a single allowed FFI site in `diagnostics::show_native`, `src-tauri/rustfmt.toml` pinned.
- **R-005** Contributor surface: [`CONTRIBUTING.md`](CONTRIBUTING.md), [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) (Contributor Covenant 2.1), [`SECURITY.md`](SECURITY.md), [`.github/ISSUE_TEMPLATE/bug.md`](.github/ISSUE_TEMPLATE/bug.md), [`.github/ISSUE_TEMPLATE/feature.md`](.github/ISSUE_TEMPLATE/feature.md), [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md).
- **R-007 floor / F-NEW-10** File-only rotating crash log (1MB × 5 backups) at the OS config directory via [`diagnostics::install_panic_hook`](src-tauri/src/diagnostics.rs). Opt-in upload still deferred to R-073.
- **LICENSE-THIRD-PARTY.md** seeded with placeholders for `adb` / `fastboot` / `scrcpy` / UAD-NG list.
- Vitest harness ([`vitest.config.ts`](vitest.config.ts), [`src/App.test.tsx`](src/App.test.tsx)) with a milestone-mapping smoke test that fails if `App.tsx` nav drifts from the roadmap.

### Changed

- **IMP-01** [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) no longer `.expect()`s the Tauri builder. Errors route through [`diagnostics::fatal_dialog`](src-tauri/src/diagnostics.rs) — native message box on each OS, `eprintln!` fallback in headless contexts.
- **IMP-02** [`src-tauri/capabilities/default.json`](src-tauri/capabilities/default.json) scoped from `shell:default` to `shell:allow-execute` with three named sidecars (`adb`, `fastboot`, `scrcpy`) and regex arg validators. Renderer cannot spawn arbitrary commands.
- **IMP-03** [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml): `thiserror 1 → 2`, `which 6 → 8`, added `os_info 3`. MSRV bumped 1.77 → 1.81 so `PanicHookInfo` lints cleanly.
- **IMP-04** [`commands::heartbeat`](src-tauri/src/commands.rs) now returns `{version, os{family,version,arch}, tauri_version, rust_version, app_data_dir, adb{path,source,version}}`. Frontend renders as a 7-row key/value grid.
- **IMP-05 (partial)** [`src/App.tsx`](src/App.tsx) nav stubs are `<button>`s with `aria-current`, hover/active states, and a roadmap-milestone badge. Active item drives the main pane header. Full placeholder routes still pending.
- **IMP-06** [`src-tauri/src/adb.rs`](src-tauri/src/adb.rs) split into typed `AdbResolution{path, source: ResolveSource, version}`. Honours `$ANDROID_HOME` / `$ANDROID_SDK_ROOT`, splits Android Studio default per-OS, Linux distro paths gated. Adds 2s timeout on `adb version` probes.

### Fixed

- **A1 — race / pipe-buffer deadlock in `adb version` probe.** [`adb::probe_version`](src-tauri/src/adb.rs) previously polled `try_wait` for up to 2 s and only read stdout afterward. A verbose `adb version` could fill the OS pipe buffer (~64 KB on Windows) and block the child waiting for a reader, producing a phantom timeout. Now reads on a worker thread concurrently with the wait loop.
- **A2 — false promise of crash log on Tauri builder error.** [`crate::run`](src-tauri/src/lib.rs) showed a dialog saying "a crash log was written" but the panic hook only fires on actual `panic!()`. Builder `Err` returns slipped past silently. Now writes through the new [`diagnostics::log_fatal`](src-tauri/src/diagnostics.rs) helper and the dialog quotes the real log path.
- **A3 — `iso_now()` was lying.** The function name implied ISO-8601 but returned raw epoch seconds. Now emits proper `YYYY-MM-DDTHH:MM:SSZ` UTC via Howard Hinnant's date algorithm; tests cover epoch, leap-year, and post-century-leap anchors.
- **A4 — panic hook silently disabled on minimal environments.** [`fallback_log_dir`](src-tauri/src/diagnostics.rs) previously returned `Option<PathBuf>` and skipped installing the hook on any container without `APPDATA` / `XDG_CONFIG_HOME` / `HOME`. Now always returns a path (falling back to `std::env::temp_dir().join("Droidsmith")`), guaranteeing the hook installs everywhere.
- **A5 — silent log rotate failures.** Rotation in [`diagnostics::rotate_if_needed`](src-tauri/src/diagnostics.rs) used to swallow `rename` errors, which would lose subsequent crash records if a file lock wedged the rename. Now logs the failure to stderr while preserving the existing crash log.
- **A8 — `adb` missed on macOS Homebrew installs.** GUI-launched apps don't inherit shell `PATH`, so `brew install android-platform-tools` was invisible. The resolver now also tries `/opt/homebrew/bin/adb` (Apple Silicon) and `/usr/local/bin/adb` (Intel) under a new `ResolveSource::Homebrew`.
- **A8b — `adb` missed on Debian/Ubuntu `apt install adb`.** The Linux candidate list now also tries `/usr/bin/adb`.
- **A9 — empty env vars produced bogus candidate paths.** On Windows `set ANDROID_HOME=` leaves the var defined-but-empty; the resolver was emitting `/platform-tools/adb` candidates. Now centralized in `read_env_path` which treats empty as unset.
- **A10 — test mutated process-global env.** The previous `candidate_paths_includes_android_home_when_set` test set/cleared `ANDROID_HOME`, which is unsafe under `cargo test` parallelism. Refactored `candidate_paths` to take a `ResolverEnv` struct so tests pass synthetic env without touching process state.
- **B1 — AppleScript escape was incomplete.** Newlines and tabs in titles/messages would produce malformed `osascript` invocations. Now escapes `\\`, `"`, `\n`, `\r`, `\t`.
- **B6 — `dev-mirror.ps1` could wipe an unrelated folder.** `robocopy /MIR` was happy to delete arbitrary destination contents. Added a `.droidsmith-mirror` sentinel: if the destination exists, is non-empty, and lacks the sentinel, the script refuses and points to `-Force`.
- **C1 — heartbeat error had no retry.** A failed `invoke("heartbeat")` left the user staring at a red string with no next action. Now offers a Retry button, a typed `LoadState` machine (`loading | ok | error`), `aria-live` for the panel, and `role="alert"` on the error message.
- **C6 — invisible keyboard focus.** Tailwind strips the browser focus ring; the nav buttons had no replacement. Added `focus-visible:ring-2 focus-visible:ring-anvil-300`.
- **C7 — Windows paths broke mid-word in the heartbeat panel.** Replaced `break-all` with `break-words` plus a `​` injector at `/` and `\` so paths wrap at segment boundaries.
- **D1 — `os_info::get()` re-read `/etc/os-release` (or the Windows registry) on every heartbeat.** Now cached behind `OnceLock`.
- **D2 — `adb version` probe ran on every heartbeat.** The whole `AdbResolution` is now cached behind `OnceLock` for the process lifetime.
- **E6 — smoke test duplicated the milestone list.** Tests now `import { NAV_ITEMS } from "./App"` and assert structural invariants (count, milestone format, descriptions, label uniqueness, ascending order).
- **TS-config — `tsc -b` was rejecting the project**. `tsconfig.json` referenced `tsconfig.node.json` but the latter wasn't composite-compliant. Consolidated into a single `tsconfig.json` covering `src` + all root configs, with `@types/node` added so `vite.config.ts` typechecks.
- **Vitest plugin type clash** — using `@vitejs/plugin-react` in `vitest.config.ts` caused a duplicate `Plugin<any>` clash between project vite 6 and vitest's bundled vite 5. Switched to esbuild's built-in `jsx: "automatic"` (we don't render in tests, only resolve `.tsx` modules).
- **Lint flat-config script** — `eslint src --ext .ts,.tsx` is the v8-era syntax; flat-config wants just `eslint .`. Updated, added matching `format:check` / `format:write` scripts.

### Removed

- `tsconfig.node.json` (consolidated into root `tsconfig.json`).
- Dead `invalidate_cache_for_tests()` stub — tests bypass the cache via the new pure `resolve()` entry point.
- `jsdom` devDep — test harness runs in Node now.

### Tooling

- `.github/dependabot.yml`: weekly cargo + npm sweeps, monthly GitHub Actions, with grouping rules so the Tauri ecosystem ships one combined PR per cycle instead of a flood.
- CI runners switched from `ubuntu-22.04` → `ubuntu-latest`, `macos-14` → `macos-latest`. Added `--locked` to cargo invocations so dependency drift on CI is caught early. Added `permissions: contents: read` and per-job timeouts.

## [0.0.1] — 2026-05-25 — Scaffold complete

Foundational milestone: repo created, planning surface in place, Tauri shell
builds cleanly on Windows. Nothing user-facing works yet; this is the start
line for feature work.

### Added

- **R-001** Repository scaffolding: [README.md](README.md), [ROADMAP.md](ROADMAP.md), planning research, [LICENSE](LICENSE), [.gitignore](.gitignore) — commit `0a82c63`.
- **R-002** Tauri 2 + React + TS + Vite + Tailwind scaffold — commit `4f7b584`.
  - Rust backend with `shell` + `dialog` plugins, `heartbeat` IPC, `adb::locate_adb` helper across Win/macOS/Linux paths.
  - React 18 + TypeScript frontend with sidebar shell and live heartbeat panel.
  - Tailwind 3 with custom `anvil` palette, ESLint flat config, Prettier, EditorConfig.
  - Full per-OS icon set via `cargo tauri icon` (Win/macOS/Linux/iOS/Android).
  - HGFS dev-mirror script ([`scripts/dev-mirror.ps1`](scripts/dev-mirror.ps1)) with `-Watch` / `-Reverse` modes for VMware Shared Folders development.
  - `dist/index.html` placeholder so `tauri::generate_context!` validates before the first `npm run build`.
  - [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) covering native and HGFS dev loops.
- **Research deep-dive** — commit `dd59888` — evidence-grounded feature and improvement plan, now summarized in [RESEARCH_REPORT.md](RESEARCH_REPORT.md) and archived under `docs/archive/research/`, with prioritized roadmap, IMP-01..IMP-07, F-NEW-01..F-NEW-10. Drives this Changelog and the integrated ROADMAP.

### Verified

- `cargo check` clean on Windows in `C:\tmp\Droidsmith` mirror.

### Known gaps (carried into Phase 0)

- `shell:default` capability is unscoped (IMP-02).
- `.expect(...)` panic on Tauri init failure (IMP-01).
- No CI matrix yet (R-003).
- Direct deps `thiserror 1`, `which 6` are stale (IMP-03).
