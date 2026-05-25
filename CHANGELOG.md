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

- _(none yet — this section accumulates as Phase 0 progresses)_

### Changed

- _(none yet)_

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
