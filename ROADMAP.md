# Droidsmith Roadmap

Single source of truth for what's planned and what's in flight. Completed items
are deleted from here and logged in [CHANGELOG.md](CHANGELOG.md). Blocked items
live in [Roadmap_Blocked.md](Roadmap_Blocked.md). Research context lives in
[RESEARCH.md](RESEARCH.md); do not duplicate that here - link instead.

## Conventions

- `[ ]` - not started
- `[~]` - in flight
- Priority tags: **P0** (must ship in v0.1) . **P1** (v0.1 desirable / v0.2 must) . **P2** (later milestones) . **P3** (cosmetic / nice-to-have)
- **R-NNN** are roadmap items; **IMP-NN** are hardening / improvement items

## Research-Driven Additions

From the 2026-08-08 RESEARCH.md pass. The dominant theme is **claims the codebase
makes that its own gates do not check**, plus upstream drift in platform-tools
and Android, privacy-safe diagnostics, and the widest remaining product gap
(fleet work is CLI-only). IDs continue from R-150 / IMP-129.

### P1

- [ ] R-134 P1 — Pin platform-tools to versioned archive URLs instead of a rolling one
  Why: a SHA-256 pinned against `platform-tools-latest-*.zip` must break on every upstream release, and the fetch script's response is to refuse the archive — so the sidecar path is guaranteed to fail rather than degrade.
  Evidence: `platform-tools-policy.json` `downloads.*.url` are the `-latest-` URLs; `scripts/fetch-platform-tools.ps1:94` throws on mismatch; probed 2026-08-04 — `platform-tools_r37.0.1-win.zip` 200, `platform-tools_r37.0.0-win.zip` 200, `platform-tools_r37.0.1-windows.zip` **404** (the Windows token is `win`); the rolling URL has served 37.0.1 since 2026-07-30 while the policy's `recommendedVersion` still says `37.0.0` (reviewed 2026-07-15); `https://dl.google.com/android/repository/repository2-1.xml` publishes rev + SHA-1 as a version oracle.
  Touches: `platform-tools-policy.json`, `scripts/fetch-platform-tools.ps1`, `scripts/fetch-platform-tools.sh`, `scripts/check-release-policy.mjs`, `README.md:196`
  Acceptance: the policy carries a `pinnedVersion` plus per-OS versioned URLs using the `win`/`darwin`/`linux` tokens; both fetch scripts download the pinned archive and verify its SHA-256; `recommendedVersion` is 37.0.1 with `reviewedOn` updated; a policy test asserts the Windows URL uses `-win.zip`; the README summary line regenerates from the policy.
  Complexity: S

- [ ] IMP-115 P1 — Add `.gitattributes` so provenance hashes stop depending on the checkout host
  Why: the provenance manifest hashes text-file bytes, and with no `.gitattributes` and `core.autocrlf=true` a Windows and a Linux checkout of the same commit produce different `SHA256SUMS` — which defeats the artifact's only purpose.
  Evidence: no `.gitattributes` in the tree; `.editorconfig` declares `end_of_line = lf` for all but `*.ps1`; `scripts/generate-provenance.mjs:47-60` reads `package.json`, `package-lock.json`, `Cargo.toml`, `Cargo.lock`, `third-party-notices.json` as UTF-8 and `:453` SHA-256s the content; `scripts/check-bindings.mjs:40` does an exact string compare of generated vs committed `src/lib/bindings.ts`; git history already carries a one-off "Cargo.lock CRLF" fix.
  Touches: `.gitattributes` (new), `scripts/generate-provenance.test.mjs`
  Acceptance: `* text=auto eol=lf` with `*.ps1 eol=crlf` and binary assets marked `-text`; the working tree renormalizes in one commit; a test asserts the provenance hash of a fixture is unchanged when the input is written with CRLF.
  Complexity: S

- [ ] R-135 P1 — Account for Project Mainline when judging CVE-2026-0073
  Why: the fix ships through the Mainline ADB subcomponent as a Google Play system update, so `ro.build.version.security_patch` under-reports patched devices and the one place Droidsmith emits a security verdict currently over-claims.
  Evidence: `src-tauri/src/adb/security_patch.rs:42-66` classifies from build patch level + SDK only; the AOSP 2026-05-01 bulletin lists ADB under Mainline-delivered components (ref A-469080888).
  Touches: `src-tauri/src/adb/security_patch.rs`, `src-tauri/src/adb/wireless.rs`, `src-tauri/fixtures/adb-transcripts/v1/`, `src/routes/Wireless.tsx`
  Acceptance: the classifier additionally consults the device's Google Play system update level (module metadata) and returns `Patched` when it is at or after the floor; when the module level cannot be read the verdict stays `AuthBypassUnpatched` **and** the UI states that the Mainline path was not checked, rather than implying a complete judgement; transcript fixtures cover patched-via-Mainline, unpatched, and module-unreadable. The exact readable property needs live validation against a device before the fixture is treated as authoritative.
  Complexity: M

