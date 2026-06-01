# Droidsmith — Research & Feature Plan

> **For the full evidence-grounded deep dive — feature inventory, competitive
> research with verified primary sources, security audit, prioritized
> checkbox roadmap with acceptance criteria — see
> [RESEARCH_DEEPDIVE.md](RESEARCH_DEEPDIVE.md).** This file is the elevator
> pitch.

This document captures (a) what we learned about ADB AppControl and the
broader ADB-GUI landscape, (b) what Droidsmith should look like to be a
strictly better open-source replacement, and (c) the technical choices we are
locking in for the initial implementation.

It is the source of truth that the [ROADMAP](ROADMAP.md) draws from. Update
this file before adding new R-NNN items.

## 1. Subject of study: ADB AppControl 1.8.6

### Feature inventory (May 2026)

App management
- Install / disable / uninstall without root
- Batch install
- APK / APKS extraction off-device
- Split-APK install
- Per-app permission management
- Quick search in Play Store, ApkMirror, F-Droid

Device control
- Virtual buttons (D-pad, volume, power, etc.)
- Screenshot
- Resolution / DPI overrides
- Hide status-bar icons
- Reboot to recovery / bootloader / fastboot

Power tools
- ADB console with favourites
- Fastboot support
- Logcat viewer
- File push
- Auto-grant permissions for popular apps
- Debloat Wizard with recommendations
- Device info panel

Distribution
- Windows 7/8/10/11, needs .NET Framework 4.6+
- Companion Android phone app
- Separate Android TV app

### Business model

Freemium. Core build is free. Extended build, unlocked via sponsor donation,
adds: Process Manager, dark theme, more batch operations.

### Weaknesses we are exploiting

1. **Closed source** — community cannot fix bugs or extend it, and the
   licence forbids modification. This is the single biggest reason an
   open replacement exists.
2. **Windows-only.** Excludes the entire macOS/Linux developer and
   power-user audience.
3. **Paywall friction** — dark theme behind a donation in 2026 is hard to
   defend.
4. **Debloat Wizard underperforms.** XDA reviewers report it removes "almost
   nothing even on the highest setting" compared with Universal Android
   Debloater Next Generation. The recommendation set is small and not
   community-maintained.
5. **Vendor-locked devices fail silently.** HyperOS (Xiaomi) and BBK family
   (OPPO/Vivo/Realme) block `pm disable` for many packages; ADB AppControl
   doesn't explain why and just looks broken.
6. **No automation surface.** No CLI, no profiles, no scripting — every
   action must be clicked.
7. **Russian-leaning project.** English UX is second-class and translations
   beyond EN/RU are crowdsourced ad-hoc.

## 2. Competitor scan

