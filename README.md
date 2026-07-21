<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="144" height="144" alt="Droidsmith logo">
</p>

<h1 align="center">Droidsmith</h1>

![Version](https://img.shields.io/badge/version-0.9.4-cyan)
![License](https://img.shields.io/badge/license-MIT-green)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue)

A cross-platform, open-source workshop for Android devices over ADB.

Droidsmith is the spiritual successor to [ADB AppControl](https://adbappcontrol.com) — a
modern, cross-platform GUI for managing Android devices through ADB, without
root, without a closed-source binary, without paywalled features.

## Status

Functional early desktop build. The Tauri shell builds and runs; shipped routes
cover device readiness, wireless ADB pairing/connect, package inventory and
actions, reversible Android 15 package archiving, atomic
APK/APKS/XAPK/APKM installation with guarded failure remedies and an opt-in
`--incremental` single-APK mode that falls back cleanly when unsupported,
hashed base/split APK export, audited permission/device-control mutations, reviewed shell
mutations, journal undo, debloat queue recovery, scrcpy launch and session
supervision, optional gnirehtet reverse-tethering ("Share Internet") when the
binary is on PATH, cancellable background shell/export/file operations, incremental
Logcat streaming and export, live cross-route device hot-plug updates, ADB
server/mDNS/Wi-Fi 2.0 health with audited guided recovery, provenance-classified
USB/TLS/legacy/unknown transports with fail-closed unsafe-TCP acknowledgement,
read-only host connection diagnostics for ADB/tool/USB/driver/udev state,
portable pre-change recovery baselines with read-only OTA drift review,
GUI-authored schema-v2 action profiles with read-only live import diffs,
native-selected scrcpy recording destinations, and fastboot inspection. A
local-only Diagnostics center previews and saves redacted support
bundles with tool/OS/ADB health, failed-operation records, and crash excerpts;
it never uploads data and can wipe disposable local diagnostic history. For
deeper device failures, users can separately acknowledge a sensitive-data
warning and capture an atomic Android bugreport ZIP plus a redacted hash
sidecar; Droidsmith never scans, opens, attaches, or uploads the report.

The Devices file manager browses and pulls files and now also supports guarded
push, folder creation, same-directory rename, and delete. Every mutation shows
the exact native-selected source, device target, and argument boundaries before
confirmation, writes a durable journal outcome, verifies the resulting device
state, and refreshes the current directory. Device paths with spaces or
non-ASCII names remain distinct ADB arguments instead of interpolated shell
text. The device dashboard also includes a read-only layout inspector: one click
captures the on-screen UI hierarchy with `uiautomator dump`, renders it as a
searchable, depth-indented node tree (class, resource-id, text, content-desc,
bounds), and exports the raw XML through a one-shot save grant. Malformed dumps
surface visible parse errors instead of being dropped.

Per-user removal now records package provenance and post-state immediately
around the mutation. A preinstalled system app is undoable from Activity only
when PackageManager proves its APK remains retained for that Android user;
user-installed `/data/app` packages and unknown/OEM states remain explicitly
irreversible. Recovery uses `install-existing`, restores the prior enabled state,
and verifies the result before linking the undo journal row.

Package export defaults to a ZIP containing every base/split APK plus a
versioned manifest with artifact hashes and hashed device/build identity. The
deprecated `adb backup` path is hidden under Advanced, preflights target SDK,
debuggable, and `allowBackup` evidence when OEM output exposes it, and emits an
uncompressed `.ab` inside a manifest-bearing ZIP only after strict header/TAR
validation. Detected data entries are not a promise of completeness or future
restore compatibility; Droidsmith does not present `adb restore` as a reliable
recovery path.

Current blockers are tracked separately in [Roadmap_Blocked.md](Roadmap_Blocked.md):
signed release pipeline, bundled platform-tools wiring, UAD-NG redistribution,
crash-log upload infrastructure, and the future plugin API/marketplace.
Remaining actionable work lives in [ROADMAP.md](ROADMAP.md); release notes live
in [CHANGELOG.md](CHANGELOG.md); design rationale is summarized in
[RESEARCH_REPORT.md](RESEARCH_REPORT.md).

## Screenshots

### Device Readiness

![Droidsmith device readiness screen](docs/screenshots/droidsmith-overview.png)

### Package Workflow

![Droidsmith package workflow screen](docs/screenshots/droidsmith-apps.png)

### Mirror Workflow

![Droidsmith mirror workflow screen](docs/screenshots/droidsmith-mirror.png)

## Why another ADB GUI?

ADB AppControl is the closest thing the Windows ecosystem has to a polished
ADB front end, but it has hard limits that an open project can fix:

| | ADB AppControl 1.8.6 | Droidsmith |
|---|---|---|
| Source | Closed | MIT, public on GitHub |
| Platforms | Windows only (.NET 4.6+) | Windows, macOS, Linux |
| Free tier | Core only — dark theme, Process Manager, batch ops are sponsor-gated | All features always free |
| Debloat lists | Static, underperforms Universal Android Debloater per user reports | Versioned YAML packs, vendor quirks, recovery baselines, and a final count/unsafe-tier review before apply; UAD-NG import is blocked on redistribution permission |
| Screen mirror | Virtual buttons + screenshots | Capability-negotiated scrcpy launch/supervision with per-device presets, encoder selection, and actionable bounded failure diagnostics; bundled scrcpy remains planned |
| Wireless ADB | Manual `adb pair` in console | First-class Android 11+ pairing, exact mDNS TLS provenance, explicit legacy/unknown TCP warnings, and privacy-bounded VPN/mDNS failure guidance |
| Automation | None | GUI-authored YAML profiles, explicit v1 migration, live dry-run diffs, and a JSON-capable headless CLI |
| Extensibility | None | Versioned local pack, quirk, and profile schemas; plugin API and marketplace are deferred |
| i18n | EN + RU | i18next-driven (DE, EN, ES, RU, ZH), contributor-friendly |
| Multi-device | One at a time | Device selector and per-device workflows; side-by-side device tabs remain planned |

## Current tech stack

- **Tauri 2** — Rust core + native webview, single-binary distribution (~10 MB
  vs Electron's ~100 MB)
- **React + TypeScript + Vite** — frontend
- **ADB shell transport** — typed Rust wrappers around the platform-tools
  `adb` binary, with direct parser coverage for device/package/process/file
  transcripts
- **Lazy APK metadata** — Apps rows near the viewport gain bounded,
  identity-cached labels and raster icons without pulling every installed APK
- **scrcpy on PATH** — version and device encoders are probed, cached against
  the binary identity, and supervised with bounded failure diagnostics for
  mirror/control sessions
- **Versioned YAML packs, quirks, and profiles** — packaged as Tauri resources
  for local linting and reproducible actions
- **Tailwind** — dark-first route surfaces
- **i18next** — translations

Bundled platform-tools and bundled scrcpy are not wired into the installer yet;
that work is held with release signing in [Roadmap_Blocked.md](Roadmap_Blocked.md).
The current extension surface is schema-only: this build accepts schema version
`"1"` for packs and quirks and version `"2"` for profiles. Profile v1 has an
explicit review-and-migrate path; future revisions are rejected with migration
guidance. The plugin API and marketplace remain deferred in
[Roadmap_Blocked.md](Roadmap_Blocked.md).
See [RESEARCH_REPORT.md](RESEARCH_REPORT.md) for the rationale and the
alternatives considered.

## Profiles and headless automation

The **Profiles** workspace builds an ordered YAML profile from supported
journaled package actions. A profile can target the owner, foreground, or an
explicit discovered Android user and can optionally constrain the device serial
prefix, manufacturer, model, and SDK range. Export validates schema v2 and uses
an atomic, native-selected destination.

Import is read-only: Droidsmith validates the schema and live device/user
constraints, then shows every current-to-expected package state, readiness
reason, and exact planned ADB command. A v1 file is converted only in memory;
the user must review and save the v2 document before it can run.

The CLI uses the same validation and planning code. `--json` emits stable
machine-readable results, and exit codes are `0` for success, `1` for a failed
operation or incompatibility, `2` for invalid input, and `3` when ADB is absent.

```bash
droidsmith-cli devices --json
droidsmith-cli migrate-v1 old-profile.yaml --output profile-v2.yaml --json
droidsmith-cli run profile-v2.yaml --device SERIAL --dry-run --json
droidsmith-cli run profile-v2.yaml --device SERIAL --apply --json
```

Legacy or unknown TCP transports additionally require
`--allow-unsafe-transport`; USB and paired TLS Wi-Fi do not.

## Portable recovery baselines

Before applying a package action in Apps or a selected Debloat batch, export a
versioned JSON recovery baseline from the review screen. The file contains the
hashed device identity, build fingerprint, Android user, optional pack revision,
requested actions, and only the package presence/enabled/system state needed for
recovery; it excludes the raw serial, APK paths, UIDs, and installer metadata.

Use **Inspect recovery baseline** in Apps after a host reinstall or OTA update.
Import is read-only: Droidsmith shows identity/build/user compatibility, packages
already matching, and every skipped mismatch before enabling the separate apply
button. Only reviewed enable/disable recovery plans use the portable baseline;
eligible retained-system-app recovery is deliberately limited to the same-device
Activity journal. User-installed and unverified historical removals are never
presented as safely undoable.

The headless CLI exposes the same schema and diff engine:

```bash
droidsmith-cli baseline-export profile.yaml --device SERIAL --output baseline.json
droidsmith-cli baseline-inspect baseline.json --device SERIAL
droidsmith-cli baseline-inspect baseline.json --device SERIAL --json
```

Legacy or unknown TCP transports require the explicit
`--allow-unsafe-transport` flag for these CLI commands; USB and paired TLS Wi-Fi
do not.

## Repository layout

```
Droidsmith/
  src-tauri/        Rust backend, Tauri commands, ADB domain, CLI binary
  src/              React + TS frontend
  packs/            Community debloat packs (YAML)
  quirks/           Vendor failure explanations and mitigations (YAML)
  scripts/          Local development, resource, and sidecar helpers
  docs/             Development notes and screenshots
  ROADMAP.md
  Roadmap_Blocked.md
  CHANGELOG.md
  RESEARCH_REPORT.md
```

## Project planning

- [ROADMAP.md](ROADMAP.md) - active and planned roadmap items.
- [Roadmap_Blocked.md](Roadmap_Blocked.md) - work paused on external blockers.
- [CHANGELOG.md](CHANGELOG.md) - shipped roadmap history and release notes.
- [RESEARCH_REPORT.md](RESEARCH_REPORT.md) - research summary and archive index.

## Local verification

```bash
npm run release:check
```

`npm run release:check` is the authoritative local release-policy gate. It
fails on frontend or Rust formatting/lint/type/test regressions, rendered-route
smoke failures, npm/Rust advisories, unreviewed Cargo licenses/sources/bans or
duplicate versions, invalid pack/quirk/profile YAML, version/resource drift,
and missing production bundle artifacts. Install its Rust tools once with
`cargo install --locked cargo-audit cargo-deny`; every temporary exception in
`release-policy.json` names an owner, rationale, and absolute expiry date.

Platform Tools compatibility is governed by
[`platform-tools-policy.json`](platform-tools-policy.json). The policy was
reviewed on 2026-07-15, recommends 37.0.0, and warns (without blocking) below
36.0.2 except for explicitly listed known-bad releases. Unrecognized newer
versions are never blocked. Both fetch scripts consume the same version,
official archive URLs, and SHA-256 pins; the release gate rejects policy,
runtime, script, or documentation drift.

Rust commands and DTOs generate `src/lib/bindings.ts` through Tauri Specta.
After changing an IPC signature, run `npm run bindings:generate`; the
authoritative gate runs `npm run bindings:check` and rejects stale generated
output. Keep renderer-only compatibility helpers in `src/lib/tauri.ts` instead
of duplicating Rust wire records.

The individual commands (`npm run format:check`, `npm run lint`,
`npm run typecheck`, `npm test`, `npm run security:audit`, `npm run ui:smoke`,
and `npm run release:smoke`) remain available for fast iteration.

Seeded optional fuzz targets for ADB/OEM text, YAML documents, and journal
JSONL live under `src-tauri/fuzz`. On a supported Unix-like host with nightly
Rust and `cargo-fuzz` installed, run them from `src-tauri` with
`cargo fuzz run adb_text` (or `yaml_documents` / `journal_jsonl`); normal builds
do not compile fuzz tooling.

`npm run ui:smoke` starts Vite with mocked Tauri IPC and checks sidebar
navigation and route focus, command-palette combobox/listbox semantics, modal
focus trapping/restoration, native table semantics, batched Logcat announcements,
document locale propagation, Apps action overlays, Debloat queue results,
ADB health/recovery review, the redacted Diagnostics preview/save/wipe flow,
the split-package install and explicit override-confirmation flow, cross-route
disconnect/reconnect behavior, incremental Logcat reconnect/cancel behavior,
unsafe-transport acknowledgement/reset behavior, and mobile/narrow overflow.
It also sweeps every route under a non-English locale at a 200%-zoom reflow,
drives the Apps loading/empty/error/stale-completion states, and asserts the
documented screenshots never show the desktop-required placeholder.
`npm run docs:screenshots` regenerates the committed README screenshots from that
same mocked-native state.
`npm run release:smoke` builds the frontend and Tauri bundle, checks bundled
resource metadata, validates third-party notices, and fails if expected local
installer artifacts are missing.

## Translation contributions

Locale files live in `src/locales/<code>.json`. Keep each locale's key tree
identical, add new supported language codes in `src/lib/i18n.ts`, and include
language selector labels plus locale/direction metadata under `language.*` and
`SUPPORTED_LANGUAGES`. Dates, numbers, document language, and document direction
derive from that metadata. Run `npm test -- src/lib/i18n.test.ts` before
submitting translation changes; it checks English/Russian parity, navigation
key coverage, and locale-sensitive formatting.

## Getting involved

Use [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for local setup and verification.
Before proposing a new feature, check [ROADMAP.md](ROADMAP.md) and
[Roadmap_Blocked.md](Roadmap_Blocked.md) so blocked signing, sidecar,
redistribution, and plugin work does not get duplicated.

## License

MIT — see [LICENSE](LICENSE).
