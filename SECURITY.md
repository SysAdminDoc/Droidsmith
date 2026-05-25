# Security Policy

## Supported versions

Droidsmith is pre-1.0. Only the most recent release on the `master` branch
receives security fixes.

| Version    | Supported          |
| ---------- | ------------------ |
| `master`   | :white_check_mark: |
| `0.0.x`    | :white_check_mark: |
| older      | :x:                |

## Reporting a vulnerability

**Please do not open public issues for security problems.**

Email: `security@droidsmith.invalid` _(placeholder — update before first
public release; the maintainers track this as part of R-006)_.

Use the subject line:

```
[Droidsmith security] <one-line description>
```

What to include:

- Affected version (the heartbeat panel shows it)
- Repro steps; include exact commands if you have them
- Impact (RCE, file read, capability escape, etc.)
- Any proof-of-concept code or recordings
- Whether you'd like credit in the changelog

We aim to acknowledge within 72 hours and ship a fix or mitigation within
30 days. If you don't hear back within 72 hours, escalate via a direct
message to a maintainer on the project's Discord (link in README) — but
please don't disclose the issue publicly until we've had a chance to fix it.

## What's in scope

- The Droidsmith binary itself (Rust core, frontend, capabilities)
- The bundled `adb` / `fastboot` / `scrcpy` sidecars (we report upstream)
- The auto-updater channel and signature verification
- The release artefacts (installers, signatures, SBOM)
- The packs/quirks loader (parser bugs reachable from a malicious pack)

## What's out of scope

- Bugs in the Android device itself
- Upstream `adb` or `scrcpy` bugs that we just pass through — file those
  with the upstream project, link us in the report
- Self-DoS via user-supplied unsigned packs (we'll add signature
  verification for community packs in R-063)

## Hardening commitments

- No telemetry by default (file-only crash log; opt-in upload deferred to R-073)
- Capability-scoped shell access (IMP-02 — see [`src-tauri/capabilities/default.json`](src-tauri/capabilities/default.json))
- SBOM published with each release (R-006)
- Reproducible release builds (R-006 — best-effort, depends on Tauri toolchain)
- Signed installers on Windows + macOS notarization (R-006)
- Ed25519 signature verification on auto-update artefacts (Tauri default)
