# Droidsmith

A cross-platform, open-source workshop for Android devices over ADB.

Droidsmith is the spiritual successor to [ADB AppControl](https://adbappcontrol.com) — a
modern, cross-platform GUI for managing Android devices through ADB, without
root, without a closed-source binary, without paywalled features.

## Status

Pre-functional but live. Tauri shell builds and runs; the in-app Heartbeat
panel reports OS, Tauri, Rust, and ADB resolution. Feature surface lands per
[ROADMAP.md](ROADMAP.md); shipped work is logged in [CHANGELOG.md](CHANGELOG.md);
design rationale is summarized in [RESEARCH_REPORT.md](RESEARCH_REPORT.md).

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
| Debloat lists | Static, underperforms Universal Android Debloater per user reports | Community-driven, per-OEM (HyperOS, MIUI, OneUI, ColorOS, OxygenOS, Pixel, Fire OS) |
| Screen mirror | Virtual buttons + screenshots | scrcpy integration (mirror + control + audio) |
| Wireless ADB | Manual `adb pair` in console | First-class pairing UI (Android 11+) |
| Automation | None | YAML profiles + headless CLI for CI / reproducible flashes |
| Extensibility | None | Plugin system for OEM modules and custom debloat sets |
| i18n | EN + RU | i18next-driven, contributor-friendly |
| Multi-device | One at a time | Side-by-side device tabs |

## Planned tech stack

- **Tauri 2** — Rust core + native webview, single-binary distribution (~10 MB
  vs Electron's ~100 MB)
- **React + TypeScript + Vite** — frontend
- **`adb_client`** Rust crate for direct ADB-protocol talk, with a bundled
  `platform-tools` `adb` binary as fallback for full compatibility
- **scrcpy** auto-downloaded or system-detected for mirror/control
- **Tailwind + shadcn/ui** — modern, dark-first design system
- **i18next** — translations

The stack is locked in once R-002 (scaffold) lands. See
[RESEARCH_REPORT.md](RESEARCH_REPORT.md) for the rationale and the alternatives
considered.

## Repository layout (planned)

```
Droidsmith/
  src-tauri/        Rust backend — ADB client, scrcpy supervisor, plugins
  src/              React + TS frontend
  packs/            Community debloat packs (YAML, one file per OEM/ROM)
  cli/              Headless CLI (`droidsmith run profile.yaml`)
  docs/             User & contributor docs
  ROADMAP.md
  COMPLETED.md
  RESEARCH_REPORT.md
```

## Project planning

- [ROADMAP.md](ROADMAP.md) - active and planned roadmap items.
- [COMPLETED.md](COMPLETED.md) - shipped roadmap history.
- [RESEARCH_REPORT.md](RESEARCH_REPORT.md) - research summary and archive index.
- [CHANGELOG.md](CHANGELOG.md) - release-level details.

## Getting involved

Once R-002 lands the repo will have a working dev shell. Until then the
roadmap and research plan are the contribution surface — issues and PRs that
refine those documents are welcome.

## License

MIT — see [LICENSE](LICENSE).
