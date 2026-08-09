import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { argv, env, execPath, platform, stdout } from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const tauriManifest = path.join(repoRoot, "src-tauri", "Cargo.toml");
const policyPath = path.join(repoRoot, "release-policy.json");
const platformToolsPolicyPath = path.join(
  repoRoot,
  "platform-tools-policy.json",
);
const languageContractPath = path.join(repoRoot, "language-contract.json");
const wingetManifestPath = path.join(
  repoRoot,
  "packaging",
  "winget",
  "SysAdminDoc.Droidsmith.yaml",
);
const scoopManifestPath = path.join(
  repoRoot,
  "packaging",
  "scoop",
  "droidsmith.json",
);
const platformToolsArchiveTokens = Object.freeze({
  windows: "win",
  linux: "linux",
  darwin: "darwin",
});

if (path.resolve(argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  main();
}

function main() {
  validatePolicy();
  if (argv.includes("--policy-only")) {
    stdout.write("Release policy metadata OK\n");
    return;
  }

  runNpm("bindings:check");
  runNpm("format:check");
  runNpm("lint");
  runNpm("typecheck");
  runNpm("test");
  runNpm("test:policy");
  runNpm("test:target-lifecycle");
  runNpm("provenance:check");
  runNpm("ui:smoke");
  run("cargo", [
    "fmt",
    "--manifest-path",
    tauriManifest,
    "--all",
    "--",
    "--check",
  ]);
  run("cargo", [
    "clippy",
    "--manifest-path",
    tauriManifest,
    "--all-targets",
    "--all-features",
    "--",
    "-D",
    "warnings",
  ]);
  run("cargo", [
    "test",
    "--manifest-path",
    tauriManifest,
    "--all-targets",
    "--all-features",
  ]);
  runNpm("security:audit");
  run("cargo", [
    "deny",
    "--locked",
    "--manifest-path",
    tauriManifest,
    "--config",
    path.join(repoRoot, "deny.toml"),
    "check",
    "bans",
    "licenses",
    "sources",
  ]);
  runSchemaLint();
  runNpm("bundle:check");
  runNpm("release:smoke");
  validateBuiltRendererBundle(readJson(policyPath).rendererBundle);

  stdout.write("Authoritative release policy gate OK\n");
}

function validatePolicy() {
  const policy = readJson(policyPath);
  assert(
    policy.schemaVersion === 1,
    "release-policy.json schemaVersion must be 1",
  );
  assert(
    Array.isArray(policy.exceptions),
    "release-policy.json exceptions must be an array",
  );
  validateAccessibilityAuditPolicy(policy.accessibilityAudit);
  validateRendererBundlePolicy(policy.rendererBundle);
  validateTrackedDocumentationPolicy(policy.trackedDocumentation);

  const byKind = new Map();
  for (const exception of policy.exceptions) {
    assert(
      exception && typeof exception === "object" && !Array.isArray(exception),
      "release policy exception must be an object",
    );
    assert(
      typeof exception.kind === "string" && exception.kind.length > 0,
      "release policy exception kind is required",
    );
    assert(
      !byKind.has(exception.kind),
      `duplicate release policy exception kind: ${exception.kind}`,
    );
    assert(
      typeof exception.owner === "string" && exception.owner.trim().length >= 3,
      `${exception.kind} exception owner is required`,
    );
    assert(
      typeof exception.rationale === "string" &&
        exception.rationale.trim().length >= 20,
      `${exception.kind} exception rationale is too short`,
    );
    validateExpiry(exception.kind, exception.expiresOn);
    byKind.set(exception.kind, exception);
  }

  validateAuditExceptions(byKind.get("rust_advisories"));
  validateDuplicateExceptions(byKind.get("duplicate_crates"));
  assert(
    byKind.size === 2,
    "release-policy.json contains an unsupported exception kind",
  );
  validateVersionParity();
  validateDependencySecurityFloors(policy.dependencySecurityFloors);
  validateTrackedDocumentationPolicyFiles(policy.trackedDocumentation);
  validatePackagingInstallerHashesOnDisk();
  validatePlatformToolsPolicy();
  validateScrcpyPolicy();
  validateLanguageContract();
  validateSubprocessCaptureContract();
  validateAutomationPolicy();
}

function validatePackagingInstallerHashesOnDisk() {
  validatePackagingInstallerHashes(
    fs.readFileSync(wingetManifestPath, "utf8"),
    fs.readFileSync(scoopManifestPath, "utf8"),
  );
}

export function validatePackagingInstallerHashes(wingetText, scoopText) {
  const wingetHash = /^\s*InstallerSha256:\s*([0-9a-f]{64})\s*$/imu.exec(
    wingetText,
  )?.[1];
  assert(
    wingetHash,
    "winget installer manifest must contain a 64-character hexadecimal SHA-256",
  );
  assert(
    !/^0{64}$/u.test(wingetHash.toLowerCase()),
    "winget installer manifest still contains a placeholder SHA-256",
  );

  let scoop;
  try {
    scoop = JSON.parse(scoopText);
  } catch (error) {
    throw new Error(`Scoop installer manifest is not valid JSON: ${error}`, {
      cause: error,
    });
  }
  const scoopHash = scoop?.architecture?.["64bit"]?.hash;
  assert(
    typeof scoopHash === "string" && /^[0-9a-f]{64}$/iu.test(scoopHash),
    "Scoop installer manifest must contain a 64-character hexadecimal SHA-256",
  );
  assert(
    !/^0{64}$/u.test(scoopHash.toLowerCase()),
    "Scoop installer manifest still contains a placeholder SHA-256",
  );
}

export function validateAccessibilityAuditPolicy(policy) {
  const expectedTags = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];
  assertEqualJson(
    policy?.tags,
    expectedTags,
    "accessibilityAudit.tags must retain the reviewed WCAG A/AA rule set",
  );
  assertEqualJson(
    policy?.contrast,
    {
      normalTextMinimum: 4.5,
      largeTextMinimum: 3,
      interactiveMinimum: 3,
      largeTextMinPx: 24,
      boldTextMinPx: 18.66,
      boldWeightMinimum: 700,
    },
    "accessibilityAudit.contrast must retain the reviewed WCAG AA thresholds",
  );
  assert(
    Array.isArray(policy?.excludedRules),
    "accessibilityAudit.excludedRules must be an array",
  );
  const ids = new Set();
  for (const exclusion of policy.excludedRules) {
    assert(
      typeof exclusion?.id === "string" && exclusion.id.length > 0,
      "accessibility audit exclusions require a rule id",
    );
    assert(
      !ids.has(exclusion.id),
      `duplicate accessibility audit exclusion: ${exclusion.id}`,
    );
    assert(
      typeof exclusion.rationale === "string" &&
        exclusion.rationale.trim().length >= 40,
      `${exclusion.id} accessibility exclusion rationale is too short`,
    );
    ids.add(exclusion.id);
  }
  assertEqualJson(
    [...ids],
    ["color-contrast"],
    "only the compositing-aware contrast rule tracked by IMP-105 may be excluded",
  );
}

