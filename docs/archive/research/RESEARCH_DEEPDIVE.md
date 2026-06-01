# Project Research and Feature Plan

> Deep-dive research pass, 2026-05-25. Companion to the lighter
> [RESEARCH_FEATURE_PLAN.md](RESEARCH_FEATURE_PLAN.md) — read that first for
> the elevator pitch, then this for the implementation-ready evidence and
> roadmap. New R-NNN items proposed here slot into the milestones in
> [ROADMAP.md](ROADMAP.md); existing items get evidence and acceptance
> criteria attached.

---

## Executive Summary

Droidsmith is at commit `4f7b584` — a fully scaffolded but feature-empty
Tauri 2 + React + TS + Vite app aiming to replace ADB AppControl with a
cross-platform, MIT-licensed, no-paywall, no-telemetry alternative. The
scaffold is clean: Rust core compiles, `heartbeat` IPC works end to end, a
robocopy `scripts/dev-mirror.ps1` handles the VMware HGFS dev-loop, and a
full per-OS icon set is generated. **Strongest current shape:** the
foundation is correct and the differentiation thesis is sound. The biggest
competitive gap in the market is _not_ another debloater — UAD-NG already
owns that — but a single tool that **integrates** the best open building
blocks (UAD-NG's curated lists, scrcpy's mirroring, `adb_client`'s pure-Rust
ADB) behind a unified workflow, with first-class wireless ADB pairing and
scriptable automation. **Highest-value direction:** ship a thin end-to-end
slice (USB + wireless device discovery → app list → single
disable/uninstall → reconnect journal) before attempting any of the heavier
features (debloat engine, scrcpy embed, plugin API).

### Top 10 opportunities, priority order

