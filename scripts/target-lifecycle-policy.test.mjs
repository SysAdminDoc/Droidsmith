import assert from "node:assert/strict";
import test from "node:test";

import { validateTargetLifecyclePolicy } from "./check-target-lifecycle.mjs";

const tauriSource = `
export async function callTarget(target: DeviceTarget): Promise<void> {}
`;
const operationSource = `
target.serial
target.transport_id
target.connection_generation
scopeKey
registerCancellation
callCancelOperation
coordinator.invalidate()
`;
const guardedSource = `
useTargetOperation(target, userId);
callTarget(target);
`;
const policy = {
  schemaVersion: 1,
  indirectTargetCalls: [],
  callSites: { "src/routes/Test.tsx": ["callTarget"] },
};

test("accepts an exact guarded target-sensitive IPC inventory", () => {
  assert.doesNotThrow(() =>
    validateTargetLifecyclePolicy({
      policy,
      sources: {
        "src/lib/tauri.ts": tauriSource,
        "src/lib/targetOperation.ts": operationSource,
        "src/routes/Test.tsx": guardedSource,
      },
      tauriSource,
      operationSource,
    }),
  );
});

test("rejects target-sensitive IPC without a lifecycle coordinator", () => {
  assert.throws(
    () =>
      validateTargetLifecyclePolicy({
        policy,
        sources: {
          "src/lib/tauri.ts": tauriSource,
          "src/lib/targetOperation.ts": operationSource,
          "src/routes/Test.tsx": "callTarget(target);",
        },
        tauriSource,
        operationSource,
      }),
    /without the shared lifecycle contract/u,
  );
});

test("rejects newly added and removed target-sensitive call sites", () => {
  for (const sources of [
    {
      "src/lib/tauri.ts": tauriSource,
      "src/lib/targetOperation.ts": operationSource,
      "src/routes/Test.tsx": guardedSource,
      "src/routes/New.tsx": "useTargetOperation(target); callTarget(target);",
    },
    {
      "src/lib/tauri.ts": tauriSource,
      "src/lib/targetOperation.ts": operationSource,
    },
  ]) {
    assert.throws(
      () =>
        validateTargetLifecyclePolicy({
          policy,
          sources,
          tauriSource,
          operationSource,
        }),
      /inventory drifted/u,
    );
  }
});

test("rejects lifecycle contracts missing any target, user, cancel, or unmount marker", () => {
  for (const marker of [
    "target.serial",
    "target.transport_id",
    "target.connection_generation",
    "scopeKey",
    "registerCancellation",
    "callCancelOperation",
    "coordinator.invalidate()",
  ]) {
    assert.throws(
      () =>
        validateTargetLifecyclePolicy({
          policy,
          sources: {
            "src/lib/tauri.ts": tauriSource,
            "src/lib/targetOperation.ts": operationSource.replace(marker, ""),
            "src/routes/Test.tsx": guardedSource,
          },
          tauriSource,
          operationSource: operationSource.replace(marker, ""),
        }),
      new RegExp(marker.replace(/[().]/gu, "\\$&"), "u"),
    );
  }
});
