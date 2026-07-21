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

- [ ] P3 — R-089 scrcpy version-gated capability surface (virtual display, camera)
  Why: net-new scrcpy capabilities that require version probing. The
  validation-heavy flags (crop/orientation/screen-off-timeout/audio-codec) and
  the window/control toggles already shipped (R-088/R-089); VP8/VP9 fallback is
  covered by the existing codec selector gated on `available_video_codecs`.
  Evidence: scrcpy v3.0 `--new-display=<res>/<dpi>`, v3.2 audio-source
  expansion (`--audio-source=mic-*`), v4.0 camera torch/zoom.
  Touches: `src-tauri/src/scrcpy.rs` (probe a `supports_new_display` /
  `supports_camera` capability from the version + `--help`, build the args),
  `src/routes/Mirror.tsx` + `src/routes/mirrorPresets.ts` (virtual-display size/
  dpi inputs, camera mirroring, audio-source picker), hidden on older scrcpy.
  Acceptance: capabilities gate on detected scrcpy version; args asserted in
  unit tests; unsupported controls hidden on older scrcpy.
  Complexity: M

- [ ] P3 — R-095 Import remote debloat pack by URL with SHA-256 pin
  Why: static bundled YAML can't gain OEM/package coverage without an app
  release; a generic pinned-URL import gives UAD-ng-style freshness without the
  blocked R-036 redistribution dependency.
  Evidence: UAD-ng ships a remote-updatable community list; Droidsmith packs are
  static (`packs/*.yaml`).
  Touches: `src-tauri/src/packs/` (fetch + SHA-256 verify + schema-validate a
  remote pack), `src/routes/Debloat.tsx` (add-remote-pack UX).
  Acceptance: a user can add a pack from a URL that is SHA-256-pinned and
  schema-validated before use; malformed/unpinned sources are rejected; unit
  test on verify + validate.
  Complexity: M

