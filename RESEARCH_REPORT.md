# Droidsmith Research Report

This is the canonical research summary. Full pre-consolidation source documents
are archived at:

- `docs/archive/research/RESEARCH_FEATURE_PLAN.md`
- `docs/archive/research/RESEARCH_DEEPDIVE.md`

## Current Findings

- Droidsmith's strongest position is not another standalone debloater; it is a
  unified open ADB workstation integrating proven pieces such as UAD-NG lists,
  scrcpy, bundled platform-tools, and scriptable profiles.
- A thin end-to-end slice remains the right execution path: device discovery,
  app/package list, single action, reversible journal, then debloat and mirror
  depth.
- Wireless ADB pairing, vendor-lock explanations, per-device undo, and
  automation profiles are the clearest differentiators versus ADB AppControl.
- Release quality depends on sidecar staging, signing/notarization, SBOM,
  platform-tools provenance, and clear telemetry policy.

## Competitor And Ecosystem Notes

- ADB AppControl is the primary closed-source benchmark.
- Universal Android Debloater NG owns list curation and debloat semantics.
- scrcpy owns mirroring/control/audio/recording.
- `adb_client` is useful for direct ADB protocol work, but Android 11+ wireless
  pairing still needs shell-out and mDNS handling.
- ya-webadb/Tango ADB proves browser ADB is viable but does not replace a native
  all-in-one tool for bulk operations.

## Archive Use

- `RESEARCH_FEATURE_PLAN.md` preserves the elevator-pitch research plan,
  design tenets, stack decisions, and initial milestone framing.
- `RESEARCH_DEEPDIVE.md` preserves evidence, source review, security audit,
  dependency notes, and implementation-ready acceptance criteria.
