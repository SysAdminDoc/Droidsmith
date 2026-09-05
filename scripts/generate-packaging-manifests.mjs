#!/usr/bin/env node
// Render the Scoop manifest from the repo's version and Tauri bundle metadata.
// The installer hash remains a placeholder until the release artifact is built.
// The builder is pure so tests can validate its shape without touching disk.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { argv, exit, stderr, stdout } from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

// Placeholder installer hash; a real release rewrites this with the artifact
// SHA-256. 64 hex chars keeps it schema-valid in the meantime.
export const PLACEHOLDER_SHA256 = "0".repeat(64);

/** Read the version and bundle metadata the manifest is rendered from. */
export function readReleaseMeta(root = repoRoot) {
  const pkg = JSON.parse(readFileSync(path.join(root, "package.json"), "utf8"));
  const tauri = JSON.parse(
    readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"),
  );
  return {
    version: pkg.version,
    productName: tauri.productName ?? "Droidsmith",
    publisher: tauri.bundle?.publisher ?? "Droidsmith contributors",
    homepage: pkg.homepage ?? "https://github.com/SysAdminDoc/Droidsmith",
    license: pkg.license ?? "MIT",
    description:
      pkg.description ??
      "Local Android device workshop for apps, diagnostics, and maintenance",
  };
}

function nsisInstallerUrl(version) {
  return `https://github.com/SysAdminDoc/Droidsmith/releases/download/v${version}/Droidsmith_${version}_x64-setup.exe`;
}

/** Build the Scoop manifest object. */
export function buildScoopManifest(meta) {
  return {
    version: meta.version,
    description: meta.description,
    homepage: meta.homepage,
    license: meta.license,
    architecture: {
      "64bit": {
        url: nsisInstallerUrl(meta.version),
        hash: PLACEHOLDER_SHA256,
      },
    },
    innosetup: false,
    bin: "droidsmith.exe",
    checkver: "github",
    autoupdate: {
      architecture: {
        "64bit": {
          url: "https://github.com/SysAdminDoc/Droidsmith/releases/download/v$version/Droidsmith_$version_x64-setup.exe",
        },
      },
    },
  };
}

const HEX64 = /^[0-9a-f]{64}$/u;

/** Validate the rendered manifest against the expected version and shape. */
export function validateManifest(scoop, expectedVersion) {
  const problems = [];
  const need = (condition, message) => {
    if (!condition) problems.push(message);
  };

  need(
    scoop.version === expectedVersion,
    "scoop version must match package.json",
  );
  need(
    typeof scoop.bin === "string" && scoop.bin.length > 0,
    "scoop bin is required",
  );
  const arch = scoop.architecture?.["64bit"] ?? {};
  need(
    typeof arch.url === "string" && arch.url.includes(expectedVersion),
    "scoop 64bit url must reference the version",
  );
  need(HEX64.test(arch.hash ?? ""), "scoop 64bit hash must be 64 hex chars");

  return problems;
}

function main() {
  const meta = readReleaseMeta();
  const scoop = buildScoopManifest(meta);
  const problems = validateManifest(scoop, meta.version);
  if (problems.length > 0) {
    stderr.write(`Manifest validation failed:\n- ${problems.join("\n- ")}\n`);
    exit(1);
  }
  const scoopDir = path.join(repoRoot, "packaging", "scoop");
  mkdirSync(scoopDir, { recursive: true });
  writeFileSync(
    path.join(scoopDir, "droidsmith.json"),
    `${JSON.stringify(scoop, null, 2)}\n`,
  );
  stdout.write(
    `Wrote Scoop manifest for v${meta.version} (placeholder installer hash)\n`,
  );
}

if (
  import.meta.url === `file://${argv[1]}` ||
  argv[1]?.endsWith("generate-packaging-manifests.mjs")
) {
  main();
}
