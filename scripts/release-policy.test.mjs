import assert from "node:assert/strict";
import test from "node:test";

import {
  collectCargoDuplicates,
  readCargoDependencyRequirement,
  validateAutomationFiles,
  validateAccessibilityAuditPolicy,
  validateDependencyFloor,
  validateNpmDependencyFloor,
  validatePackagingInstallerHashes,
  validateRendererBundleManifest,
  validateScrcpyPolicyDocument,
  validateExpiry,
  validatePlatformToolsDocumentation,
  validateTrackedDocumentation,
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

test("accessibility audit policy keeps WCAG rules and reviewed exclusions explicit", () => {
  const policy = {
    tags: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"],
    contrast: {
      normalTextMinimum: 4.5,
      largeTextMinimum: 3,
      interactiveMinimum: 3,
      largeTextMinPx: 24,
      boldTextMinPx: 18.66,
      boldWeightMinimum: 700,
    },
    excludedRules: [
      {
        id: "color-contrast",
        rationale:
          "Computed contrast is enforced by the dedicated compositing-aware IMP-105 rendered-style gate.",
      },
    ],
  };
  assert.doesNotThrow(() => validateAccessibilityAuditPolicy(policy));
  assert.throws(
    () =>
      validateAccessibilityAuditPolicy({
        ...policy,
        excludedRules: [],
      }),
    /only the compositing-aware contrast rule/u,
  );
  assert.throws(
    () =>
      validateAccessibilityAuditPolicy({
        ...policy,
        tags: ["wcag2a"],
      }),
    /reviewed WCAG A\/AA rule set/u,
  );
  assert.throws(
    () =>
      validateAccessibilityAuditPolicy({
        ...policy,
        contrast: { ...policy.contrast, normalTextMinimum: 3 },
      }),
    /reviewed WCAG AA thresholds/u,
  );
});

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

test("packaging hashes reject placeholders and malformed values", () => {
  const winget = "InstallerSha256: " + "0".repeat(64);
  const scoop = JSON.stringify({
    architecture: { "64bit": { hash: "0".repeat(64) } },
  });
  assert.throws(
    () => validatePackagingInstallerHashes(winget, scoop),
    /placeholder SHA-256/u,
  );
  assert.throws(
    () =>
      validatePackagingInstallerHashes(
        "InstallerSha256: not-a-hash",
        JSON.stringify({ architecture: { "64bit": { hash: "f".repeat(64) } } }),
      ),
    /64-character hexadecimal/u,
  );
  assert.doesNotThrow(() =>
    validatePackagingInstallerHashes(
      "InstallerSha256: " + "a".repeat(64),
      JSON.stringify({ architecture: { "64bit": { hash: "b".repeat(64) } } }),
    ),
  );
});

const documentationExpectations = {
  appVersion: "0.9.12",
  nodeRange: "^22.12.0 || >=24.0.0",
  rustRange: ">=1.81",
  tauriMajor: "2.x",
  platformToolsRecommended: "37.0.0",
  platformToolsWarningBelow: "36.0.2",
  packSchema: "1",
  quirkSchema: "1",
  profileSchema: "2",
};

const documentationFixture = {
  "README.md": `
[asset](docs/screenshot.png)
| Droidsmith source/manifests | \`0.9.12\` |
| Node.js | \`^22.12.0 \\|\\| >=24.0.0\` |
| Rust | \`>=1.81\` |
| Tauri | \`2.x\` |
| Android SDK Platform Tools | \`37.0.0\` recommended; warn below \`36.0.2\` |
| Pack / quirk documents | schema \`"1"\` / \`"1"\` |
| Profile documents | schema \`"2"\`; v1 has a reviewed import migration |
Release artifacts are unsigned and Droidsmith does not check for or install application updates.
`,
  ".github/ISSUE_TEMPLATE/bug_report.yml": `
name: Bug report
description: Report a bug
body:
  Affected workflow
  Reproduction steps
  Redacted diagnostics
`,
  ".github/ISSUE_TEMPLATE/feature_request.yml": `
name: Feature request
description: Request a feature
body:
  Problem
  Desired outcome
  Project fit
`,
  ".github/ISSUE_TEMPLATE/config.yml": `
blank_issues_enabled: false
url: https://github.com/SysAdminDoc/Droidsmith/security/advisories/new
`,
};

test("tracked documentation enforces truth, live links, and version rows", () => {
  const existingPaths = new Set(["docs/screenshot.png"]);
  const validate = (documents) =>
    validateTrackedDocumentation(
      documents,
      documentationExpectations,
      (relativePath) => existingPaths.has(relativePath),
    );
  assert.doesNotThrow(() => validate(documentationFixture));

  const readmeWith = (addition) => ({
    ...documentationFixture,
    "README.md": `${documentationFixture["README.md"]}\n${addition}`,
  });
  for (const claim of [
    "See https://example.com/releases.",
    "Downloads are signed releases.",
    "The built-in updates are automatic.",
    "We integrate adb_client.",
    "See RESEARCH_REPORT.md.",
    "These live route surfaces are complete.",
  ]) {
    assert.throws(
      () => validate(readmeWith(claim)),
      /placeholder domain|unsupported signing or updater|obsolete/u,
      claim,
    );
  }
  assert.throws(
    () =>
      validate({
        ...documentationFixture,
        "README.md": documentationFixture["README.md"].replace(
          "docs/screenshot.png",
          "docs/missing.png",
        ),
      }),
    /dead local link/u,
  );

  for (const currentValue of [
    "0.9.12",
    String.raw`^22.12.0 \|\| >=24.0.0`,
    ">=1.81",
    "2.x",
    "37.0.0",
    "36.0.2",
  ]) {
    assert.throws(
      () =>
        validate({
          ...documentationFixture,
          "README.md": documentationFixture["README.md"].replace(
            currentValue,
            "stale",
          ),
        }),
      /supported-version table differs/u,
      `${currentValue} drift must fail`,
    );
  }
  for (const currentRow of [
    '| Pack / quirk documents | schema `"1"` / `"1"` |',
    '| Profile documents | schema `"2"`; v1 has a reviewed import migration |',
  ]) {
    assert.throws(
      () =>
        validate({
          ...documentationFixture,
          "README.md": documentationFixture["README.md"].replace(
            currentRow,
            currentRow.replace("schema", "stale schema"),
          ),
        }),
      /supported-version table differs/u,
      `${currentRow} drift must fail`,
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

const tauriFloor = {
  crate: "tauri",
  minimumVersion: "2.11.1",
  cve: "CVE-2026-42184",
};

test("dependency security floor rejects requirements below the patched release", () => {
  // The pre-fix declaration. A caret "2" resolves anything in the 2.x line,
  // including the versions vulnerable to the app:// origin-confusion bug.
  assert.throws(
    () => validateDependencyFloor(tauriFloor, "2"),
    /CVE-2026-42184/,
  );
  assert.throws(
    () => validateDependencyFloor(tauriFloor, "2.11"),
    /below the 2.11.1/,
  );
  assert.throws(
    () => validateDependencyFloor(tauriFloor, "2.11.0"),
    /below the 2.11.1/,
  );
  assert.throws(
    () => validateDependencyFloor(tauriFloor, "2.10.9"),
    /below the 2.11.1/,
  );
  assert.throws(
    () => validateDependencyFloor(tauriFloor, "1.9.9"),
    /below the 2.11.1/,
  );
});

test("dependency security floor accepts the patched release and newer", () => {
  for (const declared of ["2.11.1", "2.11.2", "2.12.0", "3.0.0"]) {
    assert.doesNotThrow(() => validateDependencyFloor(tauriFloor, declared));
  }
});

test("dependency security floor rejects non-caret requirements it cannot reason about", () => {
  for (const declared of ["=2.11.1", ">=2.11.1", "2.11.1-beta.1", "*"]) {
    assert.throws(
      () => validateDependencyFloor(tauriFloor, declared),
      /plain caret floor/,
    );
  }
});

test("cargo dependency requirements are read from both inline and table form", () => {
  const manifest = [
    "[dependencies]",
    'serde_json = "1"',
    'tauri = { version = "2.11.1", features = ["isolation"] }',
    "",
    "[build-dependencies]",
    'tauri-build = { version = "2.6", features = ["isolation"] }',
  ].join("\n");
  assert.equal(readCargoDependencyRequirement(manifest, "tauri"), "2.11.1");
  assert.equal(readCargoDependencyRequirement(manifest, "tauri-build"), "2.6");
  assert.equal(readCargoDependencyRequirement(manifest, "serde_json"), "1");
  assert.equal(readCargoDependencyRequirement(manifest, "absent"), undefined);
});

test("npm dependency floors reject each vulnerable resolved version", () => {
  const braceFloor = {
    package: "brace-expansion",
    minimumVersion: "5.0.9",
    advisory: "GHSA-rgw5-rvv9-x895",
  };
  const nanoidFloor = {
    package: "nanoid",
    minimumVersion: "3.3.17",
    advisory: "GHSA-2v37-7h3g-55p8",
  };
  assert.doesNotThrow(() => validateNpmDependencyFloor(braceFloor, "5.0.9"));
  assert.doesNotThrow(() => validateNpmDependencyFloor(nanoidFloor, "3.3.18"));
  assert.throws(
    () => validateNpmDependencyFloor(braceFloor, "5.0.8"),
    /below the 5\.0\.9 security floor/u,
  );
  assert.throws(
    () => validateNpmDependencyFloor(nanoidFloor, "3.3.16"),
    /below the 3\.3\.17 security floor/u,
  );
});

const scrcpyPolicy = {
  schemaVersion: 1,
  reviewedOn: "2026-07-31",
  securityFloorVersion: "3.3.4",
  sourceUrl: "https://nvd.nist.gov/vuln/detail/CVE-2025-34449",
  rationale:
    "scrcpy 3.3.4 fixes a device-to-host buffer overflow that no dependency scanner surfaces.",
  advisories: [
    {
      id: "CVE-2025-34449",
      belowVersion: "3.3.4",
      summary:
        "Global buffer overflow in sc_device_msg_deserialize reachable from device messages.",
      sourceUrl: "https://nvd.nist.gov/vuln/detail/CVE-2025-34449",
    },
  ],
};
const scrcpyRust = `const FLOOR: &str = "3.3.4"; // CVE-2025-34449`;

test("scrcpy policy accepts a coherent document", () => {
  assert.doesNotThrow(() =>
    validateScrcpyPolicyDocument(scrcpyPolicy, scrcpyRust),
  );
});

test("scrcpy policy rejects a floor that drifts from its advisories", () => {
  assert.throws(
    () =>
      validateScrcpyPolicyDocument(
        { ...scrcpyPolicy, securityFloorVersion: "3.3.3" },
        scrcpyRust,
      ),
    /highest advisory floor/u,
  );
});

test("scrcpy policy rejects drift between the document and the Rust module", () => {
  assert.throws(
    () =>
      validateScrcpyPolicyDocument(
        scrcpyPolicy,
        `const FLOOR: &str = "3.3.4";`,
      ),
    /no longer references CVE-2025-34449/u,
  );
  assert.throws(
    () =>
      validateScrcpyPolicyDocument(
        scrcpyPolicy,
        `// CVE-2025-34449 without the floor`,
      ),
    /no longer references the policy security floor/u,
  );
});

test("scrcpy policy requires sourced, identified advisories", () => {
  for (const advisory of [
    { ...scrcpyPolicy.advisories[0], id: "bug-123" },
    { ...scrcpyPolicy.advisories[0], sourceUrl: "http://insecure.example" },
    { ...scrcpyPolicy.advisories[0], summary: "too short" },
  ]) {
    assert.throws(() =>
      validateScrcpyPolicyDocument(
        { ...scrcpyPolicy, advisories: [advisory] },
        scrcpyRust,
      ),
    );
  }
});