function validateTrackedDocumentationPolicy(policy) {
  const expectedFiles = [
    "README.md",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    ".github/ISSUE_TEMPLATE/feature_request.yml",
    ".github/ISSUE_TEMPLATE/config.yml",
  ];
  assert(
    Array.isArray(policy?.files) &&
      JSON.stringify(policy.files) === JSON.stringify(expectedFiles),
    "trackedDocumentation.files must name the README and three GitHub issue-form files",
  );
}

function validateTrackedDocumentationPolicyFiles(policy) {
  const documents = Object.fromEntries(
    policy.files.map((relativePath) => [
      relativePath,
      fs.readFileSync(path.join(repoRoot, relativePath), "utf8"),
    ]),
  );
  const packageJson = readJson(path.join(repoRoot, "package.json"));
  const cargoToml = fs.readFileSync(tauriManifest, "utf8");
  const platformTools = readJson(platformToolsPolicyPath);
  const schemas = readJson(
    path.join(repoRoot, "contribution-schema-policy.json"),
  ).schemas;
  const expectations = {
    appVersion: packageJson.version,
    nodeRange: packageJson.engines?.node,
    rustRange: `>=${cargoToml.match(/^rust-version\s*=\s*"([^"]+)"\s*$/mu)?.[1]}`,
    // The requirement carries a full security floor (see
    // dependencySecurityFloors), so take only its major for the README row.
    tauriMajor: `${cargoToml.match(/^tauri\s*=\s*\{\s*version\s*=\s*"(\d+)(?:\.\d+)*"/mu)?.[1]}.x`,
    platformToolsRecommended: platformTools.recommendedVersion,
    platformToolsWarningBelow: platformTools.warningBelowVersion,
    packSchema: schemas.pack.document_version,
    quirkSchema: schemas.quirk.document_version,
    profileSchema: schemas.profile.document_version,
  };
  validateTrackedDocumentation(documents, expectations, (relativePath) =>
    fs.existsSync(path.join(repoRoot, relativePath)),
  );
}

