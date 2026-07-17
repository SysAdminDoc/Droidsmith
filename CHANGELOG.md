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
