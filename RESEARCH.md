# Research - Droidsmith

## Executive Summary
Droidsmith is a Tauri 2 + React + Rust ADB workstation that has moved beyond its original pre-functional shell into live device discovery, wireless pairing, package actions, debloat packs, scrcpy launch, logcat, file, process, network, backup, fastboot, and i18n surfaces. Its strongest direction is still the one stated in `RESEARCH_REPORT.md`: a unified offline ADB workshop that integrates proven ecosystems instead of cloning one narrow debloater or mirror GUI. The highest-value opportunities are: ship bundled `packs/` and `quirks/` so the Debloat route works in production; patch the Vite advisory reported by `npm audit`; expose the already-built journal/undo commands in the Apps UI; add batch operation progress, cancellation, and recovery; make backup honest for modern Android; harden file/network parsers with transcript fixtures; deepen scrcpy session controls; reconcile stale docs and README claims with the current live routes; add a language switcher and translation contribution path; and create local release/audit gates that replace Dependabot noise.

## Product Map
- Core workflows: detect USB/wireless ADB devices, inspect packages and runtime permissions, disable/uninstall/enable apps with journaled actions, apply OEM debloat packs, launch scrcpy/recording, inspect logs/files/processes/network, run shell/fastboot utilities.
- User personas: Android power users replacing closed ADB AppControl workflows, repair/workbench operators handling multiple devices, privacy-focused debloat users, contributors maintaining OEM package packs and quirks.
- Platforms and distribution: Windows/macOS/Linux desktop via Tauri; current release is v0.1.0; sidecar platform-tools/scrcpy distribution remains blocked in `Roadmap_Blocked.md`.
- Key integrations and data flows: Rust shell transport resolves system `adb`; frontend IPC wrappers in `src/lib/tauri.ts`; JSONL per-device journal under app data; pack YAML in `packs/`; quirk YAML in `quirks/`; scrcpy and fastboot currently resolved from PATH.

## Competitive Landscape
- Universal Android Debloater NG: best-in-class community debloat data, warnings, wiki, update/privacy notes. Learn its list curation, removal tiers, and explicit risk framing; avoid silently depending on upstream data without redistribution permission.
- scrcpy: best-in-class mirroring/control/audio/recording with low latency, no account, no ads, and no device app. Learn its option depth, session presets, and official-source warning; avoid reimplementing video/control transport.
- App Manager: deepest package-management benchmark with permissions, app ops, signatures, trackers, backups, batch operations, logcat, profiles, file manager, and terminal. Learn density and inspection depth; avoid root-only assumptions and mobile-only UX.
- Canta: proves UAD-style recommendations can be understandable to non-root users and shows restore/uninstalled-state expectations. Learn plain warnings and package descriptions; avoid requiring Shizuku or an Android companion app for Droidsmith's desktop path.
- Escrcpy: strongest GUI benchmark for multi-device scrcpy orchestration, keyboard mapping, wireless discovery, batch screenshots/install, and control bars. Learn multi-device session management; avoid cloud or opaque automated-control features that conflict with Droidsmith's offline privacy pitch.
- Tango/ya-webadb: proves TypeScript ADB protocol layers and browser ADB are viable. Learn typed protocol boundaries and WebUSB-adjacent UX; avoid pivoting Droidsmith into a browser-only tool while native sidecars and filesystem access are core needs.
- ADB AppControl / Vysor / AirDroid: commercial benchmarks make dark theme, batch operations, mirroring, wireless use, file transfer, and recovery/paywall gaps visible. Learn polish and operator workflow packaging; avoid paywalls, accounts, telemetry, and Windows-only assumptions.

## Security, Privacy, and Reliability
- `npm audit --json` reports high-severity Vite advisory `GHSA-fx2h-pf6j-xcff` and moderate `js-yaml` advisory `GHSA-h67p-54hq-rp68`; `npm outdated --json` shows Vite 6.4.2 with wanted 6.4.3 and latest 8.1.0.
- `cargo-audit` was not installed; the first install failed while compiling `crossbeam-epoch` with no usable diagnostic in `C:\Users\xray\AppData\Local\rtk\tee\1782616489_cargo_install.log`, and the retry hung in `rustc` until terminated, so RustSec coverage still needs a working local gate.
- `src-tauri/src/commands.rs:list_packs` reads `app.path().resource_dir().join("packs")`, but `src-tauri/tauri.conf.json` has no `bundle.resources`; production debloat packs will be empty unless resources are bundled or dev-path fallback is explicit.
- `src-tauri/src/commands.rs:explain_failure` still requires `quirks_path`, so vendor-specific mitigation data is not wired into app resources.
- `src/routes/Apps.tsx` can apply destructive package actions, and `src/lib/tauri.ts` exposes `journal_list`/`journal_undo`, but there is no visible journal/undo workflow in the Apps route.
- `src/routes/Debloat.tsx` applies selected pack entries sequentially from the renderer with only an errors array; no cancel, pause, retry, live journal summary, or before/after package state verification is shown.
- `src-tauri/src/commands.rs:backup_package` shells out to `adb backup` and `src/routes/Apps.tsx` saves `${pkg}.ab` without a path picker or compatibility warning; modern Android backup behavior is app-controlled, so users need clear limitations before trusting it.

