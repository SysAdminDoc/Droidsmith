import { useEffect, useRef } from "react";

import { callCancelOperation, type DeviceTarget } from "./tauri";

export type CancelOperation = (operationId: string) => Promise<unknown>;

/** Stable identity for one observed transport generation. Authorization flags
 * are intentionally excluded: changing an acknowledgement does not turn the
 * underlying Android transport into a different target. */
export function targetFingerprint(target: DeviceTarget | null): string | null {
  if (!target) return null;
  return JSON.stringify([
    target.serial,
    target.transport_id,
    target.connection_generation,
    target.transport_kind ?? null,
    target.model,
    target.product,
    target.device,
    target.build_fingerprint,
  ]);
}

export class TargetOperationLease {
  private operationId: string | null = null;

  constructor(
    private readonly coordinator: TargetOperationCoordinator,
    readonly fingerprint: string | null,
    readonly generation: number,
  ) {}

  isCurrent(): boolean {
    return this.coordinator.isCurrent(this);
  }

  /** Register the backend operation that belongs to this lease. If target
   * invalidation won the race, immediately forward cancellation instead of
   * leaving an orphan subprocess. */
  registerCancellation(operationId: string): boolean {
    this.operationId = operationId;
    return this.coordinator.registerCancellation(this, operationId);
  }

  cancellationId(): string | null {
    return this.operationId;
  }

  commit(commit: () => void): boolean {
    if (!this.isCurrent()) return false;
    commit();
    return true;
  }

  finish(): boolean {
    return this.coordinator.finish(this);
  }

  requestCancellation(): Promise<unknown> | null {
    return this.coordinator.requestCancellation(this);
  }
}

/** One-operation coordinator shared by target-sensitive React workflows and
 * the module-level device lifecycle. Beginning a new lease supersedes the old
 * one; target changes invalidate synchronously through getFingerprint(). */
export class TargetOperationCoordinator {
  private generation = 0;
  private activeLease: TargetOperationLease | null = null;
  private activeOperationId: string | null = null;

  constructor(
    private readonly getFingerprint: () => string | null,
    private readonly cancelOperation: CancelOperation = callCancelOperation,
  ) {}

  begin(): TargetOperationLease {
    this.invalidate();
    const lease = new TargetOperationLease(
      this,
      this.getFingerprint(),
      this.generation,
    );
    this.activeLease = lease;
    return lease;
  }

  isCurrent(lease: TargetOperationLease): boolean {
    return (
      this.activeLease === lease &&
      lease.generation === this.generation &&
      lease.fingerprint === this.getFingerprint()
    );
  }

  registerCancellation(
    lease: TargetOperationLease,
    operationId: string,
  ): boolean {
    if (!this.isCurrent(lease)) {
      void this.cancelOperation(operationId);
      return false;
    }
    this.activeOperationId = operationId;
    return true;
  }

  finish(lease: TargetOperationLease): boolean {
    if (!this.isCurrent(lease)) return false;
    this.activeLease = null;
    this.activeOperationId = null;
    return true;
  }

  requestCancellation(lease: TargetOperationLease): Promise<unknown> | null {
    if (!this.isCurrent(lease)) return null;
    const operationId = this.activeOperationId ?? lease.cancellationId();
    return operationId ? this.cancelOperation(operationId) : null;
  }

  requestActiveCancellation(): Promise<unknown> | null {
    return this.activeLease ? this.requestCancellation(this.activeLease) : null;
  }

  hasActiveLease(): boolean {
    return this.activeLease?.isCurrent() ?? false;
  }

  invalidate(): Promise<unknown> | null {
    const operationId = this.activeOperationId;
    this.generation += 1;
    this.activeLease = null;
    this.activeOperationId = null;
    if (!operationId) return null;
    const cancellation = this.cancelOperation(operationId);
    void cancellation;
    return cancellation;
  }
}

/** Bind one coordinator to an immutable device target. The ref is updated
 * during render so a completion from the previous target is stale even before
 * React runs effect cleanup; cleanup owns backend cancellation. */
export function useTargetOperation(
  target: DeviceTarget | null,
  scopeKey: string | number | null = null,
) {
  const targetIdentity = targetFingerprint(target);
  const fingerprint =
    targetIdentity === null ? null : JSON.stringify([targetIdentity, scopeKey]);
  const fingerprintRef = useRef(fingerprint);
  fingerprintRef.current = fingerprint;
  const coordinatorRef = useRef<TargetOperationCoordinator | null>(null);
  if (!coordinatorRef.current) {
    coordinatorRef.current = new TargetOperationCoordinator(
      () => fingerprintRef.current,
    );
  }
  const coordinator = coordinatorRef.current;

  useEffect(
    () => () => {
      coordinator.invalidate();
    },
    [coordinator, fingerprint],
  );

  return coordinator;
}
