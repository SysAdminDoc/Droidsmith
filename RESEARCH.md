# Research — Droidsmith

Date: 2026-07-21 — replaces all prior research.

## Executive Summary

Droidsmith v0.9.6 is a mature, local-first Android workstation built with Tauri 2, Rust, React, and TypeScript. Its strongest shape is the combination of safe ADB lifecycle management, reversible package operations, debloat packs, scrcpy/gnirehtet supervision, diagnostics, inspection tools, automation profiles, five complete locales, and an unsigned cross-platform desktop package without accounts, telemetry, or device agents. The highest-value direction is not broader fleet or cloud scope; it is making the existing workstation harder to lose state in, safer under hostile subprocess/device behavior, and deeper as an offline Android inspection surface. Confidence is **Verified** unless a bullet says otherwise.

Top opportunities, in priority order:

1. Fix desktop language persistence for German, Spanish, and Chinese; the isolation policy currently rejects three shipped locales.
2. Put hard byte limits on every subprocess pipe reader and consolidate the duplicated capture implementations.
3. Make portable settings a versioned, previewable export/import round trip instead of a partial one-way export paired with a full reset.
4. Add a renderer error boundary with a useful local recovery path instead of allowing an unexpected render failure to blank the shell.
5. Capture and restore prior display density/night-mode state, then consolidate those mutations with the existing Device Settings recovery model.
6. Centralize target-generation, cancellation, and stale-result handling; recent history shows the same race class recurring across routes and supervised sessions.
7. Add bounded Perfetto trace capture and a UIAutomator-based accessibility audit as local-first developer diagnostics.
8. Complete APK trust inspection through optional official `apksigner` verification and certificate identity, without claiming trust when the tool is unavailable.

## Product Map

- **Core workflows:** discover, authorize, diagnose, and recover USB/wireless devices; inspect health, files, processes, layout, logs, and settings; install/export/disable/archive/uninstall apps with journaled recovery; apply curated or local debloat packs; mirror/control/record with scrcpy; run gnirehtet; execute YAML profiles in the GUI or CLI.
- **User personas:** privacy-conscious owners removing OEM packages; Android power users; developers diagnosing apps/devices; technicians and small IT teams using repeatable local profiles.
- **Platforms and distribution:** Windows MSI/NSIS, macOS DMG, and Linux AppImage/deb/rpm from a Rust 1.81+/Node 20+ build; artifacts are intentionally unsigned. The public repository is at v0.9.6 while the latest published GitHub release is v0.5.3, so release publication remains operator-gated rather than coding work.
- **Key integrations and data flows:** the renderer calls typed Tauri IPC through an isolation allowlist; Rust shells out to user-selected or discovered ADB/scrcpy/gnirehtet tools; target identity and Android user are bound to operations; host writes require one-shot path grants; settings, journals, profiles, packs, and support bundles remain local.

## Competitive Landscape

### scrcpy and its GUI ecosystem

- **Does well:** scrcpy is the small, agentless reference for mirroring/control; Escrcpy and QtScrcpy add embedded viewing, virtual-display controls, key maps, group input, and desktop convenience.
- **Learn:** capability detection and focused workflows beat a generic shell; keep mirroring options composable and expose only flags supported by the discovered binary.
- **Avoid:** maintaining a scrcpy fork, broadcasting destructive actions across devices, or adding an Electron-sized runtime.

### UAD-ng and Canta

- **Does well:** both make package risk legible; UAD-ng supports portable selections and package-state recovery, while Canta provides an on-device Shizuku route.
- **Learn:** portable, previewable state and explicit restore semantics are core trust features, not extras.
- **Avoid:** duplicating an on-device companion or implying that every OEM package classification is universally safe.

### App Manager and MobSF

- **Does well:** App Manager exposes deep manifest/component/signing detail; MobSF produces broad static and dynamic security findings.
- **Learn:** APK findings should distinguish observed metadata, verified signatures, and actionable exposure risks.
- **Avoid:** growing Droidsmith into a root-only app manager, malware lab, server, or noisy vulnerability scanner.

### Android Studio and Perfetto