export function validateTrackedDocumentation(
  documents,
  expectations,
  localPathExists = () => true,
) {
  const readme = documents["README.md"];
  assert(
    typeof readme === "string",
    "tracked documentation requires README.md",
  );
  const combined = Object.entries(documents)
    .map(([relativePath, content]) => `${relativePath}\n${content}`)
    .join("\n");

  const staleClaims = [
    [/\badb_client\b/iu, "obsolete adb_client claim"],
    [/\bUAD-NG\b/iu, "obsolete UAD-NG claim"],
    [/\bRESEARCH_REPORT\.md\b/iu, "obsolete research document claim"],
    [/\blive route surfaces?\b/iu, "obsolete route inventory claim"],
  ];
  for (const [pattern, label] of staleClaims) {
    assert(!pattern.test(combined), `tracked documentation contains ${label}`);
  }

  assert(
    !/https?:\/\/(?:[^/\s@]+@)?(?:www\.)?(?:example\.(?:com|org|net)|localhost|127\.0\.0\.1)(?=[:/\s]|$)/iu.test(
      combined,
    ),
    "tracked documentation contains a placeholder domain",
  );
  const unsupportedDistributionClaims = [
    /\b(?:code[- ]signed|digitally signed|signed)\s+(?:artifacts?|builds?|downloads?|installers?|releases?)\b/iu,
    /\b(?:automatic|in-app|built-in)\s+(?:application\s+)?updates?\b/iu,
    /\bauto[- ]?updater\b/iu,
    /\bupdate feed\b/iu,
  ];
  for (const pattern of unsupportedDistributionClaims) {
    assert(
      !pattern.test(combined),
      "tracked documentation claims unsupported signing or updater behavior",
    );
  }

  const normalizedReadme = readme.replace(/\s+/gu, " ");
  assert(
    normalizedReadme.includes(
      "Release artifacts are unsigned and Droidsmith does not check for or install application updates.",
    ),
    "README must state the unsigned, no-updater release contract",
  );

  const versionRows = [
    `| Droidsmith source/manifests | \`${expectations.appVersion}\` |`,
    `| Node.js | \`${escapeTableCell(expectations.nodeRange)}\` |`,
    `| Rust | \`${expectations.rustRange}\` |`,
    `| Tauri | \`${expectations.tauriMajor}\` |`,
    `| Android SDK Platform Tools | \`${expectations.platformToolsRecommended}\` recommended; warn below \`${expectations.platformToolsWarningBelow}\` |`,
    `| Pack / quirk documents | schema \`"${expectations.packSchema}"\` / \`"${expectations.quirkSchema}"\` |`,
    `| Profile documents | schema \`"${expectations.profileSchema}"\`; v1 has a reviewed import migration |`,
  ];
  for (const row of versionRows) {
    assert(
      readme.includes(row),
      `README supported-version table differs from manifests: ${row}`,
    );
  }
  const profileSchemaSentence = `version \`${JSON.stringify(expectations.profileSchema)}\` for profiles.`;
  assert(
    normalizedReadme.includes(profileSchemaSentence),
    "README profile schema contract differs from contribution-schema-policy.json",
  );

  for (const [relativePath, content] of Object.entries(documents)) {
    for (const match of content.matchAll(
      /!?\[[^\]]*\]\(([^)\s]+)(?:\s+[^)]*)?\)/gu,
    )) {
      const target = match[1].replace(/^<|>$/gu, "");
      if (target.startsWith("#") || /^(?:https?:|mailto:)/iu.test(target)) {
        continue;
      }
      const withoutSuffix = target.split(/[?#]/u, 1)[0];
      let decoded;
      try {
        decoded = decodeURIComponent(withoutSuffix);
      } catch {
        throw new Error(
          `tracked documentation has an invalid local link in ${relativePath}: ${target}`,
        );
      }
      const resolved = path.posix.normalize(
        path.posix.join(path.posix.dirname(relativePath), decoded),
      );
      assert(
        resolved !== ".." &&
          !resolved.startsWith("../") &&
          localPathExists(resolved),
        `tracked documentation has a dead local link in ${relativePath}: ${target}`,
      );
    }
  }

  validateIssueForm(
    documents[".github/ISSUE_TEMPLATE/bug_report.yml"],
    "bug report",
    ["Affected workflow", "Reproduction steps", "Redacted diagnostics"],
  );
  validateIssueForm(
    documents[".github/ISSUE_TEMPLATE/feature_request.yml"],
    "feature request",
    ["Problem", "Desired outcome", "Project fit"],
  );
  const config = documents[".github/ISSUE_TEMPLATE/config.yml"];
  assert(
    typeof config === "string" &&
      config.includes("blank_issues_enabled: false") &&
      config.includes("/security/advisories/new"),
    "issue-form config must disable blank issues and route security reports privately",
  );
}

function validateIssueForm(content, label, requiredMarkers) {
  assert(
    typeof content === "string" &&
      content.includes("name:") &&
      content.includes("description:") &&
      content.includes("body:") &&
      requiredMarkers.every((marker) => content.includes(marker)),
    `${label} issue form is missing required intake fields`,
  );
}

function validateRendererBundlePolicy(policy) {
  assert(
    Number.isSafeInteger(policy?.initialJavaScriptBudgetBytes) &&
      policy.initialJavaScriptBudgetBytes > 0 &&
      policy.initialJavaScriptBudgetBytes < 903_182,
    "renderer initialJavaScriptBudgetBytes must be a positive integer below the pre-split 903182-byte entry",
  );
  assert(
    Array.isArray(policy.dynamicRouteEntries) &&
      policy.dynamicRouteEntries.length === 11 &&
      new Set(policy.dynamicRouteEntries).size === 11 &&
      policy.dynamicRouteEntries.every((entry) =>
        /^src\/routes\/[A-Za-z]+\.tsx$/u.test(entry),
      ),
    "renderer dynamicRouteEntries must list the eleven unique route entry modules",
  );
}

function validateBuiltRendererBundle(policy) {
  const distRoot = path.join(repoRoot, "dist");
  const manifest = readJson(path.join(distRoot, ".vite", "manifest.json"));
  const assetSizes = Object.fromEntries(
    Object.values(manifest)
      .filter((entry) => typeof entry?.file === "string")
      .map((entry) => [
        entry.file,
        fs.statSync(path.join(distRoot, entry.file)).size,
      ]),
  );
  const result = validateRendererBundleManifest(policy, manifest, assetSizes);
  stdout.write(
    `Renderer entry budget OK (${result.initialBytes}/${policy.initialJavaScriptBudgetBytes} bytes; ${policy.dynamicRouteEntries.length} dynamic routes)\n`,
  );
}

