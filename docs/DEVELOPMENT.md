# Development

## Prerequisites

- **Rust** stable (1.77+) — `rustup install stable`
- **Node** 20+ — install from nodejs.org or via `fnm`/`nvm`
- **Tauri 2 prerequisites** for your OS:
  - Windows: WebView2 (ships with Win 11), MSVC build tools
  - macOS: Xcode CLT (`xcode-select --install`)
  - Linux: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`
- **adb** somewhere on PATH (or installed at the platform-default location —
  `~/AppData/Local/Android/Sdk/platform-tools/adb.exe` on Windows,
  `~/Library/Android/sdk/platform-tools/adb` on macOS, etc.). The app
  detects it automatically; bundling lands with R-010.

## First-time setup

```bash
npm install
```

That's it for the JS side. Cargo will fetch crates on first `cargo` or
`tauri dev` invocation.

## Running dev

### Non-HGFS path (most environments)

```bash
npm run tauri:dev
```

### VMware Shared Folders / HGFS path (Z:\ or W:\ on a VM)

Vite chokes on the space in `\\vmware-host\Shared Folders\`, and Cargo pays
an enormous fsync tax on HGFS. Mirror to a local SSD path first:

```powershell
./scripts/dev-mirror.ps1
cd C:\tmp\Droidsmith
npm install        # first time only
npm run tauri:dev
```

The mirror script excludes `node_modules/`, `target/`, `dist/`, `.git/`. Use
`./scripts/dev-mirror.ps1 -Watch` to keep the mirror in sync as you edit on
the HGFS side, so git stays authoritative.

## Running tests

```bash
npm run test          # vitest (frontend)
cargo test --manifest-path src-tauri/Cargo.toml
```

## Type-check + lint

```bash
npm run typecheck
npm run lint
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

## Production build

```bash
npm run tauri:build
```

Output lands in `src-tauri/target/release/bundle/` per OS.

## What's not done yet

The shell is here, the heartbeat IPC works, but every nav item in the
sidebar is a stub. Pick up R-010+ in [ROADMAP.md](../ROADMAP.md) for the
real features.
