# Research — Droidsmith

Date: 2026-08-08 — replaces all prior research.

Confidence labels: **Verified** means read or executed against the repository or
the linked primary source on 2026-08-08; **Likely** means a secondary report;
**Needs live validation** means a real device, OEM transcript, or
bundle-capable host is required.

## Executive Summary

Droidsmith v0.9.17 is a local-first Tauri 2 Android workstation. The inspected
tree has 11 lazy-loaded workspaces, 105 renderer-callable commands, 115
TypeScript/TSX files, 69 Rust files, five locale catalogs with exact key-parity
tests, a journaled action/recovery model, and an unusually strong rendered-route
and isolation policy. The product is already broad; the highest-value work is
closing places where a claim, error, artifact, or release gate is weaker than
the implementation around it. Do that before adding networked or privileged
novelty.

Priority order after the 2026-08-08 code, history, competitor, standards,
community, and security review:

1. **P0 trust boundary:** quote every device-side path before it reaches
   `adb shell` (R-133). The current file-manager path is both a correctness bug
   for spaces and a shell-injection path; the Console path has a separate guard.
2. **P0 dependency gate:** clear both high npm advisories — `brace-expansion`
   GHSA-rgw5-rvv9-x895/CVE-2026-69152 and `nanoid` GHSA-2v37-7h3g-55p8 — and
   declare an npm floor (IMP-113).
3. **P0 accessibility:** replace enumerated light-theme utility overrides with
   semantic tokens and run the rendered/axe gate in both themes (IMP-114).
4. **P1 platform truth:** pin platform-tools archives to a versioned URL before
   hashing them (R-134); the 37.0.1 USB-backend and `kill-server` diagnostics
   are now shipped (R-139/R-140).
5. **P1 security truth:** account for Project Mainline in the CVE-2026-0073
   verdict (R-135), redact native panic payloads before writing `crash.log`
   (IMP-133); raw OEM detail now stays behind a typed/localized error envelope
   (IMP-130).
6. **P1 artifact truth:** make provenance line-ending independent (IMP-115),
   reject placeholder installer hashes (IMP-116), and emit usable SBOM license
   and metadata fields (R-136).
7. **P1 product parity:** let packs choose a safe action (R-137), expose fleet
   apply in the GUI (R-138), and expose the already-stable CLI through a local
   stdio MCP surface (R-149).
8. **P2 diagnostics:** add read-only historical app-exit/ANR records beside the
   existing process list and Android 17 memory-limit status (R-151); do not
   conflate this with the blocked crash/dropbox viewer R-102.
9. **P2 verification:** the focused Vitest coverage gate now measures the
   deterministic `src/lib` helper/state surface (IMP-131 shipped); the four
   parser fuzz targets now replay checked-in seeds in stable tests and run in a
   bounded nightly/manual Linux lane (IMP-132 shipped). The cheap release-policy
   check now runs on every frontend push and pull request (IMP-134 shipped).
10. **P2 content and documentation:** import user-supplied UAD data without
    redistributing GPL content (R-146), publish the unreleased Windows artifact
    when a bundle-capable host is available (R-148), and repair SECURITY,
    development, README, and tracked-markdown contracts (IMP-117/IMP-122/IMP-129).

## Product Map

- **Core workflows:** discover and diagnose USB/Wi-Fi devices; inspect device
  info, settings, processes, files, layout, network, logs, Perfetto, and
  bugreports; install/analyze APKs; manage packages with review, journal,
  recovery, and reversible actions; apply local debloat packs; author profiles
  and fleet reports; mirror/control/record via scrcpy; and run the headless CLI.
- **Personas:** a power user who needs bootloop-safe debloating; a repair or
  refurbishing operator who needs fleet plans, resume, and audit evidence; an
  Android developer who wants logcat/layout/Perfetto/file access outside a full
  IDE; and a privacy-motivated user who requires local files, no telemetry, and
  no HTTP client.
- **Platforms/distribution:** Tauri targets Windows, macOS, and Linux. Source
  and manifests are v0.9.17; the public release list still exposes only v0.5.3
  (2026-07-17) and v0.1.0. Bundled platform-tools/scrcpy sidecars and signed
  update infrastructure remain explicitly blocked or deferred.
