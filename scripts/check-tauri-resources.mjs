import fs from "node:fs";
import path from "node:path";
import { stdout } from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = path.join(repoRoot, "src-tauri");
const configPath = path.join(tauriDir, "tauri.conf.json");
const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
const resources = config.bundle?.resources;

if (!resources || Array.isArray(resources) || typeof resources !== "object") {
  throw new Error("tauri.conf.json bundle.resources must map source directories to bundle targets");
}

const entries = Object.entries(resources).map(([source, target]) => ({
  source,
  target: String(target).replaceAll("\\", "/"),
  resolvedSource: path.resolve(tauriDir, source),
}));

for (const expected of [
  { sourceDir: path.join(repoRoot, "packs"), target: "packs/" },
  { sourceDir: path.join(repoRoot, "quirks"), target: "quirks/" },
]) {
  const match = entries.find(
    (entry) =>
      entry.resolvedSource === expected.sourceDir &&
      entry.target === expected.target,
  );
  if (!match) {
    throw new Error(
      `Missing Tauri resource mapping for ${expected.sourceDir} -> ${expected.target}`,
    );
  }

  const yamlFiles = fs
    .readdirSync(expected.sourceDir)
    .filter((name) => /\.(ya?ml)$/i.test(name));
  if (yamlFiles.length === 0) {
    throw new Error(`${expected.sourceDir} must contain at least one YAML resource`);
  }
}

stdout.write("Tauri resource contract OK\n");
