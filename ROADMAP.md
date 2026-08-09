# Droidsmith Roadmap

Single source of truth for what's planned and what's in flight. Completed items
are deleted from here and logged in [CHANGELOG.md](CHANGELOG.md). Blocked items
live in [Roadmap_Blocked.md](Roadmap_Blocked.md). Research context lives in
[RESEARCH.md](RESEARCH.md); do not duplicate that here - link instead.

## Conventions

- `[ ]` - not started
- `[~]` - in flight
- Priority tags: **P0** (must ship in v0.1) . **P1** (v0.1 desirable / v0.2 must) . **P2** (later milestones) . **P3** (cosmetic / nice-to-have)
- **R-NNN** are roadmap items; **IMP-NN** are hardening / improvement items

## Research-Driven Additions

From the 2026-08-08 RESEARCH.md pass. The dominant theme is **claims the codebase
makes that its own gates do not check**, plus upstream drift in platform-tools
and Android, privacy-safe diagnostics, and the widest remaining product gap
(fleet work is CLI-only). IDs continue from R-150 / IMP-129.

### P1

### P2

### P3

- [ ] IMP-128 P3 — Split `Apps.tsx`
  Why: it is the largest file in the frontend at 2,023 lines despite six components already extracted, and the initial bundle sits at 84% of its declared budget.
  Evidence: `src/routes/Apps.tsx` 2,023 lines against `src/routes/apps/` already holding `PackageTable`, `FilterControls`, `InstallPanels`, `JournalPanel`, `PermissionsPanel`, `RecoveryBaselinePanel`; `dist/assets/index-*.js` 380 KB against `release-policy.json` `initialJavaScriptBudgetBytes` 450000; the `commands.rs` split behind `command_registry.rs` is the precedent.
  Touches: `src/routes/Apps.tsx`, `src/routes/apps/`
  Acceptance: `Apps.tsx` becomes orchestration only, with export review, OTA restore/re-apply and the backup panels extracted; behaviour is unchanged as proven by the existing `Apps.test.tsx` and the rendered-route gate; the initial bundle does not grow.
  Complexity: M
