import { describe, expect, it, vi } from "vitest";

import {
  TargetOperationCoordinator,
  targetFingerprint,
} from "./targetOperation";
import type { DeviceTarget } from "./tauri";

function target(serial: string, generation: number): DeviceTarget {
  return {
    serial,
    transport_id: serial === "A" ? 1 : 2,
    connection_generation: generation,
    transport_kind: "usb",
    untrusted_transport_override: false,
    model: "Pixel",
    product: "oriole",
    device: "oriole",
    build_fingerprint: `google/oriole/${serial}/${generation}`,
  };
}

describe("TargetOperationCoordinator", () => {
  it("rejects late A completions across rapid A to B to A switching", () => {
    let fingerprint = targetFingerprint(target("A", 1));
    const coordinator = new TargetOperationCoordinator(() => fingerprint);
    const firstA = coordinator.begin();

    fingerprint = targetFingerprint(target("B", 1));
    expect(firstA.isCurrent()).toBe(false);
    const b = coordinator.begin();

    fingerprint = targetFingerprint(target("A", 1));
    expect(b.isCurrent()).toBe(false);
    const secondA = coordinator.begin();
    const staleCommit = vi.fn();
    expect(firstA.commit(staleCommit)).toBe(false);
    expect(staleCommit).not.toHaveBeenCalled();
    expect(secondA.isCurrent()).toBe(true);
  });

  it("invalidates and cancels an operation on disconnect", () => {
    let fingerprint = targetFingerprint(target("A", 1));
    const cancel = vi.fn(async () => true);
    const coordinator = new TargetOperationCoordinator(
      () => fingerprint,
      cancel,
    );
    const lease = coordinator.begin();
    lease.registerCancellation("pull-12345678");

    fingerprint = null;
    coordinator.invalidate();
    expect(lease.isCurrent()).toBe(false);
    expect(cancel).toHaveBeenCalledWith("pull-12345678");
  });

  it("forwards cancellation when invalidation beats registration", () => {
    const cancel = vi.fn(async () => true);
    const coordinator = new TargetOperationCoordinator(
      () => "target-A",
      cancel,
    );
    const lease = coordinator.begin();
    coordinator.invalidate();

    expect(lease.registerCancellation("logcat-12345678")).toBe(false);
    expect(cancel).toHaveBeenCalledWith("logcat-12345678");
  });

  it("keeps late success and error commits from replacing current state", () => {
    const coordinator = new TargetOperationCoordinator(() => "target-A");
    const stale = coordinator.begin();
    const current = coordinator.begin();
    const success = vi.fn();
    const error = vi.fn();

    expect(stale.commit(success)).toBe(false);
    expect(stale.commit(error)).toBe(false);
    expect(current.commit(success)).toBe(true);
    expect(success).toHaveBeenCalledTimes(1);
    expect(error).not.toHaveBeenCalled();
  });
});
