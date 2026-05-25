# Droidsmith Roadmap

R-NNN tracking. One line per item, status keyword in front. Items become
issues when the milestone they belong to opens. Ship order is roughly top to
bottom, but anything labelled `parallel` can be picked up out of order.

## Conventions

- `TODO` — not started
- `WIP` — branch open
- `DONE` — merged to `master`
- `DROP` — explicitly out of scope, leave the line so we remember why

## Milestone 0 — Foundations

- R-001 DONE Create repo, write README + ROADMAP + RESEARCH_FEATURE_PLAN + LICENSE
- R-002 TODO Scaffold Tauri 2 + React + TS + Vite + Tailwind, builds on Win/macOS/Linux
- R-003 TODO CI: GitHub Actions matrix (Win/macOS/Linux) — `cargo check`, `tsc`, `vitest`, `cargo test`
- R-004 TODO PSScriptAnalyzer-equivalent lint gates (clippy + eslint + prettier)
- R-005 TODO `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, issue & PR templates
- R-006 TODO Release pipeline: signed Windows installer, macOS `.dmg` (notarized via GH secret), Linux `.AppImage` + `.deb`
- R-007 TODO Telemetry: opt-in only, single boolean, no PII, document exactly what is sent

## Milestone 1 — Core ADB

- R-010 TODO Detect/locate `adb` binary: bundled, PATH, common install paths, Android Studio platform-tools
- R-011 TODO `adb_client` integration — pure-Rust path, fall back to bundled binary for unsupported flows
- R-012 TODO Device discovery: USB + TCP/IP, hot-plug events, multi-device tab strip
- R-013 TODO Device dashboard: serial, model, Android version, SDK level, build fingerprint, battery, storage, IP
- R-014 TODO Authorize dialog flow — handle `unauthorized` state with a clear "tap allow on device" prompt
- R-015 TODO Wireless ADB pairing UI (Android 11+): pair code + port, persist tokens, mDNS auto-discover

## Milestone 2 — App Management

- R-020 TODO App list with icons, label, package name, version, installer source, size, last-used (where `usagestats` allows)
- R-021 TODO Filters: user vs system, enabled vs disabled, has-data, large, recently-installed
- R-022 TODO Bulk select + action queue (uninstall, disable, clear data, force stop, extract APK)
- R-023 TODO Install: drag-and-drop APK, multi-APK (`apks`), split APK (`apkm`), batch install
- R-024 TODO Extract: pull APK / split APKs / OBB out of a device, optional zip-up
- R-025 TODO Permissions editor: runtime perms, `appops`, special access (notif, overlay, accessibility)
- R-026 TODO Per-app: backup with `adb backup` (legacy) and Shizuku/root paths where available

## Milestone 3 — Debloat Engine (the headline feature)

- R-030 TODO `packs/` YAML schema: pack metadata, target OEM/ROM/Android-version, list of packages with `safety` and `note`
- R-031 TODO Pack loader + validator (`droidsmith pack lint`) — CI runs on every pack change
- R-032 TODO Bundled seed packs: Pixel (vanilla), Samsung OneUI, Xiaomi HyperOS, Xiaomi MIUI, OPPO/OnePlus ColorOS, Realme, Motorola, Fire OS
- R-033 TODO Debloat wizard UI: pick pack → preview diff → dry-run → apply → undo within session
- R-034 TODO Vendor lock detection: surface "this OEM blocks `pm disable` for this package" instead of failing silently (per HyperOS / BBK reports)
- R-035 TODO Persist a per-device journal of every disable/enable so users can restore even months later
- R-036 TODO Import path from [Universal Android Debloater Next Generation](https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation) lists, with attribution

## Milestone 4 — Device Control

- R-040 TODO Embed scrcpy: bundled binary detection + auto-download, mirror window with audio (scrcpy 2+)
- R-041 TODO Virtual remote: D-pad, volume, power, home, back, recents, IME toggle
- R-042 TODO Screenshot to clipboard / file, with annotation overlay
- R-043 TODO Screen record (scrcpy `--record`) with stop button
- R-044 TODO File manager: pull/push with progress bars, in-place rename, free-space gauge
- R-045 TODO Display tuning: density, screen size, force-dark, animation-scale presets

## Milestone 5 — Power Tools

- R-050 TODO ADB console with multi-tab, command history, favourites, syntax highlighting
- R-051 TODO Logcat viewer: live tail, filters (tag, pid, level), grep, save, share-as-gist
- R-052 TODO Fastboot mode: device list, flash slot, lock/unlock warnings, partition inspector
- R-053 TODO Process manager (top-equivalent), CPU/mem live chart
- R-054 TODO Network inspector: live `netstat`-equivalent via `ss`, per-app data usage

## Milestone 6 — Automation & Extensibility

- R-060 TODO Profile YAML: declarative "do this on every freshly-set-up Pixel" — install set, debloat pack, settings tweaks
- R-061 TODO Headless CLI: `droidsmith run profile.yaml --device <serial>`, exit codes for CI use
- R-062 TODO Plugin API (Rust trait + wasm host) — third-party OEM modules, custom debloat strategies
- R-063 TODO Plugin marketplace index (static JSON in this repo, signed by maintainers)

## Milestone 7 — Polish

- R-070 TODO i18n: English + Russian (parity with ADB AppControl) + Spanish + German + Brazilian Portuguese + Simplified Chinese
- R-071 TODO Accessibility audit: keyboard-only, screen reader labels, high-contrast
- R-072 TODO Onboarding tour for new ADB users (install drivers, enable USB debug)
- R-073 TODO Crash reporter (opt-in, Sentry self-hosted)

## Out of scope (DROP)

- R-X01 DROP Mobile/TV companion apps. ADB AppControl ships those; we delegate to scrcpy and existing solutions.
- R-X02 DROP Built-in ad serving, sponsor splash, donation nag. Hard no.
