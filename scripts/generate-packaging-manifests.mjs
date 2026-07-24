#!/usr/bin/env node
// R-115: render winget (singleton) and Scoop manifests from the repo's version
// and Tauri bundle metadata. The InstallerUrl/InstallerSha256 are placeholders
// until a tagged GitHub release provides real artifact URLs and hashes — public
// submission stays tracked in Roadmap_Blocked.md. The builders are pure so the
// packaging test can validate their shape without touching the filesystem.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

export const PACKAGE_IDENTIFIER = "SysAdminDoc.Droidsmith";
export const WINGET_MANIFEST_VERSION = "1.6.0";
// Placeholder installer hash; a real release rewrites this with the artifact
// SHA-256. 64 hex chars keeps it schema-valid in the meantime.
export const PLACEHOLDER_SHA256 = "0".repeat(64);

/** Read the version + bundle metadata the manifests are rendered from. */
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
      "Cross-platform open-source GUI for managing Android devices over ADB",
  };
}

function nsisInstallerUrl(version) {
  return `https://github.com/SysAdminDoc/Droidsmith/releases/download/v${version}/Droidsmith_${version}_x64-setup.exe`;
}

/** Build the winget singleton manifest object. */
export function buildWingetManifest(meta) {
  return {
    PackageIdentifier: PACKAGE_IDENTIFIER,
    PackageVersion: meta.version,
    PackageName: meta.productName,
    Publisher: meta.publisher,
    License: meta.license,
    ShortDescription: meta.description,
    PackageUrl: meta.homepage,
    Installers: [
      {
        Architecture: "x64",
        InstallerType: "nullsoft",
        InstallerUrl: nsisInstallerUrl(meta.version),
        InstallerSha256: PLACEHOLDER_SHA256,
      },
    ],
    ManifestType: "singleton",
    ManifestVersion: WINGET_MANIFEST_VERSION,
  };
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
          url: `https://github.com/SysAdminDoc/Droidsmith/releases/download/v$version/Droidsmith_$version_x64-setup.exe`,
        },
      },
    },
  };
}

const HEX64 = /^[0-9a-f]{64}$/u;

/**
 * Validate the rendered manifests against the expected version and structural
 * schema. Returns an array of problem strings (empty when valid).
 */
export function validateManifests(winget, scoop, expectedVersion) {
  const problems = [];
  const need = (cond, message) => {
    if (!cond) problems.push(message);
  };

  need(
    winget.PackageIdentifier === PACKAGE_IDENTIFIER,
    "winget PackageIdentifier mismatch",
  );
  need(
    winget.PackageVersion === expectedVersion,
    "winget PackageVersion must match package.json",
  );
  need(
    winget.ManifestType === "singleton",
    "winget ManifestType must be singleton",
  );
  need(
    winget.ManifestVersion === WINGET_MANIFEST_VERSION,
    "winget ManifestVersion mismatch",
  );
  need(
    Array.isArray(winget.Installers) && winget.Installers.length === 1,
    "winget needs exactly one installer",
  );
  const installer = winget.Installers?.[0] ?? {};
  need(
    installer.Architecture === "x64",
    "winget installer architecture must be x64",
  );
  need(
    typeof installer.InstallerUrl === "string" &&
      installer.InstallerUrl.includes(expectedVersion),
    "winget InstallerUrl must reference the version",
  );
  need(
    HEX64.test(installer.InstallerSha256 ?? ""),
    "winget InstallerSha256 must be 64 hex chars",
  );

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

function toWingetYaml(manifest) {
  const lines = [
    "# yaml-language-server: $schema=https://aka.ms/winget-manifest.singleton.1.6.0.schema.json",
    `PackageIdentifier: ${manifest.PackageIdentifier}`,
    `PackageVersion: ${manifest.PackageVersion}`,
    `PackageName: ${manifest.PackageName}`,
    `Publisher: ${manifest.Publisher}`,
    `License: ${manifest.License}`,
    `ShortDescription: ${manifest.ShortDescription}`,
    `PackageUrl: ${manifest.PackageUrl}`,
    "Installers:",
    `  - Architecture: ${manifest.Installers[0].Architecture}`,
    `    InstallerType: ${manifest.Installers[0].InstallerType}`,
    `    InstallerUrl: ${manifest.Installers[0].InstallerUrl}`,
    `    InstallerSha256: ${manifest.Installers[0].InstallerSha256}`,
    `ManifestType: ${manifest.ManifestType}`,
    `ManifestVersion: ${manifest.ManifestVersion}`,
    "",
  ];
  return lines.join("\n");
}

function main() {
  const meta = readReleaseMeta();
  const winget = buildWingetManifest(meta);
  const scoop = buildScoopManifest(meta);
  const problems = validateManifests(winget, scoop, meta.version);
  if (problems.length > 0) {
    process.stderr.write(
      `Manifest validation failed:\n- ${problems.join("\n- ")}\n`,
    );
    process.exit(1);
  }
  const wingetDir = path.join(repoRoot, "packaging", "winget");
  const scoopDir = path.join(repoRoot, "packaging", "scoop");
  mkdirSync(wingetDir, { recursive: true });
  mkdirSync(scoopDir, { recursive: true });
  writeFileSync(
    path.join(wingetDir, `${PACKAGE_IDENTIFIER}.yaml`),
    toWingetYaml(winget),
  );
  writeFileSync(
    path.join(scoopDir, "droidsmith.json"),
    `${JSON.stringify(scoop, null, 2)}\n`,
  );
  process.stdout.write(
    `Wrote winget + Scoop manifests for v${meta.version} (placeholder installer hashes)\n`,
  );
}

if (
  import.meta.url === `file://${process.argv[1]}` ||
  process.argv[1]?.endsWith("generate-packaging-manifests.mjs")
) {
  main();
}
