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
  validatePlatformToolsPolicy();
  validateLanguageContract();
  validateSubprocessCaptureContract();
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

function validatePlatformToolsPolicy() {
  const policy = readJson(platformToolsPolicyPath);
  assert(
    policy.schemaVersion === 1,
    "platform-tools-policy.json schemaVersion must be 1",
  );
  for (const field of [
    "reviewedOn",
    "recommendedVersion",
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
    assert(
      download.url ===
        `https://dl.google.com/android/repository/platform-tools-latest-${os}.zip`,
      `${os} Platform Tools URL must be the official archive`,
    );
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
        content.includes("recommendedVersion"),
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
  assert(
    readme.includes(expected),
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

export function validateVersionValues(versions) {
  const distinct = new Set(Object.values(versions));
  assert(
    distinct.size === 1 && !distinct.has(undefined),
    `release versions differ: ${JSON.stringify(versions)}`,
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
