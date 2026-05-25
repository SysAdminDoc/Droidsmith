# Droidsmith

A cross-platform, open-source workshop for Android devices over ADB.

Droidsmith is the spiritual successor to [ADB AppControl](https://adbappcontrol.com) — a
modern, cross-platform GUI for managing Android devices through ADB, without
root, without a closed-source binary, without paywalled features.

## Status

Early scaffolding. The repository currently contains the research, roadmap, and
project ground rules. Code lands as the [ROADMAP](ROADMAP.md) items ship.

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
[RESEARCH_FEATURE_PLAN.md](RESEARCH_FEATURE_PLAN.md) for the rationale and
the alternatives considered.

## Repository layout (planned)

```
Droidsmith/
  src-tauri/        Rust backend — ADB client, scrcpy supervisor, plugins
  src/              React + TS frontend
  packs/            Community debloat packs (YAML, one file per OEM/ROM)
  cli/              Headless CLI (`droidsmith run profile.yaml`)
  docs/             User & contributor docs
  ROADMAP.md
  RESEARCH_FEATURE_PLAN.md
```

## Getting involved

Once R-002 lands the repo will have a working dev shell. Until then the
roadmap and research plan are the contribution surface — issues and PRs that
refine those documents are welcome.

## License

MIT — see [LICENSE](LICENSE).
