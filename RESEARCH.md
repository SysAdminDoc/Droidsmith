# Research — Droidsmith
Date: 2026-07-14 — replaces all prior research.

## Executive Summary

Droidsmith is a local-first Tauri 2 desktop workshop for guarded `adb`, `fastboot`, and `scrcpy` workflows. Its strongest current shape is the verified safety core already shipped: immutable device/user targets, previewed and audited mutations, recoverable journals, versioned packs/profiles, cancellation, lifecycle recovery, redacted diagnostics, and safe split-package installation. The highest-value direction is now to close the remaining trust gaps around renderer authority, host artifacts, reversibility, legacy backup claims, and host/tool diagnosis before broadening the product. Top opportunities, in priority order: (1) **Verified — Now, 5/5, M:** complete IPC/CSP hardening (**IMP-42**); (2) **Verified — Now, 5/5, M:** make all host artifacts atomic (**IMP-43**); (3) **Verified — Now, 5/5, M:** restore eligible per-user system-app removals (**IMP-54**); (4) **Verified — Now, 5/5, M:** replace the legacy backup success heuristic with a capability contract (**IMP-55**); (5) **Verified — Now, 5/5, M:** add a read-only host connection doctor (**IMP-56**); (6) **Verified — Next, 4/5, M:** unify Platform Tools compatibility policy (**IMP-57**); (7) **Verified — Next, 4/5, L:** land fake-ADB, fuzz, and rendered race gates (**IMP-47**, **IMP-51**); (8) **Verified — Next, 4/5, M:** finish accessibility/i18n hardening (**IMP-48**); (9) **Verified — Next, 4/5, M:** add privacy-gated Android bugreports (**IMP-58**); and (10) **Verified — Next, 3/5, M:** persist named structured Logcat queries (**IMP-59**).

## Product Map

### Core workflows

- Discover USB/wireless Android devices and inspect users, packages, processes, storage, battery, network, Logcat, and read-only fastboot state.
- Preview and apply user-bound package, permission, debloat-pack, file, console, and install operations through audited Rust commands.
- Journal mutations and undo actions whose inverse is modeled as safe.
- Pair wireless ADB, supervise scrcpy sessions, and capture screenshots, backups, APKs, diagnostics, and split-package installs.
- Validate/import packs and profiles; automate supported device/profile workflows through `droidsmith-cli`.

### User personas

- **Verified:** privacy-conscious Android owners and refurbishers who need reviewed local operations without root, an account, or a companion APK (`README.md`, `CLAUDE.md`).
- **Verified:** developers and support technicians diagnosing OEM, USB-driver, Android-user, package, and intermittent-connection problems.
- **Verified:** pack/profile contributors who need deterministic schema, compatibility, and migration behavior.

### Platforms and distribution

- **Verified:** Windows, macOS, and Linux through Tauri 2; Node.js 20+ and Rust 1.81 development floors (`package.json`, `src-tauri/Cargo.toml`).
- **Verified:** host-installed `adb`, `fastboot`, and `scrcpy`; signed releases, auto-update, bundled sidecars, SBOM publication, external UAD data, hosted telemetry, and a plugin marketplace remain blocked in `Roadmap_Blocked.md`.
- **Verified:** MIT licensed and intentionally local-first (`LICENSE`, `README.md`).

### Key integrations and data flows

- React routes invoke wrappers in `src/lib/tauri.ts`; Tauri commands validate input and launch tokenized processes in `src-tauri/src/commands.rs` and `src-tauri/src/adb/`.
- Canonical actions flow through `src-tauri/src/adb/actions.rs`; device-bound outcomes and inverses persist in `src-tauri/src/journal/`.
- Versioned YAML packs, profiles, and quirks flow from `packs/`, `profiles/`, and `quirks/` through Rust validation.
- Native dialogs select host paths; ADB subprocesses transfer data across the host/device boundary; redacted support bundles stay local.

## Competitive Landscape

- **Universal Android Debloater Next Generation — Verified.** Does well: conservative risk descriptions, per-user actions, state verification, and explicit `cmd package install-existing --user` recovery. Learn: distinguish restorable system-package removal from irreversible user-app uninstall and verify recovery post-state. Avoid: treating a static cross-OEM list as proof of safety.
- **Canta — Verified.** Does well: UAD-backed descriptions, disable-oriented safety, and visibility into previously removed apps. Learn: make the safer reversible action the default when supported. Avoid: Shizuku/companion-app architecture, which contradicts Droidsmith's external recovery model.
- **App Manager — Verified.** Does well: package provenance, signing/split metadata, filter-based profiles, backup distinctions, and rich batch operations. Learn: report capability and artifact contents precisely. Avoid: root/Shizuku component mutation and tracker-scanning scope.
- **scrcpy and Escrcpy — Verified.** Do well: capability-aware local mirroring, actionable session errors, recording, multi-device session visibility, and bounded controls. Learn: interrogate installed capabilities and keep session ownership above route lifetime. Avoid: broadcast input or mass mutation across devices.
- **AYA and ADBKit — Verified.** Do well: file workflows, structured command history, device analysis, fastboot safeguards, and multi-device ergonomics. Learn: add a read-only host/device doctor and reusable diagnostic views. Avoid: bundling Platform Tools before the signing/sidecar trust model is resolved.
- **Android Studio Device Explorer/Logcat — Verified.** Does well: structured Logcat keys, negation/regex, query history/favorites, and explicit detached state. Learn: persist a bounded named-query subset without persisting raw logs. Avoid: recreating an IDE or an unrestricted query language.
- **ADB AppControl and PixelFlasher — Verified.** Do well: recovery-oriented package state, command/Logcat favorites, Basic/Expert separation, and known-bad Platform Tools guidance. Learn: one version-policy source and clear capability explanations. Avoid: proprietary recovery state, bootloader unlocking, flashing, and root workflows.

