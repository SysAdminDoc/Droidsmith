import { Buffer } from "node:buffer";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { argv, stdout } from "node:process";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(scriptPath), "..");
const outputDirectory = "provenance";
const sbomRelativePath = `${outputDirectory}/SBOM.cdx.json`;
const checksumsRelativePath = `${outputDirectory}/SHA256SUMS`;

if (path.resolve(argv[1] ?? "") === scriptPath) main();

function main() {
  const unsupported = argv
    .slice(2)
    .filter((argument) => argument !== "--check");
  assert(unsupported.length === 0, `unsupported argument: ${unsupported[0]}`);

  const inputs = readInputs(repoRoot);
  const generated = generateProvenance(inputs);
  validateProvenance(generated, inputs);

  if (argv.includes("--check")) {
    stdout.write(
      `Provenance inputs OK (${generated.sbom.components.length} locked runtime components)\n`,
    );
    return;
  }

  const destination = path.join(repoRoot, outputDirectory);
  fs.mkdirSync(destination, { recursive: true });
  fs.writeFileSync(path.join(repoRoot, sbomRelativePath), generated.sbomText);
  fs.writeFileSync(
    path.join(repoRoot, checksumsRelativePath),
    generated.checksumsText,
  );
  stdout.write(
    `Wrote ${sbomRelativePath} and ${checksumsRelativePath} (${generated.sbom.components.length} components)\n`,
  );
}

export function readInputs(root) {
  return {
    packageJsonText: fs.readFileSync(path.join(root, "package.json"), "utf8"),
    packageLockText: fs.readFileSync(
      path.join(root, "package-lock.json"),
      "utf8",
    ),
    cargoManifestText: fs.readFileSync(
      path.join(root, "src-tauri", "Cargo.toml"),
      "utf8",
    ),
    cargoLockText: fs.readFileSync(
      path.join(root, "src-tauri", "Cargo.lock"),
      "utf8",
    ),
    noticesText: fs.readFileSync(
      path.join(root, "third-party-notices.json"),
      "utf8",
    ),
  };
}

export function generateProvenance(inputs) {
  const canonicalInputs = Object.fromEntries(
    Object.entries(inputs).map(([name, value]) => [name, normalizeText(value)]),
  );
  const packageJson = parseJson(
    canonicalInputs.packageJsonText,
    "package.json",
  );
  const packageLock = parseJson(
    canonicalInputs.packageLockText,
    "package-lock.json",
  );
  const npmComponents = collectNpmRuntimeComponents(packageLock);
  const cargoComponents = collectCargoRuntimeComponents(
    canonicalInputs.cargoManifestText,
    canonicalInputs.cargoLockText,
  );
  const components = deduplicateComponents([
    ...npmComponents,
    ...cargoComponents,
  ]);
  const sbom = {
    bomFormat: "CycloneDX",
    specVersion: "1.6",
    version: 1,
    metadata: {
      component: {
        type: "application",
        "bom-ref": `pkg:generic/droidsmith@${encodeURIComponent(packageJson.version)}`,
        name: packageJson.name,
        version: packageJson.version,
      },
      properties: [
        {
          name: "droidsmith:provenance-mode",
          value: "offline-lockfiles",
        },
      ],
    },
    components,
  };
  const sbomText = `${JSON.stringify(sbom, null, 2)}\n`;
  const checksumInputs = {
    "package-lock.json": canonicalInputs.packageLockText,
    "src-tauri/Cargo.lock": canonicalInputs.cargoLockText,
    "third-party-notices.json": canonicalInputs.noticesText,
    [sbomRelativePath]: sbomText,
  };
  const checksumsText = `${Object.entries(checksumInputs)
    .sort(([left], [right]) => compareText(left, right))
    .map(([relativePath, content]) => `${sha256(content)}  ${relativePath}`)
    .join("\n")}\n`;

  return { sbom, sbomText, checksumsText };
}

function normalizeText(value) {
  assert(typeof value === "string", "provenance inputs must be text");
  return value.replace(/\r\n?/gu, "\n");
}