export function validateRendererBundleManifest(policy, manifest, assetSizes) {
  validateRendererBundlePolicy(policy);
  const entries = Object.entries(manifest).filter(
    ([, value]) => value?.isEntry === true,
  );
  assert(entries.length === 1, "Vite manifest must contain one renderer entry");
  const [, entry] = entries[0];
  const dynamicImports = new Set(entry.dynamicImports ?? []);

  for (const route of policy.dynamicRouteEntries) {
    assert(
      dynamicImports.has(route),
      `renderer entry is missing dynamic route import: ${route}`,
    );
    assert(
      typeof manifest[route]?.file === "string" &&
        Number.isSafeInteger(assetSizes[manifest[route].file]) &&
        assetSizes[manifest[route].file] > 0,
      `renderer route chunk is missing or empty: ${route}`,
    );
  }

  const initialBytes = assetSizes[entry.file];
  assert(
    Number.isSafeInteger(initialBytes),
    `renderer entry asset is missing: ${entry.file}`,
  );
  assert(
    initialBytes <= policy.initialJavaScriptBudgetBytes,
    `renderer entry ${entry.file} is ${initialBytes} bytes; budget is ${policy.initialJavaScriptBudgetBytes}`,
  );
  return { entryFile: entry.file, initialBytes };
}

function validateSubprocessCaptureContract() {
  const finiteCaptureFiles = [
    "src-tauri/src/adb/actions.rs",
    "src-tauri/src/adb/health.rs",
    "src-tauri/src/adb/resolver.rs",
    "src-tauri/src/adb/transport.rs",
    "src-tauri/src/commands.rs",
    "src-tauri/src/host_diagnostics.rs",
    "src-tauri/src/scrcpy.rs",
  ];
  for (const relativePath of finiteCaptureFiles) {
    const source = fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
    assert(
      source.includes("process_capture::run"),
      `${relativePath} must use the shared bounded subprocess capture`,
    );
    assert(
      !source.includes("read_to_end"),
      `${relativePath} must not collect subprocess pipes to EOF directly`,
    );
  }

  for (const relativePath of [
    "src-tauri/src/captured_tail.rs",
    "src-tauri/src/operations.rs",
  ]) {
    const source = fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
    assert(
      source.includes("process_capture::append_tail"),
      `${relativePath} must use the shared bounded tail implementation`,
    );
  }
}

function validateLanguageContract() {
  const contract = readJson(languageContractPath);
  assert(
    contract.schemaVersion === 1,
    "language-contract.json schemaVersion must be 1",
  );
  assert(
    Array.isArray(contract.languages) && contract.languages.length > 0,
    "language-contract.json languages must be a non-empty array",
  );

  const codes = [];
  for (const language of contract.languages) {
    assert(
      language && typeof language === "object" && !Array.isArray(language),
      "language contract entries must be objects",
    );
    assert(
      typeof language.code === "string" && /^[a-z]{2}$/u.test(language.code),
      "language contract codes must be two lowercase ASCII letters",
    );
    assert(
      !codes.includes(language.code),
      `duplicate language contract code: ${language.code}`,
    );
    assert(
      typeof language.labelKey === "string" &&
        /^language\.[a-z][a-zA-Z]*$/u.test(language.labelKey),
      `language ${language.code} labelKey is invalid`,
    );
    assert(
      language.dir === "ltr" || language.dir === "rtl",
      `language ${language.code} direction must be ltr or rtl`,
    );
    assert(
      typeof language.locale === "string" &&
        /^[a-z]{2}-[A-Z]{2}$/u.test(language.locale),
      `language ${language.code} locale must use ll-CC form`,
    );
    const localePath = path.join(
      repoRoot,
      "src",
      "locales",
      `${language.code}.json`,
    );
    assert(
      fs.existsSync(localePath),
      `language ${language.code} is missing ${path.relative(repoRoot, localePath)}`,
    );
    const locale = readJson(localePath);
    assert(
      readNested(locale, language.labelKey) !== undefined,
      `language ${language.code} is missing ${language.labelKey}`,
    );
    codes.push(language.code);
  }

  const isolationSource = fs.readFileSync(
    path.join(repoRoot, "isolation", "index.js"),
    "utf8",
  );
  const isolationBlock = isolationSource.match(
    /SUPPORTED_LANGUAGE_CODES = new Set\(\[([\s\S]*?)\]\);/u,
  )?.[1];
  assert(isolationBlock, "isolation language allowlist is missing");
  const isolationCodes = [...isolationBlock.matchAll(/"([a-z]{2})"/gu)].map(
    (match) => match[1],
  );

  const settingsSource = fs.readFileSync(
    path.join(repoRoot, "src-tauri", "src", "settings.rs"),
    "utf8",
  );
  const rustBlock = settingsSource.match(
    /pub enum SettingsLanguage\s*\{([\s\S]*?)^\}/mu,
  )?.[1];
  assert(rustBlock, "Rust SettingsLanguage enum is missing");
  const rustCodes = [
    ...rustBlock.matchAll(/^\s+([A-Z][A-Za-z0-9]*),\s*$/gmu),
  ].map((match) => match[1].toLowerCase());

  assertEqualJson(isolationCodes, codes, "isolation language codes differ");
  assertEqualJson(rustCodes, codes, "Rust settings language codes differ");

  const rendererSource = fs.readFileSync(
    path.join(repoRoot, "src", "lib", "i18n.ts"),
    "utf8",
  );
  assert(
    rendererSource.includes('from "../../language-contract.json"'),
    "renderer must consume language-contract.json",
  );
}

function readNested(value, dottedPath) {
  let cursor = value;
  for (const segment of dottedPath.split(".")) {
    if (!cursor || typeof cursor !== "object" || !(segment in cursor)) {
      return undefined;
    }
    cursor = cursor[segment];
  }
  return cursor;
}

