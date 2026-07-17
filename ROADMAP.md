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

### P0

### P1

### P2

### P3

## Audit-Deferred Items

Design-consistency and minor-UX items surfaced by the 2026-07-17 audit pass
that are larger refactors or low-severity; the correctness/security/a11y
findings from that pass shipped in v0.5.2.

- [ ] P2 — Extract shared `Select`/`TextArea` primitives
  Why: many routes hand-roll `<select>`/`<textarea>` with slightly different
  height/border/background/hover, so the same control looks different per route.
  Where: `src/routes/Apps.tsx`, `Debloat.tsx`, `Logcat.tsx`, `Mirror.tsx`,
  `Devices.tsx`, `Profiles.tsx`, `SettingsDataControls.tsx`, `App.tsx` — unify
  against `common.tsx` alongside the existing `Button`/`FieldInput`.
- [ ] P3 — Reuse `TransportBadge`/`Badge` for the device-table transport chip
  Why: `TransportChip` hand-rolls a colored chip that duplicates `TransportBadge`,
  producing two visually different transport indicators.
  Where: `src/routes/Devices.tsx` (`TransportChip`), `src/routes/common.tsx`.
- [ ] P3 — Fold the permission grant/deny toggle into a shared control
  Why: one-off bordered green/red button diverging from the design system.
  Where: `src/routes/Apps.tsx` (permission toggle).
- [ ] P3 — Replace raw `rgba()` box-shadow literals with theme shadow tokens
  Why: two arbitrary `shadow-[…rgba…]` utilities bypass `shadow-panel`/`glow`.
  Where: `src/routes/Wireless.tsx`, `src/routes/Apps.tsx`.
- [ ] P3 — Converge per-route content max-width scale
  Why: content width jumps between tabs (`max-w-5xl`/`6xl`/`7xl`/`[88rem]`).
  Where: all `src/routes/*.tsx` pane wrappers.
- [ ] P3 — Surface that logcat/console command history recall skips failed entries
  Why: recall silently excludes failed commands, confusing when re-running to fix one.
  Where: `src/routes/Console.tsx` history recall.
- [ ] P3 — Give feedback when a native save dialog is cancelled in Diagnostics
  Why: cancelling the support-bundle save leaves no status; other flows reset visibly.
  Where: `src/routes/DiagnosticsCenter.tsx` `saveBundle`.

## Research-Driven Additions

### P0

### P1

### P2

### P3

## Research-Driven Additions

### P1

### P2

### P3
