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

### Docs

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
