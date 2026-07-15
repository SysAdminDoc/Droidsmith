import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { argv, execPath, exit, pid, stderr, stdout } from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const committed = path.join(repoRoot, "src", "lib", "bindings.ts");
const write = argv.includes("--write");
const output = write
  ? committed
  : path.join(os.tmpdir(), `droidsmith-bindings-${pid}.ts`);

try {
  run("cargo", [
    "run",
    "--quiet",
    "--locked",
    "--manifest-path",
    path.join(repoRoot, "src-tauri", "Cargo.toml"),
    "--bin",
    "droidsmith-bindings",
    "--",
    output,
  ]);
  run(execPath, [
    path.join(repoRoot, "node_modules", "prettier", "bin", "prettier.cjs"),
    "--write",
    output,
  ]);
  if (write) {
    stdout.write("TypeScript IPC bindings regenerated\n");
  } else {
    const expected = fs.readFileSync(committed, "utf8");
    const actual = fs.readFileSync(output, "utf8");
    if (actual !== expected) {
      throw new Error(
        "Rust IPC contract changed; run `npm run bindings:generate`",
      );
    }
    stdout.write("Generated TypeScript IPC bindings are current\n");
  }
} catch (error) {
  stderr.write(`${error instanceof Error ? error.message : error}\n`);
  exit(1);
} finally {
  if (!write) fs.rmSync(output, { force: true });
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: "pipe",
  });
  if (result.status !== 0) {
    throw new Error(
      [result.stdout, result.stderr].filter(Boolean).join("\n").trim() ||
        `${command} failed with exit code ${result.status}`,
    );
  }
}
