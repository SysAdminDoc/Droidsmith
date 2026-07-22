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

## Research-Driven Additions

Added 2026-07-20 from the RESEARCH.md pass (v0.9.1). Items are actionable and
verifiable in the current headless harness; device-verification-only ideas
(multi-device broadcast, in-GUI touch mapping, auto-update hosting) are tracked
in Roadmap_Blocked.md, not here — see the Rejected Ideas table in RESEARCH.md.

### P2

### P3

## Research-Driven Additions — 2026-07-21 (v0.9.4)

From the 2026-07-21 RESEARCH.md pass. The prior pass's frontier (wireless
reconnect, drift detection, scrcpy flag surface, bypass-low-target-sdk,
archive/unarchive, explain_failure wiring, ProcessManager force-stop, local pack
import) has all shipped through v0.9.4. These are the verified next-tier gaps;
device-only and already-shipped ideas are in the RESEARCH.md Rejected table.
IDs continue from R-096 / IMP-73.

### P3

## Research-Driven Additions — 2026-07-21 (v0.9.6)

### P1

### P2

- [ ] P2 — Migrate the Tauri bundle identifier away from the `.app` suffix
  Why: Tauri warns that `com.droidsmith.app` conflicts with the macOS bundle
  extension; changing application identity without migration can split app
  data and break installer upgrades.
  Where: `src-tauri/tauri.conf.json`, app-data migration, installer upgrade tests

## Audit Backlog — 2026-07-22 (v0.9.10)

Residual items from the v0.9.10 deep audit; everything else found in that pass
was fixed and shipped.

- [ ] P3 — Stop the Debloat apply queue on route unmount
  Why: navigating away mid-queue leaves runQueue applying the remaining
  disables in the background with no visible progress or cancel control
  (journaled, but invisible until the route is revisited).
  Where: `src/routes/Debloat.tsx` (runQueue lifecycle/cleanup)
- [ ] P3 — Add stale-response guards to loadUsers in Apps and Debloat
  Why: two rapid target switches can interleave `callListUsers` responses;
  narrow now that effects key on the scalar fingerprint, but the last-write
  race is still possible.
  Where: `src/routes/Apps.tsx`, `src/routes/Debloat.tsx`
- [ ] P3 — Localize the dynamic platform-tools version-policy rationale
  Why: `platform_tools_blocked` / `platform_tools_warning` Host Doctor
  summaries embed the Rust-side policy rationale and remain English in all
  locales (their remediation steps are localized).
  Where: `src-tauri/src/host_diagnostics.rs`, `src/routes/HostDoctor.tsx`,
  `src/locales/*.json`
