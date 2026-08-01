# Droidsmith Roadmap

Single source of truth for what's planned and what's in flight. Completed items
are deleted from here and logged in [CHANGELOG.md](CHANGELOG.md). Blocked items
live in [Roadmap_Blocked.md](Roadmap_Blocked.md). Research context lives in
[RESEARCH_REPORT.md](RESEARCH_REPORT.md); do not duplicate that here - link
instead.

## Conventions

- `[ ]` - not started
- `[~]` - in flight
- Priority tags: **P0** (must ship in v0.1) . **P1** (v0.1 desirable / v0.2 must) . **P2** (later milestones) . **P3** (cosmetic / nice-to-have)
- **R-NNN** are roadmap items; **IMP-NN** are hardening / improvement items

## Remaining

### P2

- [ ] P2 — IMP-95: Consolidate the workspace visual system
  Why: All-route captures show excessive separators/outlined controls, undersized secondary text, repeated status pills, and weak grouping—especially in Mirror, Profiles, Logcat, and Tuning.
  Evidence: `test-results/rendered-routes/design-*.png`; `src/index.css`; `src/routes/common.tsx`; `src/components/`; all 11 route files.
  Touches: `src/index.css`, `tailwind.config.js`, `src/components/`, `src/routes/common.tsx`, `src/App.tsx`, `src/routes/**/*.tsx`, rendered-route smoke.
  Acceptance: Shared page-header, section, field-group, status, and action-row primitives define typography and density; borders indicate true containment rather than every row; non-action status defaults to text/icon instead of pills; explanatory copy is shortened; controls retain 44px targets where needed and body text remains WCAG-sized; all 11 workspaces plus settings/diagnostics/onboarding pass keyboard, non-English, narrow-width, and 200%-zoom captures with no clipping.
  Complexity: L
  Note (2026-07-31): the `Touches` list above is inaccurate — `src/components/` does not exist and the config is `tailwind.config.ts`, not `.js`. Shared primitives live in `src/routes/common.tsx` (17 exports). Substance unchanged.

- [ ] P2 — R-119: Add resumable fleet execution from a prior report
  Why: Commercial fleets and OSS multi-device tools emphasize batch continuity; Droidsmith emits stable fleet JSON but cannot safely rerun only failed/skipped targets after interruption.
  Evidence: `src-tauri/src/bin/droidsmith_cli.rs`; DeviceFarmer STF; Escrcpy multi-device workflows; commercial fleet/session reporting.
  Touches: `src-tauri/src/bin/droidsmith_cli.rs`, `src-tauri/src/profile.rs`, `src-tauri/src/journal/`, CLI fixtures and smoke tests.
  Acceptance: `run --retry-from <report.json>` validates report schema, profile hash, action set, device identity, Android user, and current transport before selecting only failed/skipped devices; dry-run is required before apply when inputs drift; completed actions are never replayed implicitly; JSON records lineage to the source report and uses stable exit codes for mixed outcomes.
  Complexity: L

## Research-Driven Additions — 2026-07-31 (v0.9.12)

From the 2026-07-31 RESEARCH.md pass. The prior pass's frontier (lockfile/version
contract, npm advisories, CI restoration, target-bound lifecycles, v0.5.3 upgrade
preservation, public project metadata, ADB transcript corpus, command-boundary
split) has all shipped. These items
are dominated by **external drift** — advisories and upstream deadlines that
postdate the prior pass — plus the content and pre-mutation-guardrail gaps that
the competitive failure evidence exposes. Device-only and off-mission ideas stay
in the RESEARCH.md Rejected table or Roadmap_Blocked.md. IDs continue from
R-119 / IMP-95.

### P0

- [ ] P0 — IMP-96: Clear RUSTSEC-2026-0221 and restore a green release gate
  Why: `cargo audit --deny warnings` fails today, so `npm run security:audit` and therefore `npm run release:check` — the authoritative gate — cannot pass.
  Evidence: RUSTSEC-2026-0221 (issued 2026-07-31); `src-tauri/Cargo.lock:1087` pins `event-listener` 5.4.1; fixed in 5.4.2 (2026-07-27); local run returns `error: 1 denied warning found!`.
  Touches: `src-tauri/Cargo.lock`.
  Acceptance: `cargo update -p event-listener --precise 5.4.2` lands; `cargo audit --deny warnings` exits 0; `npm run security:audit` passes; no `audit.toml` suppression is added.
  Complexity: S