- [ ] R-136 P1 — Emit licence, timestamp, tools and serial number in the SBOM
  Why: the project maintains a strict license allowlist and a third-party notices file, but the machine-readable artifact meant to carry that information expresses none of it — every consumer's primary use of an SBOM is license and vulnerability correlation.
  Evidence: `provenance/SBOM.cdx.json` — CycloneDX 1.6, 543 components, 543 with `hashes`, **0** with `licenses`; no `serialNumber`, no `metadata.timestamp`, no `metadata.tools`; `scripts/generate-provenance.mjs` contains no reference to any of them; `deny.toml` `[licenses].allow` and `LICENSE-THIRD-PARTY.md` already encode the policy.
  Touches: `scripts/generate-provenance.mjs`, `scripts/generate-provenance.test.mjs`, `provenance/SBOM.cdx.json`
  Acceptance: every component carries `licenses` sourced offline from `cargo metadata` and `package-lock.json`, with an explicit unknown marker where upstream declares none; the document carries `serialNumber`, `metadata.timestamp` and `metadata.tools`; a test fails if any component lacks a licence entry; the timestamp is derived from a reproducible input, not wall-clock, so `provenance:check` stays deterministic.
  Complexity: M

- [ ] IMP-116 P1 — Block placeholder installer hashes in the release gate
  Why: both committed package manifests carry 64 zeros as their integrity value and nothing prevents them being tagged or submitted in that state.
  Evidence: `packaging/winget/SysAdminDoc.Droidsmith.yaml:13`, `packaging/scoop/droidsmith.json:9`; written unconditionally by `scripts/generate-packaging-manifests.mjs` (`PLACEHOLDER_SHA256`); `scripts/check-release-policy.mjs` has a placeholder rule for domains only.
  Touches: `scripts/check-release-policy.mjs`, `scripts/packaging-manifests.test.mjs`, `scripts/generate-packaging-manifests.mjs`
  Acceptance: `release:check` fails when any packaging manifest holds an all-zero or non-hex SHA-256, proven by a test that makes it fail first; the generator marks the field as unpopulated in a way the gate recognises rather than emitting a plausible-looking hash.
  Complexity: S

- [ ] IMP-117 P1 — Close the README contract drift and regenerate the screenshots
  Why: the README states a schema version the code does not accept, and its three screenshots predate a full visual-system replacement — both are user-facing claims no gate covers.
  Evidence: `README.md:168` says profiles accept schema `"2"` while `src-tauri/src/profile.rs:16` is `"3"` and `README.md:199` says `"3"`; `scripts/check-release-policy.mjs:686` validates only the Platform-Tools sentence; `docs/screenshots/*.png` last regenerated 2026-07-17 (`git log`), before the 2026-08-02 v0.9.17 redesign; `npm run docs:screenshots` already exists.
  Touches: `README.md`, `scripts/check-release-policy.mjs`, `docs/screenshots/`
  Acceptance: the README profile-schema sentence is derived from `PROFILE_SCHEMA_VERSION` the same way the Platform-Tools line is derived from its policy, and the gate fails when they diverge; the three README screenshots are recaptured from the current visual system.
  Complexity: S

- [ ] R-137 P1 — Let a debloat pack choose its action
  Why: every pack entry is planned as `Disable`, so the reversible `suspend` rung shipped in v0.9.15 — the safest action the app has — is unreachable from the flagship content format, and so are uninstall-for-user and archive.
  Evidence: `src-tauri/src/commands/packs.rs:578` hardcodes `kind: actions::ActionKind::Disable`; `packs/schema.json` `PackEntry` carries a `removal` **risk tier** (Recommended/Advanced/Expert/Unsafe) but no action; UAD-NG issue #345 is the same request.
  Touches: `packs/schema.json`, `src-tauri/src/packs/mod.rs`, `src-tauri/src/commands/packs.rs`, `src/routes/debloat/`, `packs/*.yaml`
  Acceptance: a pack entry may declare a preferred action; the Debloat review shows the resolved action per entry and lets the user downgrade to a safer one; an action the device does not advertise (per the existing `pm help` probe) is never planned; unspecified entries keep planning `Disable` so existing packs are unchanged; the pack lint rejects an action the schema version does not define.
  Complexity: M

- [ ] IMP-118 P1 — Test the Tauri command surface
  Why: ~4,649 lines across thirteen `commands/*.rs` files carry no inline tests, and that set includes the two functions holding the P0 injection defect.
  Evidence: no `#[cfg(test)]` in `commands/{actions_commands,console,devices,diagnostics,files,installs,mirror,packages,plans,profiles,settings_commands,system,wireless}.rs`; `commands/packs.rs` is the only exception.
  Touches: `src-tauri/src/commands/*.rs`
  Acceptance: each command module has a test module covering its validation and error paths against a fake transport, starting with `files.rs` and `console.rs`; `list_remote_files` and `apply_remote_file_mutation` have tests asserting the executed command string.
  Complexity: M

