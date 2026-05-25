# Droidsmith Roadmap

Single source of truth for what's planned, what's in flight, and what's
shipped. Completed items move to [CHANGELOG.md](CHANGELOG.md). Deep
evidence and design notes live in [RESEARCH_DEEPDIVE.md](RESEARCH_DEEPDIVE.md);
do not duplicate that here — link instead.

## Conventions

- `[ ]` — not started
- `[~]` — in flight
- `[x]` — done; will move to [CHANGELOG.md](CHANGELOG.md) on next consolidation pass
- `DROP` — explicitly out of scope
- Priority tags: **P0** (must ship in v0.1) · **P1** (v0.1 desirable / v0.2 must) · **P2** (later milestones) · **P3** (cosmetic / nice-to-have)
- **R-NNN** are roadmap items; **IMP-NN** are hardening / improvement items
- An item written `R-NNN / F-NEW-NN` originated as a research-pass proposal that slotted into an existing R-NNN

## Phase 0 — Foundations (harden the scaffold before features)

- [x] **R-001** P0 — Repo, README, ROADMAP, RESEARCH_FEATURE_PLAN, LICENSE
- [x] **R-002** P0 — Scaffold Tauri 2 + React + TS + Vite + Tailwind
- [x] **IMP-02** P0 — Scope `shell:default` capability to enumerated sidecar commands with arg validators
- [x] **R-003** P0 — GitHub Actions CI matrix (Win/macOS/Linux): `cargo fmt/check/clippy/test`, frontend `typecheck/lint/prettier/test`
- [x] **R-004** P0 — Lint gates: clippy `-D warnings`, ESLint flat config, Prettier check, rustfmt.toml, `unsafe_code = deny`
- [x] **R-005** P1 — `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `.github/ISSUE_TEMPLATE/{bug,feature}.md`, PR template
- [x] **IMP-01** P1 — Replace `.expect()` startup panic with native error dialog + file-only crash log path
- [x] **IMP-03** P2 — Bumped `thiserror 1→2`, `which 6→8`; added `os_info 3`
- [x] **IMP-04** P2 — Heartbeat reports OS family/version/arch, Tauri, Rust MSRV, app_data_dir, adb path+source+version
- [x] **IMP-05** P3 — Each nav item now resolves to its own React component; Devices is a fully-live route, the other six render dedicated placeholder cards listing planned behaviour + IPC commands with ready/todo badges.
- [x] **IMP-06** P2 — `adb::locate_adb` honours `$ANDROID_HOME`, `$ANDROID_SDK_ROOT`; Linux paths gated to cfg(target_os = "linux")
- [x] **IMP-07** P3 — `scripts/dev-mirror.sh` POSIX companion: rsync-based, sentinel-guarded, `--watch`/`--reverse`/`--force`/`--dest` flags
- [~] **R-006 / R-010** P1 — Sidecar fetch scripts (`scripts/fetch-platform-tools.{ps1,sh}`) shipped with pinned-SHA-256 placeholders; final release pipeline (signing, notarization, SBOM, externalBin wiring) still pending real certs
- [x] **R-007 / F-NEW-10** P1 — File-only crash log floor (rotating 1MB × 5 backups) — opt-in upload still deferred to R-073
- [x] **LICENSE-THIRD-PARTY** P1 — `LICENSE-THIRD-PARTY.md` seeded with placeholders for adb / fastboot / scrcpy / UAD-NG

## Phase 1 — Core ADB (thin end-to-end slice)

Goal: A user installs Droidsmith, connects a Pixel (USB **or** wireless),
sees the apps list with real labels and icons, disables Facebook, and undoes
it — all in one session, with no Android SDK install required.

- [~] **R-010 / F-NEW-02** P0 — Fetch scripts done (`scripts/fetch-platform-tools.{ps1,sh}`); binary staging + `bundle.externalBin` wiring deferred to R-006 release-pipeline
- [x] **R-011** P0 — Transport abstraction (`adb::AdbTransport` + `ShellTransport` + `MockTransport`) shipped. `adb_client` crate adoption still deferred — current shell-out works for every v0.1 flow.
- [x] **R-012** P0 — `adb devices -l` parser handles real-world quirks (daemon chatter, no-permissions, wireless serials); `list_devices` Tauri command + live Devices UI route with refresh, empty/error states
- [ ] **R-013** P1 — Device dashboard: serial, model, Android version, SDK level, build fingerprint, battery, storage, IP
- [ ] **R-014** P1 — Authorize-on-device prompt with "Allow USB debugging" instructions
- [ ] **R-015 / F-NEW-01** P0 — Wireless ADB pairing wizard: 6-digit code, QR code, mDNS auto-discover

## Phase 2 — App Management

- [~] **R-020 / F-NEW-09** P0 — `pm list packages` parser + `AppPackage` typed surface + `list_packages` Tauri command shipped; renderer-side Apps route is a placeholder until real labels/icons (F-NEW-09 still TODO via pure-Rust APK parsing)
- [x] **R-021** P0 — `PackageFilter::{All,User,System,Enabled,Disabled}` shipped at the domain layer; UI filter chips land with R-020 UI
- [x] **R-022 / F-NEW-07** P0 — `actions::{plan, apply}` with two-step plan→apply, in-band `pm` Failure marker detection, `plan_action`/`apply_action` Tauri commands. Bulk queue UI lands with R-020.
- [ ] **R-023** P1 — Install: drag-and-drop APK (silent install à la scrcpy), multi-APK (`apks`), split APK (`apkm`), batch install
- [ ] **R-024** P1 — Extract: pull APK / split APKs / OBB out of a device, optional zip-up
- [ ] **R-024.5 / F-NEW-08** P2 — Ctrl+K command palette (fuzzy nav across packages, actions, settings, recent commands)
- [ ] **R-025** P2 — Permissions editor: runtime perms, `appops`, special access (notif, overlay, accessibility)
- [ ] **R-026** P2 — Per-app: backup with `adb backup` (legacy) and Shizuku/root paths where available
- [x] **R-026.5 / F-NEW-03** P0 — Per-device undo journal (JSON-Lines, not SQLite — HGFS-safe). `journal::Journal::{open, record, record_undo}`, corrupt-line tolerance, `journal_list` + `journal_undo` Tauri commands.

## Phase 3 — Debloat Engine (the headline feature)

- [x] **R-030** P0 — Pack schema (`Pack`/`PackTargets`/`PackEntry`/`RemovalLevel`) shipped in `src-tauri/src/packs/`. UAD-NG-aligned semantics.
- [x] **R-031** P0 — `droidsmith-pack-lint` binary shipped; exit codes 0/1/2 for clean/issues/usage. `packs/_example.yaml` seeds the contributor copy.
- [ ] **R-032** P0 — Seed packs: Pixel (vanilla AOSP)
- [ ] **R-032** P1 — Seed packs: OneUI, HyperOS, ColorOS (+ Realme alias), MIUI, Motorola, Fire OS
- [ ] **R-033** P0 — Debloat wizard UI: pick pack → preview diff → dry-run → apply → undo from journal
- [x] **R-034 / F-NEW-04** P0 — Quirks engine (`src-tauri/src/quirks/`) with `DeviceContext` matcher + `explain_failure` Tauri command. `quirks/hyperos.yaml` seed rule for the Xiaomi `pm disable` block.
- [x] **R-035** P0 — Folded into **R-026.5** (same journal).
- [ ] **R-036** P1 — UAD-NG list import path with attribution; produces `packs/_uad-supplement.yaml` from pinned upstream revision

## Phase 4 — Device Control

- [ ] **R-040 / F-NEW-06** P1 — scrcpy 4.x sidecar; Mirror route with audio + recording + drag-APK-installs
- [ ] **R-041** P2 — Virtual remote: D-pad, volume, power, home, back, recents, IME toggle
- [ ] **R-042** P2 — Screenshot to clipboard / file, with annotation overlay
- [ ] **R-043** P2 — Screen record (scrcpy `--record`) with stop button
- [ ] **R-044** P2 — File manager: pull/push with progress bars, rename, free-space gauge
- [ ] **R-045** P2 — Display tuning: density, screen size, force-dark, animation-scale presets

## Phase 5 — Power Tools

- [ ] **R-050** P1 — ADB console with multi-tab, command history, favourites, syntax highlighting
- [ ] **R-051** P1 — Logcat viewer: live tail, filters (tag, pid, level), grep, save
- [ ] **R-052** P2 — Fastboot mode: device list, flash slot, lock/unlock warnings, partition inspector
- [ ] **R-053** P2 — Process manager (top-equivalent), CPU/mem live chart
- [ ] **R-054** P3 — Network inspector: live `netstat`-equivalent via `ss`, per-app data usage

## Phase 6 — Automation & Extensibility

- [x] **R-060 / F-NEW-05** P1 — `src-tauri/src/profile.rs` ships the YAML schema + load/lint/requests_for. Profiles are serial-agnostic; serial bound at run-time.
- [x] **R-061** P1 — `droidsmith-cli` binary with `devices` and `run <profile.yaml> --device <serial> [--dry-run|--apply]`; exit codes 0/1/2/3.
- [ ] **R-062** P3 — Plugin API (Rust trait + wasm host) — defer to v0.3+
- [ ] **R-063** P3 — Plugin marketplace index — defer to v0.3+

## Phase 7 — Polish

- [ ] **R-070** P2 — i18n: en, ru (parity with ADB AppControl), es, de, pt-BR, zh-CN
- [ ] **R-071** P1 — Accessibility audit: keyboard-only, screen reader labels, AA contrast on anvil palette
- [ ] **R-072** P2 — Onboarding tour for new ADB users (drivers, USB debug, wireless pair)
- [ ] **R-073** P3 — Opt-in upload of crash logs to self-hosted Sentry (extends R-007 file-only floor)

## Open questions (block correct prioritization)

See [RESEARCH_DEEPDIVE.md §Open Questions](RESEARCH_DEEPDIVE.md#open-questions) for the full list. Summary:

1. GitHub org for the remote (Cargo.toml declares `SysAdminDoc/Droidsmith`)
2. Code-signing certificates funding for R-006
3. UAD-NG list redistribution permission (email maintainers before R-036)
4. Telemetry policy doc owner for R-007
5. Trademark search for "Droidsmith" before R-006

## Out of scope (DROP)

- **R-X01** Mobile/TV companion apps. We delegate to scrcpy + wireless ADB.
- **R-X02** Built-in ad serving, sponsor splash, donation nag. Hard no.
- **R-X03** Re-implementing scrcpy or `adb` from scratch. Integrate, don't reinvent.
- **R-X04** Rooting / unlocking devices. Magisk handles this well; legally fraught.
- **R-X05** Custom-ROM flashing UI. Different audience and trust model.
- **R-X06** In-app purchases. Hard no, per design tenet.
- **R-X07** Telemetry on by default. File-only crash log floor only.
- **R-X08** Web/browser deployment. ya-webadb covers that lane.
- **R-X09** Closed-source bundled deps. Every binary we ship must be MIT-redistributable.