- **Does well:** bounded system traces, side-by-side APK analysis, manifest reconstruction, layout trees, and inspectable trace artifacts.
- **Learn:** capture/export workflows can provide serious diagnostics without embedding a full IDE or uploading device data.
- **Avoid:** reimplementing the Perfetto viewer or presenting unbounded captures; save interoperable artifacts that existing offline tools can open.

### ADB AppControl

- **Does well:** packages batch operations, history, process management, drag/drop, and polished recovery into a consumer-facing desktop product.
- **Learn:** clear previews, undo paths, and portable state are commercially valuable because users fear breaking a device.
- **Avoid:** paywall-shaped artificial limits, opaque package recommendations, or credential/key protection features that do not fit Droidsmith's open local model.

### Vysor, AirDroid Business, and TeamViewer

- **Does well:** simplify remote access, unattended sessions, device grouping, kiosk/fleet administration, and team workflows.
- **Learn:** connection recovery and status clarity matter more than feature count when a device is intermittently reachable.
- **Avoid:** accounts, hosted relays, unattended agents, remote wipe, subscription infrastructure, and multi-tenant administration; these contradict the local-first workstation scope.

### DeviceFarmer/STF

- **Does well:** device pools, booking, access roles, browser control, and distributed hardware labs.
- **Learn:** target identity must remain explicit and immutable throughout an operation.
- **Avoid:** a server/database/device-farm architecture; it adds operational and security assumptions far beyond a single-user desktop tool.

### Flipper

- **Does well:** a coherent shell for logs, layout, network, and extension-provided debugging surfaces.
- **Learn:** shared diagnostic primitives and consistent result states reduce duplicated route logic.
- **Avoid:** an SDK/device-agent plugin contract and an open-ended desktop plugin ecosystem; the repository is archived and Droidsmith's blocked roadmap already rejects that maintenance boundary.

## Security, Privacy, and Reliability

- **P0 — shipped locale persistence is broken:** `src/lib/i18n.ts:11-17` and `src-tauri/src/settings.rs:39-47` support `de`, `en`, `es`, `ru`, and `zh`, but `isolation/index.js:706-708` permits only `en` and `ru`. `src/App.tsx:543-548` discards the rejected persistence promise, so the UI appears changed until restart. Add one shared/generated locale contract and an isolation test covering all accepted and rejected values.
- **Unbounded process output:** `src-tauri/src/adb/transport.rs:341-368` and `src-tauri/src/commands.rs:2826-2858` read child pipes to EOF without a byte budget; similar readers exist in `src-tauri/src/adb/actions.rs`, `health.rs`, and `resolver.rs`. A timeout limits duration, not memory. Reuse the bounded pattern already present in `src-tauri/src/operations.rs`, kill the full child tree on overflow, and return a typed truncated/limit error.
- **Partial backup beside full reset:** `SettingsDocument` stores language, mirror presets, logcat queries, wireless history, auto-reconnect, and device fingerprints (`src-tauri/src/settings.rs:326-340`), while `SettingsExportDocument` exports only language and mirror presets (`:368-375`, `:736-744`) and no import exists. `src/routes/SettingsDataControls.tsx` offers an all-settings reset. Export portable state, explicitly exclude machine-local fingerprints by default, validate before import, and preserve a rollback copy.
- **Renderer failure recovery is absent:** `src/main.tsx:8-24` renders `App` directly. React documents error boundaries for render failures; add a localized fallback that can reload the renderer, open the app-data/log location, and copy redacted diagnostics without resetting settings.
- **Display controls are not reversible:** `src/routes/devices/DeviceControls.tsx:109-165` applies density and `ui_night_mode` mutations, but `src-tauri/src/commands.rs:2520-2565` does not capture the previous values for journal undo. Read and persist the effective/raw values first, or route the controls through the existing Device Settings preview/recovery machinery.
- **Security posture is otherwise strong:** `npm audit` reports zero vulnerabilities on 2026-07-21; `cargo audit --deny warnings` reports no unaccepted vulnerability and only the 18 time-bounded exceptions in `release-policy.json`. Locked Tauri 2.11.2 includes the fix for GHSA-7gmj-67g7-phm9. Continue patch-level maintenance; no major dependency migration is justified by current risk.
- **Privacy boundary:** keep Perfetto traces, APKs, settings exports, and UI dumps local and user-selected. Trace/UI dumps can expose package names, on-screen labels, and device identifiers; every export needs an explicit sensitivity note and the existing one-shot host-path grant.