- [ ] R-138 P1 — Fleet apply in the GUI
  Why: `--all-devices` exists only in the CLI and the Profiles workspace can only *review* a saved report, so the one capability every commercial competitor paywalls is invisible to anyone who does not open a terminal.
  Evidence: `--all-devices` appears in `src-tauri/src/bin/droidsmith_cli.rs` and in locale strings only; `src/routes/profiles/FleetReportPanel.tsx` is read-only; escrcpy 3.0.8 ships multi-device batch install/screenshot/input-broadcast; ADB AppControl gates batch operations behind "Extended", AirDroid Business behind per-device licensing.
  Touches: `src/routes/Profiles.tsx`, `src/routes/profiles/`, `src-tauri/src/commands/profiles.rs`, `src-tauri/src/fleet_report.rs`
  Acceptance: a profile can be planned and applied across every connected authorized device from the GUI, reusing the CLI's skip semantics (unauthorized/offline/unsafe-TCP are skipped, not aborted); progress is per-device and cancellable; the run writes the same schema-2 fleet report the CLI writes, and the existing report viewer opens it; resume stays a CLI operation and the GUI points at `--retry-from`.
  Complexity: L

- [ ] IMP-130 P1 — Make native and renderer failures locale-safe without hiding OEM detail
  Why: the locale catalogs have exact static-key parity, but the renderer recovery fallback is hard-coded English and most routes render raw `CommandError.message` strings directly, so a failure can be both untranslated and more revealing than its surrounding UI.
  Evidence: `src/RendererErrorBoundary.tsx:63-66`; `src/lib/tauri.ts:568-579`; direct render sites include `src/routes/Apps.tsx`, `src/routes/Devices.tsx`, `src/routes/ApkAnalyzer.tsx`, `src/routes/Logcat.tsx`, and `src/routes/Wireless.tsx`; `src-tauri/src/commands.rs:77-88` already supplies stable error codes; i18next fallback guidance recommends an explicit production fallback language.
  Touches: `src/lib/tauri.ts`, `src/lib/rendererError.ts`, `src/RendererErrorBoundary.tsx`, `src/locales/*.json`, route error surfaces, tests
  Acceptance: known command codes map to localized summaries in all five locales; exact OEM/device text remains verbatim in a labelled technical-details disclosure while renderer-added host paths/identifiers are separately redacted; the nested recovery fallback has locale-safe static copy and a test exercises it when the i18n tree itself fails.
  Complexity: M

- [ ] IMP-133 P1 — Redact native panic payloads before writing `crash.log`
  Why: the diagnostics module claims no PII, but its panic hook persists arbitrary payload strings while support-bundle sanitization happens only later during export.
  Evidence: `src-tauri/src/diagnostics.rs:15-16,189-212` writes the payload verbatim; `src-tauri/src/support_bundle.rs:778-825` tests sanitized excerpts but not the raw hook; Rust `PanicHookInfo::payload` is explicitly caller-provided data.
  Touches: `src-tauri/src/diagnostics.rs`, `src-tauri/src/support_bundle.rs`, native diagnostics tests
  Acceptance: crash records retain timestamp, panic class, and source location but bound or redact arbitrary payload content before persistence; a test payload containing a serial, path, email, and command output never appears in `crash.log` or rotated excerpts; support-bundle and wipe behavior remain unchanged.
  Complexity: S

### P2

- [ ] IMP-119 P2 — Add the two missing empty states
  Why: a healthy host and an empty app inventory currently render as chrome with no content, which is indistinguishable from a render failure.
  Evidence: `src/routes/HostDoctor.tsx:76-113` maps `findings` with no zero-length branch while `src-tauri/src/host_diagnostics.rs:161-360` pushes findings only conditionally; `src/routes/Mirror.tsx:886-913` renders a zero count with no copy; the shared `EmptyState` at `src/routes/common.tsx:395` is used correctly by every other collection route.
  Touches: `src/routes/HostDoctor.tsx`, `src/routes/Mirror.tsx`, `src/locales/*.json`
  Acceptance: HostDoctor renders an explicit "no problems found" state; Mirror's app inventory renders `EmptyState`; both are asserted in `scripts/check-rendered-routes.mjs`; all five locales carry the new keys.
  Complexity: S

- [ ] IMP-120 P2 — Run `cargo deny` in the local security gate
  Why: `release:check` now runs cargo-deny, but the standalone `npm run security:audit` path used by contributors and CI still omits the license, ban, and source policy, so the two local security entry points can disagree.
  Evidence: `scripts/check-release-policy.mjs` invokes `cargo deny --locked ... check bans licenses sources`; `.github/workflows/ci.yml:97` repeats it; `package.json` `security:audit` still runs only npm audit, `audit-rust.mjs`, and isolation.
  Touches: `package.json`, `scripts/audit-rust.mjs`
  Acceptance: `npm run security:audit` runs `cargo deny` with the same arguments as CI and fails on a violation; a missing `cargo-deny` binary is a clear actionable error, not a silent skip.
  Complexity: S