- **Data flow:** the renderer uses typed bindings only. Rust validates serial,
  transport, user, path, host grants, argv, schema versions, and journal
  transitions before invoking host `adb`/`fastboot`/scrcpy/gnirehtet/apksigner.
  Settings, packs, quirks, profiles, recovery baselines, support bundles, and
  diagnostics are local and versioned. The backend has no HTTP client.
- **Quality shape:** `scripts/check-rendered-routes.mjs` covers all 11 routes,
  failure/retry/disconnect paths, keyboard semantics, 200% zoom, and all locale
  catalogs in selected flows. The native matrix runs formatting, clippy, and
  tests on three operating systems. The release policy is authoritative, but
  full release smoke is scheduled/manual rather than pull-request gated.

## Competitive Landscape

### Universal Android Debloater NG

UAD-NG remains the content and community benchmark: per-package descriptions,
risk tiers, package-state checks, cross-user detection, and a continuously
updated list. Its issues request a package-specific action and per-ROM removal
provenance, which map directly to R-137 and R-145. Its launch-time list fetch
and reports of unsafe recommendations show why Droidsmith should retain
revisioned local packs, explicit review, and no automatic remote content.

### scrcpy and escrcpy

scrcpy 4.1 provides the upstream mirror/control contract, codec recovery, and
display/camera/input flags; Droidsmith already supervises the binary and probes
capabilities. escrcpy adds multi-device control, batch install/screenshot,
input broadcast, and a local MCP assistant. The useful parity item is fleet
workflow (R-138/R-149), not an embedded scrcpy client or an online AI copilot.

### ADB Explorer and AppManager

ADB Explorer demonstrates permission-aware file-operation affordances and a
focused file-manager information hierarchy; AppManager demonstrates expressive
package filters and reversible package operations. Droidsmith already has the
filter/predicate and journal foundations, so the remaining direct item is
permission-based file gating (IMP-127), not zip editing or verification bypasses.

### ADB AppControl, Vysor, and AirDroid Business

Commercial products consistently charge for batch/fleet operations, premium
mirroring, and guided debloat. Droidsmith already ships local wireless and
reviewed debloat workflows without a paywall; GUI fleet apply is the credible
open-source differentiator. Kiosk policy, cloud MDM, accounts, and telemetry do
not fit the stated mission.

### Shizuku, ya-webadb, DeviceFarmer/STF, and agent-device

These projects show typed protocol access, browser/device automation, fleet
orchestration, and agent-friendly surfaces. Replacing the existing ADB boundary
would be a large additive rewrite with no current need; the bounded local MCP
adapter (R-149) and typed error/command contracts provide the useful slice.

### Android TV tooling and curated lists

The Android TV/Fire TV ecosystem has maintained debloat lists but little
cross-platform desktop tooling. The existing pack target model makes R-150 a
content-sized expansion. It is intentionally below path safety, release truth,
and fleet parity.

### Awesome lists

`awesome-adb` and `awesome-android` confirm that users assemble a toolbox from
ADB, scrcpy, debloat, profiling, and device-inspection utilities. They are
discovery signals, not authority for unsafe commands; primary Android/AOSP and
upstream project documentation wins when behavior conflicts.

## Security, Privacy, and Reliability

### Device-side command construction — Verified statically; Needs live validation

`src-tauri/src/remote_files.rs:54-75` rejects control characters, traversal,
duplicate separators, and trailing separators but permits shell metacharacters
and spaces. `commands/files.rs:195-205` passes those paths to
`adb/transport.rs:211-217`, where shell argv is joined. The repository itself
documents that `adb shell` joins arguments and runs the result through the
device shell (`commands/console.rs:132-137`). A filename such as
`/sdcard/My files/a;next` therefore both splits incorrectly and can append a
command beyond `ProtectedPath`. The existing test checks the planned argv, not
the joined command. This is the root-cause P0 (R-133); Android's ADB guidance
also requires a second level of quoting for remote shell words.

### npm dependency gate — Verified by execution on 2026-08-08

`npm audit --audit-level=moderate --json` exits 1 with two high advisories:
`brace-expansion` 5.0.8 is below fixed 5.0.9 for GHSA-rgw5-rvv9-x895, and
`nanoid` 3.3.16 is below fixed 3.3.17 for GHSA-2v37-7h3g-55p8. The Rust side
has a local `cargo audit --deny warnings` wrapper and `release:check` now invokes
`cargo deny` directly, but `npm run security:audit` still omits cargo-deny. The
existing IMP-113 should cover both npm advisories; IMP-120 should be narrowed to
making the standalone security script use the same cargo-deny policy.