## Security, Privacy, and Reliability

- **Verified — clean current gates:** on 2026-07-14, 34 Vitest tests and 160 Rust tests passed; typecheck, ESLint, Prettier, Clippy, npm audit, and the repository RustSec gate also passed. No current dependency advisory justifies an emergency framework migration.
- **Verified — renderer authority remains broad:** production CSP still permits `style-src 'unsafe-inline'` and high-risk commands lack Tauri isolation (`src-tauri/tauri.conf.json`, `src-tauri/src/commands.rs`); existing **IMP-42** is correctly P1.
- **Verified — host artifact failure can destroy trust:** pull, screenshot, backup, and extraction still need one atomic staging policy (`src-tauri/src/commands.rs`); existing **IMP-43** must precede new bugreport output.
- **Verified — eligible removals are marked irreversible:** `src-tauri/src/adb/actions.rs` gives `UninstallForUser` no inverse, although AOSP and UAD document `cmd package install-existing --user` for system packages retained in package-manager state. User-installed `/data/app` packages must remain explicitly irreversible.
- **Verified — legacy backup is over-claimed:** `src-tauri/src/commands.rs` runs `adb backup -apk` and classifies only an empty/header-only artifact after execution. Android 12 excludes target-SDK-31+ app data unless the app is debuggable; OEM behavior also varies. The UI must describe known, blocked, and unknown capability before work begins and never equate a nonempty `.ab` file with recoverable data.
- **Verified — connection setup lacks host diagnosis:** onboarding explains drivers/udev generically, but no command distinguishes missing `adb`, an Android USB device lacking the correct Windows driver, missing Linux group/udev access, unauthorized/offline state, an unsuitable USB mode/cable, or a competing ADB server (`src/routes/Onboarding.tsx`, `src-tauri/src/adb/resolver.rs`). Diagnostics must be read-only and must not install drivers, elevate, or retain ADB keys/serials.
- **Verified — Platform Tools policy drifts:** runtime health recommends newer behavior while development fetch scripts still pin 35.0.2; official 36.0.2 fixed Samsung/older-device and Windows transfer failures, and 37.0.0 changed the default mDNS backend. One dated policy must drive resolver status, fetch scripts, support bundles, and release checks.
- **Verified — Android bugreports are sensitive:** official `adb bugreport` output can contain system state and logs. Capture must require a separate privacy warning, use atomic local output, remain opt-in, and never be auto-attached or silently redacted.
- **Verified — public security instructions are not usable:** `SECURITY.md` names `security@droidsmith.invalid` and claims sidecars, updater, signing/SBOM, and support channels that are not shipped; GitHub private vulnerability reporting is disabled. Correcting the contact requires maintainer input.

## Architecture Assessment

- **Action boundary:** add an explicit `RestoreExistingForUser` canonical action rather than issuing ad hoc package-manager commands. Its precondition must prove a system package remains in `pm list packages -u`; its inverse sequence must restore the recorded enabled state on the same immutable device/user target (`src-tauri/src/adb/{actions,packages}.rs`, `src-tauri/src/journal/`).
- **Backup boundary:** move package backup/export capability, validation, and a versioned artifact manifest out of the oversized `src-tauri/src/commands.rs`; keep reliable APK/split export distinct from deprecated legacy-data backup.
- **Host diagnostics boundary:** add normalized read-only diagnostic codes behind platform adapters; UI and support-bundle rendering should consume codes, not parse free-form subprocess output. Fixture each Windows/Linux/macOS branch without elevated or destructive probes.
- **Tool policy boundary:** a single serializable policy should contain supported/recommended/known-bad status, rationale, source URL, and review date. Runtime detection and development/release scripts must fail if their policy views diverge.
- **Logcat boundary:** store versioned query definitions through the typed-settings work in **IMP-50**; apply bounded filters in `src/routes/Logcat.tsx` or a new Rust query module beside `src-tauri/src/operations.rs`, and never persist raw Logcat lines by default.
- **IPC/test boundary:** `src/lib/tauri.ts` manually mirrors Rust DTOs; **IMP-49** should generate schemas/bindings, while **IMP-47** and **IMP-51** must cover fake-ADB argv, malformed OEM output, cancellation, disk-full/partial output, reconnect races, route unmounts, non-English text, keyboard operation, and 200% zoom.
- **Documentation:** `SECURITY.md`, `CONTRIBUTING.md`, `docs/DEVELOPMENT.md`, and `docs/screenshots/` contain stale future-state or browser-placeholder claims. Keep documentation changes coupled to the corresponding implementation/release gate; do not claim signing, CI, sidecars, updater, or SBOMs before they exist.
- **Upgrade strategy:** take current patch releases through the normal gates, but defer React 19, Vite 8/Rolldown, Tailwind 4, i18next 26, and TypeScript 7 as a coordinated compatibility project after the renderer/race harness. No current advisory or required capability makes that migration a near-term user benefit.