- [ ] IMP-121 P2 — Fix the repository discoverability metadata
  Why: the project is mis-tagged with a language it does not contain and is missing every topic a prospective user would search, on 9 stars.
  Evidence: `gh repo view` reports topics `android, kotlin, rust` — there is no Kotlin in the tree — with an empty `homepageUrl`; `bundle.homepage` in `src-tauri/tauri.conf.json` is set.
  Touches: repository settings (no files)
  Acceptance: `kotlin` removed; `tauri`, `typescript`, `adb`, `scrcpy`, `debloat`, `android-debloat`, `device-management` added; homepage points at the repository or its releases page.
  Complexity: S

- [ ] IMP-122 P2 — Retire the local documents that mislead a reader
  Why: four ignored/untracked documents or templates state things that are false, including a security contact that does not exist alongside a disclosure path that does.
  Evidence: `SECURITY.md` (untracked) names `security@droidsmith.invalid`, a Discord absent from the README, a `0.0.x` supported line, and lists signing / SBOM publication / Ed25519 update verification as current hardening commitments while all are in `Roadmap_Blocked.md`; `.github/ISSUE_TEMPLATE/config.yml` already routes to GitHub private advisories; `Roadmap_Blocked.md` still carries a light-theme entry that IMP-112 shipped in v0.9.16; `.github/ISSUE_TEMPLATE/bug.md` and `feature.md` are untracked duplicates of the tracked `.yml` forms; `docs/DEVELOPMENT.md` is untracked, says Node 20+, and omits tuning/APK Analyzer routes.
  Touches: `SECURITY.md`, `docs/DEVELOPMENT.md`, `Roadmap_Blocked.md`, `.github/ISSUE_TEMPLATE/bug.md`, `.github/ISSUE_TEMPLATE/feature.md`
  Acceptance: `SECURITY.md` names the GitHub private-advisory path, states the real supported line, and separates shipped hardening from planned; `docs/DEVELOPMENT.md` either becomes the tracked accurate development guide or is removed from the contributor link; the stale light-theme entry is removed from `Roadmap_Blocked.md`; the duplicate `.md` issue templates are deleted.
  Complexity: S

- [ ] IMP-123 P2 — Correct the MSRV rationale comment
  Why: the comment justifying the 1.90 floor cites a Tauri MSRV bump that has not landed in the released line, and this repo's own convention is to fix a note in place the moment it is found wrong.
  Evidence: `src-tauri/Cargo.toml` `rust-version = "1.90"` with a comment stating Tauri merged a bump to 1.90 on `dev` and that the next minor hard-blocks at 1.81; `tauri` 2.11.x still declares `rust-version = 1.77.2`.
  Touches: `src-tauri/Cargo.toml`
  Acceptance: the floor stays at 1.90 and the comment states the actual current reason (toolchain features in use and the pinned `rust-toolchain.toml`), with the Tauri claim removed or dated and sourced.
  Complexity: S

- [ ] R-139 P2 — Surface the `adb kill-server` blame chain
  Why: platform-tools 37.0.1 makes `kill-server` report which process requested it, which is the missing half of the most common ADB failure — another tool restarting the server underneath Droidsmith.
  Evidence: platform-tools 37.0.1 release notes, "`kill-server` prints the requesting process command-line chain"; `src-tauri/src/adb/health.rs` already parses `server-status`; UAD-NG issue #67 asks for a safe disconnect.
  Touches: `src-tauri/src/adb/health.rs`, `src/routes/HostDoctor.tsx`, `src/routes/devices/AdbHealthPanel.tsx`
  Acceptance: guided ADB recovery captures and displays the blame chain when the server version is 37.0.1 or newer; older servers report the capability as unavailable rather than showing an empty field.
  Complexity: S

- [ ] R-140 P2 — Report the platform-tools 37.0.1 USB backend flip per OS
  Why: 37.0.1 moves Windows onto `libadbusb` and *disables* `libusb` on macOS — opposite directions in one release — and only `adb server-status` reveals which backend is live, so a user diagnosing a detection failure has no way to know what changed.
  Evidence: platform-tools 37.0.1 release notes (`libadbusb` replaces `libusb` on Windows, `ADB_USB_LEGACY=1` to disable; `libusb` disabled on macOS, `ADB_LIBUSB=1` to re-enable); `src-tauri/src/adb/health.rs` already reads `usb_backend`; the equivalent mDNS reliability gate at `health.rs:65` is the pattern to copy.
  Touches: `src-tauri/src/adb/version_policy.rs`, `src-tauri/src/adb/health.rs`, `src/routes/HostDoctor.tsx`
  Acceptance: Host Doctor states the expected backend for the host OS and server version and names the exact environment variable that changes it; the guidance is suppressed when the version cannot be determined, matching how `mdns_backend_reliable` behaves.
  Complexity: S

