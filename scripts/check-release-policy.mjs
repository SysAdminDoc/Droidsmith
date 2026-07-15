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

if (path.resolve(argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  main();
}

function main() {
  validatePolicy();
  if (argv.includes("--policy-only")) {
    stdout.write("Release policy metadata OK\n");
    return;
  }

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