## Rejected Ideas

- **Root, Shizuku, or a required mobile companion — Rejected, Verified:** Canta and App Manager gain device-local power by violating Droidsmith's no-root, external-recovery philosophy.
- **Cloud fleet management, team accounts, remote relays, or a remote ADB server — Rejected, Verified:** Vysor and DeviceFarmer solve different multi-operator/remote-access problems and would expand Droidsmith's trust boundary materially.
- **Bootloader unlock, ROM flashing, rooting, or automated bootloop repair — Rejected, Verified:** PixelFlasher's OEM-image/data-loss model is incompatible with Droidsmith's read-only fastboot scope.
- **App-owned ADB key vault — Rejected for now, Likely:** ADB AppControl paywalls key protection, but Droidsmith currently uses the host ADB server and does not own its key lifecycle; adding a second key/server path would create misleading isolation.
- **Automatic crowd-sourced debloat data or reputation scores — Rejected for now, Verified:** UAD/Canta evidence varies by OEM, build, and Android user; external list governance/provenance is already blocked in `Roadmap_Blocked.md`.
- **Dynamic bytecode-based debloating — Rejected, Verified:** 3DNDroid requires a customized Android OS and management app, outside a conservative host workshop.
- **Plugin marketplace, telemetry upload, signed updater, embedded sidecars, and SBOM publication — Rejected from the actionable roadmap, Verified:** ownership, privacy, signing, hosting, and threat-model decisions remain recorded blockers in `Roadmap_Blocked.md`.
- **Immediate React 19/Vite 8/Tailwind 4 migration — Rejected for now, Verified:** current audits are clean and Vite 8's Rolldown/Tailwind 4's configuration changes add cross-WebView migration risk without closing a user-facing gap.
- **Full Android Studio Logcat grammar — Rejected for now, Verified:** named bounded presets cover Droidsmith's workflow without introducing an IDE-scale parser or unsafe unbounded expressions.
- **Flatpak packaging now — Rejected for now, Verified:** raw USB, udev, host-binary discovery, and filesystem grants conflict with useful confinement until the sidecar/distribution architecture is resolved.

## Sources

### Open-source and adjacent projects

- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/wiki/FAQ
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/releases/tag/v1.2.0
- https://github.com/samolego/Canta
- https://github.com/MuntashirAkon/AppManager/releases/tag/v4.1.0
- https://github.com/DeviceFarmer/stf
- https://github.com/viarotel-org/escrcpy
- https://github.com/liriliri/aya
- https://github.com/Drenzzz/ADBKit
- https://github.com/badabing2005/PixelFlasher
- https://github.com/Genymobile/scrcpy/releases/tag/v4.1

### Commercial products

- https://adbappcontrol.com/
- https://adbappcontrol.com/en/extended/
- https://www.vysor.io/

### Standards and platform APIs

- https://developer.android.com/tools/adb
- https://developer.android.com/tools/releases/platform-tools
- https://developer.android.com/about/versions/12/behavior-changes-12
- https://developer.android.com/studio/debug/bug-report
- https://developer.android.com/studio/run/device
- https://android.googlesource.com/platform/frameworks/base/+/master/services/core/java/com/android/server/pm/PackageManagerShellCommand.java
- https://developer.android.com/studio/debug/logcat

### Dependencies and security

- https://v2.tauri.app/security/csp/
- https://v2.tauri.app/concept/inter-process-communication/isolation/
- https://vite.dev/blog/announcing-vite8
- https://tailwindcss.com/blog/tailwindcss-v4
- https://rustsec.org/

### Awesome lists, community, and research

- https://github.com/mzlogin/awesome-adb
- https://github.com/tauri-apps/awesome-tauri
- https://www.reddit.com/r/AndroidQuestions/comments/12x0dga/usb_debugging_issue/
- https://lobste.rs/s/rxqobd/tauri_2_0_stable_release
- https://arxiv.org/abs/2501.04963

## Open Questions

- **Needs maintainer decision:** which real private vulnerability-reporting channel should replace `security@droidsmith.invalid` in `SECURITY.md`? GitHub private vulnerability reporting is disabled, and no public source establishes an authorized alternative.