### Native crash-log privacy — Verified

`src-tauri/src/diagnostics.rs:15-16` claims “No PII”, but
`write_panic_record` at `:189-212` writes the panic location and arbitrary
string payload to rotating `crash.log`. A panic payload can include a serial,
path, email, command output, or OEM response. Support-bundle sanitization tests
(`src-tauri/src/support_bundle.rs:778-825`) do not prove that the raw local log
is safe before export or before the user opens the diagnostics directory.
IMP-133 should preserve useful panic class/location while bounding or redacting
payload content and add a direct-hook test.

### Error fidelity versus localization — Verified

`CommandError` intentionally preserves exact backend/OEM text, but the command
boundary previously returned it directly and many routes rendered it without a
locale-aware summary. The boundary now maps stable codes to localized
summaries, keeps exact device/OEM text in a labelled details line, redacts
renderer-originated host paths or identifiers, and keeps the recovery fallback
render-safe even if the i18n tree fails (IMP-130).

### Artifact and supply-chain claims — Verified

The provenance generator hashes UTF-8 file bytes without a repository
`.gitattributes`; `core.autocrlf=true` can therefore make the same commit produce
different `SHA256SUMS` on Windows and Linux (IMP-115). Winget and Scoop
manifests contain all-zero hashes and the release gate does not reject them
(IMP-116). `provenance/SBOM.cdx.json` is CycloneDX 1.6 with 543 components but
zero `licenses`, no serial number, no timestamp, and no tools metadata (R-136).
CycloneDX and SPDX both provide the fields needed to make the artifact useful;
the implementation must retain deterministic inputs rather than inserting a
wall-clock value.

### Android and host drift — Verified or Needs live validation

- Platform-tools 37.0.1 changes USB backend defaults per OS, removes the
  openscreen mDNS implementation, and adds a `kill-server` requester chain.
  Droidsmith now surfaces the verified USB policy and bounded recovery chain
  (R-139/R-140); the policy still hashes rolling `-latest-` URLs (R-134).
- The Android 2026-05-01 bulletin places CVE-2026-0073 in an ADB Mainline
  component. `security_patch.rs` reads only `ro.build.version.security_patch`,
  so the verdict can be a false positive until a device's Google Play system
  update/module level is included (R-135; live property validation required).
- Google documents developer-verification dates and a 20-device limited-
  distribution tier. Its official FAQ also states that ADB installs work
  without verification. R-141 must therefore classify non-ADB/store failures
  and preserve unknown states; it must not claim that ordinary ADB installs are
  threatened.
- Android 17 adds memory limits on a subset of devices and exposes
  `am memory-limiter status`; Droidsmith already has a read-only backend command,
  fixture, binding, and Process Manager status card. The missing opportunity is
  historical process-exit/ANR records, not another memory-limiter probe (R-151).

### Existing guardrails and recovery

CSP/isolation and `core:default` capabilities are restrictive; links use
`noreferrer`; YAML/profile inputs are bounded; serial, host, wireless, package,
and path validators reject flag-injection shapes; and the JSONL journal,
recovery baselines, OTA restore/re-apply pair, and fleet resume model are real
strengths. Do not re-add telemetry, auto-fetch, or opaque destructive actions.
The optional bundled platform-tools/scrcpy sidecars remain a documented release
blocker rather than a new roadmap duplicate (blocked R-006/R-010/R-110). The
remaining roadmap items are independently grounded: IMP-119 covers the two
missing collection empty states; IMP-121 the repository topics/homepage;
IMP-123 the stale MSRV rationale; IMP-124 the unshipped third-party notices;
R-143 CLI pack application; and R-144 the additional reversible `pm` rungs.
IMP-126 shipped a dated decision to retain `serde_yaml_ng` pending a fixture-
compatible migration; IMP-112 is the shipped light-theme baseline;
IMP-114 is the still-open total-theme gate.

## Architecture Assessment

- **Command boundary:** the command registry and generated bindings keep the
  renderer typed, but 13 command modules have no inline tests. Start with
  `commands/files.rs` and `commands/console.rs`, then cover validation and
  failure envelopes across the remaining modules (IMP-118).
- **Large route surfaces:** `Apps.tsx` is about 2,023 lines, followed by
  `Mirror.tsx`, `Profiles.tsx`, `Logcat.tsx`, and `Wireless.tsx`. The initial
  renderer bundle is near 84% of its 450,000-byte budget. `Apps.tsx` should stay
  orchestration-only as existing extracted panels grow (IMP-128).
