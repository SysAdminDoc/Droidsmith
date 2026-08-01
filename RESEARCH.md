# Research — Droidsmith

Date: 2026-07-31 — replaces all prior research.

## Executive Summary

Droidsmith v0.9.12 is a mature local-first Android workstation: 11 typed Tauri
workspaces, 98 IPC commands, ~528 automated tests, zero TODO/FIXME markers in
37.6k lines of Rust and 30.3k lines of TypeScript, and a governance layer
(`release-policy.json`, `target-lifecycle-policy.json`,
`contribution-schema-policy.json`, `platform-tools-policy.json`,
`language-contract.json`) that most projects this size do not have. The
2026-07-29 pass's top five items have all shipped: the lockfile/version contract
is consistent at 0.9.12 across seven files, `npm audit` reports zero
vulnerabilities across 326 dependencies, `.github/workflows/ci.yml` restores a
three-OS matrix, `commands.rs` fell from 5,342 to 1,329 lines behind a
`command_registry.rs`, and `fixtures/upgrade/v0.5.3/` proves state preservation
from the only public binary.

The frontier has therefore moved. It is no longer feature surface or hygiene —
it is **external-world drift**: upstream security advisories, upstream deadlines,
and upstream behaviour changes that silently invalidate what this codebase
currently asserts. Four of the top six findings below did not exist on
2026-07-29. Confidence is **Verified** unless marked otherwise.

Top opportunities, in priority order:

1. The authoritative release gate is **red today** — `cargo audit --deny
   warnings` fails on RUSTSEC-2026-0221, issued 2026-07-31.
2. `src-tauri/Cargo.toml` declares `tauri = "2"`, which permits versions
   vulnerable to CVE-2026-42184 (origin confusion → remote pages invoking
   local-only IPC on Windows).
3. Droidsmith's own wireless pairing arms **CVE-2026-0073** (CVSS 8.8, adbd
   mutual-auth bypass) on devices below the 2026-05-01 patch level; the app
   already parses `ro.build.version.security_patch` but only displays it.
4. Host Doctor surfaces `mdns_backend`, a field that became **stale and
   misleading** in platform-tools 37.x.
5. Persistent device identity is serial-derived; duplicate or blank serials
   collide undo journals and per-device settings across physically distinct
   devices.
6. Rust MSRV 1.81 is a **scheduled wall** — Tauri merged an MSRV bump to 1.90 on
   `dev` (2026-07-02); it also forces a permanent `paste` advisory suppression.
7. Bundled scrcpy detection has no known-vulnerable floor, and CVE-2025-34449
   (host attacked by a malicious device, scrcpy ≤3.3.3) is invisible to every
   automated scanner.
8. The vendor quirks engine — a documented differentiator — ships **one rule**,
   against a documented corpus of real-world debloat bootloops.
9. No reversible action tier below `disable`/`uninstall` (`pm suspend`,
   `suspend-quarantine`, `unstop`), which is precisely the safety rung the
   competitive failure evidence demands.
10. Two blocked items are now unblockable by building headless harnesses:
    a computed-contrast harness (unblocks the light theme) and an SBOM/checksum
    generator (unblocks the non-signing half of R-110).

## Product Map

- **Core workflows:** connect and diagnose USB/Wi-Fi devices; inspect state,
  files, processes, network, logs, traces, layout, and settings; manage packages
  with review, journal, and recovery; apply local debloat packs; author and run
  profiles (GUI + headless fleet CLI); mirror, control, and record via scrcpy;
  analyze local APKs offline.
- **User personas:** privacy-conscious device owners, Android power users,
  app/device developers, repair technicians, small refurbishing and IT teams.
- **Platforms and distribution:** Tauri 2 desktop for Windows, macOS, Linux;
  Node ≥20.19 and Rust ≥1.81; host-installed platform-tools, scrcpy, optional
  gnirehtet and Android Build Tools. Source is v0.9.12; the newest public binary
  remains v0.5.3 (2026-07-17). Unsigned by policy.
- **Key integrations and data flows:** React → Specta-generated IPC → Rust
  orchestration → local `adb`/`fastboot`/`scrcpy`/`gnirehtet`/`apksigner`; local
  JSON/YAML journals, settings, profiles, packs, baselines, redacted support
  bundles. No backend HTTP client, no telemetry.

## Competitive Landscape

### AppManager 4.1.0 (2026-06-29) — highest steal density

