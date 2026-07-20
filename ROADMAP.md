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

- [ ] P3 — R-084: Gnirehtet reverse tethering integration
  Why: Gnirehtet (7,779 stars, same dev as scrcpy) shares PC internet with
  Android via USB. Escrcpy integrates this. Useful when device has no Wi-Fi
  or in restricted networks.
  Evidence: Gnirehtet GitHub; Escrcpy feature list
  Touches: `src-tauri/src/` (new `gnirehtet.rs` module for binary detection
  and session supervision, similar to `scrcpy.rs`), `src/routes/Devices.tsx`
  or new route, locale files
  Acceptance: If Gnirehtet is on PATH, Devices shows a "Share Internet" toggle.
  Clicking it starts/stops the reverse tethering session with status feedback.
  Complexity: M

- [ ] P3 — R-085: RTL language architecture
  Why: Arabic (ar), Hebrew (he), and Persian (fa) are RTL languages. The i18n
  system supports `dir` metadata per locale, but the CSS uses physical
  properties (`margin-left`, `padding-right`) throughout, which break under
  RTL. CSS logical properties (`margin-inline-start`, `padding-inline-end`)
  are the correct foundation.
  Evidence: i18next RTL docs; MDN CSS logical properties
  Touches: all `src/routes/*.tsx` (replace physical with logical CSS
  properties), `tailwind.config.ts` (logical property utilities),
  `src/lib/i18n.ts` (direction propagation)
  Acceptance: Adding an RTL locale (e.g., ar) correctly mirrors the layout.
  No physical margin/padding properties remain in route files.
  Complexity: L
