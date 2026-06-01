# Droidsmith Completed Work

This file summarizes shipped roadmap history. Active work lives in `ROADMAP.md`;
release-level details live in `CHANGELOG.md`.

## v0.1.0

- First tagged release with pre-built Windows installer and portable `.exe`.
- Device detection through action queue and undo journal.
- ADB domain layer with typed transport, device parsing, and live Devices route.
- Package enumeration with user/system/enabled/disabled filters.
- Action planning/application for disable, enable, uninstall-for-user, clear
  data, and force-stop.
- Per-device JSONL undo journal with corrupt-line tolerance.
- Pack framework, pack lint binary, example pack, and UAD-NG-aligned schema.
- Vendor quirks engine with HyperOS seed rule and `explain_failure` command.
- Headless CLI and YAML profile schema.
- Shared UTC timestamp module.
- Platform-tools sidecar fetch scripts.
- Per-pane placeholder routes and typed Tauri invoke wrappers.

## Phase 0 Foundation

- Tauri 2, React, TypeScript, Vite, and Tailwind scaffold.
- GitHub Actions CI matrix.
- Rust/frontend lint gates.
- Contributor, code-of-conduct, security, issue-template, and PR-template
  surface.
- Startup panic replaced by native error dialog and rotating file-only crash
  logs.
- Heartbeat diagnostics and ADB resolution improvements.
- POSIX/PowerShell dev-mirror scripts.
- Third-party license placeholder inventory.