- [ ] P0 — IMP-97: Make the CVE-2026-42184 fix a manifest requirement, not a lockfile accident
  Why: `src-tauri/Cargo.toml` declares `tauri = "2"`, which permits versions with an origin-confusion bug that lets a remote page invoke local-only IPC commands on Windows — against a surface of 98 privileged device-mutation commands.
  Evidence: GHSA-7gmj-67g7-phm9 / CVE-2026-42184 (2026-05-06, CVSS 8.8), `is_local_url()` `split_once('.')`, fixed in 2.11.1; `src-tauri/Cargo.lock` currently resolves 2.11.2.
  Touches: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.
  Acceptance: `tauri = "2.11.1"` and `tauri-build = "2.6"` are the declared floors; a `cargo update` cannot resolve below the fix; the release gate asserts the floor so it cannot silently regress.
  Complexity: S

- [ ] P0 — R-120: Gate wireless debugging on the device security-patch level
  Why: Droidsmith's own pairing flow plants an RSA host key in the device trust store, which is a stated precondition of CVE-2026-0073 — an adbd mutual-auth bypass giving remote shell-user code execution with no user interaction.
  Evidence: AOSP bulletin 2026-05-01 (CVSS 8.8, Critical, Android 14/15/16); `EVP_PKEY_cmp` `-1` treated as truthy in `adbd_tls_verify_cert`; `ro.build.version.security_patch` is already parsed at `src-tauri/src/adb/device_info.rs:117` but only rendered as a neutral field at `src/routes/devices/DeviceDetail.tsx:179`.
  Touches: `src-tauri/src/adb/device_info.rs`, `src-tauri/src/adb/wireless.rs`, `src/routes/Wireless.tsx`, `src/routes/devices/DeviceDetail.tsx`, `src/locales/*.json`, Rust + rendered-route tests.
  Acceptance: patch levels below `2026-05-01` surface an explicit, localized risk explanation before pairing or connecting over TCP, name USB as the safe alternative, and require acknowledgement; an unparseable or absent patch level reports `unknown` and never fabricates a verdict; USB flows are unaffected; fake-tool fixtures cover below/at/above/absent.
  Complexity: M

### P1

- [ ] P1 — IMP-98: Stop surfacing the stale `mdns_backend` field
  Why: Host Doctor displays a backend name that platform-tools 37.x can no longer report correctly, so the panel asserts something false on the recommended version.
  Evidence: 37.0.1 deleted the openscreen backend and made `ADB_MDNS_OPENSCREEN` a no-op, but AOSP `adb.cpp` still sets `AdbServerStatus.mdns_backend` to only `BONJOUR`/`OPENSCREEN` — the proto enum has no `LIBADBMDNS` value; surfaced via `src/routes/adbHealth.ts` and `src/routes/devices/AdbHealthPanel.tsx`, parsed in `src-tauri/src/adb/health.rs`.
  Touches: `src-tauri/src/adb/health.rs`, `src/routes/adbHealth.ts`, `src/routes/devices/AdbHealthPanel.tsx`, `src-tauri/src/support_bundle.rs`, `src/locales/*.json`, tests.
  Acceptance: on platform-tools ≥37.0.0 the backend name is not presented as fact; `mdns_enabled` remains surfaced; the support bundle records the raw value as unverified rather than as a health claim; a fixture at 36.0.2 and 37.x proves the version-dependent behaviour. Also update the blocked R-116 remainder note in Roadmap_Blocked.md.
  Complexity: S

