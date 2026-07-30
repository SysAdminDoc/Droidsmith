import fs from "node:fs";
import path from "node:path";
import { argv, stdout } from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

if (path.resolve(argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  const policy = readJson(path.join(repoRoot, "target-lifecycle-policy.json"));
  const sources = readRendererSources(path.join(repoRoot, "src"));
  validateTargetLifecyclePolicy({
    policy,
    sources,
    tauriSource: sources["src/lib/tauri.ts"],
    operationSource: sources["src/lib/targetOperation.ts"],
  });
  stdout.write("Target lifecycle IPC inventory OK\n");
}

export function validateTargetLifecyclePolicy({
  policy,
  sources,
  tauriSource,
  operationSource,
}) {
  assert(
    policy?.schemaVersion === 1,
    "lifecycle policy schemaVersion must be 1",
  );
  assert(
    Array.isArray(policy.indirectTargetCalls),
    "lifecycle policy indirectTargetCalls must be an array",
  );
  assert(
    policy.callSites &&
      typeof policy.callSites === "object" &&
      !Array.isArray(policy.callSites),
    "lifecycle policy callSites must be an object",
  );

  const directTargetCalls = exportedTargetCalls(tauriSource);
  const sensitiveCalls = new Set([
    ...directTargetCalls,
    ...policy.indirectTargetCalls,
  ]);
  const actualCallSites = {};

  for (const [relativePath, source] of Object.entries(sources)) {
    if (
      relativePath === "src/lib/tauri.ts" ||
      relativePath.endsWith(".test.ts") ||
      relativePath.endsWith(".test.tsx")
    ) {
      continue;
    }
    const calls = [...sensitiveCalls]
      .filter((call) => new RegExp(`\\b${call}\\s*\\(`, "u").test(source))
      .sort();
    if (calls.length === 0) continue;
    actualCallSites[relativePath] = calls;
    assert(
      /\b(?:useTargetOperation|TargetOperationCoordinator)\b/u.test(source),
      `${relativePath} invokes target-sensitive IPC without the shared lifecycle contract`,
    );
  }

  const expectedCallSites = Object.fromEntries(
    Object.entries(policy.callSites)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([relativePath, calls]) => {
        assert(
          Array.isArray(calls) && calls.length > 0,
          `${relativePath} lifecycle inventory must be a non-empty array`,
        );
        return [relativePath, [...new Set(calls)].sort()];
      }),
  );
  const sortedActual = Object.fromEntries(
    Object.entries(actualCallSites).sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  );
  assertEqual(
    sortedActual,
    expectedCallSites,
    "target-sensitive IPC inventory drifted; review the call site and update target-lifecycle-policy.json",
  );

  for (const marker of [
    "target.serial",
    "target.transport_id",
    "target.connection_generation",
    "scopeKey",
    "registerCancellation",
    "callCancelOperation",
    "coordinator.invalidate()",
  ]) {
    assert(
      operationSource.includes(marker),
      `shared target lifecycle contract is missing ${marker}`,
    );
  }
}

function exportedTargetCalls(source) {
  const calls = [];
  const signature =
    /export async function (call[A-Za-z0-9]+)\s*\(([\s\S]*?)\)\s*:\s*Promise/gu;
  for (const match of source.matchAll(signature)) {
    if (/\bDeviceTarget\b/u.test(match[2])) calls.push(match[1]);
  }
  return calls;
}

function readRendererSources(root) {
  const sources = {};
  const visit = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(absolute);
      } else if (/\.(?:ts|tsx)$/u.test(entry.name)) {
        const relative = path
          .relative(repoRoot, absolute)
          .split(path.sep)
          .join("/");
        sources[relative] = fs.readFileSync(absolute, "utf8");
      }
    }
  };
  visit(root);
  return sources;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function assertEqual(actual, expected, message) {
  assert(
    JSON.stringify(actual) === JSON.stringify(expected),
    `${message}\nactual: ${JSON.stringify(actual)}\nexpected: ${JSON.stringify(expected)}`,
  );
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
