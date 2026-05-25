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

- _(none yet)_

## [0.0.1] — 2026-05-25 — Scaffold complete

Foundational milestone: repo created, planning surface in place, Tauri shell
builds cleanly on Windows. Nothing user-facing works yet; this is the start
line for feature work.

### Added

- **R-001** Repository scaffolding: [README.md](README.md), [ROADMAP.md](ROADMAP.md), [RESEARCH_FEATURE_PLAN.md](RESEARCH_FEATURE_PLAN.md), [LICENSE](LICENSE), [.gitignore](.gitignore) — commit `0a82c63`.
- **R-002** Tauri 2 + React + TS + Vite + Tailwind scaffold — commit `4f7b584`.
  - Rust backend with `shell` + `dialog` plugins, `heartbeat` IPC, `adb::locate_adb` helper across Win/macOS/Linux paths.
  - React 18 + TypeScript frontend with sidebar shell and live heartbeat panel.
  - Tailwind 3 with custom `anvil` palette, ESLint flat config, Prettier, EditorConfig.
  - Full per-OS icon set via `cargo tauri icon` (Win/macOS/Linux/iOS/Android).
  - HGFS dev-mirror script ([`scripts/dev-mirror.ps1`](scripts/dev-mirror.ps1)) with `-Watch` / `-Reverse` modes for VMware Shared Folders development.
  - `dist/index.html` placeholder so `tauri::generate_context!` validates before the first `npm run build`.
  - [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) covering native and HGFS dev loops.
- **Research deep-dive** — commit `dd59888` — evidence-grounded feature & improvement plan ([RESEARCH_DEEPDIVE.md](RESEARCH_DEEPDIVE.md)) with prioritized roadmap, IMP-01..IMP-07, F-NEW-01..F-NEW-10. Drives this Changelog and the integrated ROADMAP.

### Verified

- `cargo check` clean on Windows in `C:\tmp\Droidsmith` mirror.

### Known gaps (carried into Phase 0)

- `shell:default` capability is unscoped (IMP-02).
- `.expect(...)` panic on Tauri init failure (IMP-01).
- No CI matrix yet (R-003).
- Direct deps `thiserror 1`, `which 6` are stale (IMP-03).