## Architecture Assessment
- Move repeated device selection, load/error state, and ADB-not-found handling from route components into shared hooks/components; `Apps.tsx`, `Debloat.tsx`, `Devices.tsx`, and `Wireless.tsx` duplicate the same authorized-device filtering and picker shape.
- Replace ad hoc parsers in `src-tauri/src/commands.rs` (`parse_ls_output`, `parse_ss_output`, `parse_ps_output`, fastboot output handling) with transcript-driven unit tests and parser modules; current parsing is brittle across Android toybox/busybox/OEM output variants.
- Add a production bundle smoke test that runs `npm run build` plus `tauri build`/resource checks enough to prove `packs/`, `quirks/`, icons, and sidecar placeholders resolve in an installed app.
- Add renderer coverage for destructive workflows: Apps action overlay, permission toggles, backup warnings, debloat queue progress, and journal/undo states.
- `docs/DEVELOPMENT.md` still says every nav item is a stub, while recent commits shipped live route surfaces; README also claims side-by-side device tabs and plugin system that are not present yet.
- `README.md` and `RESEARCH_REPORT.md` describe no-telemetry/offline positioning; add a user-facing privacy/update policy before any update checks, crash upload, or pack-fetch flow lands.
- i18n is wired for English/Russian with parity tests, but `src/lib/i18n.ts` only auto-detects `navigator.language`; there is no in-app language selector, persistence, pseudo-locale, or contribution pipeline.

## Rejected Ideas
- Mobile companion/Shizuku mode from Canta: useful for phone-only debloat, but it conflicts with Droidsmith's desktop ADB workstation and adds Android app distribution overhead.
- Cloud account remote control from AirDroid/Vysor: commercially proven, but it violates the current no-telemetry, no-account, local-first philosophy.
- Natural-language device control from Escrcpy: interesting but privacy, determinism, and safety risks outweigh value before core ADB recovery workflows are mature.
- ROM flashing/bootloader unlock suite: adjacent to fastboot, but destructive device-bricking risk is too high for the current package-management workbench.
- Direct UAD-NG list vendoring now: high fit, but `Roadmap_Blocked.md` already records redistribution permission as the blocker.
- Plugin marketplace in v0.2: already deferred in `Roadmap_Blocked.md`; core resources, journal, and distribution must stabilize first.

## Sources
Competitors:
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation
- https://github.com/0x192/universal-android-debloater
- https://github.com/Genymobile/scrcpy
- https://github.com/viarotel-org/escrcpy
- https://github.com/MuntashirAkon/AppManager
- https://github.com/samolego/Canta
- https://github.com/yume-chan/ya-webadb
- https://adbappcontrol.com/
- https://www.vysor.io/
- https://www.airdroid.com/personal/
Platform docs:
- https://developer.android.com/tools/adb
- https://developer.android.com/tools/logcat
- https://developer.android.com/studio/debug/device-file-explorer
- https://developer.android.com/identity/data/autobackup
- https://developer.android.com/studio/command-line/adb
Dependencies and security:
- https://github.com/advisories/GHSA-fx2h-pf6j-xcff
- https://github.com/advisories/GHSA-v6wh-96g9-6wx3
- https://github.com/advisories/GHSA-h67p-54hq-rp68
- https://github.com/tauri-apps/tauri/releases
- https://github.com/vitejs/vite/releases
- https://github.com/i18next/i18next/releases
- https://github.com/pmndrs/zustand/releases
Adjacent references:
- https://github.com/Genymobile/gnirehtet
- https://github.com/DeviceFarmer/adbkit
- https://tangoadb.dev/

## Open Questions
- Which signing/notarization path will unblock `Roadmap_Blocked.md` R-006/R-010?
- Can UAD-NG maintainers grant redistribution permission for derived pack imports?
- Which telemetry/update policy owner approves future crash upload or update-check behavior?
- What physical device matrix is available for Android 9-16, Samsung, Xiaomi/HyperOS, Oppo/ColorOS, Pixel, Fire OS, USB, and wireless ADB validation?