| Tool | Open source | Cross-platform | Strength | Why it doesn't replace ADB AppControl |
|---|---|---|---|---|
| [Universal Android Debloater NG](https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation) | GPL-3.0, Rust + iced | Yes | Best debloat list curation in the wild | Debloat-only — no app install, no scrcpy, no logcat |
| [scrcpy](https://github.com/Genymobile/scrcpy) | Apache-2.0, C | Yes | Best-in-class mirror + control + audio | No GUI for app/debloat/log management |
| [Tango ADB](https://github.com/yume-chan/ya-webadb) | MIT, TypeScript | Browser | Pure-web ADB over WebUSB | Browser-only, slow on bulk ops |
| Aya — Android ADB Desktop | Closed | Win/macOS | Modern UI | Same closed-source problem as ADB AppControl |
| Xtreme ADB | Closed | Win | Power-user oriented | Closed |
| Android Ultimate Toolbox Pro | Closed | Win | Broad features | Closed, paid |

The opportunity: **none of the open tools cover the full ADB AppControl
surface area.** UAD-NG owns debloat. scrcpy owns mirroring. Nothing owns the
"all-in-one daily driver" slot in an open form. Droidsmith fills that gap by
**integrating** the proven open pieces (scrcpy, UAD-NG lists) rather than
re-implementing them.

## 3. Design tenets

1. **All features free, forever.** No sponsor gate, no nag screens, no
   telemetry-by-default. Donations link in About, that's it.
2. **Cross-platform from commit 1.** No Windows-only APIs in the Rust core.
3. **Bring your own ADB.** Bundle a known-good `platform-tools` build, but
   let the user point at a system `adb` for compatibility with their
   existing setup.
4. **Don't lie to the user.** When an OEM blocks a debloat target, surface
   the exact reason and the workaround (e.g. "HyperOS blocks `pm disable
   --user 0` for this package; mi-account workaround required").
5. **Reversible by default.** Every destructive action goes into a
   per-device journal so the user can undo months later.
6. **Plugin-first for OEM quirks.** OEM-specific behaviour lives in a
   plugin (`packs/hyperos/`, `packs/oneui/`) so contributors who own one
   device family can move quickly.
7. **Automation parity.** Anything clickable in the GUI is scriptable from
   the CLI, and vice versa. CI flows are a first-class user.

## 4. Tech stack — locked

| Layer | Pick | Why this and not the alternative |
|---|---|---|
| Shell | **Tauri 2** | ~10 MB binary vs Electron's ~100 MB; system webview; mature in 2026 |
| Backend lang | **Rust** | `adb_client` exists, scrcpy supervisor is straightforward, future wasm plugin host is simplest in Rust |
| Frontend | **React 18 + TypeScript + Vite** | Largest contributor pool, mature, fits the user's other projects |
| Styling | **Tailwind + shadcn/ui** | Dark-first, accessible, swap-out-able primitives |
| State | **Zustand** | Lightweight, no Redux ceremony |
| i18n | **i18next + react-i18next** | De-facto standard, easy contributor flow |
| ADB | **`adb_client` crate, fallback to bundled `adb`** | Pure-Rust path for speed, binary path for compatibility |
| Mirror | **scrcpy (external process)** | Don't re-invent the wheel |
| Tests | **`cargo test` + Vitest + Playwright** | Standard |
| CI | **GitHub Actions** | Matrix Win/macOS/Linux |
| Packaging | **`tauri build`** | Per-OS installers; Windows code-sign + macOS notarization via GH secrets |

### Rejected alternatives

- **Electron** — bundle size and memory cost are unjustifiable for a tool
  meant to sit in the tray.
- **Flutter Desktop** — Dart pool is small for desktop contributors; tooling
  around `adb` is weaker than Rust.
- **Wails (Go)** — viable, but Go's ADB library landscape is thinner than
  Rust's, and the user's other Tauri experience tips the balance.
- **WPF / .NET MAUI** — re-creates the ADB AppControl mistake of locking
  ourselves to one platform.

### Known build gotcha to plan around

The VM development host mounts repos over VMware HGFS, where Vite chokes on
the space in `\\vmware-host\Shared Folders\`. Dev loop will need a
`mirror-to-C:\tmp\Droidsmith` step, scripted in `scripts/dev-mirror.ps1`.
This is documented and will land with R-002.

## 5. Initial milestone — what "v0.1" looks like

The smallest release that is already useful on its own:

- Single binary on Windows, macOS, Linux
- Detect USB + wireless devices
- App list with filters (user/system/disabled)
- Single uninstall, single disable, single APK extract
- Pixel + OneUI + HyperOS debloat pack with preview-diff + undo journal
- scrcpy launched from a "Mirror" button
- ADB console tab
- All free, all open, no telemetry

That is ROADMAP items R-001 through R-035 minus the bits flagged `parallel`.

## 6. Open questions

- **Licence — MIT or GPL-3.0?** MIT chosen for adoption; revisit if a
  closed-source fork emerges.
- **Distribution** — winget + Homebrew cask + Flathub. Order TBD; depends on
  R-006 release pipeline.
- **Telemetry** — start with none. Decision on opt-in metrics deferred until
  after v0.2 when we have real usage to learn from.
- **`adb_client` vs bundled binary as the default path** — benchmark first,
  pick after R-011.