- [ ] R-141 P2 — Make APK install failures legible ahead of developer-verification enforcement
  Why: enforcement starts 2026-09-30 in select regions, and store/non-ADB distribution may fail for reasons the current error surface cannot explain; Google's official FAQ now explicitly says ADB installs work without verification, so this item must not imply that ordinary local ADB installs are threatened.
  Evidence: https://developer.android.com/developer-verification and https://developer.android.com/developer-verification/guides/faq (enforcement date, limited-distribution 20-device cap, and the explicit ADB-install exemption); `src-tauri/src/install.rs` and `src/routes/apps/InstallPanels.tsx` classify install failures.
  Touches: `src-tauri/src/install.rs`, `src/routes/apps/InstallPanels.tsx`, `quirks/`
  Acceptance: verification-related `pm install` failure strings are classified and explained rather than surfaced as opaque `INSTALL_FAILED_*`; the explanation states what is known and what is not, and cites the developer-verification page; no workaround is asserted that has not been observed.
  Complexity: S

- [ ] R-142 P2 — Capture and surface the exact `pm` failure text for uninstall-for-user
  Why: users report `pm uninstall -k --user 0` failing on Android 17 where it worked on Android 16, and **no documented Android 17 behavior change touches `pm` or `cmd package`** — so the only honest response is to report precisely what the device said instead of guessing a cause.
  Evidence: https://xdaforums.com/t/android-17-no-longer-able-to-uninstall-bloatware-via-adb.4795845/ (Pixel 8, Android 17 stable, Chrome/Gmail/YouTube); https://developer.android.com/about/versions/17/behavior-changes-all contains no package-management change; `src-tauri/src/adb/actions.rs` `pm_failure_marker` already recognises `Failure [...]` and `Error:` shapes.
  Touches: `src-tauri/src/adb/actions.rs`, `src/routes/apps/`, `quirks/pixel.yaml`, `src-tauri/fixtures/adb-transcripts/v1/`
  Acceptance: an uninstall-for-user failure records and displays the device's verbatim `pm` output, the package, the Android user and the SDK level, and offers the existing `install-existing` recovery path; a quirk rule is added only once a real transcript is captured — this is a reporting change, not an assumed workaround. Needs live validation on an Android 17 device.
  Complexity: M

- [ ] IMP-124 P2 — Surface licence and third-party notices in the About dialog
  Why: the app bundles Apache-2.0 tooling and maintains a notices inventory, but neither the notices nor the project licence are reachable from the running application, and the notices file is not even shipped.
  Evidence: `src/App.tsx:596-655` shows only name, tagline, version and runtime; `third-party-notices.json` is absent from `bundle.resources` in `src-tauri/tauri.conf.json`; `LICENSE-THIRD-PARTY.md` exists and `check-release-policy.mjs` validates it.
  Touches: `src-tauri/tauri.conf.json`, `src/App.tsx`, `src-tauri/src/commands/system.rs`, `src/locales/*.json`
  Acceptance: `third-party-notices.json` ships as a resource; About renders the MIT licence, the bundled-component notices with their licences, and a repository link opened in the system browser; no network request is made.
  Complexity: S

- [ ] R-143 P2 — Apply a debloat pack from the CLI
  Why: packs are the project's flagship content format and the headless surface cannot use them, so no fleet or CI workflow can apply curated vendor content.
  Evidence: `src-tauri/src/bin/droidsmith_cli.rs` dispatches only `devices`, `run`, `migrate-v1`, `migrate-v2`, `baseline-export`, `baseline-inspect`, `baseline-apply`, `help`; `src-tauri/src/commands/packs.rs` `plan_pack` is GUI-only and single-device.
  Touches: `src-tauri/src/bin/droidsmith_cli.rs`, `src-tauri/src/packs/mod.rs`, `src-tauri/tests/cli_smoke.rs`
  Acceptance: `droidsmith-cli pack list|plan|apply` exists with `--device`/`--all-devices`, `--dry-run`/`--apply`, `--json` and the same exit codes; unsafe-tier entries require an explicit flag; a run writes the same journal and fleet report the GUI and `run` produce.
  Complexity: M

- [ ] R-144 P2 — Add the remaining reversible `pm` rungs
  Why: several verified-present subcommands sit between "leave it alone" and "disable", which is exactly the safety gradient the bootloop evidence calls for.
  Evidence: AOSP `PackageManagerShellCommand.java` on `main` dispatches `unstop`, `hide`/`unhide`, `disable-until-used`, `default-state`, `suspend-quarantine`, `get-archived-package-metadata`; `src-tauri/src/adb/actions.rs` `ActionKind` covers disable, uninstall-for-user, clear, force-stop, archive, unarchive, suspend/unsuspend.
  Touches: `src-tauri/src/adb/actions.rs`, `src-tauri/src/adb/packages.rs`, `src-tauri/src/commands/actions_commands.rs`, `src/routes/apps/`
  Acceptance: each new action is runtime-probed from the device's own `pm help` exactly as suspension is, captures before/after state, requires an exact transition, and records a verified journal inverse; a device that does not advertise a subcommand never shows it; `unstop` and `suspend-quarantine` carry explicit copy noting AOSP publishes no help text for them.
  Complexity: M

