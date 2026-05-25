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

The mirror script excludes `node_modules/`, `target/`, `.git/`. Use
`./scripts/dev-mirror.ps1 -Watch` to keep the mirror in sync as you edit on
the HGFS side, so git stays authoritative.

### WSL2 / Linux on /mnt/c / macOS on a slow share

A POSIX companion to the PowerShell mirror ships in
[`scripts/dev-mirror.sh`](../scripts/dev-mirror.sh) — same semantics
(rsync with `--delete`, sentinel guard against accidental wipes,
optional `--watch` mode). Default destination is
`~/.droidsmith-mirror`:

```bash
./scripts/dev-mirror.sh            # one-shot mirror
./scripts/dev-mirror.sh --watch    # keep in sync (1-second poll)
./scripts/dev-mirror.sh --reverse  # copy build artefacts back to the repo
cd ~/.droidsmith-mirror
npm install
npm run tauri:dev
```

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

The current production build uses the same resolver as development: it
looks for `adb` on `PATH`, under `ANDROID_HOME` / `ANDROID_SDK_ROOT`, and
in common platform locations. Bundled platform-tools are planned for
R-010, but `bundle.externalBin` is not wired yet.

When R-010 enables bundled platform-tools, binaries must still be fetched
per host because they are not committed:

```powershell
# Windows
.\scripts\fetch-platform-tools.ps1
```

```bash
# macOS / Linux
./scripts/fetch-platform-tools.sh
```

Then:

```bash
npm run tauri:build
```

Output lands in `src-tauri/target/release/bundle/` per OS.

### Sidecars in dev

The dev loop (`npm run tauri:dev`) does not require sidecars. Until
R-010 lands, the resolver never falls back to bundled binaries; install
Android Studio Platform Tools locally or set `ANDROID_HOME` /
`ANDROID_SDK_ROOT`.

## What's not done yet

The shell is here, the heartbeat IPC works, but every nav item in the
sidebar is a stub. Pick up R-010+ in [ROADMAP.md](../ROADMAP.md) for the
real features.
