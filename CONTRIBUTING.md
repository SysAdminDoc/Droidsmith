# Contributing to Droidsmith

Welcome. Droidsmith is a cross-platform open-source GUI for managing Android
devices over ADB. We want PRs, debloat packs, vendor-quirk rules, translations,
and bug reports. This document tells you how to land them.

## Ground rules

1. **MIT, no paywalls, no telemetry-on-by-default.** Every feature ships free.
2. **Integrate, don't re-implement.** We orchestrate `adb_client`, `scrcpy`,
   UAD-NG's curated package list. We do not re-invent any of them.
3. **Reversibility is a feature.** Every destructive action lands in the per-device
   journal so the user can undo months later. New destructive actions must
   plumb through the journal.
4. **Cross-platform from day one.** No Windows-only or macOS-only APIs in
   the Rust core without an explanatory comment.
5. **Don't lie to the user.** When an OEM blocks a `pm` operation, surface
   the exact reason via the quirks engine — never "Operation failed".

## Development setup

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the dev loop, including
the VMware HGFS workaround for repos hosted on `\\vmware-host\Shared Folders\`.

Quick start:

```bash
npm install
npm run tauri:dev
```

## Project layout

```
.
├── src/                # React + TS frontend
├── src-tauri/          # Rust backend
│   ├── src/
│   │   ├── adb.rs            # Binary resolution + version probe
│   │   ├── commands.rs       # Tauri #[command] glue (thin)
│   │   ├── diagnostics.rs    # Panic hook + native error dialog
│   │   └── lib.rs            # Builder, plugin registration
│   ├── capabilities/         # Tauri permission scopes
│   ├── icons/                # App icons (all platforms)
│   └── tauri.conf.json       # App config + bundle settings
├── packs/              # Debloat packs (community contributions) — coming in R-030
├── quirks/             # OEM behaviour rules — coming in R-034
├── scripts/            # Dev/release scripts
├── docs/               # Developer docs
├── ROADMAP.md          # What's planned, what's in flight
├── CHANGELOG.md        # What's shipped
└── RESEARCH_DEEPDIVE.md # Evidence-grounded design rationale
```

## Working with the roadmap

Open [ROADMAP.md](ROADMAP.md) to find an unchecked item. Each line has a
priority tag (P0/P1/P2/P3). Start with P0 items in Phase 0 and Phase 1 unless
the maintainers have explicitly opened a parallel item.

When you pick up a task:

1. Comment on the linked issue (or open one if none exists) saying you're on it.
2. Open a draft PR early so others can see it's in flight.
3. Use the conventional commit prefix matching the R-NNN scope:
   - `feat:` for new features and behaviours
   - `fix:` for bug fixes
   - `docs:` for docs only
   - `chore:` for tooling, dep bumps, CI
   - `refactor:` for code reshuffles with no behaviour change
   - `test:` for test-only changes
   - `perf:` for measurable performance changes

## Commit style

- Conventional commit subject, under 72 characters.
- Body wrapped to 80, explains the **why** more than the what.
- Reference the roadmap item: `Closes R-012` or `Implements IMP-04`.
- **Do not add a `Co-Authored-By: Claude` trailer.** This project keeps a
  single-author appearance in commit history.

## Code style

- Rust: rustfmt defaults; `cargo clippy --all-targets -- -D warnings` must pass.
- TS: ESLint flat config; Prettier defaults; `npm run typecheck` clean.
- Markdown: no specific linter required, but keep lines under ~100 chars.

## Test expectations

Every PR should add tests for new code paths where reasonable. For the
ADB domain layer specifically:

- Backend logic that doesn't talk to a real device must be unit-tested
  against the mock transport (lands with R-011).
- Backend logic that does talk to a real device should ship behind a
  `#[cfg(feature = "live-adb-tests")]` gate so CI doesn't fail without a device.

## Debloat packs (R-030+)

Pack contributions follow `packs/schema.json` (lands with R-030). Until
then, hold off on pack PRs.

Briefly: each pack is a YAML file targeting a specific OEM/ROM/Android version
range with package entries categorized by removal-level safety. We import
UAD-NG's curated data set as a supplementary source; original Droidsmith
packs cover gaps and OEM-specific quirks.

## Quirks rules (R-034+)

Vendor-lock detection rules live in `quirks/*.yaml`. Each rule pairs a
detection signature (regex against `adb` stderr + OEM/ROM match) with a
human-readable explanation and a suggested mitigation. Lands with R-034.

## Translations

Once R-070 lands, translations live in `src/i18n/<locale>.json`. Pull
strings live in `src/i18n/en.json`. PRs that add a new locale should also
add the language to the picker in Settings.

## Security disclosures

See [SECURITY.md](SECURITY.md). Don't open public issues for security
problems.

## Code of conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Short version: be kind, focus
on the work, leave the politics elsewhere.

## License

By contributing you agree your contribution is licensed under MIT, matching
the rest of the project. If you bundle a third-party binary or data set, add
its notice to [LICENSE-THIRD-PARTY.md](LICENSE-THIRD-PARTY.md).