1. **P0** — Replace `shell:default` capability with scoped `shell:allow-execute` whose argument validators reject anything outside a known `adb`/`fastboot`/`scrcpy` subcommand set. Current scaffold gives the renderer arbitrary shell access. **Evidence:** [`src-tauri/capabilities/default.json:9`](src-tauri/capabilities/default.json).
2. **P0** — Bundle `adb` as a Tauri sidecar (`bundle.externalBin`) so users don't need a system Android SDK install. Detect-system-first, fall-back-to-bundled, with version pinning. **Evidence:** [`src-tauri/src/adb.rs:1-29`](src-tauri/src/adb.rs) detects but never bundles; [Tauri sidecar docs](https://v2.tauri.app/develop/sidecar/).
3. **P0** — Implement Android 11+ wireless pairing UI. Neither `adb_client` v3.2.1 nor ADB AppControl handle this cleanly; both `adb pair`-shell-out and mDNS auto-discovery are needed. **Evidence:** [Android wireless debugging docs](https://developer.android.com/tools/adb); [adb_client API notes](https://github.com/cocool97/adb_client) — pair/mTLS is missing.
4. **P1** — Ingest UAD-NG's `uad_lists.json` (CC-licensed-friendly per their wiki + attribution) instead of hand-curating. The schema is small and stable (`list`/`removal`/`description`/`dependencies`/`neededBy`/`labels`). **Evidence:** verified entry shape `{list: "Oem"|"Aosp"|"Misc"|"Carrier", removal: "Recommended"|"Advanced"|"Expert"|"Unsafe", ...}` from raw JSON.
5. **P1** — Persistent per-device undo journal. UAD-NG has snapshot/restore; ADB AppControl Extended has "device connection history"; nothing pairs them with full reversibility for every disable/enable. **Evidence:** [UAD-NG wiki](https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/wiki) mentions selection export + snapshots.
6. **P1** — Vendor-lock detection and explanatory errors. HyperOS blocks `pm disable` for many packages and ADB AppControl fails silently; XDA threads document this is the #1 user-confusion source. **Evidence:** [XDA search results](https://xdaforums.com/tags/debloat/) and the project's own [RESEARCH_FEATURE_PLAN.md §1](RESEARCH_FEATURE_PLAN.md).
7. **P1** — Headless CLI (`droidsmith run profile.yaml`) so this is usable from CI / scripts. None of ADB AppControl, Aya, Xtreme ADB, or UAD-NG offer this; it's a real differentiator.
8. **P1** — scrcpy 4.0 wrapper (mirror + audio + recording) with per-device session state instead of re-implementing mirroring. Apache-2.0 lets us bundle binaries directly. **Evidence:** [scrcpy v4.0 release](https://github.com/Genymobile/scrcpy).
9. **P2** — Replace `tauri::generate_context!().expect(...)` panic at [`src-tauri/src/lib.rs:13`](src-tauri/src/lib.rs) with a graceful startup failure dialog. Users hitting a corrupt install today get a process that crashes silently.
10. **P2** — Stale-dep bump batch: `thiserror 1 → 2`, `which 6 → 8`, plus `toml`/`generic-array` (surfaced by `cargo check` resolver warnings). One commit, no behavior change.

---

## Evidence Reviewed

### Local files inspected (Verified)

- [`README.md`](README.md) — vision, comparison table, planned tech stack
- [`ROADMAP.md`](ROADMAP.md) — R-001..R-073 across 8 milestones; R-001 and R-002 marked DONE
- [`RESEARCH_FEATURE_PLAN.md`](RESEARCH_FEATURE_PLAN.md) — competitor scan, design tenets, locked tech stack
- [`LICENSE`](LICENSE) — MIT
- [`.gitignore`](.gitignore) — Tauri + Node + HGFS exclusions with `dist/` allowlist for placeholder
- [`package.json`](package.json) — Tauri 2.1, React 18.3, TS 5.7, Vite 6, Tailwind 3.4, Zustand 5
- [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml) — `tauri 2`, `tauri-plugin-shell 2`, `tauri-plugin-dialog 2`, `serde 1`, `serde_json 1`, `thiserror 1`, `which 6`
- [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) — productName, identifier `com.droidsmith.app`, dark theme, CSP, bundle icons
- [`src-tauri/capabilities/default.json`](src-tauri/capabilities/default.json) — `core:default + dialog:default + shell:default`
- [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) — Builder, plugin registration, `heartbeat` handler, `.expect` panic
- [`src-tauri/src/adb.rs`](src-tauri/src/adb.rs) — `locate_adb()`, candidate paths for Win/macOS/Linux
- [`src-tauri/src/commands.rs`](src-tauri/src/commands.rs) — `Heartbeat` struct + command
- [`src/App.tsx`](src/App.tsx) — sidebar with 7 nav stubs, heartbeat panel
- [`src/main.tsx`](src/main.tsx), [`src/index.css`](src/index.css), [`index.html`](index.html)
- [`scripts/dev-mirror.ps1`](scripts/dev-mirror.ps1) — robocopy to `C:\tmp\Droidsmith`
- [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) — native + HGFS dev loops
- Lint/format: [`eslint.config.js`](eslint.config.js), [`.prettierrc`](.prettierrc), [`.editorconfig`](.editorconfig)

### Git history reviewed (Verified)

- `0a82c63` chore: scaffold Droidsmith repo (planning surface)
- `4f7b584` feat: R-002 scaffold Tauri 2 + React + TS + Vite + Tailwind

Two commits total. No branches, no tags, no remote.

### Build / test / docs / release artifacts inspected

- `cargo check` clean on Windows in the C:\ mirror (Verified — see commit
  message for `4f7b584`)
- No CI configured (no `.github/` directory)
- No tests yet (vitest configured, `cargo test` works but no test files)
- No release pipeline (R-006 still TODO)
- No `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, issue templates, or PR template

### Stale-dependency signals surfaced by `cargo check` (Verified)

| Dep | Current | Available |
|---|---|---|
| `thiserror` | 1.0.69 | 2.0.18 |
| `which` | 6.0.3 | 8.0.2 |
| `toml` | 0.8.2 | 0.8.23 (transitive) |
| `toml_datetime` | 0.6.3 | 0.6.11 (transitive) |
| `toml_edit` | 0.20.2 | 0.20.7 (transitive) |
| `generic-array` | 0.14.7 | 0.14.9 (transitive) |

Direct deps (`thiserror`, `which`) are worth bumping; the rest are
transitive through Tauri.

### External sources reviewed (Verified)

- [Universal Android Debloater NG](https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation) — v1.2.0 (2026-01-12), 6.7k stars, GPL-3.0, Rust + Iced
- UAD-NG raw [`uad_lists.json`](https://raw.githubusercontent.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/main/resources/assets/uad_lists.json) — schema verified: `{list: enum, removal: enum, description, dependencies[], neededBy[], labels[]}`
- [scrcpy](https://github.com/Genymobile/scrcpy) v4.0 (2026-05-12), Apache-2.0 — verified features: audio, virtual displays, OTG, camera, V4L2, HID, drag-APK-installs, drag-file pushes to `/sdcard/Download` configurable via `--push-target`
- [Shizuku](https://github.com/RikkaApps/Shizuku) v13.6.0, Apache-2.0 — Android-side framework; not a direct desktop primitive, but worth documenting as a complementary upgrade path for users
- [adb_client crate](https://github.com/cocool97/adb_client) v3.2.1 (2026-05-03), MIT — supports USB/TCP/server-proxy, mDNS, shell, push/pull, install/uninstall, logcat, framebuffer. **Gaps:** Android 11+ wireless pairing (mTLS), fastboot protocol.
- [Android wireless ADB debugging](https://developer.android.com/tools/adb) — confirmed `adb pair ipaddr:port` + 6-digit code or QR; mDNS auto-discovery; default ADB port 5037; emulator port pairs 5554/5555..5584/5585
- [Tauri 2 sidecar guide](https://v2.tauri.app/develop/sidecar/) — `bundle.externalBin`, target-triple suffix, `app.shell().sidecar("name").args([...])`, capability with `shell:allow-execute + sidecar:true + args validators`
- [Tauri 2 updater](https://v2.tauri.app/plugin/updater/) — Ed25519 signing, `TAURI_SIGNING_PRIVATE_KEY` env, static JSON manifest, `{{target}}/{{arch}}/{{current_version}}` template
- [Tauri 2 shell plugin](https://v2.tauri.app/plugin/shell/) — scoped commands with `cmd`/`args`/`sidecar` and arg validators (regex)
- [Tauri 1 → 2 migration](https://v2.tauri.app/start/migrate/from-tauri-1/) — capability-based permissions replaced allowlist; updater dialog is gone (apps must drive it)
- [ya-webadb / Tango ADB](https://github.com/yume-chan/ya-webadb) — MIT, TypeScript, WebUSB
- [ADB AppControl Extended page](https://adbappcontrol.com/en/extended) — verified paywall list (see Competitive Research below)
- XDA tags + thread search via [XDA debloat](https://xdaforums.com/tags/debloat/) — surfaced HyperOS `pm disable` block (uninstall still works), "apps reinstalling after removal" pattern, "remove anything with Facebook in the name" multi-pkg coupling

### Areas not verified (Likely or Assumption)

- **Assumption:** `npm install` succeeds clean on Windows without further setup. The package.json is well-formed but transitive resolution hasn't been exercised. Verify with `npm install --dry-run` on next CI pass.
- **Likely:** Tauri 2.11 `generate_context!()` + the current capability set works at build time on macOS and Linux. We've verified Windows; the other two are unverified until R-003 CI runs.
- **Assumption:** `adb_client` 3.2.1's mDNS discovery works on Windows hosts without elevated permissions. Their README claims yes; should be exercised in R-012.
- **Needs live validation:** HyperOS / OneUI / ColorOS specific `pm disable` failure modes. XDA reports are dated and OEM-specific; need real-device testing to write the explanatory error layer (R-034).

---

## Current Product Map

### Stack (Verified)

- **Shell:** Tauri 2.11.x, native webview, single-binary per OS
- **Backend:** Rust 1.95, edition 2021, `rust-version = "1.77"`
- **Frontend:** React 18.3 + TypeScript 5.7 + Vite 6 + Tailwind 3.4 + Zustand 5
- **Lint/format:** ESLint 9 flat config, Prettier 3.4, EditorConfig
- **Package manager:** npm 11 (pnpm not assumed; no `pnpm-lock.yaml`)
- **Build:** `cargo` for Rust, `vite` for frontend, `tauri` for bundling
- **Test:** vitest + cargo test (no fixtures yet)

### Distribution channels (Planned)

- Windows signed `.exe` / `.msi`
- macOS notarized `.dmg`
- Linux `.AppImage` + `.deb`
- winget / Homebrew cask / Flathub — order TBD per R-006

### Storage (Planned)

- Per-user config: Tauri default app data dir
- Per-device undo journal: SQLite via `rusqlite` or `tauri-plugin-store` (open question)
- Debloat packs: bundled `packs/` resource + remote refresh

### Permissions / network (Verified-current, Planned-future)

- Current: `core:default` + `dialog:default` + `shell:default`. The shell
  permission is unscoped — this is **a security debt** carried from the
  scaffold and must be tightened before the first release.
- Future network surface: HTTP fetch for pack updates, mDNS for wireless
  ADB, optional updater endpoint, optional crash reporter.

### User personas (Inferred from ADB AppControl audience + ROADMAP scope)

- **P1 — The debloater.** Daily-driving a Pixel/Samsung/Xiaomi, wants the OEM bloat off without root, doesn't trust closed binaries. Currently uses UAD-NG.
- **P2 — The QA / developer.** Manages multiple test devices, wants device snapshots and headless flashing flows. Currently scripts `adb` by hand.
- **P3 — The power user / tinkerer.** Roots, flashes, mods. Wants a unified console + logcat + fastboot. Currently uses ADB AppControl extended + scrcpy + UAD-NG.
- **P4 — The technician / refurbisher.** Wipes and resets dozens of phones a week, wants reproducible profiles. Currently no good tool.

---

## Feature Inventory

The project is pre-functional. Everything here is either scaffold or stub.

| # | Feature | User value | Entry point | Code | Maturity | Tests/docs | Improvement |
|---|---|---|---|---|---|---|---|
| F-01 | `heartbeat` IPC | Smoke-test the Rust↔TS bridge | App launch | [`commands.rs:11`](src-tauri/src/commands.rs#L11), [`App.tsx:14`](src/App.tsx#L14) | Complete (scaffold) | No tests; mentioned in [DEVELOPMENT.md](docs/DEVELOPMENT.md) | Add a `cargo test` + Vitest pair to exercise it; remove when first real feature ships |
| F-02 | `adb::locate_adb` helper | Find system adb so devices can be listed | Called from `heartbeat` | [`adb.rs:3`](src-tauri/src/adb.rs#L3) | Partial — finds binary, doesn't use it | None | Not yet wired to a device list; **also** doesn't include a bundled sidecar fallback |
| F-03 | Sidebar nav shell | Visual scaffold for milestones | Always visible | [`App.tsx:21-37`](src/App.tsx#L21-L37) | Stub (7 disabled items) | None | Every label is "Not implemented yet". Hover title is the only signal |
| F-04 | Dark-first window | Match developer-tool aesthetic | App launch | [`tauri.conf.json:24`](src-tauri/tauri.conf.json#L24), [`index.html:2`](index.html#L2) | Complete | None | No light-theme toggle; high-contrast unverified |
| F-05 | HGFS dev-mirror | Make Vite/Cargo work over VMware Shared Folders | `./scripts/dev-mirror.ps1` | [`scripts/dev-mirror.ps1`](scripts/dev-mirror.ps1) | Complete | Documented in [DEVELOPMENT.md §HGFS](docs/DEVELOPMENT.md) | Windows-only (uses robocopy); macOS/Linux equivalents not needed if dev tree is mounted natively, but `-Reverse` mode is untested |
| F-06 | Icon set | Identity in installer / dock | Build-time | [`src-tauri/icons/*`](src-tauri/icons/) | Complete | None | The "D" glyph is placeholder — fine for v0 but worth a real logo before R-006 release pipeline |
| F-07 | `dist/index.html` placeholder | Lets `cargo check` validate `frontendDist` before first build | Build-time | [`dist/index.html`](dist/index.html) | Complete | Documented inline | Slightly hacky — alternative would be a `tauri-build` config flag; leave for now |

**Hidden / undocumented features:** None. The scaffold is what it claims to be.

**Stale / dead code:** None.

**Disabled features:** All seven sidebar nav items.

---

## Competitive and Ecosystem Research

### ADB AppControl 1.8.6 (closed, Windows, freemium)

**Notable capabilities:** App list with filters, batch install, APK extract,
split-APK install, permissions editor, virtual remote, screenshot, fastboot,
logcat, ADB console, debloat wizard, file push, auto-grant permissions,
device info.

**Extended (paywalled) features — verified from the official page:**

- All Debloat Wizard recommendation levels (free is lowest only)
- Batch applications installation
- Quick app search on 10+ services (Play, XDA, etc.)
- Process Manager
- Dark interface theme
- Customizable font size and icons
- Application sorting options
- Unrestricted file transfers
- Device connection history (wireless reconnection)
- Drag-and-drop install
- "Exclusive ADB key protection feature"

**Learn from:** The all-in-one feature surface; how it sequences "find
device → list apps → bulk action → debloat" as one workflow.

**Intentionally avoid:** Paywalling table-stakes features (dark theme,
sorting, font sizing, batch install). Hidden silent failures on
vendor-locked ROMs. Closed-source release artifacts.

### Universal Android Debloater NG (UAD-NG) — Rust + Iced, GPL-3.0

**Notable capabilities:** Curated debloat list with `list` (Oem/Aosp/Misc/Carrier)
and `removal` (Recommended/Advanced/Expert/Unsafe) categorization, dependency
graph (`neededBy`), per-app description, selection export/import, snapshot
& restore.

**Learn from:** The JSON list schema; the explicit `Unsafe` removal level;
dependency tracking; snapshot/restore as a first-class flow.

**Intentionally avoid:** Iced UI (small contributor pool vs React); GPL-3.0
contagion via linking — we can _consume_ their list (which is data, not
linked code) under attribution.

### scrcpy 4.0 — C, Apache-2.0

**Notable capabilities:** Audio forwarding, virtual displays, OTG, camera
mirror, V4L2 webcam, HID keyboard/mouse, gamepad, recording, screen-off,
drag-APK-installs, drag-file pushes (`/sdcard/Download`), clipboard sync.

**Learn from:** Drag-APK = silent install was a power-user surprise. Worth
copying. The `--push-target` flexibility for non-APK files is also worth
exposing.

**Intentionally avoid:** Re-implementing video pipeline. Just orchestrate
the binary — license permits direct bundling.

### Tango ADB / ya-webadb — TypeScript, MIT

**Notable capabilities:** Pure-web ADB over WebUSB; runs in Chrome / Chrome
for Android / Electron / Node. Library + hosted demo app.

**Learn from:** The pure-TypeScript ADB protocol implementation is a
real artifact — if `adb_client`'s wireless-pair gap proves stubborn, we
could embed `@yume-chan/adb` in the renderer as a fallback. WebUSB on
Windows requires WinUSB drivers (Google's `usb_driver` works).

**Intentionally avoid:** Browser-first deployment (not what we are).
WebUSB driver friction on Windows.

### Aya — Android ADB Desktop (closed)

**Notable capabilities:** Modern dark UI, Win/macOS, fast performance.

**Learn from:** UI polish as a competitive moat.

**Intentionally avoid:** Closed source — same anti-pattern as ADB AppControl.

### Android Studio Device Manager

**Notable capabilities:** Device discovery, wireless pair (QR + code), emulator
management.

**Learn from:** The Device Manager UI pattern (devices listed left,
detail pane right). Familiar to developers.

**Intentionally avoid:** Coupling to the entire JetBrains/IntelliJ surface.

---

## Highest-Value New Features

Each item below is a new feature (not yet in [ROADMAP.md](ROADMAP.md)) or a
material expansion of one that's there. R-numbers proposed for ones that
should slot into existing milestones.

### F-NEW-01 — Wireless ADB pairing wizard with mDNS auto-discover

- **Title:** Pairing wizard for Android 11+ wireless debugging
- **User problem:** Pairing wireless ADB is a six-step terminal dance most users get wrong. ADB AppControl extended only does *reconnection*, not initial pairing.
- **Evidence:** [Android wireless debugging docs](https://developer.android.com/tools/adb); [adb_client v3.2.1](https://github.com/cocool97/adb_client) lacks `adb pair` mTLS; XDA threads show "device unauthorized" is a top onboarding blocker
- **Proposed behavior:**
  1. Detect mDNS service `_adb-tls-pairing._tcp` on the LAN
  2. Show discovered devices in a "ready to pair" list
  3. User taps a device → modal opens with a 6-digit code field (or QR scanner via webcam)
  4. We shell out `adb pair <ip>:<port>` and capture stderr for retry/error UX
  5. On success, `adb connect <ip>:5555` and persist the host fingerprint in the device journal
- **Implementation areas:** `src-tauri/src/adb/pair.rs` (new), [`src/App.tsx`](src/App.tsx) Pairing route, `src-tauri/src/mdns.rs` (new) using `mdns-sd` crate
- **Data model:** `paired_devices.json` in app data dir — `{serial, friendly_name, last_ip, fingerprint, paired_at, last_seen}`
- **Risks / edge cases:** Some corporate Wi-Fi blocks mDNS; manual IP-port entry must remain available. Some routers block client isolation. Android 11+ specific — show clear unsupported state for older devices.
- **Verification:** Pair a real Pixel and a real Samsung over Wi-Fi. Confirm reconnect after device reboot. Confirm graceful error when wrong code entered.
- **Complexity:** L
- **Priority:** P0 (slots into R-015)

### F-NEW-02 — Bundled `adb` sidecar with auto-update channel

- **Title:** Ship a vendored `platform-tools` `adb` so users don't need Android SDK
- **User problem:** Today the app prints "adb: not detected" if the user has no Android SDK. Asking users to install platform-tools is a 5-minute detour and a common drop-off.
- **Evidence:** [`src-tauri/src/adb.rs:3-12`](src-tauri/src/adb.rs#L3-L12) returns `None`; [Tauri sidecar docs](https://v2.tauri.app/develop/sidecar/) confirm per-target-triple suffix approach
- **Proposed behavior:**
  1. Bundle `adb` (+ `fastboot`) as sidecars for each target triple
  2. Detection order: system PATH → Android Studio default → bundled sidecar
  3. Settings page shows the resolved path and a "use bundled instead" toggle
  4. CI fetches the latest platform-tools per release and verifies SHA against Google's checksums
- **Implementation areas:** [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) `bundle.externalBin`, [`src-tauri/capabilities/default.json`](src-tauri/capabilities/default.json) sidecar permission, [`src-tauri/src/adb.rs`](src-tauri/src/adb.rs) resolver, `scripts/fetch-platform-tools.ps1` and `.sh`
- **Data model:** None new; bundled binary lives at `src-tauri/binaries/adb-<triple>[.exe]`
- **Risks:** Distribution size grows ~25MB; platform-tools Apache-2.0 license requires NOTICE file. Windows code-sign may need to sign sidecar binaries too. Verify Google's redistribution terms.
- **Verification:** `npm run tauri:build` produces installers >= 50MB; on a clean Win VM with no SDK, app finds `adb` immediately.
- **Complexity:** M
- **Priority:** P0 (slots into R-010)

### F-NEW-03 — Per-device undo journal (universal reversibility)

- **Title:** Every destructive action is logged and reversible
- **User problem:** "I disabled an app three weeks ago and now my dialer won't work — what did I do?" UAD-NG has snapshots; nothing has a chronological journal you can scrub through.
- **Evidence:** UAD-NG wiki references export/import + snapshots ([source](https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/wiki)); ADB AppControl has none.
- **Proposed behavior:**
  1. Every `pm disable-user`, `pm uninstall --user 0`, `appops set`, `pm grant/revoke` writes a row to `journal.db` (SQLite) with device serial, package, before/after state, ISO timestamp, and the exact command run
  2. "Activity" tab shows a chronological list per device
  3. Each row has a one-click "undo" button that synthesizes the inverse command
  4. Journal exports as JSON for sharing/backup
- **Implementation areas:** `src-tauri/src/journal/{mod.rs,schema.sql}` (new), Activity React page, [`commands.rs`](src-tauri/src/commands.rs) for `journal_list`/`journal_undo`/`journal_export`
- **Data model:** SQLite — see below in Architecture
- **Risks:** SQLite on HGFS is slow (memory: [`vmware-hgfs-limitations`]) → keep journal on user's local config dir, not the repo. Undo of `pm uninstall --user 0` requires re-installing from system image — surface as "cannot undo, app removed from /data/app" with explanatory tooltip.
- **Verification:** Disable app → see journal entry → click undo → app re-enabled. Roundtrip across app restart.
- **Complexity:** L
- **Priority:** P1 (new — propose **R-026.5**)

### F-NEW-04 — Vendor-lock detection & explanatory errors

- **Title:** "Why didn't this work?" — first-class vendor-restriction surfacing
- **User problem:** HyperOS, OneUI, ColorOS, BBK and others block `pm disable` for specific packages. ADB AppControl reports "Operation failed" with no diagnosis. Most XDA support traffic for ADB AppControl traces back to this.
- **Evidence:** XDA threads on HyperOS debloat; this project's own [README.md comparison table](README.md); MIUI/ColorOS user complaints across forums
- **Proposed behavior:**
  1. Maintain a `quirks/` rules file: `{oem, rom, android_version_range, package_pattern, behavior, mitigation}`
  2. After a failed `pm` command, pattern-match the error against quirks
  3. Show a structured error: "**This won't work on HyperOS 1.x.** Xiaomi blocks `pm disable --user 0` for `com.miui.gallery`. Try `pm uninstall --user 0` instead (data preserved but app removed)."
  4. The quirks file is community-maintained
- **Implementation areas:** `src-tauri/src/quirks/` (new), `quirks/{hyperos.yaml, oneui.yaml, coloros.yaml, fireos.yaml}`, error wrapper in [`adb.rs`](src-tauri/src/adb.rs)
- **Data model:** YAML quirks files versioned in the repo
- **Risks:** False positives — a quirk might fire when the real problem is something else. Mitigation: always show the raw `adb` error too, with the quirks UI as supplementary advice not replacement.
- **Verification:** Real-device tests on HyperOS, OneUI, ColorOS. CI lints quirks YAML against schema.
- **Complexity:** M
- **Priority:** P1 (slots into R-034)

### F-NEW-05 — Profile YAML + headless CLI (`droidsmith run`)

- **Title:** Declarative device setup as reproducible scripts
- **User problem:** Refurbishers and QA labs flash dozens of devices identically; clicking through a GUI is the wrong tool. None of the competitors offer this.
- **Evidence:** No competitor offers it; [ROADMAP R-060](ROADMAP.md) and R-061 already reserve this
- **Proposed behavior:**
  ```yaml
  # profile.yaml
  name: "Fresh Pixel setup"
  device:
    matches: "model=Pixel 8*"
  debloat:
    pack: "google-pixel"
    level: "Advanced"   # Recommended | Advanced | Expert
  install:
    - "./apks/firefox-release.apk"
    - "./apks/k9-mail.apk"
  settings:
    animation_scale: 0.5
    accessibility_high_contrast: true
  ```
  Run via `droidsmith run profile.yaml --device <serial>` from a terminal,
  exit 0 on success, non-zero per step.
- **Implementation areas:** `src-tauri/src/cli/` (new), `src-tauri/src/profile.rs`, `cli/` separate Cargo binary target
- **Data model:** YAML schema validated with `serde_yaml`; outputs JSON to stdout for CI consumers (`--format=json`)
- **Risks:** CLI vs GUI feature divergence — every new GUI action must also become a CLI verb. Enforce via integration tests.
- **Verification:** `droidsmith run examples/pixel-fresh.yaml --device emulator-5554 --dry-run` produces a complete plan; `--apply` executes it; CI runs it against an emulator
- **Complexity:** XL
- **Priority:** P1 (slots into R-060/R-061; expand to ship dry-run before apply)

### F-NEW-06 — scrcpy embedded mirror with per-device session state

- **Title:** Wrap scrcpy 4.0 as the mirror engine
- **User problem:** ADB AppControl's "virtual remote" is buttons-only — no screen mirror. Users currently launch scrcpy separately, lose context.
- **Evidence:** [scrcpy 4.0](https://github.com/Genymobile/scrcpy) (Apache-2.0) bundleable; ROADMAP R-040 already reserves this
- **Proposed behavior:**
  1. Bundle `scrcpy` + `scrcpy-server.jar` as sidecar binaries
  2. Per-device, a "Mirror" tab spawns scrcpy with sensible defaults (audio on, 60fps, max-size matched to window)
  3. Window-level menu: "Record session", "Drop APK to install" (literal drag handler), "Push file" (uses scrcpy's `--push-target`)
  4. Bitrate / size / audio toggles in a sidebar; persist per device
- **Implementation areas:** `src-tauri/src/scrcpy.rs` (new), `src-tauri/binaries/scrcpy*` sidecars, Mirror React route
- **Data model:** `device_session.json` with per-device scrcpy preferences
- **Risks:** scrcpy needs `scrcpy-server.jar` deployed to the device on each launch — make sure paths and permissions are handled. Linux distro packaging may already provide scrcpy; settings should let users prefer system binary.
- **Verification:** Connect Pixel → click Mirror → window opens, audio plays, recording outputs MP4
- **Complexity:** L
- **Priority:** P1 (slots into R-040)

### F-NEW-07 — Bulk action queue with diff preview

- **Title:** Queue up many disables/uninstalls, preview the diff, apply atomically
- **User problem:** ADB AppControl extended has batch install (paid) but no batch-disable preview. UAD-NG has the closest pattern but rolls all selections into a single click with no diff.
- **Evidence:** ADB AppControl Extended paywalled "Batch applications installation"; UAD-NG selection model is checkbox-list, no diff view
- **Proposed behavior:**
  1. Selecting items in the app list adds to a "Queue" pane (anvil-themed, top-right)
  2. Each queued item shows action (disable / uninstall / clear-data / grant-perm)
  3. "Preview" button shows the synthesized adb commands and the resulting state diff
  4. "Apply" runs them in order with per-step status; failures pause the queue
- **Implementation areas:** Apps React route, `src-tauri/src/queue.rs` (new), persistence in journal (F-NEW-03)
- **Data model:** Queue is in-memory; results land in journal
- **Risks:** Order-dependence for related packages (Facebook coupling per XDA reports). Surface a "this batch may affect X dependents" warning by walking UAD-NG's `neededBy` graph.
- **Verification:** Queue 10 disables, preview diff matches actual outcome, undoing the batch reverses cleanly
- **Complexity:** L
- **Priority:** P1 (slots into R-022)

### F-NEW-08 — Quick search drawer (Ctrl+K)

- **Title:** Spotlight-style global navigation
- **User problem:** With 7+ panes and many actions per pane, mouse-only nav is slow. Power users live by Ctrl+K. None of the competitors offer one.
- **Evidence:** Convention from VS Code, GitHub, Linear, Raycast
- **Proposed behavior:**
  1. Cmd/Ctrl+K opens a centered command palette
  2. Fuzzy matches packages, settings actions, debloat packs, recent commands
  3. Enter executes; Esc closes
- **Implementation areas:** `src/components/CommandPalette.tsx` (new), Zustand store
- **Data model:** Optional MRU cache in localStorage
- **Risks:** Conflicts with browser-style shortcuts in the webview — bind via Tauri's global accelerator API for consistency
- **Verification:** From Apps tab, Ctrl+K → "disable Facebook" → enter → action queued
- **Complexity:** M
- **Priority:** P2 (new — propose **R-024.5**)

### F-NEW-09 — Per-app icon + label resolution

- **Title:** Show real Android app labels and icons in the apps list
- **User problem:** Package names like `com.android.providers.media.module` mean nothing to non-developers. ADB AppControl extracts icons; the free tier doesn't show labels for all apps cleanly.
- **Evidence:** ADB AppControl Extended page emphasizes "Customizable font size and icons" as a paid feature, implying base icon support is shaky
- **Proposed behavior:**
  1. After enumerating packages via `pm list packages -f`, batch-resolve `cmd package list packages --show-versioncode --apex-only` and `aapt2 dump badging` (bundled) for labels + icons
  2. Cache per-device in `package_meta.db`
  3. Fall back to package name on AAPT2 failure
- **Implementation areas:** `src-tauri/src/packages.rs` (new), bundled `aapt2` sidecar or pure-Rust `apkparser` crate
- **Data model:** SQLite cache, expire on app version change
- **Risks:** `aapt2` size (~20MB) — alternative: parse manifest binary XML in Rust via `apk-parser` crate (~50KB)
- **Verification:** Apps list shows "Camera", "Phone", "Messages" with icons instead of fully-qualified package names
- **Complexity:** M
- **Priority:** P1 (slots into R-020)

### F-NEW-10 — Telemetry-free crash reporter (file-only)

- **Title:** Crashes write to a rotating file the user can attach to a bug report
- **User problem:** Closed competitors collect crash data automatically; we explicitly opt out. But we still need diagnostics when users report bugs.
- **Evidence:** ROADMAP R-007 (opt-in telemetry, "no PII") and R-073 (crash reporter, "opt-in Sentry self-hosted")
- **Proposed behavior:**
  1. Rust panics + Promise rejections in JS log to `crash.log` in app data dir (rotating, 1MB cap, last 5)
  2. Settings → "Open crash log folder" button
  3. No network. Optional opt-in upload to a self-hosted Sentry deferred to R-073
- **Implementation areas:** `src-tauri/src/diagnostics.rs` (new), `src/lib/diagnostics.ts`, panic hook in [`lib.rs`](src-tauri/src/lib.rs)
- **Data model:** Plain text
- **Risks:** Logs can contain device serials, app package names — sensitive in some contexts. Add a "scrub before share" feature that masks serials.
- **Verification:** Force a panic → log file appears with stack trace → settings can open the folder
- **Complexity:** S
- **Priority:** P2 (slots into R-007 and supersedes "opt-in telemetry" for v0.1)

---

## Existing Feature Improvements

### IMP-01 — Replace `.expect("error while running droidsmith")` panic with a real failure UX

- **Current:** [`src-tauri/src/lib.rs:13`](src-tauri/src/lib.rs#L13) panics the process if Tauri fails to start
- **Problem:** Users see a closed window. No clue what happened. Corrupt install / missing webview / capability schema mismatch all look the same.
- **Recommended change:** Wrap in a `Result`. On error, log to `crash.log` (F-NEW-10) and show a native message-box via the OS (no webview needed) with the error and a "open log folder" button.
- **Code locations:** [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs), [`src-tauri/src/main.rs`](src-tauri/src/main.rs)
- **Back-compat:** None — pre-release
- **Verify:** Corrupt `tauri.conf.json`, launch, see message box instead of silent crash
- **Complexity:** S
- **Priority:** P1

### IMP-02 — Tighten `shell:default` capability to scoped sidecar commands

- **Current:** [`capabilities/default.json:9`](src-tauri/capabilities/default.json#L9) grants `shell:default`, which permits arbitrary commands from JS
- **Problem:** Even though our renderer only calls Rust IPC handlers today, the capability is a foot-gun. A future XSS-equivalent escape becomes arbitrary shell.
- **Recommended change:** Replace with `shell:allow-execute` entries enumerating exactly the sidecar names + arg regex validators per the [shell plugin docs](https://v2.tauri.app/plugin/shell/). The renderer shouldn't shell out directly at all — always via a Rust command — but defense in depth.
- **Verify:** With the renderer DevTools open, `Command.create('cmd.exe').execute()` returns a permission-denied error
- **Complexity:** S
- **Priority:** P0

### IMP-03 — Bump direct dependencies surfaced by `cargo check`

- **Current:** `thiserror = "1"`, `which = "6"` in [`Cargo.toml`](src-tauri/Cargo.toml)
- **Problem:** Both are stale per resolver warnings
- **Recommended change:**
  - `thiserror = "2"` (API-compatible rename of derive macros)
  - `which = "8"`
- **Code locations:** [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml); no source changes expected for `which`; `thiserror` 2 has minor breaking changes if you use `#[from]` in non-standard positions
- **Verify:** `cargo check` and `cargo test` both pass on Win/macOS/Linux
- **Complexity:** S
- **Priority:** P2

### IMP-04 — `heartbeat` should report Tauri/Rust versions and OS

- **Current:** [`commands.rs:11`](src-tauri/src/commands.rs#L11) returns only `{version, adb_resolved}`
- **Problem:** When users file bugs, we need build/OS context. Better to surface in About panel proactively.
- **Recommended change:** Add `os`, `tauri_version`, `rust_version` (via `env!("CARGO_PKG_RUST_VERSION")` and build-script), `app_data_dir`, `adb_version` (parsed from `adb version` stdout)
- **Code locations:** [`src-tauri/src/commands.rs`](src-tauri/src/commands.rs), new About React component
- **Verify:** About panel shows the full diagnostic; copy-to-clipboard
- **Complexity:** S
- **Priority:** P2

### IMP-05 — Sidebar stubs should preview future shape, not just "Not implemented"

- **Current:** [`App.tsx:70-78`](src/App.tsx#L70-L78) renders all 7 nav items disabled with title="Not implemented yet"
- **Problem:** A contributor opening the app today sees a wall of "not implemented" with no design hint. The disabled state is a missed opportunity to communicate scope.
- **Recommended change:** Each stub becomes a real route that renders a "Coming in R-NNN" panel with a mockup screenshot or wireframe. Same one-screen-per-pane structure as the future shipped UI, but with placeholders.
- **Code locations:** [`src/App.tsx`](src/App.tsx); new `src/routes/{devices,apps,debloat,mirror,console,logcat,fastboot}/` directories
- **Verify:** Visually inspect each route on dev server
- **Complexity:** S
- **Priority:** P3 (cosmetic)

### IMP-06 — `locate_adb` should also try `$ANDROID_HOME` and `$ANDROID_SDK_ROOT`

- **Current:** [`adb.rs:15-29`](src-tauri/src/adb.rs#L15-L29) checks PATH + four hard-coded locations
- **Problem:** Many developers set `$ANDROID_HOME`; we should respect it
- **Recommended change:** Prepend `$ANDROID_HOME/platform-tools/adb[.exe]` and `$ANDROID_SDK_ROOT/platform-tools/adb[.exe]` to candidate list
- **Verify:** Set `ANDROID_HOME` to a temp dir with a fake `adb`, restart app, heartbeat resolves to that fake
- **Complexity:** S
- **Priority:** P2

### IMP-07 — Dev-mirror should also handle WSL paths

- **Current:** [`scripts/dev-mirror.ps1`](scripts/dev-mirror.ps1) targets `C:\tmp\Droidsmith` from `\\vmware-host\Shared Folders\…`
- **Problem:** WSL2 devs hit a similar slow-IO issue when the repo lives on `/mnt/c/...`; they need a sibling shell script
- **Recommended change:** Add `scripts/dev-mirror.sh` (rsync-based) for Linux/WSL/macOS
- **Verify:** Run on WSL with `--watch`, confirm mirror at `~/tmp/Droidsmith` stays in sync
- **Complexity:** S
- **Priority:** P3

---

## Reliability, Security, Privacy, and Data Safety

### Risks identified

1. **R-Sec-01 (Verified, P0):** Unscoped `shell:default` capability. See IMP-02.
2. **R-Sec-02 (Likely, P1):** No CSP for connect-src to mDNS endpoints yet — when wireless ADB lands, the CSP at [`tauri.conf.json:28`](src-tauri/tauri.conf.json#L28) will need updating. Document the security model alongside R-015.
3. **R-Rel-01 (Verified, P1):** `.expect(...)` panic on Tauri init failure (IMP-01).
4. **R-Rel-02 (Likely, P1):** `which::which("adb")` calls a syscall — if PATH is huge (e.g. Conda environments) and the binary doesn't exist, latency can be 50-100ms. Acceptable but log timing.
5. **R-Priv-01 (Assumption, P0):** No telemetry, no analytics, no third-party network calls today. **Verify** every new dep doesn't add hidden phone-home (e.g. `mdns-sd` only multicasts on LAN; updater plugin only hits configured endpoint).
6. **R-Data-01 (Likely, P1):** Future SQLite journal must live outside the repo / outside HGFS to avoid corruption (memory: [`vmware-hgfs-limitations`]). Default to `~/.config/Droidsmith/` on Linux, Tauri's `path::app_data_dir()` on all platforms.
7. **R-Data-02 (Verified, P0):** `pm uninstall --user 0` is **irreversible** at the OS level for the current user. The journal must clearly distinguish "disable" (reversible) from "uninstall" (effectively permanent unless a factory reset re-installs from system image). Naming the action in UI matters.

### Missing guardrails

- Confirmation dialog for `pm uninstall --user 0` and `pm clear-data` actions, with the exact command shown before "Apply"
- A "safe mode" toggle that hides `Expert` / `Unsafe` debloat-level packages from selection until explicitly opted in
- Per-action timeout (default 10s) so a hung `adb shell pm ...` doesn't deadlock the queue

### Recovery / rollback

- The undo journal (F-NEW-03) is the primary recovery surface
- A "Restore from snapshot" flow that re-enables every package the user disabled within a date range — important for "I broke something three weeks ago" scenarios
- Profile YAML imports should run a dry-run preview by default

### Logging / diagnostics

- File-only crash log (F-NEW-10) is the floor
- Per-session ADB transcript saved to `transcripts/<device-serial>/<iso8601>.log` so users can attach the exact command sequence when filing a bug
- Verbose mode toggle in Settings: `RUST_LOG=droidsmith=debug` semantically

---

## UX, Accessibility, and Trust

### Onboarding gaps

- **First-run experience:** Today, on first launch the user sees "adb: not detected" with no next step. Need a 3-step onboarding: (1) detect/bundle adb, (2) help enable USB debugging or wireless debugging, (3) pair a device.
- **Empty state for apps list / debloat:** Need empty states with explanatory copy + actionable buttons ("Pair a device" / "Connect USB").
- **No tooltips on debloat package descriptions:** UAD-NG includes a description per package — we must too, and surface it on hover/expand.

### Loading / error / disabled states

- The `heartbeat` panel at [`App.tsx:49-63`](src/App.tsx#L49-L63) is the only current state; it has `error: {err}` rendered red but no retry button. Every Rust IPC call needs the same shape: loading → error-with-retry → success.
- Disabled nav items: prefer a "Coming in R-NNN" badge over `cursor-not-allowed` (IMP-05).

### Destructive / irreversible actions

- See R-Data-02. Two confirmations for `uninstall --user 0`: a primary "Are you sure?" and a typed-confirmation for batches >5 items.

### Settings clarity

- "Use bundled adb" toggle (F-NEW-02) must show both paths (system + bundled) and the resolved version of each.
- "Telemetry" setting should be off and labeled "We collect nothing. This setting exists in case we ever do, so you can refuse." Until then it's a no-op informational row.

### Accessibility

- ROADMAP R-071 reserves an accessibility audit. Earlier-than-that low-hanging fruit:
  - All nav items must be `<button>` not `<div>` for keyboard nav (currently [`App.tsx:71-78`](src/App.tsx#L71-L78))
  - Set `aria-label` on icon-only buttons before they ship
  - Tailwind's `anvil` palette needs a contrast verification pass — `anvil-400` on `anvil-900` is ~3.5:1; below WCAG AA for body text. Tighten before R-070.

### Microcopy / trust signals

- About panel should prominently say "MIT-licensed, no telemetry, no paywall" with a link to the source and to `LICENSE`
- Settings → Data → "Where is my data?" — a single page that lists every file we create with its absolute path. UAD-NG does not do this; almost nothing does. Strong trust signal.

---

## Architecture and Maintainability

### Module / boundary suggestions

The current `src-tauri/src/` is `lib.rs + main.rs + adb.rs + commands.rs`.
That's right-sized for today. Proposed structure once features land:

```
src-tauri/src/
  lib.rs              # Tauri Builder, plugin registration only
  main.rs             # process entry
  adb/                # ADB transport, device model, sidecar resolver
    mod.rs
    transport.rs      # USB / TCP / server-proxy
    device.rs         # Device, App, Permission domain types
    pair.rs           # Android 11+ wireless pair (shells out)
  scrcpy.rs           # scrcpy supervisor
  fastboot.rs         # fastboot supervisor
  packs/              # debloat pack loader + validator
  quirks/             # OEM quirks engine
  journal/            # SQLite undo log
  profile.rs          # YAML profile runner
  cli/                # headless CLI entry (separate [[bin]])
  diagnostics.rs      # crash log, transcript log
  commands.rs         # only Tauri #[command] glue, no business logic
```

Keep `commands.rs` thin (Tauri command annotations + serialization only);
business logic lives in domain modules so it's `cargo test`-able without a
Tauri runtime.

### Refactor candidates

- None today (the codebase is too young). When the queue (F-NEW-07) and
  journal (F-NEW-03) land, extract a `OperationLog` trait so both can share
  the same persistence layer.

### Test gaps

- **Zero tests today.** Establish the floor in R-003:
  - Rust: `cargo test --workspace` runs against a mock ADB transport
  - Frontend: Vitest unit tests for stores + reducers, Playwright for end-to-end (defer until R-006)
  - Pack lint: `cargo run --bin pack-lint -- packs/*.yaml` as a separate `[[bin]]`

### Documentation gaps

- `CONTRIBUTING.md` doesn't exist yet (ROADMAP R-005)
- `docs/ARCHITECTURE.md` for new contributors — defer until first feature lands so it documents reality not aspiration
- `docs/SECURITY.md` for vuln disclosure — needed before first release
- `docs/PACKS.md` — schema reference for community pack authors (companion to R-030)

### Release / build gaps

- No CI (R-003)
- No code signing (R-006). Windows: EV cert; macOS: Apple Developer ID + notarization; Linux: distro packages don't strictly need signing
- No SBOM generation — for a security-adjacent tool, ship CycloneDX SBOM with each release

---

## Prioritized Roadmap

Slots into the existing milestone structure in [ROADMAP.md](ROADMAP.md).
New R-numbers proposed where they don't already exist. Each item is shaped
as the brief requires.

### Phase 0 — Harden the scaffold (before any features)

- [ ] P0 — **IMP-02** Scope `shell:default` to enumerated sidecar commands
  - Why: Unscoped shell capability is a foot-gun even pre-XSS
  - Evidence: [`capabilities/default.json:9`](src-tauri/capabilities/default.json#L9); [Tauri shell plugin permission model](https://v2.tauri.app/plugin/shell/)
  - Touches: [`src-tauri/capabilities/default.json`](src-tauri/capabilities/default.json)
  - Acceptance: Renderer-side `Command.create("cmd")` fails with permission-denied; sidecar `adb devices` from Rust still works
  - Verify: From devtools, `import { Command } from "@tauri-apps/plugin-shell"; await Command.create("powershell").execute()` rejects
- [ ] P0 — **R-003** CI matrix (Win/macOS/Linux): `cargo check`, `cargo clippy`, `cargo test`, `npm run typecheck`, `npm run lint`, `npm run test`
  - Why: First feature should ship green; without CI we're flying blind on macOS/Linux
  - Evidence: No `.github/` directory exists
  - Touches: `.github/workflows/ci.yml` (new)
  - Acceptance: Workflow turns green on each push to `master`
  - Verify: Open the workflow run in the Actions tab
- [ ] P1 — **IMP-01** Replace `.expect(...)` startup panic with a graceful native error dialog
  - Why: Silent crashes lose first-impression users
  - Evidence: [`src-tauri/src/lib.rs:13`](src-tauri/src/lib.rs#L13)
  - Touches: [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs), `src-tauri/src/main.rs`
  - Acceptance: Force an init failure (e.g., remove `icon.png`); see a message-box; process exits nonzero
  - Verify: Manual — temporarily rename `icon.png` and launch
- [ ] P2 — **IMP-03** Bump `thiserror 1→2`, `which 6→8`
  - Why: Drift is cheap to fix now; expensive later when something depends on the major
  - Evidence: `cargo check` resolver warnings (see Evidence section)
  - Touches: [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml)
  - Acceptance: `cargo check` and `cargo test` clean on all three OSes
  - Verify: CI matrix from R-003

### Phase 1 — End-to-end thin slice

Goal: A user can install Droidsmith, connect a Pixel, see the apps list,
disable Facebook, and undo it — all in one session, with no Android SDK.

- [ ] P0 — **R-010 (expand) / F-NEW-02** Bundle `adb` and `fastboot` as Tauri sidecars
  - Why: Removes the biggest onboarding friction
  - Evidence: [Tauri sidecar guide](https://v2.tauri.app/develop/sidecar/); [`adb.rs`](src-tauri/src/adb.rs) returns None today
  - Touches: [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) (`bundle.externalBin`), [`capabilities/default.json`](src-tauri/capabilities/default.json), [`src-tauri/src/adb.rs`](src-tauri/src/adb.rs), `scripts/fetch-platform-tools.{ps1,sh}` (new), `src-tauri/binaries/` (new)
  - Acceptance: On a clean Windows VM with no Android SDK, the heartbeat panel resolves to the bundled adb path
  - Verify: Wipe `%LOCALAPPDATA%\Android`, launch, check heartbeat
- [ ] P0 — **R-011** Wire `adb_client` 3.2.1 for device list, with a sidecar `adb` fallback
  - Why: Faster than shelling for the hot path; matches "Bring your own adb" tenet
  - Evidence: [adb_client crate](https://github.com/cocool97/adb_client)
  - Touches: [`Cargo.toml`](src-tauri/Cargo.toml) (add `adb_client = "3"`), `src-tauri/src/adb/transport.rs` (new)
  - Acceptance: `invoke("list_devices")` returns `[{serial, state, model}]`
  - Verify: USB-attach a phone, see it appear in <500ms
- [ ] P0 — **R-012** Devices route: device strip + hotplug events
  - Why: First real UI surface
  - Evidence: ROADMAP R-012 is currently TODO
  - Touches: `src/routes/devices/`, `src-tauri/src/adb/hotplug.rs` (new — uses tokio `watch` on `adb track-devices`)
  - Acceptance: Plugging/unplugging a device updates the strip without refresh
  - Verify: Plug → see device → unplug → see "disconnected" greyed
- [ ] P0 — **R-015 (expand) / F-NEW-01** Wireless pairing wizard
  - Why: Most-requested missing feature in competitive products
  - Evidence: [Android wireless ADB docs](https://developer.android.com/tools/adb)
  - Touches: `src-tauri/src/adb/pair.rs` (new), `src-tauri/src/mdns.rs` (new, depends on `mdns-sd` crate), `src/routes/devices/Pair.tsx` (new)
  - Acceptance: Pair a real Pixel via 6-digit code and via mDNS auto-discovery
  - Verify: Real-device test on home Wi-Fi
- [ ] P0 — **R-020** Apps list with filters
  - Why: Core workflow
  - Evidence: ROADMAP R-020/R-021 TODO
  - Touches: `src/routes/apps/`, `src-tauri/src/adb/packages.rs` (new)
  - Acceptance: Lists user + system apps with label, package, version; filters (user/system/disabled) work
  - Verify: Pixel + Samsung — counts match `pm list packages` output
- [ ] P0 — **F-NEW-09** Resolve real labels + icons (no AAPT2 dep — use pure-Rust apk parsing)
  - Why: Package names are unreadable; this is the single biggest UX upgrade
  - Evidence: Competitive analysis above; ADB AppControl Extended paywall
  - Touches: `src-tauri/src/adb/packages.rs`, `Cargo.toml` add `apk-parser` (~50KB) or `axmldecoder`
  - Acceptance: List shows "Camera", "Messaging" with icons instead of package names
  - Verify: Compare to Android Settings → Apps screen
- [ ] P0 — **R-022 + F-NEW-07** Single-action disable / uninstall with confirmation
  - Why: First destructive action; must establish UX patterns now
  - Evidence: Risk R-Data-02
  - Touches: `src/routes/apps/AppRow.tsx`, `src-tauri/src/adb/actions.rs` (new)
  - Acceptance: Disable Facebook → confirmation dialog shows exact command → applied → app shows as disabled
  - Verify: Real device; subsequent `pm list packages -d` includes the package
- [ ] P0 — **F-NEW-03 / proposed R-026.5** Per-device undo journal
  - Why: Reversibility is a design tenet; ship with the first destructive action, not later
  - Evidence: Risk R-Data-02; UAD-NG snapshot+restore pattern
  - Touches: `src-tauri/src/journal/{mod.rs,schema.sql}` (new), `src/routes/activity/` (new)
  - Acceptance: Disable Facebook → journal row appears → click undo → app re-enabled and journal row marked reverted
  - Verify: Restart app between disable and undo; both rows persist

### Phase 2 — Debloat engine (the headline)

- [ ] P1 — **R-030** Define `packs/*.yaml` schema with JSON Schema validation
  - Why: Pack quality depends on a strict shape; community PRs need a CI lint gate
  - Evidence: UAD-NG schema (`list`/`removal`/`description`/`dependencies`/`neededBy`/`labels`) is a proven baseline
  - Touches: `packs/schema.json` (new), `cli/pack-lint/` (new `[[bin]]`)
  - Acceptance: `cargo run -p pack-lint -- packs/*.yaml` exits 0 on valid, nonzero with specific errors on invalid
  - Verify: Hand-craft a broken pack, run lint
- [ ] P1 — **R-032** Seed pack: Pixel
  - Why: Easiest to validate (vanilla AOSP behavior, well-documented)
  - Evidence: UAD-NG ships a Pixel-relevant set
  - Touches: `packs/pixel.yaml` (new)
  - Acceptance: Lints clean; covers ≥30 known-safe Pixel packages with descriptions and removal levels
  - Verify: Compare against UAD-NG's Pixel-tagged entries
- [ ] P1 — **R-032+** Seed packs: OneUI (Samsung), HyperOS (Xiaomi), ColorOS (Oppo/OnePlus)
  - Why: Biggest market share OEMs; XDA traffic concentrated here
  - Evidence: XDA debloat tags + UAD-NG OEM coverage
  - Touches: `packs/{oneui,hyperos,coloros}.yaml` (new)
  - Acceptance: Each lints clean, covers ≥50 entries, includes vendor-lock notes for known-blocked packages
  - Verify: Sanity-check on a representative device per OEM
- [ ] P1 — **R-033** Debloat wizard UI with preview diff + undo
  - Why: The pack data is useless without a flow that applies it safely
  - Evidence: Risk R-Data-02 + F-NEW-07 batch pattern
  - Touches: `src/routes/debloat/`
  - Acceptance: Pick pack → see categorized list with checkboxes pre-set by removal-level → preview → apply → journal rows for every action
  - Verify: Apply on test device, undo via journal, verify all packages re-enabled
- [ ] P1 — **F-NEW-04 / R-034** Vendor-lock quirks engine
  - Why: Silent failures are the #1 ADB AppControl complaint; we explicitly solve this
  - Evidence: XDA HyperOS / BBK threads
  - Touches: `src-tauri/src/quirks/` (new), `quirks/*.yaml`
  - Acceptance: On HyperOS, attempting to disable a vendor-locked package shows "Xiaomi blocks `pm disable` for this package; use `pm uninstall --user 0` instead" with a one-click alternative
  - Verify: Real-device HyperOS test
- [ ] P2 — **R-036** Import UAD-NG list as supplementary data source
  - Why: Their list is the best in the world; don't compete, integrate
  - Evidence: UAD-NG GPL-3.0 license (list data, not linked code) + their `uad_lists.json` schema
  - Touches: `scripts/sync-uad-list.{ps1,sh}` (new), `packs/_uad-supplement.yaml` (generated)
  - Acceptance: A pinned `uad_lists.json` revision lands as a supplementary pack with attribution
  - Verify: Check NOTICE file credits UAD-NG; pack lints clean

### Phase 3 — Mirror, console, logcat

- [ ] P1 — **R-040 / F-NEW-06** scrcpy 4.0 sidecar + Mirror route
  - Why: Mirror is on the ADB AppControl free tier (buttons only); ours starts at full mirror
  - Evidence: scrcpy 4.0 is Apache-2.0; bundle is fine
  - Touches: `src-tauri/binaries/scrcpy-*`, `src-tauri/src/scrcpy.rs` (new), `src/routes/mirror/`
  - Acceptance: Click Mirror → window opens with audio; drag APK in installs; drag file in pushes
  - Verify: Real-device test
- [ ] P1 — **R-050** ADB console (multi-tab, history, favourites)
  - Why: Power-user staple
  - Touches: `src/routes/console/`, `src-tauri/src/adb/shell.rs` (new — uses `adb_client` shell)
  - Acceptance: Tab 1 runs `pm list packages`, tab 2 runs `dumpsys battery` simultaneously
  - Verify: Real-device
- [ ] P1 — **R-051** Logcat viewer with tag/pid/level filters
  - Why: Standard developer tool
  - Touches: `src/routes/logcat/`, `src-tauri/src/adb/logcat.rs` (new)
  - Acceptance: Live tail; filter by `tag=WindowManager`; clear; save to file
  - Verify: Open Settings → see Settings tag in logs

### Phase 4 — Automation & power tools

- [ ] P1 — **R-060 / F-NEW-05** Profile YAML runner (dry-run + apply)
  - Why: No competitor offers this; real differentiator for refurbishers/QA
  - Evidence: ROADMAP R-060/R-061
  - Touches: `src-tauri/src/profile.rs` (new), `examples/profiles/*.yaml`
  - Acceptance: `droidsmith run examples/profiles/pixel-fresh.yaml --device emulator-5554 --dry-run` outputs a plan; `--apply` executes
  - Verify: CI runs against an emulator
- [ ] P1 — **R-061** Headless CLI as a separate `[[bin]]`
  - Why: Tauri GUI build is large; CI consumers want a small headless binary
  - Touches: `src-tauri/Cargo.toml` (`[[bin]] name = "droidsmith-cli"`), `src-tauri/src/cli/`
  - Acceptance: `cargo build --release --bin droidsmith-cli` produces a <20MB binary
  - Verify: `droidsmith-cli --help` lists subcommands
- [ ] P2 — **F-NEW-08 / proposed R-024.5** Ctrl+K command palette
  - Why: Power-user navigation
  - Touches: `src/components/CommandPalette.tsx` (new)
  - Acceptance: Ctrl+K → fuzzy search → enter executes
  - Verify: Manual

### Phase 5 — Polish & release

- [ ] P1 — **R-006** Release pipeline: signed Win MSI, notarized macOS dmg, Linux AppImage + deb
  - Why: Trust signal; reduces SmartScreen / Gatekeeper friction
  - Evidence: [Tauri updater + signing docs](https://v2.tauri.app/plugin/updater/)
  - Touches: `.github/workflows/release.yml` (new), Ed25519 keys in repo secrets
  - Acceptance: Tagging `v0.1.0` produces six artefacts + checksums + SBOM + signature
  - Verify: Download installer, run on clean VM
- [ ] P1 — **F-NEW-10 / R-007 (expand)** File-only crash log + scrub-before-share
  - Why: Diagnostics without telemetry
  - Touches: `src-tauri/src/diagnostics.rs` (new), Settings → Diagnostics page
  - Acceptance: Force a panic → log appears; "Open log folder" works; "Scrub serials" produces a redacted copy
  - Verify: Manual panic injection
- [ ] P2 — **R-070** i18n: en, ru (parity with ADB AppControl), es, de, pt-BR, zh-CN
  - Touches: `src/i18n/`, `i18next` config
  - Acceptance: Language switcher in Settings; reload-free language change
- [ ] P2 — **R-071** Accessibility audit (keyboard nav, ARIA, contrast)
  - Acceptance: All interactive elements keyboard-reachable; AA contrast everywhere

---

## Quick Wins

Each is <30 min once R-003 CI is in place.

1. **IMP-02** scope shell capability — 10 min
2. **IMP-03** bump `thiserror` and `which` — 5 min
3. **IMP-04** add OS/Tauri/Rust version to heartbeat — 15 min
4. **IMP-06** add `ANDROID_HOME` / `ANDROID_SDK_ROOT` to adb resolver — 5 min
5. **IMP-01** Tauri init error dialog — 30 min
6. Tighten CSP — drop `'unsafe-inline'` for styles once Tailwind extraction works (Vite already does this in build mode)
7. Add `.github/ISSUE_TEMPLATE/bug.md` and `feature.md` — 15 min
8. Add `SECURITY.md` with disclosure email + PGP fingerprint placeholder — 10 min
9. Replace `<div>` nav stubs with `<button>` (IMP-05 prefix) — 10 min
10. Add a `LICENSE-THIRD-PARTY.md` that will accumulate notices as we bundle adb, scrcpy — 5 min

---

## Larger Bets

These need explicit design rounds before implementation.

1. **Plugin / wasm-host architecture (R-062, R-063)** — Loading third-party packs is easy (YAML data); loading third-party _code_ that can run new ADB flows is hard. Open questions: wasm-component-model vs. Rust dylib? Capability model for plugins? Marketplace signing? Defer until v0.3.
2. **Mobile / TV companion** — ADB AppControl has Android phone + TV companion apps. This is explicitly DROP'd in ROADMAP but worth re-evaluating after v0.2 once core flows are stable; scrcpy + wireless ADB already covers most of the use case.
3. **Cloud-synced profiles + journal** — Useful for refurbishing fleets; introduces auth, server, encryption. Treat as a separate product (`droidsmith-fleet`) not a core feature.
4. **Auto-detect-and-fix common issues** — "Your phone reboots randomly → here are 3 likely apps to disable based on crash logs." This is an ML/heuristic engine, not a debloater. Genuinely high-value but a different project.
5. **Driver helper for Windows (WinUSB)** — Most ADB onboarding friction on Windows is driver installation. A bundled Zadig-style helper would be powerful but risky (UAC, signed driver issues, OEM-specific drivers). Defer until v0.4.

---

## Explicit Non-Goals

1. **Re-implementing scrcpy or `adb` from scratch.** Both are mature, open, and well-licensed. Integrate, don't reinvent.
2. **Rooting / unlocking devices.** Out of scope — too OEM-specific, legally fraught in some jurisdictions, and tools like Magisk already handle this well.
3. **Custom ROM flashing UI.** Different audience, different trust model.
4. **In-app purchases, donation nags, sponsor splash screens.** Hard no, per design tenet.
5. **Telemetry on by default.** Hard no. Crash logs go to disk only until R-073 introduces opt-in upload — and even then, file-only stays available.
6. **Web/browser deployment.** ya-webadb already covers that lane.
7. **Mobile companion app.** scrcpy + wireless ADB is the modern answer; we don't need our own Android app for v0/v1.
8. **Closed-source dependencies.** Every bundled binary must be re-buildable from source under a license compatible with MIT redistribution.

---

## Open Questions

Only questions that block correct prioritization or implementation. Not
questions answerable by reading code or public docs.

1. **What's the GitHub org for the remote?** [`src-tauri/Cargo.toml:7`](src-tauri/Cargo.toml#L7) declares `SysAdminDoc/Droidsmith`. The memory file [`sysadmindoc-git-auth`] notes some SysAdminDoc/* repos hit 403 from this VM. Confirm before R-006 release pipeline so the GitHub Actions setup uses the right org and we know whether pushes will work from the dev VM or only from the main desktop PC.
2. **Code-signing certificates.** Windows EV cert (~$300/yr) and Apple Developer ID (~$99/yr) are the gating items for R-006. Is the user willing to fund these, or do we ship unsigned with SmartScreen friction documented?
3. **UAD-NG list redistribution.** UAD-NG is GPL-3.0 for *code*. Their `uad_lists.json` is a data file — typically not viral. But we should email the maintainers and confirm before shipping a derived pack (`packs/_uad-supplement.yaml`) at R-036. Default to "ask first" for community goodwill.
4. **Telemetry policy.** ROADMAP R-007 says "opt-in only, single boolean, no PII, document exactly what is sent." Need to write that document before any code lands. Specifically: do we keep the "off until you opt in" stance forever, or eventually flip to "anonymous usage stats on by default"? Recommend the former; need explicit signoff.
5. **Trademark / branding for "Droidsmith".** Quick USPTO + EU TM search advisable before R-006 to avoid a forced rename post-launch.

---

*End of research pass. Next obvious action: R-003 (CI matrix), which unlocks
the Phase 0 quick wins.*