## Architecture Assessment

- **Target-bound async work is repeated rather than modeled:** commits `f774de6`, `44a6c67`, `de99305`, `c3bbbe5`, and `ebb399f` fixed stale completions, target bleed, or orphaned sessions in different features. Introduce one operation-generation primitive that binds target fingerprint, registers cancellation, invalidates on target/lifecycle change, and suppresses stale completion updates; migrate risky routes incrementally.
- **Subprocess execution has split ownership:** `src-tauri/src/commands.rs` is 5,017 lines and contains a second capture runner beside `src-tauri/src/adb/transport.rs`. Extract a bounded process-output service and keep command modules focused on validation/orchestration. Do not perform a wholesale `commands.rs` rewrite.
- **Initial renderer payload is monolithic:** all 11 routes are static imports in `src/App.tsx:17-27`; the current production asset `dist/assets/index-BPQu9Fuy.js` is 903,182 bytes. Use `React.lazy`/dynamic imports per route, a localized Suspense state, preload the next likely route, handle preload errors, and enforce a release-check budget for the initial chunk.
- **Live-region/dialog semantics are over-applied:** `StatePanel` makes every non-danger panel `role="status"` and every danger panel `role="alert"` (`src/routes/common.tsx:280-313`); the inline force-stop strip is an `alertdialog` without modal focus behavior (`src/routes/devices/ProcessManager.tsx:124-140`). Make announcements opt-in and use a true focus-managed dialog or a non-dialog confirmation region.
- **Locale-aware formatting and UI coverage have small holes:** `src/routes/ApkAnalyzer.tsx:178-182` and `src/routes/Wireless.tsx:993` call ambient `toLocaleString`; pass the selected locale through the existing formatter helpers. Add APK Analyzer to mobile and 200% reflow routes in `scripts/check-rendered-routes.mjs:820-875`, and assert the corrected live-region/dialog contracts.
- **Layout Inspector has an evidence-rich next step:** `src/routes/devices/LayoutInspector.tsx` already parses UIAutomator node text, resource IDs, classes, bounds, and clickability. It can deterministically flag missing labels, duplicate non-empty resource IDs, and clickable bounds below 48dp, but it cannot honestly infer color contrast from XML alone.
- **APK analysis must preserve confidence:** the current analyzer reports manifest metadata, permissions, components, DEX counts, archive sizes, and signature-scheme presence. Use optional official `apksigner verify --print-certs` for cryptographic verification and certificate rotation details; never label a package verified from scheme markers alone.
- **Public documentation is not self-contained:** `.gitignore:55-57` ignores Markdown except README, while `README.md` links `Roadmap_Blocked.md`, `RESEARCH_REPORT.md`, and `docs/DEVELOPMENT.md`, none of which are tracked or present on the public default branch. Replace those links with concise in-README guidance or stable public targets and state the unsigned/local-build distribution policy accurately.
- **Testing strategy is strong but concentrated:** `scripts/check-rendered-routes.mjs` is 3,248 lines and has changed frequently. Keep the deterministic headless harness, but add focused contract tests for locale allowlists, bounded process output, settings migration/import, target-generation invalidation, error-boundary recovery, accessibility semantics, and route chunk budgets.

## Rejected Ideas