- **Collection scale:** the smoke harness mocks only a few packages and
  `PackageTable` uses lazy metadata rather than row virtualization. Render and
  measure a 1,000-package inventory before choosing a virtualization strategy
  (IMP-125).
- **Empty/error states:** Host Doctor with zero findings and Mirror with zero
  packages still need explicit copy (IMP-119), while install/uninstall and
  package actions should preserve exact OEM output through R-141/R-142 and the
  localized envelope shipped in IMP-130.
- **Accessibility and themes:** the rendered/axe harness is strong in dark mode,
  but does not set `data-theme="light"`; the native config pins `Dark`. Narrow
  locale/zoom coverage is concentrated in one locale even though key parity is
  exact. IMP-114 must gate both themes; new error copy must join the locale
  contract rather than bypass it.
- **Testing:** `vitest.config.ts` declares text/JSON/HTML coverage reporters,
  but package scripts and CI never enable coverage or thresholds. Component
  rendering is intentionally absent from Vitest, so the Playwright smoke gate is
  the primary route test. Existing fuzz targets for ADB text, YAML, journal
  JSONL, and scrcpy text have checked-in seeds replayed by stable tests, with
  the scheduled/manual lane bounded to 30 seconds per target.
- **Release/docs:** `release:check` includes provenance, UI smoke, npm audit,
  cargo-deny, schema/resource checks, and bundle checks; the standalone npm
  security script is narrower. CI's `release-smoke` job is schedule/manual only,
  so a pull request can pass frontend/native/security jobs without building a
  production bundle. `docs/DEVELOPMENT.md` is untracked, still describes Node
  20+, and omits later routes; SECURITY.md is untracked and contradicts the
    private-advisory path. IMP-117 covers README/schema/screenshots; IMP-122
    addresses the remaining document contract without creating more markdown
    tracking files. IMP-129 and IMP-134 shipped their respective policy fixes.
- **Observability:** diagnostics are intentionally file-only and no-network;
  that is compatible with the privacy posture. The missing hardening is
  redaction and structured local error codes, not a telemetry SDK.

## Rejected Ideas

- **Remote pack/list auto-fetch or an in-app updater** — conflicts with the
  no-HTTP/local-first rule and duplicates blocked R-075/R-095/R-036; UAD-NG's
  launch-time fetch and bootloop reports are a cautionary signal.
- **Embedded scrcpy pane, OTG/gamepad control, or a full ADB protocol rewrite**
  — large surface with device-only verification and no user value proportional
  to the risk; keep the supervised sidecar and expose bounded flags (R-147).
- **Online AI copilot** — violates the no-network default. A local stdio MCP
  adapter with explicit mutation confirmations is materially different (R-149).
- **Shizuku/on-device companion, mobile client, cloud MDM, account sync, or
  multi-user cloud workspaces** — off mission or already blocked, with no safe
  local migration path demonstrated.
- **Telemetry/crash reporting SaaS** — conflicts with “no telemetry”; local
  redacted logs, support bundles, and user-initiated exports are sufficient.
- **Bundling UAD-NG data, automatic APK-verification bypass, and guessed Android
  17 uninstall workarounds** — licensing, safety, and evidence failures. Keep
  user-supplied local conversion (R-146) and verbatim OEM failure reporting
  (R-141/R-142).
- **Global coverage percentages, a broad plugin marketplace, and a major React,
  Vite, Tailwind, or Tauri migration** — do not pay down the observed boundary,
  accessibility, or release risks; use focused gates and existing extension
  points first.
- **New mobile/offline/multi-user/migration features in this pass** — the app is
  already local/offline-first and its profile/settings migration contracts are
  versioned; no public evidence justified reopening the blocked architecture.

## Sources

### Advisories and standards
- https://github.com/advisories/GHSA-rgw5-rvv9-x895
- https://github.com/advisories/GHSA-2v37-7h3g-55p8
- https://github.com/advisories/GHSA-7gmj-67g7-phm9
- https://nvd.nist.gov/vuln/detail/CVE-2025-34449
- https://source.android.com/docs/security/bulletin/2026/2026-05-01
- https://cyclonedx.org/specification/overview/
- https://spdx.dev/wp-content/uploads/sites/31/2024/12/SPDX-3.0.1-1.pdf