export function platformToolsArchiveUrl(os, version) {
  assert(
    Object.hasOwn(platformToolsArchiveTokens, os),
    `unsupported Platform Tools operating system: ${os}`,
  );
  assertSemver(`platform-tools ${os} archive version`, version);
  return `https://dl.google.com/android/repository/platform-tools_r${version}-${platformToolsArchiveTokens[os]}.zip`;
}

export function validatePlatformToolsArchiveUrl(url, os, version) {
  assert(
    url === platformToolsArchiveUrl(os, version),
    `${os} Platform Tools URL must be the versioned official archive`,
  );
  return true;
}

function validatePlatformToolsPolicy() {
  const policy = readJson(platformToolsPolicyPath);
  assert(
    policy.schemaVersion === 1,
    "platform-tools-policy.json schemaVersion must be 1",
  );
  for (const field of [
    "reviewedOn",
    "recommendedVersion",
    "pinnedVersion",
    "warningBelowVersion",
    "sourceUrl",
    "rationale",
  ]) {
    assert(
      typeof policy[field] === "string" && policy[field].trim().length > 0,
      `platform-tools policy ${field} is required`,
    );
  }
  assertAbsoluteDate("platform-tools policy reviewedOn", policy.reviewedOn);
  assertSemver("platform-tools recommendedVersion", policy.recommendedVersion);
  assertSemver("platform-tools pinnedVersion", policy.pinnedVersion);
  assert(
    policy.pinnedVersion === policy.recommendedVersion,
    "platform-tools pinnedVersion must equal recommendedVersion",
  );
  assertSemver(
    "platform-tools warningBelowVersion",
    policy.warningBelowVersion,
  );
  assert(
    compareVersions(policy.recommendedVersion, policy.warningBelowVersion) >= 0,
    "platform-tools recommendedVersion must not predate warningBelowVersion",
  );
  assert(
    policy.sourceUrl ===
      "https://developer.android.com/tools/releases/platform-tools",
    "platform-tools policy must cite the official Android release notes",
  );
  assert(
    typeof policy.downloads === "object" && policy.downloads !== null,
    "platform-tools policy downloads are required",
  );
  for (const os of ["windows", "linux", "darwin"]) {
    const download = policy.downloads[os];
    assert(download && typeof download === "object", `missing ${os} download`);
    validatePlatformToolsArchiveUrl(download.url, os, policy.pinnedVersion);
    assert(
      /^[0-9a-f]{64}$/u.test(download.sha256),
      `${os} Platform Tools SHA-256 must be pinned`,
    );
  }
  assert(
    Array.isArray(policy.knownBadRules),
    "platform-tools knownBadRules must be an array",
  );
  const knownBadVersions = new Set();
  for (const rule of policy.knownBadRules) {
    assertSemver("platform-tools known-bad version", rule.version);
    assert(
      rule.status === "blocked",
      "platform-tools known-bad rules must explicitly use blocked status",
    );
    assert(
      typeof rule.rationale === "string" && rule.rationale.length >= 40,
      `platform-tools ${rule.version} known-bad rationale is too short`,
    );
    assert(
      rule.sourceUrl === policy.sourceUrl,
      `platform-tools ${rule.version} known-bad rule needs the official source`,
    );
    assert(
      !knownBadVersions.has(rule.version),
      `duplicate platform-tools known-bad version ${rule.version}`,
    );
    knownBadVersions.add(rule.version);
  }

  const readme = fs.readFileSync(path.join(repoRoot, "README.md"), "utf8");
  validatePlatformToolsDocumentation(policy, readme);
  for (const script of [
    path.join(repoRoot, "scripts", "fetch-platform-tools.ps1"),
    path.join(repoRoot, "scripts", "fetch-platform-tools.sh"),
  ]) {
    const content = fs.readFileSync(script, "utf8");
    assert(
      content.includes("platform-tools-policy.json") &&
        content.includes("pinnedVersion"),
      `${path.basename(script)} must consume platform-tools-policy.json`,
    );
    assert(
      !content.includes("35.0.2"),
      `${path.basename(script)} retains the stale Platform Tools pin`,
    );
  }
  const rustPolicy = fs.readFileSync(
    path.join(repoRoot, "src-tauri", "src", "adb", "version_policy.rs"),
    "utf8",
  );
  assert(
    rustPolicy.includes("platform-tools-policy.json") &&
      rustPolicy.includes("include_str!"),
    "Rust runtime must embed platform-tools-policy.json",
  );
  const health = fs.readFileSync(
    path.join(repoRoot, "src-tauri", "src", "adb", "health.rs"),
    "utf8",
  );
  const resolver = fs.readFileSync(
    path.join(repoRoot, "src-tauri", "src", "adb", "resolver.rs"),
    "utf8",
  );
  assert(
    health.includes("version_policy::is_recommended") &&
      resolver.includes("version_policy::assess"),
    "ADB resolver and health probes must consume the shared version policy",
  );
}

export function validatePlatformToolsDocumentation(policy, readme) {
  const expected = `reviewed on ${policy.reviewedOn}, recommends ${policy.recommendedVersion}, and warns (without blocking) below\n${policy.warningBelowVersion}`;
  const normalizedReadme = readme.replace(/\r\n/gu, "\n");
  assert(
    normalizedReadme.includes(expected),
    "README Platform Tools policy summary differs from platform-tools-policy.json",
  );
}

