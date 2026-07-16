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