- **Does well:** *filter-based profiles* — a profile carries predicates (system
  vs user, installer package, SDK, disabled state, permission held, tracker
  count) combined with a boolean expression (`&`, `|`, parentheses), resolved
  against the live device at run time. Also a rich installer-option surface
  (update ownership, package source, origin URI).
- **Learn:** Droidsmith's profile schema v2 is an ordered list of *concrete*
  package actions, so a profile is effectively device-specific. Predicates would
  make fleet `run` and the planned `--retry-from` (R-119) dramatically more
  useful, and the codebase already has the versioned-schema-plus-explicit-
  migration machinery to land it as v3.
- **Avoid:** its Shizuku/root-mode breadth and on-device architecture; Droidsmith
  is a desktop host orchestrator.

### Universal Android Debloater NG 1.2.0 (2026-01-12)

- **Does well:** approachable no-root debloating, per-user removal, restore, and
  the largest curated package list in the space.
- **Learn — this is the strongest evidence in the entire pass:** its tracker is a
  running log of *Recommended-tier removals bricking devices* — issues #1400
  (Xiaomi bootloop, 2026-05-09), #1311 (Samsung SM-A202F, 2026-02-25), #1295
  (Samsung A40), #1069 (Xiaomi 13T half-bricked, no backup), #1150
  (`com.android.overlay.circletosearch` soft-bricks HyperOS 2.0), #1168
  (`com.android.phone` → SIM undetected), #1096 (Parental Controls removal broke
  Google One VPN on Pixel). Its own FAQ concedes *"We can't guarantee it 100%"*
  and the recovery path is a factory reset. Issue #1164 explicitly asks for
  per-ROM "verified removed safely" evidence in the UI; #770 asks for system-UID
  packages to be auto-marked unsafe; #559 asks for a warning when reinstalling
  will be impossible. **Nobody ships any of these.** Droidsmith's quirks engine
  is the right vehicle and currently holds one rule.
- **Avoid:** UAD-NG fetches package definitions from the network at launch, and
  that pipeline is what shipped the bootlooping "Recommended" entries. This is
  the empirical justification for Droidsmith's no-HTTP posture — cite it rather
  than treating the posture as ideology. Their list repo
  (`universal-android-preinstalled-lists`) is LGPL-3.0 and last pushed
  2025-01-06, confirming that R-036's redistribution blocker is correct.

### scrcpy 4.1 (2026-07-12) and the scrcpy-GUI cohort

- **Does well:** upstream performance and compatibility authority. 4.0/4.1 added
  flex display, VP8/VP9, `--keep-active`, `--background-color`, `--render-fit`,
  `--min-size-alignment`, mDNS TCP detection, and serials-with-spaces handling.
- **Learn:** `mirrorPresets.ts` tracks a subset of the 4.0/4.1 flag delta;
  R-118 covered one flag. Also note kil0bit-kb/scrcpy-gui (created 2026-01-21,
  1.2k stars, **Tauri v2 + React + Rust + MIT** — the same stack and licence)
  now ships graphics-renderer selection, camera-lens enumeration with a failsafe
  size, OTG HID input, and wireless connection history.
- **Avoid:** keyboard-to-touch mapping (73 reactions upstream since 2019, but a
  different product owned by QtScrcpy and scrcpy-mask, and unverifiable without a
  device — this confirms blocked R-081), and scrcpy-gui's launch-time polling of
  GitHub's release API.
- **Security consequence:** CVE-2025-34449 (global buffer overflow in
  `sc_device_msg_deserialize`, scrcpy ≤3.3.3, fixed 3.3.4) lets a **compromised
  device attack the desktop host** — exactly Droidsmith's threat model. scrcpy's
  own release notes describe the fix only as "Fix UHID_OUTPUT message parsing",
  and the GitHub advisory list for the repo is empty, so no dependency scanner
  will ever flag it.

### ADB Explorer 1.0.26070 / beta 26072 (2026-07-19)

- **Does well:** best OSS device file manager; the beta adds on-device zip
  browsing and tar read/write. Uses Weblate for translations.