function assertAbsoluteDate(label, value) {
  assert(/^\d{4}-\d{2}-\d{2}$/u.test(value), `${label} must use YYYY-MM-DD`);
  const parsed = new Date(`${value}T00:00:00Z`);
  assert(
    !Number.isNaN(parsed.valueOf()) && parsed.toISOString().startsWith(value),
    `${label} is invalid`,
  );
}

function assertSemver(label, value) {
  assert(/^\d+\.\d+\.\d+$/u.test(value), `${label} must be x.y.z`);
}

export function validateExpiry(kind, value, now = new Date()) {
  assert(
    typeof value === "string" && /^\d{4}-\d{2}-\d{2}$/u.test(value),
    `${kind} exception expiresOn must use YYYY-MM-DD`,
  );
  const [year, month, day] = value.split("-").map(Number);
  const expiry = new Date(`${value}T23:59:59.999Z`);
  assert(
    !Number.isNaN(expiry.valueOf()) &&
      expiry.getUTCFullYear() === year &&
      expiry.getUTCMonth() + 1 === month &&
      expiry.getUTCDate() === day,
    `${kind} exception expiry is invalid`,
  );
  assert(
    expiry >= now,
    `${kind} exception expired on ${value}; remove or re-review it before release`,
  );
}

function validateAuditExceptions(exception) {
  assert(exception, "rust_advisories exception metadata is required");
  assert(
    Array.isArray(exception.subjects),
    "rust_advisories subjects must be an array",
  );
  const expected = sortedUnique(exception.subjects);
  const auditConfig = fs.readFileSync(
    path.join(repoRoot, "src-tauri", ".cargo", "audit.toml"),
    "utf8",
  );
  const actual = sortedUnique(
    [...auditConfig.matchAll(/RUSTSEC-\d{4}-\d{4}/gu)].map((match) => match[0]),
  );
  assertEqualJson(
    actual,
    expected,
    "cargo-audit ignores differ from expiring rust_advisories policy",
  );
}

function validateDuplicateExceptions(exception) {
  assert(exception, "duplicate_crates exception metadata is required");
  assert(
    exception.subjects &&
      typeof exception.subjects === "object" &&
      !Array.isArray(exception.subjects),
    "duplicate_crates subjects must map crate names to exact versions",
  );

  const lock = fs.readFileSync(
    path.join(repoRoot, "src-tauri", "Cargo.lock"),
    "utf8",
  );
  const actual = collectCargoDuplicates(lock);

  const expected = Object.fromEntries(
    Object.entries(exception.subjects)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([name, versions]) => {
        assert(
          Array.isArray(versions) && versions.length > 1,
          `duplicate_crates ${name} must list at least two versions`,
        );
        return [name, [...versions].sort(compareVersions)];
      }),
  );
  assertEqualJson(
    actual,
    expected,
    "Cargo.lock duplicate graph differs from the expiring reviewed exception",
  );
}

export function collectCargoDuplicates(lock) {
  const packages = new Map();
  for (const match of lock.matchAll(
    /^\[\[package\]\]\r?\nname = "([^"]+)"\r?\nversion = "([^"]+)"/gmu,
  )) {
    const versions = packages.get(match[1]) ?? [];
    versions.push(match[2]);
    packages.set(match[1], versions);
  }

  return Object.fromEntries(
    [...packages.entries()]
      .filter(([, versions]) => versions.length > 1)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([name, versions]) => [name, [...versions].sort(compareVersions)]),
  );
}

function validateVersionParity() {
  const packageJson = readJson(path.join(repoRoot, "package.json"));
  const packageLock = readJson(path.join(repoRoot, "package-lock.json"));
  const tauriConfig = readJson(
    path.join(repoRoot, "src-tauri", "tauri.conf.json"),
  );
  const cargoToml = fs.readFileSync(tauriManifest, "utf8");
  const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"\s*$/mu)?.[1];
  const readme = fs.readFileSync(path.join(repoRoot, "README.md"), "utf8");
  const badgeVersion = readme.match(/badge\/version-([\d.]+)-/u)?.[1];
  const versions = {
    "package.json": packageJson.version,
    "package-lock.json": packageLock.packages?.[""]?.version,
    "src-tauri/Cargo.toml": cargoVersion,
    "src-tauri/tauri.conf.json": tauriConfig.version,
    "README.md badge": badgeVersion,
  };
  validateVersionValues(versions);
}

function validateScrcpyPolicy() {
  const policy = readJson(path.join(repoRoot, "scrcpy-policy.json"));
  const rustSource = fs.readFileSync(
    path.join(repoRoot, "src-tauri", "src", "scrcpy_policy.rs"),
    "utf8",
  );
  validateScrcpyPolicyDocument(policy, rustSource);
}

