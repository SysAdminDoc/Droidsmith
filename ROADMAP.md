# Droidsmith Roadmap

Actionable work only. Historical and completed roadmap material is archived in CHANGELOG.md; blocked work is kept in Roadmap_Blocked.md.

## Actionable Items

- [ ] IMP-128 P3 — Split `Apps.tsx`
  Why: it is the largest file in the frontend at 2,023 lines despite six components already extracted, and the initial bundle sits at 84% of its declared budget.
  Evidence: `src/routes/Apps.tsx` 2,023 lines against `src/routes/apps/` already holding `PackageTable`, `FilterControls`, `InstallPanels`, `JournalPanel`, `PermissionsPanel`, `RecoveryBaselinePanel`; `dist/assets/index-*.js` 380 KB against `release-policy.json` `initialJavaScriptBudgetBytes` 450000; the `commands.rs` split behind `command_registry.rs` is the precedent.
  Touches: `src/routes/Apps.tsx`, `src/routes/apps/`
  Acceptance: `Apps.tsx` becomes orchestration only, with export review, OTA restore/re-apply and the backup panels extracted; behaviour is unchanged as proven by the existing `Apps.test.tsx` and the rendered-route gate; the initial bundle does not grow.
  Complexity: M
