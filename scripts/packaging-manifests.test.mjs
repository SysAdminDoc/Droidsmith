// R-115: verify the winget/Scoop manifest generator renders schema-valid
// manifests whose version tracks package.json.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  buildScoopManifest,
  buildWingetManifest,
  readReleaseMeta,
  validateManifests,
  PACKAGE_IDENTIFIER,
} from "./generate-packaging-manifests.mjs";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const pkgVersion = JSON.parse(
  readFileSync(path.join(repoRoot, "package.json"), "utf8"),
).version;

test("readReleaseMeta reports the package.json version", () => {
  assert.equal(readReleaseMeta().version, pkgVersion);
});

test("generated manifests pass schema validation for the current version", () => {
  const meta = readReleaseMeta();
  const winget = buildWingetManifest(meta);
  const scoop = buildScoopManifest(meta);
  assert.deepEqual(validateManifests(winget, scoop, pkgVersion), []);
  assert.equal(winget.PackageIdentifier, PACKAGE_IDENTIFIER);
  assert.equal(scoop.version, pkgVersion);
});

test("validation rejects a version mismatch", () => {
  const meta = readReleaseMeta();
  const problems = validateManifests(
    buildWingetManifest(meta),
    buildScoopManifest(meta),
    "9.9.9",
  );
  assert.ok(
    problems.length >= 2,
    `expected version-mismatch problems, got ${problems}`,
  );
});

test("installer URLs reference the release version", () => {
  const meta = { ...readReleaseMeta(), version: "1.2.3" };
  const winget = buildWingetManifest(meta);
  const scoop = buildScoopManifest(meta);
  assert.ok(winget.Installers[0].InstallerUrl.includes("v1.2.3/"));
  assert.ok(scoop.architecture["64bit"].url.includes("1.2.3"));
});
