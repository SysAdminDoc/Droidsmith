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
- [ ] **IMP-02** P0 — Scope `shell:default` capability to enumerated sidecar commands with arg validators
- [ ] **R-003** P0 — GitHub Actions CI matrix (Win/macOS/Linux): `cargo check`, `cargo clippy`, `cargo test`, `npm run typecheck`, `npm run lint`, `npm run test`, pack-lint
- [ ] **R-004** P0 — Lint gates wired locally: clippy `-D warnings`, eslint `--max-warnings 0`, prettier check
- [ ] **R-005** P1 — `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `.github/ISSUE_TEMPLATE/{bug,feature}.md`, PR template
- [ ] **IMP-01** P1 — Replace `.expect()` startup panic with native error dialog + crash log path
- [ ] **IMP-03** P2 — Bump `thiserror 1→2`, `which 6→8`
- [ ] **IMP-04** P2 — Heartbeat reports OS, Tauri, Rust, app_data_dir, adb_version
- [ ] **IMP-05** P3 — Replace `<div>` nav stubs with `<button>` + placeholder routes ("Coming in R-NNN")
- [ ] **IMP-06** P2 — `adb::locate_adb` also tries `$ANDROID_HOME`, `$ANDROID_SDK_ROOT`
- [ ] **IMP-07** P3 — `scripts/dev-mirror.sh` companion to the PowerShell mirror (Linux/macOS/WSL)
- [ ] **R-006** P1 — Release pipeline: signed Win MSI, notarized macOS dmg, Linux AppImage + deb, SBOM, signature
- [ ] **R-007 / F-NEW-10** P1 — File-only crash log (rotating, 1MB×5) with "scrub serials" export; opt-in upload deferred to R-073
- [ ] **LICENSE-THIRD-PARTY** P1 — Maintain `LICENSE-THIRD-PARTY.md` accumulating notices for every bundled binary or vendored data set

## Phase 1 — Core ADB (thin end-to-end slice)

Goal: A user installs Droidsmith, connects a Pixel (USB **or** wireless),
sees the apps list with real labels and icons, disables Facebook, and undoes
it — all in one session, with no Android SDK install required.

- [ ] **R-010 / F-NEW-02** P0 — Bundle `adb` and `fastboot` as Tauri sidecars; detection order PATH → Android Studio → bundled
- [ ] **R-011** P0 — `adb_client` 3.x integration, with sidecar `adb` fallback for unsupported flows
- [ ] **R-012** P0 — Device discovery USB + TCP/IP + hot-plug; multi-device strip
- [ ] **R-013** P1 — Device dashboard: serial, model, Android version, SDK level, build fingerprint, battery, storage, IP
- [ ] **R-014** P1 — Authorize-on-device prompt with "Allow USB debugging" instructions
- [ ] **R-015 / F-NEW-01** P0 — Wireless ADB pairing wizard: 6-digit code, QR code, mDNS auto-discover

## Phase 2 — App Management

- [ ] **R-020 / F-NEW-09** P0 — App list with **real labels and icons** (via pure-Rust APK parsing, no AAPT2 dep), package name, version, installer source, size
- [ ] **R-021** P0 — Filters: user/system, enabled/disabled, has-data, large, recently-installed
- [ ] **R-022 / F-NEW-07** P0 — Bulk select → action queue with preview diff → apply atomically; failures pause the queue
- [ ] **R-023** P1 — Install: drag-and-drop APK (silent install à la scrcpy), multi-APK (`apks`), split APK (`apkm`), batch install
- [ ] **R-024** P1 — Extract: pull APK / split APKs / OBB out of a device, optional zip-up
- [ ] **R-024.5 / F-NEW-08** P2 — Ctrl+K command palette (fuzzy nav across packages, actions, settings, recent commands)
- [ ] **R-025** P2 — Permissions editor: runtime perms, `appops`, special access (notif, overlay, accessibility)
- [ ] **R-026** P2 — Per-app: backup with `adb backup` (legacy) and Shizuku/root paths where available
- [ ] **R-026.5 / F-NEW-03** P0 — Per-device undo journal (SQLite): every destructive action reversible from an Activity tab; export-as-JSON

## Phase 3 — Debloat Engine (the headline feature)

- [ ] **R-030** P0 — `packs/*.yaml` schema + JSON Schema validator
- [ ] **R-031** P0 — Pack lint CLI bin (`droidsmith-pack-lint`); CI gate on every pack PR
- [ ] **R-032** P0 — Seed packs: Pixel (vanilla AOSP)
- [ ] **R-032** P1 — Seed packs: OneUI, HyperOS, ColorOS (+ Realme alias), MIUI, Motorola, Fire OS
- [ ] **R-033** P0 — Debloat wizard UI: pick pack → preview diff → dry-run → apply → undo from journal
- [ ] **R-034 / F-NEW-04** P0 — Vendor-lock detection + explanatory error layer (HyperOS / BBK known cases)
- [ ] **R-035** P0 — Per-device journal of disable/enable history (= **R-026.5**, this is the same thing — keep one)
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

- [ ] **R-060 / F-NEW-05** P1 — Profile YAML schema + dry-run runner + apply mode
- [ ] **R-061** P1 — Headless CLI bin (`droidsmith-cli`) — `droidsmith-cli run profile.yaml --device <serial> [--dry-run|--apply]`
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
