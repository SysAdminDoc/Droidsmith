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

### P3

- [ ] P3 — Persist gnirehtet reverse-tethering across navigation
  Why: the "Share Internet" toggle now stops its supervised session when the
  selected device changes or the Devices route unmounts, so tethering does not
  survive navigating away (e.g. to install something that needs the shared
  connection). Safe default, but persistence would be friendlier.
  Where: `src/routes/devices/InternetSharing.tsx` (drop the cleanup stop),
  `src-tauri/src/gnirehtet.rs` + `src-tauri/src/commands.rs` (add a
  list-sessions-by-serial command so a remount can re-attach to a running
  session instead of showing "start" and spawning a duplicate).