export function collectNpmRuntimeComponents(lock) {
  assert(
    Number.isSafeInteger(lock?.lockfileVersion) && lock.lockfileVersion >= 2,
    "package-lock.json must use lockfileVersion 2 or newer",
  );
  assert(
    lock.packages && typeof lock.packages === "object",
    "package-lock.json packages map is required",
  );

  const components = [];
  for (const [packagePath, record] of Object.entries(lock.packages)) {
    if (
      packagePath === "" ||
      !packagePath.includes("node_modules/") ||
      record.dev === true ||
      record.link === true
    ) {
      continue;
    }
    const name = record.name ?? packageNameFromLockPath(packagePath);
    assert(name, `cannot derive npm package name from ${packagePath}`);
    assert(
      typeof record.version === "string" && record.version.length > 0,
      `npm package ${name} is missing a locked version`,
    );
    const purl = npmPurl(name, record.version);
    const component = {
      type: "library",
      "bom-ref": purl,
      name,
      version: record.version,
      purl,
      scope: record.optional === true ? "optional" : "required",
      properties: [
        { name: "droidsmith:ecosystem", value: "npm" },
        { name: "droidsmith:lock-path", value: packagePath },
      ],
    };
    const hashes = integrityHashes(record.integrity);
    if (hashes.length > 0) component.hashes = hashes;
    components.push(component);
  }
  return deduplicateComponents(components);
}

export function collectCargoRuntimeComponents(manifestText, lockText) {
  const manifestPackage = parseCargoManifestPackage(manifestText);
  const directDependencies = parseCargoRuntimeDependencyNames(manifestText);
  const packages = parseCargoLock(lockText);
  const byName = Map.groupBy(packages, (entry) => entry.name);
  const root = packages.find(
    (entry) =>
      entry.name === manifestPackage.name &&
      entry.version === manifestPackage.version &&
      entry.source === undefined,
  );
  assert(
    root,
    `Cargo.lock is missing workspace package ${manifestPackage.name} ${manifestPackage.version}`,
  );

  const rootRefs = new Map(
    root.dependencies.map((entry) => [entry.name, entry]),
  );
  const queue = [...directDependencies].map((name) => {
    const reference = rootRefs.get(name);
    assert(reference, `Cargo.lock root is missing runtime dependency ${name}`);
    return resolveCargoReference(reference, byName);
  });
  const collected = new Map();
  while (queue.length > 0) {
    const entry = queue.shift();
    const identity = cargoIdentity(entry);
    if (collected.has(identity)) continue;
    collected.set(identity, entry);
    for (const reference of entry.dependencies) {
      queue.push(resolveCargoReference(reference, byName));
    }
  }

  return [...collected.values()]
    .map((entry) => {
      const purl = `pkg:cargo/${encodeURIComponent(entry.name)}@${encodeURIComponent(entry.version)}`;
      const component = {
        type: "library",
        "bom-ref": purl,
        name: entry.name,
        version: entry.version,
        purl,
        scope: "required",
        properties: [{ name: "droidsmith:ecosystem", value: "cargo" }],
      };
      if (entry.checksum) {
        component.hashes = [
          { alg: "SHA-256", content: entry.checksum.toUpperCase() },
        ];
      }
      return component;
    })
    .sort(compareComponents);
}