- [ ] P1 — IMP-99: Make persisted device identity survive duplicate or blank serials
  Why: undo journals and per-device settings are keyed on the serial, so two devices reporting the same serial share one journal — an undo row recorded against device A becomes offerable against device B.
  Evidence: `src-tauri/src/journal/mod.rs:22,293` writes `<app_data>/journal/<serial>.jsonl`; `src-tauri/src/settings.rs::device_scope` hashes a caller-supplied identity string; duplicate/blank serials are documented upstream (scrcpy #1148, #3537). Runtime addressing is already correct — `DeviceTarget::adb_selector()` prefers `-t <transport_id>` — but `transport_id` is per-server-session and cannot key persistence.
  Touches: `src-tauri/src/journal/mod.rs`, `src-tauri/src/settings.rs`, `src-tauri/src/recovery_baseline.rs`, `src-tauri/src/commands/devices.rs`, `src/lib/deviceStore.ts`, migration + Rust tests.
  Acceptance: persisted identity mixes the build fingerprint (or an equivalent stable device attribute) with the serial; two fake devices sharing a serial but differing in fingerprint get distinct journals, settings scopes, and baseline identities; existing single-device data migrates in place with no loss and the legacy path stays readable; a blank serial does not produce a shared or empty-named store.
  Complexity: M

- [ ] P1 — R-121: Add a scrcpy known-vulnerable version policy
  Why: Droidsmith supervises a host scrcpy binary, and scrcpy ≤3.3.3 has a memory-safety bug in which a malicious *device* attacks the desktop *host* — exactly Droidsmith's threat model. No scanner will ever flag it.
  Evidence: CVE-2025-34449 (2025-12-18), global buffer overflow in `sc_device_msg_deserialize`, fixed in 3.3.4 (commit `3e40b24`, issue #6415); the scrcpy release notes describe it only as "Fix UHID_OUTPUT message parsing" and the repo's GitHub advisory list is empty. `src-tauri/src/scrcpy.rs` gates on capability only; `platform-tools-policy.json` already models `knownBadRules`.
  Touches: new `scrcpy-policy.json`, `src-tauri/src/scrcpy.rs`, `scripts/check-release-policy.mjs`, `src/routes/Mirror.tsx`, `src/locales/*.json`, tests.
  Acceptance: a reviewed policy file mirrors the platform-tools policy shape with a `3.3.4` security floor and a cited rationale/URL; a detected scrcpy below the floor surfaces a localized host-risk warning naming the CVE, never blocks newer versions, and reports `unknown` when the version cannot be parsed; the release gate rejects drift between the policy, the Rust constant, and the documentation.
  Complexity: M

- [ ] P1 — IMP-100: Move off the end-of-life Node 20 runtime
  Why: the project declares and gates on a runtime that reached EOL on 2026-04-30, so CI green no longer means "tested on a supported runtime".
  Evidence: `package.json` `"node": ">=20.19.0"`; `.github/workflows/ci.yml` pins `node-version: 20.19.0` in all three jobs; nodejs/Release `schedule.json` gives Node 20 EOL 2026-04-30, Node 24 Active LTS until 2026-10-20.
  Touches: `package.json`, `.github/workflows/ci.yml`, `README.md` supported-versions table, `scripts/check-release-policy.mjs`.
  Acceptance: the engines floor becomes `^22.12.0 || >=24.0.0`; CI runs on a supported LTS; the README supported-versions row and the release gate's derived check agree with the manifest; `npm ci && npm run release:check` passes on the new floor.
  Complexity: S

- [ ] P1 — IMP-101: Raise the Rust MSRV from 1.81 to 1.90
  Why: the floor is a scheduled wall set by an upstream project, and it is actively costing security posture rather than only convenience.
  Evidence: Tauri merged an MSRV bump to 1.90 on `dev` (PR #13221, 2026-07-02) with a stated "latest − 3" policy, so the next Tauri minor hard-blocks; `src-tauri/Cargo.toml` comments name the floor as the reason for `proptest = "=1.8.0"`, and `.cargo/audit.toml` names it as the reason RUSTSEC-2024-0436 (`paste`, unmaintained) is permanently suppressed; `specta = "=2.0.0-rc.22"` and `schemars = "=1.2.1"` are pinned for the same reason.
  Touches: `src-tauri/Cargo.toml`, new `rust-toolchain.toml`, `.cargo/audit.toml`, `src-tauri/Cargo.lock`, `README.md`, `.github/workflows/ci.yml`, `scripts/check-release-policy.mjs`.
  Acceptance: `rust-version = "1.90"` with a committed `rust-toolchain.toml` pinning the exact stable; the `=` pins on `proptest`, `specta`, `tauri-specta`, and `schemars` are relaxed or re-justified on their own merits; the `paste` suppression is removed from `audit.toml` rather than re-dated; `cargo check/clippy/test --all-targets --all-features` and the README supported-versions row all agree. Edition 2024 migration is explicitly out of this item.
  Complexity: M

- [ ] P1 — IMP-102: Adopt Tauri 2.11.5 for the isolation-pattern correctness fix
  Why: Droidsmith runs `"pattern": {"use": "isolation"}`, and Tauri 2.7.0 fixed a bug where the isolation pattern created iframes-within-iframes on Windows — a live correctness issue on the primary platform.
  Evidence: `crates/tauri/CHANGELOG.md` 2.7.0 (isolation iframe fix), 2.6.0 (`dynamic-acl` behind a default feature; async-command dispatch compile-time win), 2.5.0 (channel perf fix for small payloads — relevant to Logcat streaming); locked at 2.11.2, latest 2.11.5 (2026-07-01).
  Touches: `src-tauri/Cargo.lock`, `package.json` (`@tauri-apps/api`, `@tauri-apps/cli`), `isolation/index.js` if behaviour shifts, `scripts/isolation-policy.test.mjs`.
  Acceptance: Tauri crates and JS packages move to current 2.11.x together; `npm run security:isolation` and `npm run ui:smoke` pass; the isolation policy tests still assert the full validated command contract; evaluate `default-features = false` to drop the ACL reference tables and record the binary-size delta.
  Complexity: S

- [ ] P1 — R-122: Prove reinstall feasibility before permitting uninstall-for-user
  Why: Droidsmith records package provenance around the mutation, but nothing proves `pm install-existing` will succeed *before* the irreversible step — which is the exact moment users need the answer.
  Evidence: UAD-NG #559 ("warn if reinstalling an app isn't possible", unmet); the bootloop corpus (#1069 user half-bricked with no backup, #1400, #1311) shows the failure is always discovered too late; Droidsmith's `RestoreExistingForUser` path already knows how to verify retention (`src-tauri/src/adb/actions.rs`).
  Touches: `src-tauri/src/adb/actions.rs`, `src-tauri/src/adb/packages.rs`, `src-tauri/src/commands/plans.rs`, `src/routes/apps/PackageTable.tsx`, `src/routes/debloat/ApplyReview.tsx`, `src/locales/*.json`, fake-tool fixtures.
  Acceptance: every uninstall-for-user plan carries an explicit recoverable / not-recoverable / unknown verdict derived from proven APK retention for that Android user, shown in the review screen and the debloat batch summary before apply; `unknown` is never presented as recoverable; the verdict is written into the journal row so later undo decisions use the pre-mutation evidence; fixtures cover retained system, `/data/app` user app, and OEM-ambiguous states.
  Complexity: M

### P2

- [ ] P2 — R-123: Add a reversible action tier below disable and uninstall
  Why: the whole documented failure mode is users reaching for an irreversible action because no safer rung is offered; Android exposes several fully reversible package states that no desktop GUI surfaces.
  Evidence: `pm suspend`/`unsuspend`, `pm suspend-quarantine`, `pm unstop`, `pm hide-notifications`, `pm set-distracting-restriction` are all present in AOSP `PackageManagerShellCommand.java`; Canta #148 ("allow disabling apps", 14 reactions) is the top request of the leading mobile debloater; grep confirms none of `appops`, `suspend`, or `standby` appears anywhere in `src-tauri/src`.
  Touches: `src-tauri/src/adb/actions.rs` (`ActionKind`), `src-tauri/src/adb/packages.rs`, `src-tauri/src/journal/`, `src/routes/Apps.tsx`, `src/routes/apps/PackageTable.tsx`, `src/locales/*.json`, `packs/schema.json`, tests.
  Acceptance: suspend/unsuspend join the journaled action set with a proven inverse and post-state verification; the review screen ranks available actions by reversibility and defaults to the least destructive one that achieves the user's intent; every new subcommand is runtime-probed per device (parse `pm help`) and hidden rather than broken when absent; pack schema v1 remains valid and unchanged.
  Complexity: M

- [ ] P2 — R-124: Grow the vendor quirk corpus from the documented failure record
  Why: the quirks engine — schema, loader, `explain_failure`, and UI hint surface — is fully built and tested but ships exactly one rule, so a headline differentiator is inert.
  Evidence: `quirks/hyperos.yaml` is the only rule file (1 rule); UAD-NG's tracker documents specific, reproducible hazards with package names and ROMs: `com.android.overlay.circletosearch` soft-bricks HyperOS 2.0 (#1150), `com.android.phone` removal breaks SIM detection (#1168), Parental Controls removal broke Google One VPN on Pixel (#1096), `com.samsung.android.timezone.data_R` and `com.android.uwb.resources` bootloops (#1358/#1353/#1302), Samsung SM-A202F and A40 bootloops (#1311/#1295).
  Touches: `quirks/*.yaml`, `src-tauri/src/quirks/`, `src-tauri/src/commands/packs.rs`, `src/routes/debloat/QuirkHint.tsx`, `contribution-schema-policy.json`, tests.
  Acceptance: quirk rules exist for each vendor that ships a pack, each citing a public source URL and the ROM/build it was observed on; rules match on package plus manufacturer/ROM rather than error text alone where the hazard is pre-emptive; the debloat review surfaces a hazard before apply, not only after a failure; every rule validates against schema `"1"` with no schema change; each rule states its evidence basis so an unobserved combination reports `unknown` rather than implying verification. Note: this adds *rules describing publicly reported behaviour with attribution*, which is distinct from redistributing UAD-NG's curated list — that remains blocked as R-036.
  Complexity: M

- [ ] P2 — IMP-103: Auto-classify shared-system-UID packages as unsafe
  Why: packages sharing `android.uid.system` are a deterministic, offline-detectable bootloop risk that current tiering does not account for.
  Evidence: UAD-NG #770 ("treat system UIDs as unsafe", unmet); UAD-NG #1311/#1295/#1400 are all Recommended-tier removals that bricked devices.
  Touches: `src-tauri/src/adb/packages.rs`, `src-tauri/src/packs/mod.rs`, `src/routes/debloat/ApplyReview.tsx`, `src/routes/debloat/PackPreview.tsx`, `src/locales/*.json`, fake-tool fixtures.
  Acceptance: shared-UID detection runs from existing package enumeration with no extra device round trip where possible; a package sharing the system UID is raised to the unsafe tier regardless of its pack tier, is called out by name in the final count/unsafe review, and cannot be silently included in a batch; the override remains possible but requires explicit per-package acknowledgement.
  Complexity: S

- [ ] P2 — IMP-104: Add automated accessibility assertions to the rendered-route smoke
  Why: substantial hand-built accessibility work (forced-colors, 2.5.8 target sizing, focus trapping, live regions, reduced motion) is protected by review alone; nine-plus a11y fix commits in the last 200 show it regresses.
  Evidence: `src/index.css` carries explicit IMP-61 WCAG work; `scripts/check-rendered-routes.mjs` (4,379 lines) contains no axe or contrast assertions; `src/lib/contrast.test.ts` checks tokens only, not rendered surfaces.
  Touches: `scripts/check-rendered-routes.mjs`, `package.json`, `.github/workflows/ci.yml`, `release-policy.json`.
  Acceptance: an axe-core pass runs against every route in the existing mocked-IPC harness plus modals, the command palette, and onboarding; the rule set and any documented exclusions are declared in a reviewed config with rationale; violations fail `npm run ui:smoke`; the baseline is zero violations at introduction, not a suppressed snapshot.
  Complexity: M

- [ ] P2 — IMP-105: Add a computed-contrast harness over rendered surfaces
  Why: this is the single stated blocker on the light-theme item, and it is a harness gap rather than a hardware gap — the environment can compute rendered contrast, it just does not.
  Evidence: Roadmap_Blocked.md light-theme entry says the harness "checks console errors and document overflow, not contrast ratios"; `src/lib/contrast.test.ts` already implements WCAG relative-luminance maths against Tailwind tokens; Playwright can read computed styles per element.
  Touches: `scripts/check-rendered-routes.mjs`, `src/lib/contrast.test.ts` (extract shared helper), `release-policy.json`, `Roadmap_Blocked.md`.
  Acceptance: the smoke harness resolves effective foreground/background for text and interactive surfaces across every route — including the 17 files using `white/<opacity>` overlays — and fails below WCAG AA for the element's size class; the pass runs against the current dark theme first and is green before any theme work starts; on success, move the light-theme item back to ROADMAP.md with this harness named as its acceptance mechanism.
  Complexity: M

- [ ] P2 — IMP-106: Ship the dependency-free half of the provenance bundle
  Why: R-110 is blocked on a minisign key decision and a bundle-capable host, but SBOM and checksum generation read lockfiles and require neither — the blocked note conflates the two halves.
  Evidence: `Roadmap_Blocked.md` R-110 states the SBOM/checksum generation "is dependency-free"; `npm sbom --sbom-format cyclonedx --omit dev` is built into npm; `cargo-cyclonedx` 0.5.9 (2026-03-19) covers the Rust half; the project already maintains `third-party-notices.json` and a `deny.toml` licence allow-list.
  Touches: new `scripts/generate-provenance.mjs`, `scripts/check-release-policy.mjs`, new `*.test.mjs`, `README.md`.
  Acceptance: a unit-tested generator emits a merged CycloneDX SBOM covering runtime npm and cargo dependencies plus a `SHA256SUMS` file, deterministically and without network access or a built bundle; the release gate validates the SBOM parses and matches the lockfiles; README documents the verification steps; minisign signing and `cargo auditable` remain out of scope and stay in Roadmap_Blocked.md under R-110.
  Complexity: M

- [ ] P2 — IMP-107: Extend the ADB transcript corpus to every vendor that ships a pack
  Why: the corpus covers three OEM/version combinations while nine vendor packs exist, and parser variance is the second-most-repeated fix theme in the project's history.
  Evidence: `src-tauri/fixtures/adb-transcripts/v1/` holds AOSP 36.0.2, Samsung 37.0.0, and Xiaomi 37.0.1 only; `packs/` ships Pixel, OnePlus, Oppo, Realme, Motorola, Nothing, and Amazon FireOS with no matching fixtures; recurring parser fixes include wrapped `df` rows, bracketed `getprop`, bracketed IPv6 mDNS, and `ls` row variance.
  Touches: `src-tauri/fixtures/adb-transcripts/v1/`, `src-tauri/tests/fake_tool_contract.rs`, `src-tauri/src/adb/parsers.rs`, `src-tauri/src/adb/device_info.rs`.
  Acceptance: each packed vendor has a sanitized schema-v1 transcript exercising device listing, package enumeration, properties, storage, users, and services; every fixture is marked with its provenance and whether values are observed or synthesized; malformed and unknown rows still surface visible `parse_error` entries rather than being dropped; the corpus manifest stays version-gated at `"1"`.
  Complexity: M

- [ ] P2 — R-125: Consume the structured ADB device-tracking channel
  Why: the app regex-parses `adb devices -l`, which cannot express connection states or link speed that AOSP already publishes as structured data — and this retires the premise of the blocked R-101 note.
  Evidence: AOSP `services.cpp` exposes `host:track-devices-proto-binary` / `-proto-text`; `proto/adb_host.proto` defines `Device{serial, state, bus_address, product, model, device, connection_type, negotiated_speed, max_speed, transport_id}` and a `ConnectionState` including `DETACHED`, `RESCUE`, `NOPERMISSION`; Roadmap_Blocked.md R-101 correctly notes `server-status` has no USB-speed field — the speed lives on the per-device message instead. Needs live validation: the service names are verified from AOSP source, but the host `adb` CLI surface for reaching them at the supported versions is not — probe before building.
  Touches: `src-tauri/src/adb/transport.rs`, `src-tauri/src/adb/parsers.rs`, `src-tauri/src/adb/device.rs`, `src-tauri/src/commands/devices.rs`, `src/lib/deviceStore.ts`, `src/routes/Devices.tsx`, fixtures, `Roadmap_Blocked.md`.
  Acceptance: the text-proto path is used when the host `adb` supports it and falls back to the existing text parser otherwise, decided by runtime probe rather than a version assumption and with no new protobuf dependency; `DETACHED`/`NOPERMISSION`/`RESCUE` become distinct, explained states instead of collapsing into "unauthorized" or "offline"; `negotiated_speed`/`max_speed` are surfaced only when the device message actually carries them; fixtures cover both paths and a malformed proto response degrades to the text parser rather than failing. On success, close out the R-101 USB-link-speed remainder in Roadmap_Blocked.md.
  Complexity: L

- [ ] P2 — R-126: Add a guided pre-OTA restore and post-OTA re-apply round trip
  Why: debloated devices break on OTA updates, and the accepted community workflow — restore everything, update, re-debloat — is entirely manual today.
  Evidence: XDA guidance ("Do not install OTA update on a debloated phone, as you will face boot loops") and recovery threads; Droidsmith already has read-only OTA drift review, portable recovery baselines, and profiles, so only the orchestration is missing.
  Touches: `src-tauri/src/recovery_baseline.rs`, `src-tauri/src/upgrade.rs`, `src-tauri/src/profile.rs`, `src/routes/Apps.tsx`, `src/routes/apps/RecoveryBaselinePanel.tsx`, `src-tauri/src/bin/droidsmith_cli.rs`, `src/locales/*.json`, tests.
  Acceptance: a reviewed pre-OTA plan restores every recoverable package to its baseline state and reports exactly which packages cannot be restored before the user updates; after an OTA the drift review pairs with a re-apply plan derived from the same baseline, requires a dry-run diff, and never replays actions against packages whose post-OTA state already matches; the CLI exposes the same plan/apply pair with stable exit codes; irreversible packages are named explicitly at both ends.
  Complexity: L

- [ ] P2 — R-127: Detect Advanced Protection Mode as an explicit heuristic
  Why: when AAPM restricts debugging the device simply stops responding, and an opaque failure is the worst outcome for the user; a sourced key now exists where the blocked note assumed only guesswork was possible.
  Evidence: AOSP `Settings.java` defines `Settings.Secure.ADVANCED_PROTECTION_MODE = "advanced_protection_mode"`; adb shell holds `READ_SECURE_SETTINGS`. The key is `@hide` and unexercised on a device, so this modifies — not clears — the R-092 remainder blocker in Roadmap_Blocked.md.
  Touches: `src-tauri/src/adb/device_settings.rs`, `src-tauri/src/host_diagnostics.rs`, `src-tauri/src/install.rs`, `src/routes/HostDoctor.tsx`, `src/locales/*.json`, `Roadmap_Blocked.md`, fake-tool fixtures.
  Acceptance: the state is read via `settings get secure advanced_protection_mode` and classified as enabled / disabled / **unknown**, with `unknown` the default for any absent, empty, or unparseable value; an enabled result adds an explanatory note to install and connection failures but never blocks an operation or asserts a cause on its own; the UI states that the signal is a heuristic on an unstable key; fixtures cover all four response shapes. Update the R-092 blocker note to record that a candidate signal now exists but remains device-unverified.
  Complexity: M

- [ ] P2 — R-128: Add filter-based profile predicates as schema v3
  Why: profiles are ordered lists of concrete package actions, so they are effectively device-specific — which limits fleet `run` and the planned `--retry-from` to fleets of identical devices.
  Evidence: AppManager 4.1.0 (2026-06-29) ships filter-based profiles resolving predicates at run time with boolean expressions over `&`, `|`, and parentheses; Droidsmith already has versioned schemas with explicit review-and-migrate paths (`contribution-schema-policy.json`, profile v1→v2).
  Touches: `src-tauri/src/profile.rs`, `profiles/schema.json`, `src-tauri/src/bin/droidsmith_cli.rs`, `src/routes/Profiles.tsx`, `contribution-schema-policy.json`, `src/locales/*.json`, fixtures and tests.
  Acceptance: profile schema `"3"` supports predicates over attributes Droidsmith already enumerates (system vs user, enabled state, installer package, Android user, archived state) combined by a bounded, non-backtracking boolean grammar; v2 files migrate explicitly with review, exactly as v1→v2 does, and v2 remains loadable; the import diff resolves predicates against the live device and shows every matched package and planned command before apply; expression evaluation is total — an unresolvable attribute excludes the package and is reported, never silently matched.
  Complexity: L

- [ ] P2 — R-129: Render a local fleet report from existing run output
  Why: the CLI emits stable per-device JSON that nothing renders, and reporting is precisely what commercial fleet tools paywall.
  Evidence: `droidsmith-cli run --all-devices --json` emits a `devices[]` array with `outcome: ran | error | skipped`; AirDroid Business gates Alerts and Reports behind its Standard tier; DeviceFarmer STF's value is operational status across a pool.
  Touches: `src-tauri/src/bin/droidsmith_cli.rs`, `src/routes/Profiles.tsx`, new renderer module under `src/routes/`, `src/locales/*.json`, fixtures.
  Acceptance: a saved fleet report JSON can be opened read-only and rendered as a per-device outcome summary with failure reasons, skip causes, and per-action detail; rendering performs no device access and no network access; hashed device identity is shown rather than raw serials, matching the redaction rules already used by recovery baselines; the renderer rejects unknown schema versions with migration guidance; it composes with R-119's `--retry-from` rather than duplicating it.
  Complexity: M

### P3

- [ ] P3 — IMP-108: Stop announcing internal roadmap IDs to screen readers
  Why: every pane header exposes an internal tracker ID to assistive technology only — meaningless to users, invisible to sighted readers, and pure noise on every route change.
  Evidence: `src/routes/common.tsx:234` renders `MilestoneBadge` as `<span className="sr-only">{t("common.roadmap", {milestone})}</span>`; `src/locales/en.json:1755` is `"roadmap": "Roadmap {{milestone}}"`; `NAV_ITEMS` in `src/App.tsx` carries `milestone: "R-012"` and so on for all 11 routes.
  Touches: `src/routes/common.tsx`, `src/App.tsx`, `src/locales/*.json`, `scripts/check-rendered-routes.mjs`.
  Acceptance: pane headers announce the route name and description only; the milestone metadata either leaves `NAV_ITEMS` or stops reaching the accessibility tree; the rendered-route smoke asserts no roadmap identifier appears in any accessible name.
  Complexity: S

- [ ] P3 — IMP-109: Validate the bundled platform-tools policy at build time
  Why: a malformed edit to a bundled policy asset becomes a runtime panic on first use instead of a build failure.
  Evidence: `src-tauri/src/adb/version_policy.rs:81` calls `.expect("platform-tools-policy.json must match the Rust policy schema")` on an `include_str!` asset and asserts `schema_version`; `src-tauri/src/commands/packs.rs:564` has the same shape for `expect("assessment covers every pack entry")`.
  Touches: `src-tauri/build.rs`, `src-tauri/src/adb/version_policy.rs`, `src-tauri/src/commands/packs.rs`.
  Acceptance: the policy asset is parsed and schema-checked in `build.rs` so a malformed file fails compilation; the runtime path either keeps the now-unreachable invariant with a comment justifying it or degrades to a documented default; the packs assessment invariant returns a skip with a reported reason instead of panicking.
  Complexity: S

- [ ] P3 — R-130: Report accurate per-app storage from PackageManager
  Why: accurate app sizes are a sponsor-gated feature in the closest commercial competitor and are available from a single documented command.
  Evidence: `pm get-package-storage-stats [--user] <PKG>` is present in AOSP `PackageManagerShellCommand.java`; ADB AppControl 1.8.6 gates "accurate app sizes (A8+)" behind its paid tier.
  Touches: `src-tauri/src/adb/packages.rs`, `src-tauri/src/apk_metadata.rs`, `src/routes/apps/PackageTable.tsx`, `src/locales/*.json`, fixtures.
  Acceptance: app/data/cache sizes are read per package on demand within the existing lazy, bounded, identity-cached metadata path with no full-inventory scan; the command is runtime-probed and the column reports `unavailable` rather than an estimate where unsupported; sizes never block row rendering.
  Complexity: S

- [ ] P3 — R-131: Resolve marketing device names from build properties
  Why: device lists show codenames, which makes multi-device selection error-prone — and picking the wrong device is the highest-consequence mistake in this app.
  Evidence: DeviceFarmer STF #644 requests exactly this and is unmet; Droidsmith already parses `ro.product.*` in `src-tauri/src/adb/device_info.rs`.
  Touches: new offline name map resource, `src-tauri/src/adb/device_info.rs`, `src/routes/devices/DeviceTable.tsx`, `src/routes/common.tsx` (`DevicePicker`), `tauri.conf.json` resources.
  Acceptance: a bundled offline map resolves codename to marketing name with a cited source and revision date; the raw codename and serial remain visible alongside it; an unmapped device shows the codename unchanged with no guessed name; the map ships as a versioned resource checked by `npm run bundle:check`.
  Complexity: S

- [ ] P3 — IMP-110: Make onboarding resolve against live host and device state
  Why: the tour is a static five-step carousel that tells every user the same thing regardless of what is actually wrong, while the most-viewed content in the entire ADB corpus is failure diagnosis.
  Evidence: `src/routes/Onboarding.tsx` renders a fixed `STEPS` array and embeds `HostDoctor` below it, with no branching on host OS or live device state; Stack Overflow's ADB questions on unauthorized/offline/not-listed devices total well over 3M views.
  Touches: `src/routes/Onboarding.tsx`, `src/routes/HostDoctor.tsx`, `src-tauri/src/host_diagnostics.rs`, `src/locales/*.json`, `scripts/check-rendered-routes.mjs`.
  Acceptance: steps already satisfied by live evidence are marked resolved rather than presented as instructions; the driver step branches on host OS; the step matching the current failure is opened first; every claim is backed by a probe result and an unprobeable step is shown as informational rather than asserted; the smoke harness drives at least the all-clear, no-device, and unauthorized states.
  Complexity: M

- [ ] P3 — IMP-111: Add component-level tests for the highest-risk routes
  Why: all 22 frontend test files target pure-logic modules and `App.test.tsx` is the only `.tsx` test, so a single 4,379-line Playwright script is the sole regression gate for every route's UI.
  Evidence: `scripts/check-rendered-routes.mjs` is 4,379 lines; `src/routes/Apps.tsx` (1,933) and `src/routes/Mirror.tsx` (1,427) have no direct test; stale-response and device-switch races are the most repeated fix theme in git history.
  Touches: `src/routes/Apps.tsx`, `src/routes/Mirror.tsx`, `src/routes/Debloat.tsx` and sibling test files, `vitest.config.ts`, `package.json`.
  Acceptance: component tests cover the destructive-review, stale-completion, and device-switch paths for Apps, Debloat, and Mirror against mocked IPC; a test fails when an in-flight response resolves after a device change; the rendered-route smoke keeps its cross-route responsibility rather than absorbing these cases.
  Complexity: M

- [ ] P3 — R-132: Surface Android 17 app memory limits
  Why: Android 17 added a shell-reachable memory-limiter surface that no desktop ADB tool exposes, and it explains otherwise-inscrutable app kills.
  Evidence: Android 17 (API 37, released 2026-06-16) ships `am memory-limiter status | ignore <uid>|none|all | manual <pid> <limit>|max|none`, with kill attribution readable as `ApplicationExitInfo.getDescription() == "MemoryLimiter:AnonSwap"`; applies only to a device subset.
  Touches: `src-tauri/src/adb/device_info.rs`, `src-tauri/src/commands/devices.rs`, `src/routes/devices/ProcessManager.tsx`, `src/locales/*.json`, fixtures.
  Acceptance: `status` is read-only and shown only when the command is runtime-probed as present on an SDK 37+ device; the mutating `ignore`/`manual` forms either stay out of scope or enter the journaled reviewed-action path with a proven inverse, never a bare passthrough; absence reports `unsupported` rather than an error.
  Complexity: S
