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

- [ ] P3 — Add a light theme toggle
  Why: Surfaces are dark-only Tailwind; a light option is expected desktop polish and an accessibility preference for bright environments.
  Evidence: no theme state in `src/App.tsx` or `src/lib/`; dark-first `tailwind.config.ts`
  Touches: `tailwind.config.ts`, `src/App.tsx`, route components, a persisted `droidsmith.theme` key, locales
  Acceptance: A persisted theme toggle switches dark/light (default dark) across every route without contrast regressions; the choice survives restart; ui:smoke covers both themes.
  Complexity: M

## Audit-Deferred Items

## Research-Driven Additions

### P0

### P1

### P2

### P3

## Research-Driven Additions

### P1

### P2

### P3

- [ ] P3 — **IMP-60** Add package-name Logcat filtering via PID→package resolution
  Why: IMP-59 shipped structured Logcat query presets over tag/message/PID/level/age with negation and a linear-time-safe regex subset, but package/process-*name* filtering was deferred: the stream now uses `-v threadtime` (timestamp + PID) which still carries no package or process name, and Android Studio resolves those from a live PID→package map the app does not yet build.
  Evidence: `src-tauri/src/commands.rs::stream_logcat` (threadtime, PID only); `src/routes/logcatQueries.ts` (pidFilter); Android Studio Logcat `package:`/`process:` semantics.
  Touches: a periodic `ps`/`pm` PID→package snapshot in the backend, `LogcatQuery` package/process fields, `src/routes/logcatQueries.ts` matching, `src/routes/Logcat.tsx`, locales, tests.
  Acceptance: A query can filter (and negate) by package or process name; the PID→package map refreshes on a bounded cadence without blocking the stream; lines whose PID is unmapped are surfaced rather than silently dropped; presets round-trip the new fields.
  Complexity: M