export function validateProvenance(generated, inputs) {
  const parsed = parseJson(generated.sbomText, "generated CycloneDX SBOM");
  assert(parsed.bomFormat === "CycloneDX", "SBOM bomFormat must be CycloneDX");
  assert(parsed.specVersion === "1.6", "SBOM specVersion must be 1.6");
  assert(parsed.version === 1, "SBOM version must be 1");
  assert(Array.isArray(parsed.components), "SBOM components must be an array");

  const expected = generateExpectedPurls(inputs);
  const actual = parsed.components.map((component) => component.purl);
  assert(
    new Set(actual).size === actual.length,
    "SBOM contains duplicate package URLs",
  );
  assertEqualJson(
    actual,
    expected,
    "SBOM components differ from runtime lock graphs",
  );

  const checksums = parseChecksums(generated.checksumsText);
  const expectedChecksums = {
    "package-lock.json": sha256(inputs.packageLockText),
    "src-tauri/Cargo.lock": sha256(inputs.cargoLockText),
    "third-party-notices.json": sha256(inputs.noticesText),
    [sbomRelativePath]: sha256(generated.sbomText),
  };
  assertEqualJson(
    checksums,
    Object.fromEntries(
      Object.entries(expectedChecksums).sort(([left], [right]) =>
        compareText(left, right),
      ),
    ),
    "SHA256SUMS differs from provenance inputs",
  );
}

function generateExpectedPurls(inputs) {
  const npm = collectNpmRuntimeComponents(
    parseJson(inputs.packageLockText, "package-lock.json"),
  );
  const cargo = collectCargoRuntimeComponents(
    inputs.cargoManifestText,
    inputs.cargoLockText,
  );
  return deduplicateComponents([...npm, ...cargo]).map(
    (component) => component.purl,
  );
}

function parseCargoManifestPackage(manifest) {
  const packageHeader = manifest.match(/^\[package\]\r?\n/mu);
  assert(packageHeader, "Cargo.toml [package] table is required");
  const remainder = manifest.slice(
    packageHeader.index + packageHeader[0].length,
  );
  const nextTable = remainder.search(/^\[/mu);
  const block = nextTable < 0 ? remainder : remainder.slice(0, nextTable);
  const name = block.match(/^name\s*=\s*"([^"]+)"/mu)?.[1];
  const version = block.match(/^version\s*=\s*"([^"]+)"/mu)?.[1];
  assert(name && version, "Cargo.toml package name and version are required");
  return { name, version };
}

function parseCargoRuntimeDependencyNames(manifest) {
  let section = "";
  const names = new Set();
  for (const rawLine of manifest.split(/\r?\n/u)) {
    const table = rawLine.match(/^\[([^\]]+)\]\s*$/u)?.[1];
    if (table) {
      section = table;
      continue;
    }
    const isRuntimeTable =
      section === "dependencies" ||
      section === "build-dependencies" ||
      /^target\..+\.(?:dependencies|build-dependencies)$/u.test(section);
    if (!isRuntimeTable) continue;
    const declaration = rawLine.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/u);
    if (!declaration) continue;
    const packageAlias = declaration[2].match(
      /\bpackage\s*=\s*"([^"]+)"/u,
    )?.[1];
    names.add(packageAlias ?? declaration[1]);
  }
  assert(names.size > 0, "Cargo.toml has no runtime or build dependencies");
  return names;
}

function parseCargoLock(lock) {
  const packages = lock
    .split(/^\[\[package\]\]\s*$/mu)
    .slice(1)
    .map((block) => {
      const name = block.match(/^name\s*=\s*"([^"]+)"/mu)?.[1];
      const version = block.match(/^version\s*=\s*"([^"]+)"/mu)?.[1];
      assert(name && version, "Cargo.lock package is missing name or version");
      const source = block.match(/^source\s*=\s*"([^"]+)"/mu)?.[1];
      const checksum = block.match(/^checksum\s*=\s*"([a-fA-F0-9]+)"/mu)?.[1];
      const dependenciesBlock = block.match(
        /^dependencies\s*=\s*\[\r?\n([\s\S]*?)^\]\s*$/mu,
      )?.[1];
      const dependencies = dependenciesBlock
        ? [...dependenciesBlock.matchAll(/^\s*"([^"]+)",?\s*$/gmu)].map(
            (match) => parseCargoReference(match[1]),
          )
        : [];
      return { name, version, source, checksum, dependencies };
    });
  assert(packages.length > 0, "Cargo.lock contains no packages");
  return packages;
}