// scrcpy publishes no GitHub security advisory for CVE-2025-34449 and NVD does
// not index it under the project name, so nothing automated tracks this floor.
// The gate is the only thing keeping the policy, the Rust module, and the
// documented rationale from drifting apart.
export function validateScrcpyPolicyDocument(policy, rustSource) {
  assert(
    policy?.schemaVersion === 1,
    "scrcpy-policy.json schemaVersion must be 1",
  );
  assertAbsoluteDate("scrcpy policy reviewedOn", policy.reviewedOn);
  assertSemver("scrcpy security floor", policy.securityFloorVersion);
  assert(
    typeof policy.rationale === "string" &&
      policy.rationale.trim().length >= 40,
    "scrcpy-policy.json rationale is too short",
  );
  assert(
    typeof policy.sourceUrl === "string" &&
      policy.sourceUrl.startsWith("https://"),
    "scrcpy-policy.json needs an https source URL",
  );
  assert(
    Array.isArray(policy.advisories) && policy.advisories.length > 0,
    "scrcpy-policy.json must list at least one advisory",
  );
  let highestFloor = "0.0.0";
  for (const advisory of policy.advisories) {
    assert(
      typeof advisory.id === "string" && /^(?:CVE|GHSA)-/u.test(advisory.id),
      "scrcpy advisory id must be a CVE or GHSA identifier",
    );
    assertSemver(`${advisory.id} belowVersion`, advisory.belowVersion);
    assert(
      typeof advisory.summary === "string" &&
        advisory.summary.trim().length >= 20,
      `${advisory.id} summary is too short`,
    );
    assert(
      typeof advisory.sourceUrl === "string" &&
        advisory.sourceUrl.startsWith("https://"),
      `${advisory.id} needs an https source URL`,
    );
    if (compareVersions(advisory.belowVersion, highestFloor) > 0) {
      highestFloor = advisory.belowVersion;
    }
  }
  assert(
    policy.securityFloorVersion === highestFloor,
    `scrcpy securityFloorVersion must equal the highest advisory floor (${highestFloor})`,
  );
  // The Rust module reads the JSON at compile time; keep its documented floor
  // and the advisory it cites from drifting away from the policy.
  assert(
    rustSource.includes(policy.securityFloorVersion),
    "src-tauri/src/scrcpy_policy.rs no longer references the policy security floor",
  );
  for (const advisory of policy.advisories) {
    assert(
      rustSource.includes(advisory.id),
      `src-tauri/src/scrcpy_policy.rs no longer references ${advisory.id}`,
    );
  }
}

function validateDependencySecurityFloors(floors) {
  assert(
    Array.isArray(floors) && floors.length > 0,
    "release-policy.json dependencySecurityFloors must be a non-empty array",
  );
  for (const floor of floors) {
    const manifestPath = path.join(repoRoot, floor.manifest ?? "");
    assert(
      typeof floor.manifest === "string" && fs.existsSync(manifestPath),
      `dependency security floor manifest is missing: ${floor.manifest}`,
    );
    const npmFloor = floor.manifest.endsWith("package-lock.json");
    const dependencyName = npmFloor ? floor.package : floor.crate;
    assert(
      typeof dependencyName === "string" && dependencyName.length > 0,
      npmFloor
        ? "npm dependency security floor package is required"
        : "dependency security floor crate is required",
    );
    assertSemver(`${dependencyName} security floor`, floor.minimumVersion);
    assert(
      typeof floor.rationale === "string" &&
        floor.rationale.trim().length >= 20,
      `${floor.crate} security floor rationale is too short`,
    );
    assert(
      typeof floor.sourceUrl === "string" &&
        floor.sourceUrl.startsWith("https://"),
      `${dependencyName} security floor needs an https source URL`,
    );
    assert(
      typeof floor.advisory === "string" &&
        /^(?:CVE|GHSA)-/u.test(floor.advisory),
      `${dependencyName} security floor needs a CVE or GHSA advisory id`,
    );
    if (npmFloor) {
      const lock = readJson(manifestPath);
      const resolved = readNpmDependencyVersion(lock, floor.package);
      assert(
        resolved !== undefined,
        `${floor.package} is not resolved in ${floor.manifest}`,
      );
      validateNpmDependencyFloor(floor, resolved);
      continue;
    }
    const declared = readCargoDependencyRequirement(
      fs.readFileSync(manifestPath, "utf8"),
      floor.crate,
    );
    assert(
      declared !== undefined,
      `${floor.crate} is not declared in ${floor.manifest}`,
    );
    validateDependencyFloor(floor, declared);
  }
}

export function readNpmDependencyVersion(lock, packageName) {
  return lock?.packages?.[`node_modules/${packageName}`]?.version;
}

export function validateNpmDependencyFloor(floor, resolved) {
  assertSemver(`${floor.package} resolved version`, resolved);
  assert(
    compareVersions(resolved, floor.minimumVersion) >= 0,
    `${floor.package} resolves to ${resolved}, below the ${floor.minimumVersion} security floor (${floor.cve ?? floor.advisory})`,
  );
}

// A caret requirement only guarantees the advisory fix when the declared floor
// is at or above the patched release; `tauri = "2"` would silently allow a
// vulnerable resolve on a fresh lockfile.
export function validateDependencyFloor(floor, declared) {
  assert(
    /^\d+(\.\d+){0,2}$/u.test(declared),
    `${floor.crate} requirement must be a plain caret floor, found "${declared}"`,
  );
  const required = floor.minimumVersion.split(".").map(Number);
  const actual = declared.split(".").map(Number);
  for (let index = 0; index < required.length; index += 1) {
    const left = actual[index];
    if (left === undefined) {
      // "2" or "2.11" cannot express the 2.11.1 floor.
      assert(
        required[index] === 0,
        `${floor.crate} requirement "${declared}" is below the ${floor.minimumVersion} security floor (${floor.cve ?? floor.advisory})`,
      );
      continue;
    }
    if (left > required[index]) return;
    assert(
      left === required[index],
      `${floor.crate} requirement "${declared}" is below the ${floor.minimumVersion} security floor (${floor.cve ?? floor.advisory})`,
    );
  }
}

