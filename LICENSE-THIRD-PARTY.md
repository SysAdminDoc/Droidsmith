# Third-party licenses

Droidsmith is MIT-licensed. We bundle, vendor, or fetch third-party software
and data sets that carry their own licenses; this file is the running notice.

It is updated whenever a bundled binary, vendored data file, or runtime
dependency that requires attribution lands in the repo.

## Bundled binaries (planned — lands with R-010 / R-040)

### Android Platform-Tools (`adb`, `fastboot`)

- **Source:** [Google — Android Platform-Tools](https://developer.android.com/tools/releases/platform-tools)
- **License:** Apache License 2.0 (binary distribution permitted under that license)
- **Use:** Bundled as Tauri sidecars per [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) `bundle.externalBin`
- **Status:** Not bundled yet. Fetch script lands with R-010.

### scrcpy

- **Source:** [Genymobile — scrcpy](https://github.com/Genymobile/scrcpy)
- **License:** Apache License 2.0
- **Use:** External mirror engine; bundled per OS
- **Status:** Not bundled yet. Lands with R-040.

## Vendored data sets

### Universal Android Debloater Next Generation — `uad_lists.json`

- **Source:** [UAD-NG repository](https://github.com/Universal-Debloater-Alliance/universal-android-debloater-next-generation)
- **License:** GPL-3.0 (project code) — data file separately treated as
  reference data; we pin to a specific revision and attribute clearly
- **Use:** Optional supplementary pack (`packs/_uad-supplement.yaml`) generated
  from a pinned upstream revision
- **Status:** Not vendored yet. Lands with R-036. See [RESEARCH_DEEPDIVE.md
  Open Questions §3](RESEARCH_DEEPDIVE.md#open-questions) — we will ask the
  UAD-NG maintainers before shipping this.

## Rust crate dependencies

Rust crates pulled in via `Cargo.toml` carry their own licenses (mostly
MIT/Apache-2.0 dual-licensed). Full list is captured in the CycloneDX SBOM
generated per release (R-006). No attribution rendering inside the app is
required for those, but the SBOM is the canonical inventory.

## Frontend dependencies

Same arrangement: `package.json` deps, lockfile, and a per-release `cyclonedx-bom`
output.

## Icons & assets

- App icon glyph: original work, MIT, generated via
  [`src-tauri/app-icon.png`](src-tauri/app-icon.png) using the standard
  `cargo tauri icon` pipeline.
- Inter / JetBrains Mono fonts (if bundled later): SIL Open Font License —
  attribution will land here once R-006 ships the bundled font fallback.
