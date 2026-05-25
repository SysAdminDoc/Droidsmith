<!--
Thanks for the contribution! A few quick checks before review:

- Title is conventional (feat:/fix:/docs:/chore:/test:/refactor:/perf:)
- ROADMAP.md item referenced if applicable (R-NNN / IMP-NN)
- CI is green (Rust matrix + frontend)
- For destructive ADB operations: the change includes a confirmation flow
- For new bundled binaries / vendored data: LICENSE-THIRD-PARTY.md updated
- For UI changes: screenshots or a recording attached
-->

## Summary

<!-- One paragraph. What changes, why. -->

## Roadmap items

<!-- Closes R-NNN / IMP-NN, or "n/a" if this is groundwork. -->

## Verification

- [ ] `cargo check --all-targets` clean
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo test --all-targets` passes
- [ ] `npm run typecheck` clean
- [ ] `npm run lint` clean
- [ ] `npm run test` passes
- [ ] Manual verification noted below

## Manual verification

<!-- Step-by-step what you did. "Plugged in a Pixel 8, opened Apps tab, ..." -->