- [ ] R-145 P2 — Record per-entry "verified removable on" provenance in packs
  Why: the most-discussed unmet ask in the debloat space is knowing which ROM a removal was actually proven safe on, and the pack schema records provenance only at pack level.
  Evidence: UAD-NG issue #1164 (13 comments); `packs/schema.json` `PackEntry` has `id`, `removal`, `description`, `depends_on`, `needed_by`, `labels` — no verification record; pack-level `targets` and `provenance` exist but describe the whole document.
  Touches: `packs/schema.json`, `src-tauri/src/packs/mod.rs`, `src-tauri/src/bin/pack_lint.rs`, `src/routes/debloat/PackPreview.tsx`, `packs/*.yaml`
  Acceptance: an entry may carry verification records (build fingerprint prefix, Android level, outcome, date, source); the Debloat preview shows whether the connected device matches any record and says "not verified on this build" when it does not; `pack_lint` rejects a record missing a source; absence of records is rendered as unknown, never as safe.
  Complexity: M

- [ ] R-146 P2 — Import a user-supplied UAD-NG list
  Why: content depth is the weakest axis — 138 bundled pack entries and 11 quirk rules against a continuously-maintained upstream list — and the local-file import pattern already used for R-095 closes it without redistributing GPL-3.0 data.
  Evidence: `packs/*.yaml` total 138 entries (including the example pack); `quirks/*.yaml` total 11 rules; UAD-NG is GPL-3.0 and its data file is `resources/assets/uad_lists.json` (already cited by pinned commit in `quirks/samsung-oneui.yaml`); `packs/schema.json` `RemovalLevel` already mirrors UAD-NG's tiers and `depends_on`/`needed_by` mirror its dependency graph; `import_pack` in `src-tauri/src/commands/packs.rs:275` is the audited host-path grant model to reuse. Depends on the licensing-posture open question in RESEARCH.md.
  Note: this does **not** supersede blocked **R-036** (bundling the UAD-NG list with attribution), which stays blocked on redistribution permission. This item ships zero upstream data and is the same dependency-free substitution that resolved R-095's local-file half.
  Touches: `src-tauri/src/commands/packs.rs`, `src-tauri/src/packs/mod.rs`, `src/routes/debloat/PackPicker.tsx`
  Acceptance: a user-selected `uad_lists.json` converts locally to a schema-valid pack with tier mapping and attribution recording the source file's SHA-256 and licence; nothing from UAD-NG is committed to this repository; the resulting pack is badged as imported and removable; conversion failures name the offending record rather than dropping it.
  Complexity: M

- [ ] R-147 P2 — Expose the remaining scrcpy flags
  Why: display selection in particular is the difference between mirroring a device and mirroring the *right surface* on a device that has more than one.
  Evidence: `src-tauri/src/scrcpy.rs` builds `--flex-display`, `--keep-active`, `--new-display`, `--start-app`, vp8/vp9 and `--ignore-video-encoder-constraints`, but contains no `--display-id`, `--list-displays`, `--mouse=`, `--camera-torch`, `--camera-zoom`, `--capture-orientation`, `--no-clipboard-autosync` or `--push-target`; scrcpy `doc/video.md` and `doc/control.md`.
  Touches: `src-tauri/src/scrcpy.rs`, `src/routes/Mirror.tsx`, `src/routes/mirrorPresets.ts`
  Acceptance: displays are enumerated via `--list-displays` and selectable; mouse mode, camera torch/zoom, capture orientation, clipboard autosync and push target are exposed; each is gated on the probed binary advertising it, following the existing encoder-probe pattern, and stored in the per-device preset.
  Complexity: M

- [ ] IMP-125 P2 — Render a real-scale package inventory in the rendered-route gate
  Why: the gate exercises a handful of packages while real devices report 500-900, and `PackageTable` is not row-virtualized — so whether large inventories are usable is currently unmeasured in either direction.
  Evidence: `scripts/check-rendered-routes.mjs` mocks `list_packages` with a small fixture keyed on `com.example.app`; `src/routes/apps/PackageTable.tsx:405-409` uses `IntersectionObserver` for lazy metadata only, with no row windowing.
  Touches: `scripts/check-rendered-routes.mjs`, `src/routes/apps/PackageTable.tsx`, `release-policy.json`
  Acceptance: the gate renders a 1,000-package inventory, exercises filter/search/sort, and asserts a declared interaction budget; virtualization is introduced only if the measurement fails the budget, and the budget lives in `release-policy.json` alongside the bundle budget.
  Complexity: M

- [ ] IMP-126 P2 — Re-evaluate the YAML parser
  Why: `serde_yaml_ng` parses every untrusted document the app accepts and has had no release in roughly 27 months; the previously-preferred alternative was rejected only for being hours old.
  Evidence: `src-tauri/Cargo.toml` depends on `serde_yaml_ng = "0.10"`, last published 2024-05-26; RESEARCH.md 2026-07-31 deferred `serde-saphyr` pending patch history and the MSRV move, both of which have since happened (floor is now 1.90).
  Touches: `src-tauri/Cargo.toml`, `src-tauri/src/packs/mod.rs`, `src-tauri/src/quirks/mod.rs`, `src-tauri/src/profile.rs`, `deny.toml`
  Acceptance: a decision is recorded either way with a dated rationale; if migrating, all existing pack/quirk/profile fixtures parse identically and the proptest boundary suite passes unchanged; if staying, the reason and a re-review date are recorded in `Cargo.toml` next to the dependency.
  Complexity: M

