import assert from "node:assert/strict";
import test from "node:test";

import {
  collectCargoDuplicates,
  validateExpiry,
  validatePlatformToolsDocumentation,
  validateVersionValues,
} from "./check-release-policy.mjs";

test("exception dates are absolute, valid, and unexpired", () => {
  const now = new Date("2026-07-15T12:00:00Z");
  assert.doesNotThrow(() => validateExpiry("test", "2026-07-15", now));
  assert.throws(
    () => validateExpiry("test", "2026-07-14", now),
    /expired on 2026-07-14/u,
  );
  assert.throws(
    () => validateExpiry("test", "2026-02-31", now),
    /expiry is invalid/u,
  );
  assert.throws(() => validateExpiry("test", "07/15/2026", now), /YYYY-MM-DD/u);
});

test("Platform Tools documentation is generated from policy values", () => {
  const policy = {
    reviewedOn: "2026-07-15",
    recommendedVersion: "37.0.0",
    warningBelowVersion: "36.0.2",
  };
  const matching =
    "reviewed on 2026-07-15, recommends 37.0.0, and warns (without blocking) below\n36.0.2";
  assert.doesNotThrow(() =>
    validatePlatformToolsDocumentation(policy, matching),
  );
  assert.throws(
    () =>
      validatePlatformToolsDocumentation(
        { ...policy, recommendedVersion: "38.0.0" },
        matching,
      ),
    /summary differs/u,
  );
});

test("Cargo lock duplicate inventory retains exact versions", () => {
  const lock = `[[package]]
name = "alpha"
version = "1.0.0"

[[package]]
name = "beta"
version = "2.0.0"

[[package]]
name = "alpha"
version = "2.0.0"
`;
  assert.deepEqual(collectCargoDuplicates(lock), {
    alpha: ["1.0.0", "2.0.0"],
  });
});

test("release versions must all exist and match", () => {
  assert.doesNotThrow(() =>
    validateVersionValues({ package: "0.1.0", cargo: "0.1.0" }),
  );
  assert.throws(
    () => validateVersionValues({ package: "0.1.0", cargo: "0.2.0" }),
    /release versions differ/u,
  );
  assert.throws(
    () => validateVersionValues({ package: "0.1.0", cargo: undefined }),
    /release versions differ/u,
  );
});