### Android platform and tooling
- https://developer.android.com/tools/releases/platform-tools
- https://developer.android.com/tools/adb
- https://dl.google.com/android/repository/repository2-1.xml
- https://developer.android.com/about/versions/17/behavior-changes-all
- https://developer.android.com/reference/android/app/ApplicationExitInfo
- https://developer.android.com/reference/android/app/ActivityManager.html
- https://developer.android.com/tools/dumpsys
- https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/services/core/java/com/android/server/pm/PackageManagerShellCommand.java
- https://android.googlesource.com/platform/frameworks/base/+/master/core/java/android/app/ApplicationExitInfo.java
- https://android.googlesource.com/platform/system/core/+/master/fastboot/README.md
- https://developer.android.com/developer-verification
- https://developer.android.com/developer-verification/guides/faq
- https://developer.android.com/blog/posts/prioritizing-memory-efficiency-essential-steps-for-android-17
- https://raw.githubusercontent.com/google/perfetto/main/CHANGELOG

### Direct OSS competitors
- https://github.com/Genymobile/scrcpy/releases
- https://raw.githubusercontent.com/Genymobile/scrcpy/master/doc/video.md
- https://raw.githubusercontent.com/Genymobile/scrcpy/master/doc/control.md
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/releases
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/345
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1164
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1400
- https://github.com/viarotel-org/escrcpy
- https://github.com/Alex4SSB/ADB-Explorer/releases
- https://github.com/MuntashirAkon/AppManager/releases/tag/v4.1.0
- https://github.com/barry-ran/QtScrcpy
- https://github.com/yume-chan/ya-webadb
- https://github.com/RikkaApps/Shizuku

### Commercial, adjacent, and discovery sources
- https://github.com/DeviceFarmer/stf
- https://adbappcontrol.com/en/
- https://vysor.org/vysor-pro/
- https://www.airdroid.com/wiki/understanding-airdroid-business-plans-pricing/
- https://developer.android.com/studio/releases/past-releases/as-otter-3-feature-drop-release-notes
- https://github.com/callstack/agent-device
- https://github.com/mzlogin/awesome-adb
- https://github.com/JStumpp/awesome-android
- https://github.com/seun-novodev/android-tv-debloat-toolkit

### Dependencies, testing, and CI
- https://github.com/nodejs/Release/blob/main/schedule.json
- https://github.com/tauri-apps/tauri/releases
- https://crates.io/crates/serde_yaml_ng
- https://github.com/rust-secure-code/cargo-auditable
- https://v3.vitest.dev/config/
- https://github.com/rust-fuzz/cargo-fuzz
- https://rust-fuzz.github.io/book/
- https://doc.rust-lang.org/std/panic/struct.PanicHookInfo.html
- https://www.i18next.com/principles/fallback.html
- https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow

### Community and academic/engineering signal
- https://xdaforums.com/t/android-17-no-longer-able-to-uninstall-bloatware-via-adb.4795845/
- https://xdaforums.com/t/how-to-recover-from-bootloop-after-bad-debloat.4690784/
- https://xdaforums.com/t/adb-and-one-ui-7-android-15-boot-loop.4733316/
- https://news.ycombinator.com/item?id=48114356
- https://securitycafe.ro/2026/02/02/mobile-pentesting-101-the-death-of-adb-backup-modern-data-extraction-in-2026/
- https://arxiv.org/abs/2602.05312

## Open Questions

- **Bundle-capable host:** can a Windows host build and smoke-test an unsigned
  Tauri bundle now? This determines whether R-148 can move from a plan to an
  execution batch; source inspection cannot answer it.
- **GPL conversion posture:** is a user-selected UAD-NG file converted locally,
  hashed, and attributed acceptable for this MIT project? This is a maintainer
  licensing decision, not a coding inference.
- **Dependency automation:** keep Dependabot as the routine npm mechanism, or
  rely on the newly required npm floor plus the local audit gate? The 2026-08-08
  red audit shows the existing choice did not close the gap.
- **Mainline property and OEM samples:** which Google Play system-update property
  is readable across the supported Android/OEM matrix, and which real Android 17
  transcripts explain `pm uninstall --user` failures? R-135 and R-142 must remain
  unknown rather than guessing until a device sample exists.
- **PR release cost:** IMP-134 now runs `release:check --policy-only` on every
  frontend push and pull request; the scheduled lane retains the expensive
  multi-OS bundle smoke.