- **Learn:** action history (their open #162) is something Droidsmith already has
  as a journal and under-advertises; pulling OBB alongside APK (#284) is a small
  extension of `extract_apk`.
- **Avoid:** they ship crash telemetry ("fixes from Grafana reports") and an
  auto-updater — both incompatible with Droidsmith's posture.

### aya 1.14.2 — abandoned since 2025-12-25

- Droidsmith's closest functional twin (mirror, files, apps, processes, layout
  inspector, logcat) is **dead**, and AGPL-3.0 blocks reuse. Its two features
  Droidsmith lacks are a per-package CPU/memory/FPS monitor and portable-mode
  distribution. The niche is currently open.

### ADB AppControl 1.8.6, Vysor, AirDroid Business

- **What they paywall is the signal.** ADB AppControl sponsor-gates the Debloat
  Wizard, Process Manager, dark theme, batch APK install, accurate app sizes,
  connection history, and **silent mode** (suppressing per-action confirmations).
  Vysor gates wireless mode and drag-and-drop. AirDroid gates Alerts and Reports.
- **Learn:** Droidsmith already gives away almost all of it. The two genuine
  gaps are accurate per-app sizes (`pm get-package-storage-stats` makes this a
  one-command fix) and a local report renderer over the fleet JSON it already
  emits.
- **Avoid:** enrollment, cloud relays, accounts, kiosk policy, telemetry.

### Tango/ya-webadb and DeviceFarmer STF

- Tango is the best-triaged repo in the survey (7 open issues) and its one
  significant gap — Android 11+ wireless pairing (#784, `effort: high`, open
  since 2025-08-22) — is something **Droidsmith already ships**, including
  QR-code pairing (`WIFI:T:ADB;...` rendered locally in `Wireless.tsx:68`) and
  mDNS TLS provenance. State this in the README comparison; it is a real lead.
- STF's device-pool continuity model is already reflected in R-119.

## Security, Privacy, and Reliability

- **Verified — the release gate is red as of 2026-07-31.** `cargo audit --deny
  warnings` (run locally) fails with `error: 1 denied warning found!` on
  RUSTSEC-2026-0221: `event-listener` 5.4.1 (`src-tauri/Cargo.lock:1087`) allows
  `!Send` tags to cross thread boundaries via `StackSlot`. Fixed in 5.4.2
  (2026-07-27). Path is `tauri-plugin-single-instance` → `zbus` →
  `event-listener`, so the affected code is Linux-only, but
  `scripts/audit-rust.mjs` denies warnings, which fails `npm run security:audit`
  and therefore `npm run release:check`. One `cargo update -p event-listener`
  clears it.
- **Verified — the Tauri version floor does not require the CVE fix.**
  `src-tauri/Cargo.toml` declares `tauri = "2"`. CVE-2026-42184 /
  GHSA-7gmj-67g7-phm9 (published 2026-05-06, CVSS 8.8) is an origin-confusion
  bug: `is_local_url()` used `split_once('.')`, so `http://app.evil.com` matched
  the `app://` custom protocol **on Windows**, letting a remote page invoke
  local-only IPC commands. Fixed in 2.11.1. `Cargo.lock` currently resolves
  2.11.2, so the shipped build is safe — but that is a lockfile accident, not a
  manifest requirement, and Droidsmith is a Windows-primary app whose IPC
  surface is 98 privileged device-mutation commands. Pin `tauri = "2.11.1"`.
- **Verified — Droidsmith's wireless pairing arms CVE-2026-0073.** AOSP bulletin
  2026-05-01, CVSS 8.8, rated Critical, affects Android 14/15/16: in
  `adbd_tls_verify_cert`, `if (EVP_PKEY_cmp(...))` treats the `-1` "different key
  types" return as truthy, so presenting an EC client certificate against a
  stored RSA host key authorises the connection. Secondary analyses report that
  exploitation additionally requires a previously paired RSA host key in the
  device trust store — which is exactly what Droidsmith's pairing flow plants —
  but that precondition is **Likely**, not Verified, and the warning below does
  not depend on it: the bug affects wireless ADB on any unpatched Android 14/15/16
  device. The app already parses
  `ro.build.version.security_patch` (`src-tauri/src/adb/device_info.rs:117`) but
  only renders it as a neutral field (`src/routes/devices/DeviceDetail.tsx:179`).
  It should gate wireless-debugging affordances behind an explicit warning below
  the 2026-05-01 patch level and prefer USB.
- **Verified — Host Doctor displays a field that is now wrong.** Platform-tools
  37.0.1 deleted the openscreen mDNS backend and made `ADB_MDNS_OPENSCREEN` a
  no-op, but AOSP `adb.cpp` still populates `AdbServerStatus.mdns_backend` with
  only `BONJOUR` or `OPENSCREEN` — the proto enum has no `LIBADBMDNS` value. So
  on 37.x the backend name Droidsmith surfaces in `AdbHealthPanel.tsx` /
  `src/routes/adbHealth.ts` is stale; only `mdns_enabled` is trustworthy. This
  also refines the blocked R-116 remainder: the `.local` hostname is real
  per-service mDNS metadata, but the *backend* field should stop being shown.
- **Verified — persistent device identity collides on duplicate serials.** The
  journal is written to `<app_data>/journal/<serial>.jsonl`
  (`src-tauri/src/journal/mod.rs:22,293`) and settings scope is
  `SHA-256("droidsmith-settings-device-v1\0" + device_identity)`
  (`src-tauri/src/settings.rs::device_scope`). Runtime *addressing* is already
  correct — `DeviceTarget::adb_selector()` prefers `-t <transport_id>` over
  `-s <serial>` (`src-tauri/src/adb/device.rs`), which is ahead of scrcpy
  (#1148, open since 2020). But `transport_id` is per-server-session and cannot
  key persistence, so two devices reporting the same serial (blank, or the
  well-known `0123456789ABCDEF` on low-cost hardware) share one undo journal and
  one settings scope. Undo rows recorded against device A become offerable
  against device B. Mix the build fingerprint into the persisted identity.
- **Verified — bundled/host scrcpy has no vulnerability floor.** There is a
  `platform-tools-policy.json` with `knownBadRules`, but no equivalent for
  scrcpy, and `scrcpy.rs` gates only on feature capability. CVE-2025-34449
  (≤3.3.3) is a device-attacks-host memory-safety bug with no GitHub advisory
  and no NVD hit under the keyword "scrcpy" — it must be tracked manually.
- **Verified — Node 20 reached EOL on 2026-04-30.** `package.json` declares
  `"node": ">=20.19.0"` and `.github/workflows/ci.yml` pins
  `node-version: 20.19.0` in all three jobs. The project is testing and gating
  on a dead runtime. Node 24 is the current Active LTS.
- **Verified — the Rust 1.81 floor is actively costing security posture.** Tauri
  merged an MSRV bump to 1.90 on `dev` (PR #13221, 2026-07-02) with a stated
  "latest − 3" policy, so the next Tauri minor will hard-block. Separately, the
  floor is the documented reason `.cargo/audit.toml` permanently suppresses
  RUSTSEC-2024-0436 (`paste`, unmaintained), and the reason `proptest`,
  `specta`, `schemars`, and `tauri-specta` carry `=` pins in `Cargo.toml`.
- **Verified — npm and isolation posture are strong.** `npm audit` reports zero
  vulnerabilities across 326 dependencies; the margin on postcss
  (GHSA-r28c-9q8g-f849, 2026-07-24) and brace-expansion (GHSA-mh99-v99m-4gvg,
  2026-07-24) is exactly one patch release each, so the current lockfile
  discipline is load-bearing. All build-tool advisories are dev-only —
  `frontendDist` is static output. The isolation layer
  (`isolation/index.js`, 1,063 lines) validates argument shape per command with
  read-only/sensitive partitioning, and `capabilities/default.json` grants only
  `core:default`.
- **Verified — no panic paths on attacker-controlled input.** All 206 `.unwrap()`
  calls but four are in `#[cfg(test)]` modules; the four production ones
  (`apk_analysis.rs:616,634,638`, `device_info.rs:308`) are provably bounded,
  including the APK signing-block parser. Two `expect` calls are hard-abort
  paths on internal invariants rather than user input:
  `src-tauri/src/adb/version_policy.rs:81` turns a malformed bundled
  `platform-tools-policy.json` into a runtime panic instead of a build error, and
  `src-tauri/src/commands/packs.rs:564` panics if the assessment map ever
  diverges from `pack.packages`.
- **Recovery need:** the debloat corpus evidence above (bootloops, cross-package
  dependency breakage, OTA-after-debloat failures) shows the missing guardrail is
  *pre-mutation*, not post-mutation. Droidsmith records provenance around the
  mutation and can undo a retained system app, but it does not prove
  `pm install-existing` will succeed *before* permitting the uninstall, and it
  offers no reversible rung below `disable`.

## Architecture Assessment

- **The command-boundary split landed well.** `commands.rs` is now 1,329 lines of
  shared boundary primitives with zero `#[tauri::command]`, and 14 domain files
  under `commands/` carry the 98 commands behind a single ordered
  `command_registry.rs`. No further decomposition is warranted here.
- **Remaining review hotspots**, to extract only when a roadmap item touches
  them: `src-tauri/src/adb/actions.rs` (2,381), `src-tauri/src/settings.rs`
  (2,396), `src-tauri/src/scrcpy.rs` (1,801), `src-tauri/src/bin/droidsmith_cli.rs`
  (1,693), `src/routes/Apps.tsx` (1,933), `src/routes/Mirror.tsx` (1,427).
- **`scripts/check-rendered-routes.mjs` is 4,379 lines** and is the *only*
  coverage for route UI: all 22 frontend test files target pure-logic `.ts`
  modules, and `App.test.tsx` is the sole `.tsx` test. A single monolithic
  Playwright script is now the load-bearing UI regression gate.
- **Two blocked items are unblockable by building a harness, not by acquiring
  hardware.** The light-theme block states that the smoke harness "checks console
  errors and document overflow, not contrast ratios" — but `src/lib/contrast.test.ts`
  already computes WCAG ratios over Tailwind tokens, and Playwright can read
  computed styles per surface. Likewise, R-110's blocked note claims SBOM
  generation waits on key management and a bundle-capable host; SBOM and checksum
  generation read lockfiles and need neither (only `cargo auditable` needs a
  build host, and minisign needs the key decision).
- **No automated accessibility assertions exist.** There is substantial manual
  a11y work — forced-colors support, 2.5.8 target sizing, reduced motion, focus
  trapping, live regions, token contrast tests — and nine-plus accessibility
  commits in the last 200. None of it is protected by an axe-core style gate, so
  regressions are caught only by review.
- **The ADB transcript corpus is narrower than the pack surface.**
  `src-tauri/fixtures/adb-transcripts/v1/` covers three combinations (AOSP
  36.0.2, Samsung 37.0.0, Xiaomi 37.0.1) while `packs/` ships nine vendor packs
  — Pixel, OnePlus, Oppo, Realme, Motorola, Nothing, and Amazon FireOS have
  curated package lists but no transcript fixtures. Parser variance is the
  second-most-repeated fix theme in git history.
- **Content, not code, is the thinnest layer.** `quirks/` contains one rule file
  with one rule (HyperOS `pm disable-user`), and the nine packs total 145 package
  entries. The quirks engine, schema, loader, `explain_failure` wiring, and UI
  hint surface all exist and are tested; only the corpus is missing.
- **Structured device tracking is available and unused.** AOSP exposes
  `host:track-devices-proto-binary` / `-proto-text` alongside the protobuf
  `AdbServerStatus`. The `Device` message carries `transport_id`,
  `negotiated_speed`, `max_speed`, and `ConnectionState` values (`DETACHED`,
  `RESCUE`, `NOPERMISSION`) that `adb devices -l` text cannot express. This
  directly retires the premise of the blocked R-101 note, which correctly
  observed that `server-status` has no USB-speed field — the speed lives on the
  per-device message instead. **Needs live validation:** the AOSP service names
  are Verified from `services.cpp`, but whether the host `adb` CLI exposes them
  as a `track-devices --proto-text` subcommand at the versions Droidsmith
  supports has not been exercised. Any implementation must runtime-probe and fall
  back to the existing text parser rather than assume availability.
- **Documentation drift is confined to untracked files.** `.gitignore` excludes
  `*.md` except `README.md`, but `CHANGELOG.md`, `ROADMAP.md`, and `RESEARCH.md`
  predate the rule and remain tracked (README links to all three, and the release
  gate rejects dead local links, so this is correct). `CONTRIBUTING.md`,
  `SECURITY.md`, `CODE_OF_CONDUCT.md`, `COMPLETED.md`, and
  `.github/PULL_REQUEST_TEMPLATE.md` are untracked and therefore never served by
  GitHub; `CONTRIBUTING.md:10-11` still claims `adb_client` and UAD-NG
  integration that do not exist, and `SECURITY.md` still carries
  `security@droidsmith.invalid`. IMP-92 correctly moved durable truth into the
  tracked README; the stale local copies should be deleted rather than
  maintained. Note also that the repo `CLAUDE.md` claim "All `.md` files except
  README.md are gitignored" is inaccurate for the three tracked files.
- **Shipped-but-unreleased work is accumulating.** `CHANGELOG.md` has an
  `[Unreleased]` section with 11 entries above the 0.9.12 heading while every
  manifest still reads 0.9.12, and the working tree carries uncommitted R-118
  work plus two untracked files (`src/routes/mirrorRecovery.ts`,
  `mirrorRecovery.test.ts`). R-118 is complete per the changelog and has been
  removed from ROADMAP.md accordingly.
- **Roadmap accuracy note:** IMP-95's `Touches` list names `src/components/` and
  `tailwind.config.js`; neither exists. Shared primitives live in
  `src/routes/common.tsx` (17 exports) and the config is `tailwind.config.ts`.
  The item's substance is unchanged.

## Rejected Ideas

- **Keyboard-to-touch mapping / on-screen control overlay** — scrcpy #712 (73
  reactions, open since 2019): the single largest unmet demand in the space, but
  it is a different product owned by QtScrcpy and scrcpy-mask, requires embedding
  a custom scrcpy client, and cannot be verified without a device. Confirms
  blocked R-081.
- **Remote pack/list auto-fetch and self-update** — UAD-NG's launch-time fetch is
  the pipeline that shipped the bootlooping "Recommended" entries in #1311/#1400;
  scrcpy-gui and ADB Explorer both poll upstream on launch. Conflicts with the
  no-HTTP posture and duplicates blocked R-095/R-036.
- **Crash telemetry** — ADB Explorer ships it ("fixes from Grafana reports").
  Contradicts the local-only posture and duplicates blocked R-073.
- **On-device archive browse/edit in the file manager** — ADB Explorer beta
  26072's strongest differentiator, but it expands Droidsmith's file manager from
  a means into a product, and zip/tar mutation on-device is a large new
  destructive surface for a workspace that is not the mission.
- **GitHub artifact attestations / SLSA provenance** — technically not code
  signing (ephemeral Sigstore certs, no long-lived keys), but structurally
  requires building release artifacts in GitHub Actions, which project rules
  forbid. `cargo auditable` + SBOM + `SHA256SUMS` is the reachable path.
- **`serde_yaml_ng` → `serde-saphyr` now** — `serde-saphyr` 1.0.0 published
  2026-07-31 (hours old) and requires edition 2024 / Rust ≥1.85. Revisit in Q4
  2026 after the MSRV move and after 1.0.x accrues patch history.
- **TypeScript 7** — `@typescript-eslint/parser@8.65.0` declares peer
  `typescript: ">=4.8.4 <6.1.0"`, and TS 7 exposes no stable programmatic API
  (typescript-eslint closed the request as *not planned*). TypeScript 6.0.3 is
  the correct staging target, not 7.
- **Weblate-hosted translations** — ADB Explorer's approach and a genuine scaling
  win for the five-language contract, but it is external hosting plus a
  contributor-workflow change, i.e. a maintainer decision rather than a coding
  item.
- **Cloud/MDM console, mobile Shizuku companion, plugin marketplace, online AI
  device control, full APK decompilation** — unchanged from the 2026-07-29 pass;
  each contradicts the local-only, reviewed-mutation, bounded-inspector posture.
- **Immediate React 19 / Vite 8 / Tailwind 4 migration** — no user-facing
  blocker. Vite 6.4.3 is the supported latest-6 minor, Tailwind 3.4.19 is the
  maintained `v3-lts` tag, and Tailwind 4's dark-mode default flip is
  disproportionately costly for a dark-by-default app. Sequencing note for
  whenever it happens: `@vitejs/plugin-react` 5.2.0 is the only bridge that
  supports both Vite 6 and 8; v6 is Vite-8-only. i18next 24→26 and react-i18next
  15→17 must move together (peer `i18next >= 26.2.0`).
- **QR-code wireless pairing** — repeatedly cited as unsolved across the cohort
  (scrcpy #6509, Tango #784), but Droidsmith already ships it
  (`src/routes/Wireless.tsx:68`). Not a gap; a lead to advertise.
- **Transport-id addressing** — scrcpy #1148 has been open since 2020, but
  `DeviceTarget::adb_selector()` already prefers `-t`. Only the *persistence*
  half is a real defect (see Security section).

## Sources

### Advisories and standards

- https://rustsec.org/advisories/RUSTSEC-2026-0221
- https://github.com/tauri-apps/tauri/security/advisories/GHSA-7gmj-67g7-phm9
- https://nvd.nist.gov/vuln/detail/CVE-2026-42184
- https://source.android.com/docs/security/bulletin/2026/2026-05-01
- https://nvd.nist.gov/vuln/detail/CVE-2025-34449
- https://www.vulncheck.com/advisories/genymobile-scrcpy-global-buffer-overflow
- https://github.com/advisories/GHSA-r28c-9q8g-f849
- https://github.com/advisories/GHSA-mh99-v99m-4gvg
- https://github.com/advisories/GHSA-p9ff-h696-f583

### Android platform and AOSP

- https://developer.android.com/tools/releases/platform-tools
- https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/proto/adb_host.proto
- https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/adb.cpp
- https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/daemon/auth.cpp
- https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/services.cpp
- https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/services/core/java/com/android/server/pm/PackageManagerShellCommand.java
- https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/java/android/provider/Settings.java
- https://developer.android.com/about/versions/17/behavior-changes-all
- https://developer.android.com/privacy-and-security/advanced-protection-mode
- https://developer.android.com/developer-verification
- https://github.com/google/perfetto/blob/main/CHANGELOG

### Competitors

- https://github.com/Genymobile/scrcpy/releases/tag/v4.1
- https://github.com/Genymobile/scrcpy/issues/712
- https://github.com/Genymobile/scrcpy/issues/1148
- https://github.com/MuntashirAkon/AppManager/releases/tag/v4.1.0
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1164
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1400
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1311
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1096
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/770
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/559
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/issues/1315
- https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation/wiki/FAQ
- https://github.com/Alex4SSB/ADB-Explorer/releases
- https://github.com/liriliri/aya
- https://github.com/kil0bit-kb/scrcpy-gui
- https://github.com/yume-chan/ya-webadb/issues/784
- https://github.com/viarotel-org/escrcpy/issues/419
- https://adbappcontrol.com/en/

### Dependencies and toolchain

- https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/CHANGELOG.md
- https://github.com/tauri-apps/tauri/pull/13221
- https://github.com/nodejs/Release/blob/main/schedule.json
- https://vite.dev/blog/announcing-vite8
- https://tailwindcss.com/docs/upgrade-guide
- https://devblogs.microsoft.com/typescript/announcing-typescript-6-0/
- https://github.com/rust-secure-code/cargo-auditable
- https://github.com/bourumir-wyngs/serde-saphyr
- https://reproducible-builds.org/docs/rust/

### Community signal

- https://stackoverflow.com/questions/23081263/adb-android-device-unauthorized
- https://stackoverflow.com/questions/79445793/android-phone-doesnt-show-rsa-fingerprint
- https://stackoverflow.com/questions/79630511/adb-returns-error-unknown-host-service-when-pairing-device-using-scrcpy-wirel
- https://stackoverflow.com/questions/14654718/how-to-use-adb-shell-when-multiple-devices-are-connected-fails-with-error-mor
- https://news.ycombinator.com/item?id=49045159
- https://news.ycombinator.com/item?id=49048348
- https://github.com/Genymobile/scrcpy/blob/master/FAQ.md
- https://www.androidauthority.com/android-sideloading-changes-timeline-3679204/

## Open Questions

- **Needs maintainer decision:** `SECURITY.md` is untracked and still names
  `security@droidsmith.invalid`. Either enable GitHub private vulnerability
  reporting or name a real monitored channel; until then Droidsmith has no
  disclosure path for findings like CVE-2026-0073's downstream implications.
  (Carried unchanged from 2026-07-29.)
- **Needs maintainer decision:** `.github/dependabot.yml` is tracked and
  configures npm, cargo, and github-actions updates. It is almost certainly why
  the postcss and brace-expansion patches are current — the margin on both is one
  patch release — but it conflicts with the operator's standing "no Dependabot"
  rule. Decide explicitly: keep it as the routine-bump mechanism, or delete it
  and accept that `npm run security:audit` covers only the security half.
- **Needs live validation:** `adb shell settings get secure advanced_protection_mode`
  is sourced from AOSP `Settings.java` (`ADVANCED_PROTECTION_MODE`, `@hide`, not
  `@Readable`) but has not been exercised on a device. It is a heuristic, not a
  contract, and the key can be renamed without notice — any implementation must
  degrade to `unknown` rather than assert a state.