| Idea | Decision and reason | Source |
|---|---|---|
| Hosted fleet, accounts, remote relay, multi-user administration | **Rejected:** AirDroid/TeamViewer/STF prove demand, but this contradicts the single-user, local-first, zero-agent product boundary; multi-device broadcast is already blocked. | AirDroid Business; TeamViewer; DeviceFarmer/STF; `Roadmap_Blocked.md` IMP-52/IMP-53 |
| Desktop plugin marketplace or device SDK | **Rejected:** Flipper demonstrates extensibility but also its runtime/SDK maintenance cost; desktop plugins and plugin lifecycle work are already blocked. | Flipper; `Roadmap_Blocked.md` R-062/R-063 |
| Mobile/Shizuku companion | **Rejected:** Canta serves this use case well; a companion would duplicate product scope, distribution, and device-side trust work. | Canta |
| Cloud/LLM device automation | **Rejected:** AutoDroid-style natural-language control is nondeterministic around destructive ADB actions and conflicts with offline reproducibility. | https://arxiv.org/abs/2308.15272 |
| Dynamic/root code debloating | **Rejected:** research prototypes require a customized OS or privileges and solve application-code loading, not Droidsmith's package lifecycle. | https://arxiv.org/abs/2501.04963 |
| Full MobSF clone or home-grown APK signature verifier | **Rejected:** both create a security-maintenance boundary disproportionate to this workstation. Invoke the official optional verifier and label unavailable evidence honestly. | MobSF; Android `apksigner` documentation |
| Embedded/uploaded Perfetto viewer | **Rejected:** the maintained viewer already opens interoperable traces locally/offline; embedding it adds substantial web/runtime scope and uploading breaks privacy expectations. | Perfetto documentation |
| Restore hosted CI | **Rejected:** commit `70f0b8b` intentionally moved the project to local builds. Strengthen `release:check`; do not reinterpret the absence of workflows as accidental. | repository history; `CLAUDE.md` |
| Acquire certificates or add code signing | **Rejected:** the governing desktop test policy explicitly prohibits signing. Publish unsigned artifacts with checksums/SBOMs where operator-gated release work permits. | root `AGENTS.md`; `Roadmap_Blocked.md` |
| Big-bang React 19/Vite 8/Tailwind 4/i18next 26 migration | **Rejected for this roadmap:** audits show no current vulnerability requiring the churn; patch updates and isolated major evaluations belong in routine dependency maintenance. | `package.json`; `package-lock.json`; 2026-07-21 `npm audit`/`npm outdated` |
| Android 17 post-quantum signing UI | **Under consideration:** Android 17 preview exposes PQC APK signing, but analyzer semantics and tool support are not stable enough for a user-facing trust claim. | Android 17 summary/release notes |
| Automatic first-run wizard | **Under consideration:** `src/App.tsx:142` keeps the existing onboarding closed by default, but a high-traffic public ADB support thread shows setup pain. Add only after measuring whether Host Doctor plus Help leaves a real activation gap; avoid interrupting experienced users on every upgrade. | Stack Overflow ADB offline thread; `src/routes/Onboarding.tsx` |

## Sources

### Project

- https://github.com/SysAdminDoc/Droidsmith
- https://github.com/SysAdminDoc/Droidsmith/releases/latest

### Open-source and adjacent products

- https://github.com/Genymobile/scrcpy
- https://github.com/viarotel-org/escrcpy/releases
- https://github.com/barry-ran/QtScrcpy
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation
- https://github.com/samolego/Canta
- https://github.com/MuntashirAkon/AppManager
- https://mobsf.github.io/Mobile-Security-Framework-MobSF/
- https://github.com/DeviceFarmer/stf
- https://github.com/facebook/flipper

### Commercial and community signal

- https://adbappcontrol.com/en/extended/
- https://www.vysor.app/
- https://www.airdroid.com/pricing/airdroid-business/
- https://www.teamviewer.com/en-us/global/support/knowledge-base/teamviewer-classic/mobile/android/remote-control-an-android-device-via-unattended-access/
- https://news.ycombinator.com/item?id=39730962
- https://stackoverflow.com/questions/14993855/android-adb-device-offline-cant-issue-commands

### Standards and platform capabilities

- https://developer.android.com/tools/adb
- https://developer.android.com/about/versions/17/summary
- https://perfetto.dev/docs/getting-started/system-tracing
- https://developer.android.com/studio/debug/layout-inspector
- https://developer.android.com/guide/topics/ui/accessibility/testing
- https://support.google.com/accessibility/android/answer/7101858
- https://developer.android.com/tools/apksigner
- https://developer.android.com/studio/debug/apk-analyzer
- https://source.android.com/docs/security/features/apksigning
- https://www.w3.org/TR/wai-aria/

### Framework and security

- https://react.dev/reference/react/Component#catching-rendering-errors-with-an-error-boundary
- https://react.dev/reference/react/lazy
- https://github.com/tauri-apps/tauri/security/advisories/GHSA-7gmj-67g7-phm9

## Open Questions

None block prioritization or implementation. Device-dependent acceptance cases should use fixtures in the headless gate first, then follow the repository's existing real-device release checklist when hardware is available.