- [ ] R-148 P2 — Publish the unreleased versions
  Why: twelve releases exist only in source; the newest downloadable artifact is v0.5.3 from 2026-07-17, so no user can obtain any work from the last three weeks.
  Evidence: `gh release list` returns only `v0.5.3` (2026-07-17) and `v0.1.0`; manifests are at 0.9.17; `README.md:106` already discloses the gap. Depends on the bundle-capable-host open question in RESEARCH.md.
  Note: this is the **Windows unsigned** artifact only, which project policy permits. It does not supersede the blocked "unsigned multi-platform distribution" entry (Linux bundles, still blocked on no Linux host), **R-006/R-010** (signing, notarization, `externalBin`) or **R-110** (minisign-signed provenance) — those blockers are unchanged.
  Touches: release process, `packaging/`, `CHANGELOG.md`
  Acceptance: an unsigned Windows MSI and NSIS installer are built, `release:check` including `release:smoke` passes against the real bundle, the artifacts are attached to a `v0.9.x` GitHub release with generated notes, `SHA256SUMS` is published alongside, and the packaging manifests carry the real installer hashes.
  Complexity: M

- [ ] R-149 P2 — Expose the headless CLI as an MCP server
  Why: agent-driven device workflows are where this space is moving, and Droidsmith can serve them over stdio with no HTTP client, no telemetry and no new network surface — the one differentiator its architecture makes cheap and its competitors' architectures make expensive.
  Evidence: `src-tauri/src/bin/droidsmith_cli.rs` already emits stable `--json` for every operation with documented exit codes; escrcpy 3.0.8 ships an MCP-protocol assistant; `callstack/agent-device` (3.9k stars) and Android Studio Otter 3's agent tooling target the same workflows.
  Touches: `src-tauri/src/bin/` (new binary), `src-tauri/src/fleet_report.rs`, `README.md`
  Acceptance: a `droidsmith-mcp` stdio server exposes read-only tools (list devices, list packages, plan profile, inspect baseline, read fleet report) plus explicitly-flagged mutating tools; every mutating tool refuses without the same confirmation the GUI requires; the server makes no network connection and holds no state the CLI does not already own.
  Complexity: L

- [ ] R-151 P2 — Add historical app-exit and ANR diagnostics to Process Manager
  Why: Process Manager shows the live `ps` snapshot and Android 17 memory-limit status, but it cannot explain why an app previously died — the diagnosis users currently obtain from an opaque `dumpsys` command.
  Evidence: `src/routes/devices/ProcessManager.tsx` has no exit-history query; `src-tauri/src/adb/device_info.rs` and `src-tauri/src/commands/devices.rs` expose only `am memory-limiter status`; Android's `ApplicationExitInfo` defines stable crash/ANR/low-memory/package-state reasons and `dumpsys` is the supported bounded system-service inspection path.
  Touches: `src-tauri/src/adb/`, `src-tauri/src/commands/devices.rs`, `src/routes/devices/ProcessManager.tsx`, `src/locales/*.json`, transcript fixtures, rendered-route smoke
  Acceptance: a selected package can request a bounded, read-only `dumpsys activity exit-info` history; parsed rows show timestamp, user, process, reason, status, RSS/PSS where present, and an explicit unknown state for OEM formats; traces and raw dumps are not captured automatically; package/user selection and target lifecycle guards match the existing process query.
  Complexity: M

- [ ] IMP-131 P2 — Turn the existing frontend coverage configuration into a focused gate
  Why: Vitest declares coverage reporters but no script, provider enablement, or threshold, so the repository cannot tell whether safety-critical helpers are losing tests while route rendering remains intentionally Playwright-only.
  Evidence: `vitest.config.ts:25-29` defines reporters/include/exclude; `package.json` has no coverage script; `.github/workflows/ci.yml` never invokes `vitest --coverage`; Vitest supports scoped glob thresholds and negative uncovered-item budgets.
  Touches: `package.json`, `vitest.config.ts`, `.github/workflows/ci.yml`, `src/lib/*.test.ts`, `src/routes/**/*.test.ts`
  Acceptance: `npm run test:coverage` uses a pinned provider, reports artifacts, and enforces measured thresholds for `src/lib` and command/state helpers rather than a vanity global percentage; CI runs it on pull requests and the threshold ratchets only with a test-backed change.
  Complexity: M

