#!/usr/bin/env node
// Local Rust dependency gate: runs `cargo audit --deny warnings` against the
// Tauri crate so any new vulnerability, unmaintained, or unsound advisory fails
// locally. Narrowly-documented, unfixable transitive exceptions live in
// src-tauri/.cargo/audit.toml (auto-loaded because cwd is src-tauri).
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const srcTauri = join(root, "src-tauri");

const version = spawnSync("cargo", ["audit", "--version"], {
  encoding: "utf8",
});
if (version.status !== 0) {
  console.error(
    "cargo-audit is not installed. Install the gate with:\n" +
      "  cargo install cargo-audit --locked",
  );
  process.exit(1);
}

const result = spawnSync("cargo", ["audit", "--deny", "warnings"], {
  cwd: srcTauri,
  stdio: "inherit",
});

if (result.status !== 0) {
  console.error(
    "\nRust dependency gate failed. Fix the advisory, upgrade the crate, or " +
      "add a narrowly-documented exception to src-tauri/.cargo/audit.toml.",
  );
  process.exit(result.status ?? 1);
}

console.log("Rust dependency gate passed (cargo audit --deny warnings).");