function parseCargoReference(value) {
  const match = value.match(
    /^([A-Za-z0-9_-]+)(?:\s+([^\s()]+))?(?:\s+\(([^)]+)\))?$/u,
  );
  assert(match, `unsupported Cargo.lock dependency reference: ${value}`);
  return { name: match[1], version: match[2], source: match[3] };
}

function resolveCargoReference(reference, byName) {
  let candidates = byName.get(reference.name) ?? [];
  if (reference.version) {
    candidates = candidates.filter(
      (entry) => entry.version === reference.version,
    );
  }
  if (reference.source) {
    candidates = candidates.filter(
      (entry) => entry.source === reference.source,
    );
  }
  assert(
    candidates.length === 1,
    `Cargo.lock dependency ${reference.name}${reference.version ? ` ${reference.version}` : ""} resolves to ${candidates.length} packages`,
  );
  return candidates[0];
}

function deduplicateComponents(components) {
  const byPurl = new Map();
  for (const component of components) {
    const existing = byPurl.get(component.purl);
    if (!existing) {
      byPurl.set(component.purl, component);
      continue;
    }
    if (
      component.properties?.some(
        (property) => property.name === "droidsmith:lock-path",
      )
    ) {
      const paths = new Set(
        [...existing.properties, ...component.properties]
          .filter((property) => property.name === "droidsmith:lock-path")
          .map((property) => property.value),
      );
      existing.properties = existing.properties.filter(
        (property) => property.name !== "droidsmith:lock-path",
      );
      existing.properties.push({
        name: "droidsmith:lock-path",
        value: [...paths].sort(compareText).join(","),
      });
    }
  }
  return [...byPurl.values()].sort(compareComponents);
}

function packageNameFromLockPath(packagePath) {
  const tail = packagePath.slice(packagePath.lastIndexOf("node_modules/") + 13);
  const segments = tail.split("/");
  return segments[0].startsWith("@")
    ? `${segments[0]}/${segments[1] ?? ""}`
    : segments[0];
}

function npmPurl(name, version) {
  if (name.startsWith("@")) {
    const [scope, packageName] = name.split("/", 2);
    assert(packageName, `invalid scoped npm package name: ${name}`);
    return `pkg:npm/${encodeURIComponent(scope)}/${encodeURIComponent(packageName)}@${encodeURIComponent(version)}`;
  }
  return `pkg:npm/${encodeURIComponent(name)}@${encodeURIComponent(version)}`;
}

function integrityHashes(integrity) {
  if (typeof integrity !== "string") return [];
  const algorithms = new Map([
    ["sha256", "SHA-256"],
    ["sha384", "SHA-384"],
    ["sha512", "SHA-512"],
  ]);
  const hashes = [];
  for (const token of integrity.split(/\s+/u)) {
    const separator = token.indexOf("-");
    const algorithm = algorithms.get(token.slice(0, separator));
    if (!algorithm || separator < 0) continue;
    const content = Buffer.from(token.slice(separator + 1), "base64")
      .toString("hex")
      .toUpperCase();
    if (content) hashes.push({ alg: algorithm, content });
  }
  return hashes;
}

function parseChecksums(text) {
  const entries = text
    .trim()
    .split(/\r?\n/u)
    .map((line) => {
      const match = line.match(/^([a-f0-9]{64}) {2}(.+)$/u);
      assert(match, `invalid SHA256SUMS line: ${line}`);
      return [match[2], match[1]];
    });
  assert(
    new Set(entries.map(([name]) => name)).size === entries.length,
    "SHA256SUMS contains duplicate paths",
  );
  return Object.fromEntries(entries);
}

function cargoIdentity(entry) {
  return `${entry.name}\u0000${entry.version}\u0000${entry.source ?? ""}`;
}

function compareComponents(left, right) {
  return compareText(left.purl, right.purl);
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function sha256(content) {
  return crypto.createHash("sha256").update(content).digest("hex");
}

function parseJson(text, label) {
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(`${label} is not valid JSON: ${error.message}`, {
      cause: error,
    });
  }
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