- [ ] IMP-132 P2 — Run the existing cargo-fuzz targets in a bounded scheduled lane
  Why: ADB/OEM text, YAML, journal JSONL, and scrcpy parsers are security-sensitive and already have fuzz targets, but the only execution path is a manual nightly command.
  Evidence: `src-tauri/fuzz/fuzz_targets/{adb_text,yaml_documents,journal_jsonl,scrcpy_text}.rs`; `README.md:505-509`; `.github/workflows/ci.yml` has no nightly `cargo fuzz` job; cargo-fuzz is Linux/Unix nightly tooling and its own documentation supports fixed-time runs and corpus minimization.
  Touches: `.github/workflows/ci.yml`, `src-tauri/fuzz/`, `README.md`, corpus/replay tests
  Acceptance: a scheduled or manually dispatched Linux job installs the pinned nightly/cargo-fuzz toolchain, runs each target with an explicit bounded budget, uploads minimized corpus/crash artifacts, and replays checked-in corpus cases in the normal parser test lane; it does not make Windows/macOS release jobs depend on nightly LLVM.
  Complexity: M

- [ ] IMP-134 P2 — Run the cheap release-policy check on pull requests
  Why: frontend/native/security jobs run on pushes and pull requests, but the production bundle smoke job is schedule/manual only, allowing a merge to break release metadata or the renderer-policy contract without immediate feedback.
  Evidence: `.github/workflows/ci.yml:99-106` gates `release-smoke` on `schedule` or `workflow_dispatch`; `scripts/check-release-policy.mjs` supports `--policy-only` and checks policy/version/schema metadata without building a native bundle.
  Touches: `.github/workflows/ci.yml`, release-policy documentation/tests
  Acceptance: push and pull-request CI runs `npm run release:check -- --policy-only` (or an equivalent direct node invocation) and fails on policy, version, schema, accessibility, provenance, or dependency-floor drift; the scheduled multi-OS full bundle smoke remains in place for expensive native verification.
  Complexity: S

### P3

- [ ] IMP-127 P3 — Grey out file operations the device will refuse
  Why: the file manager currently lets a user confirm a mutation that cannot succeed, and the permission bits needed to know better are already parsed.
  Evidence: `src-tauri/src/adb/parsers.rs` parses `ls -la` permission columns; `src/routes/devices/FileManager.tsx` enables actions unconditionally; ADB Explorer v1.0.26070 ships permission-based action gating.
  Touches: `src/routes/devices/FileManager.tsx`, `src-tauri/src/remote_files.rs`
  Acceptance: push, rename, delete and mkdir are disabled with a reason when the parsed permissions or the protected-path list forbid them; an unparseable permission string leaves the action enabled rather than guessing.
  Complexity: M

- [ ] R-150 P3 — Ship an Android TV / Fire TV debloat pack
  Why: TV boxes are an underserved surface with the same problem and no maintained desktop tooling, and the vendor pack framework already covers Fire OS.
  Evidence: `packs/amazon-fireos.yaml` exists with 12 entries; `seun-novodev/android-tv-debloat-toolkit` (549 stars) is the closest thing to a maintained list.
  Touches: `packs/`, `quirks/`, `src-tauri/tauri.conf.json`
  Acceptance: a pack targeting Android TV / Google TV builds ships with `targets` distinguishing them from handsets, every entry carries a description and provenance, and `pack_lint` passes; the pack is not offered on devices whose characteristics do not match.
  Complexity: M

- [ ] IMP-129 P3 — Make the markdown ignore rules match what is actually tracked
  Why: the documented rule and the repository state disagree, so a contributor or an agent is told roadmap and changelog edits are local-only when they are in fact public tracked files linked from the README.
  Evidence: `.gitignore` declares `*.md` with only `!README.md` and `!.github/ISSUE_TEMPLATE/*.yml` excepted; `CLAUDE.md:101` states "All `.md` files except README.md are gitignored"; `git ls-files` returns `CHANGELOG.md`, `RESEARCH.md`, `ROADMAP.md` as tracked, and `README.md:109-112` links to all three.
  Touches: `.gitignore`, `CLAUDE.md`
  Acceptance: `.gitignore` explicitly un-ignores the three tracked documents alongside `README.md`; the `CLAUDE.md` file-hygiene rule states which markdown files are public and which are local-only; `git check-ignore` reports no tracked file as ignored.
  Complexity: S

- [ ] IMP-128 P3 — Split `Apps.tsx`
  Why: it is the largest file in the frontend at 2,023 lines despite six components already extracted, and the initial bundle sits at 84% of its declared budget.
  Evidence: `src/routes/Apps.tsx` 2,023 lines against `src/routes/apps/` already holding `PackageTable`, `FilterControls`, `InstallPanels`, `JournalPanel`, `PermissionsPanel`, `RecoveryBaselinePanel`; `dist/assets/index-*.js` 380 KB against `release-policy.json` `initialJavaScriptBudgetBytes` 450000; the `commands.rs` split behind `command_registry.rs` is the precedent.
  Touches: `src/routes/Apps.tsx`, `src/routes/apps/`
  Acceptance: `Apps.tsx` becomes orchestration only, with export review, OTA restore/re-apply and the backup panels extracted; behaviour is unchanged as proven by the existing `Apps.test.tsx` and the rendered-route gate; the initial bundle does not grow.
  Complexity: M