export function readCargoDependencyRequirement(manifest, crate) {
  const inline = manifest.match(
    new RegExp(`^${crate}\\s*=\\s*"([^"]+)"\\s*$`, "mu"),
  );
  if (inline) return inline[1];
  const table = manifest.match(
    new RegExp(`^${crate}\\s*=\\s*\\{([^}]*)\\}`, "mu"),
  );
  return table?.[1].match(/version\s*=\s*"([^"]+)"/u)?.[1];
}

/** A literal `|` closes a markdown table cell, so semver ranges that use `||`
 * are escaped in the README and must be escaped here to match. */
function escapeTableCell(value) {
  return String(value).replaceAll("|", String.raw`\|`);
}

export function validateVersionValues(versions) {
  const distinct = new Set(Object.values(versions));
  assert(
    distinct.size === 1 && !distinct.has(undefined),
    `release versions differ: ${JSON.stringify(versions)}`,
  );
}

function validateAutomationPolicy() {
  validateAutomationFiles(
    fs.readFileSync(
      path.join(repoRoot, ".github", "workflows", "ci.yml"),
      "utf8",
    ),
    fs.readFileSync(path.join(repoRoot, ".github", "dependabot.yml"), "utf8"),
  );
}

export function validateAutomationFiles(ciWorkflow, dependabot) {
  const requiredWorkflowMarkers = [
    "pull_request:",
    "push:",
    "schedule:",
    "workflow_dispatch:",
    "permissions:\n  contents: read",
    "\n  frontend:\n",
    "\n  native:\n",
    "\n  security:\n",
    "\n  release-smoke:\n",
    "os: [ubuntu-latest, windows-latest, macos-latest]",
    "npm ci",
    "npm run ui:smoke",
    "cargo test --locked",
    "npm run security:audit",
    "npm run release:check",
    "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'",
  ];
  for (const marker of requiredWorkflowMarkers) {
    assert(
      ciWorkflow.includes(marker),
      `CI workflow is missing required marker: ${marker}`,
    );
  }

  const actionReferences = [
    ...ciWorkflow.matchAll(/^\s*-\s+uses:\s+[^@\s]+@([^\s#]+)/gmu),
  ].map((match) => match[1]);
  assert(actionReferences.length > 0, "CI workflow has no action references");
  assert(
    actionReferences.every((reference) => /^[0-9a-f]{40}$/u.test(reference)),
    "CI actions must be pinned to full commit SHAs",
  );

  const requiredDependabotMarkers = [
    "version: 2",
    "package-ecosystem: npm",
    "package-ecosystem: cargo",
    "package-ecosystem: github-actions",
    "directory: /src-tauri",
    "interval: weekly",
    "open-pull-requests-limit:",
    "groups:",
  ];
  for (const marker of requiredDependabotMarkers) {
    assert(
      dependabot.includes(marker),
      `Dependabot policy is missing required marker: ${marker}`,
    );
  }
  const pullRequestLimits = [
    ...dependabot.matchAll(/open-pull-requests-limit:\s*(\d+)/gu),
  ].map((match) => Number.parseInt(match[1], 10));
  assert(
    pullRequestLimits.length === 3 &&
      pullRequestLimits.every((limit) => limit > 0 && limit <= 4),
    "Dependabot ecosystems must each cap open pull requests at four",
  );
}

function runSchemaLint() {
  const args = [
    "run",
    "--quiet",
    "--locked",
    "--manifest-path",
    tauriManifest,
    "--bin",
    "droidsmith-schema-lint",
    "--",
    "--check-generated",
    repoRoot,
  ];
  appendSchemaFiles(args, "--pack", path.join(repoRoot, "packs"));
  appendSchemaFiles(args, "--quirk", path.join(repoRoot, "quirks"));
  appendSchemaFiles(
    args,
    "--profile",
    path.join(repoRoot, "src-tauri", "fixtures", "profiles"),
  );
  run("cargo", args);
}

function appendSchemaFiles(args, flag, directory) {
  const files = fs
    .readdirSync(directory, { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isFile() && /\.(?:yaml|yml)$/u.test(entry.name.toLowerCase()),
    )
    .map((entry) => path.join(directory, entry.name))
    .sort();
  assert(
    files.length > 0,
    `no schema inputs found in ${path.relative(repoRoot, directory)}`,
  );
  for (const file of files) args.push(flag, file);
}

function runNpm(scriptName) {
  if (env.npm_execpath && fs.existsSync(env.npm_execpath)) {
    run(execPath, [env.npm_execpath, "run", scriptName]);
    return;
  }
  if (platform === "win32") {
    run("cmd.exe", ["/d", "/s", "/c", `npm run ${scriptName}`]);
    return;
  }
  run("npm", ["run", scriptName]);
}

function run(command, args) {
  stdout.write(`\n[release-check] ${command} ${args.join(" ")}\n`);
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} exited with ${result.status}`,
    );
  }
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function sortedUnique(values) {
  return [...new Set(values)].sort();
}

function compareVersions(left, right) {
  return left.localeCompare(right, "en", { numeric: true });
}

function assertEqualJson(actual, expected, message) {
  assert(
    JSON.stringify(actual) === JSON.stringify(expected),
    `${message}\nactual: ${JSON.stringify(actual)}\nexpected: ${JSON.stringify(expected)}`,
  );
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
