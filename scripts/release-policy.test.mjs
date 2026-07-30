import assert from "node:assert/strict";
import test from "node:test";

import {
  collectCargoDuplicates,
  validateAutomationFiles,
  validateRendererBundleManifest,
  validateExpiry,
  validatePlatformToolsDocumentation,
  validateVersionValues,
} from "./check-release-policy.mjs";

const rendererRoutes = [
  "Devices",
  "Wireless",
  "Apps",
  "Debloat",
  "Profiles",
  "Mirror",
  "Console",
  "Logcat",
  "Fastboot",
  "DeviceSettings",
  "ApkAnalyzer",
].map((name) => `src/routes/${name}.tsx`);

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
  const versions = {
    "package.json": "0.1.0",
    "package-lock.json": "0.1.0",
    "src-tauri/Cargo.toml": "0.1.0",
    "src-tauri/tauri.conf.json": "0.1.0",
    "README.md badge": "0.1.0",
  };
  assert.doesNotThrow(() => validateVersionValues(versions));

  for (const source of Object.keys(versions)) {
    assert.throws(
      () => validateVersionValues({ ...versions, [source]: "0.2.0" }),
      /release versions differ/u,
      `${source} drift must fail the release gate`,
    );
    assert.throws(
      () => validateVersionValues({ ...versions, [source]: undefined }),
      /release versions differ/u,
      `${source} absence must fail the release gate`,
    );
  }
});

test("tracked automation keeps required blocking jobs and immutable actions", () => {
  const ciWorkflow = `
on:
  push:
  pull_request:
  schedule:
  workflow_dispatch:
permissions:
  contents: read
jobs:
  frontend:
  native:
    os: [ubuntu-latest, windows-latest, macos-latest]
  security:
  release-smoke:
    if: github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262
      - run: npm ci
      - run: npm run ui:smoke
      - run: cargo test --locked
      - run: npm run security:audit
      - run: npm run release:check
`;
  const dependabot = `
version: 2
updates:
  - package-ecosystem: npm
    directory: /
    schedule:
      interval: weekly
    open-pull-requests-limit: 4
    groups:
  - package-ecosystem: cargo
    directory: /src-tauri
    schedule:
      interval: weekly
    open-pull-requests-limit: 4
    groups:
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
    open-pull-requests-limit: 2
    groups:
`;
  assert.doesNotThrow(() => validateAutomationFiles(ciWorkflow, dependabot));

  for (const job of ["frontend", "native", "security", "release-smoke"]) {
    assert.throws(
      () =>
        validateAutomationFiles(
          ciWorkflow.replace(`\n  ${job}:\n`, `\n  removed-${job}:\n`),
          dependabot,
        ),
      /CI workflow is missing required marker/u,
      `${job} must remain a required CI job`,
    );
  }
  assert.throws(
    () =>
      validateAutomationFiles(
        ciWorkflow.replace("11d5960a326750d5838078e36cf38b85af677262", "v4"),
        dependabot,
      ),
    /full commit SHAs/u,
  );
  assert.throws(
    () =>
      validateAutomationFiles(
        ciWorkflow,
        dependabot.replace("package-ecosystem: cargo", "ecosystem: removed"),
      ),
    /Dependabot policy is missing required marker/u,
  );
});

test("renderer manifest keeps every route dynamic and the entry under budget", () => {
  const policy = {
    initialJavaScriptBudgetBytes: 450_000,
    dynamicRouteEntries: rendererRoutes,
  };
  const manifest = {
    "index.html": {
      file: "assets/index.js",
      isEntry: true,
      dynamicImports: rendererRoutes,
    },
    ...Object.fromEntries(
      rendererRoutes.map((route, index) => [
        route,
        { file: `assets/route-${index}.js`, isDynamicEntry: true },
      ]),
    ),
  };
  const assetSizes = {
    "assets/index.js": 449_999,
    ...Object.fromEntries(
      rendererRoutes.map((_, index) => [`assets/route-${index}.js`, 1_024]),
    ),
  };

  assert.deepEqual(
    validateRendererBundleManifest(policy, manifest, assetSizes),
    { entryFile: "assets/index.js", initialBytes: 449_999 },
  );
  assert.throws(
    () =>
      validateRendererBundleManifest(policy, manifest, {
        ...assetSizes,
        "assets/index.js": 450_001,
      }),
    /budget is 450000/u,
  );
  assert.throws(
    () =>
      validateRendererBundleManifest(
        policy,
        {
          ...manifest,
          "index.html": {
            ...manifest["index.html"],
            dynamicImports: rendererRoutes.slice(1),
          },
        },
        assetSizes,
      ),
    /missing dynamic route import/u,
  );
});
