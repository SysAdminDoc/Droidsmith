import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callApplyAction,
  callExportRecoveryBaseline,
  callInspectRecoveryBaseline,
  callSelectHostPath,
  type BaselineRoundTrip,
  type DeviceTarget,
} from "../../lib/tauri";
import {
  targetFingerprint,
  useTargetOperation,
} from "../../lib/targetOperation";
import type { ActionState, RecoveryState } from "./types";

type RecoveryBaselineOptions = {
  target: DeviceTarget | null;
  userId: number;
  actionState: ActionState;
  loadPackages: () => Promise<void>;
  loadJournal: () => Promise<void>;
};

/** Own the recovery-baseline export and OTA restore/re-apply workflow.
 * AppsRoute supplies the current target and action review, while this hook
 * keeps native file grants, target leases, and recovery feedback together. */
export function useRecoveryBaseline({
  target,
  userId,
  actionState,
  loadPackages,
  loadJournal,
}: RecoveryBaselineOptions) {
  const { t } = useTranslation();
  const [recoveryState, setRecoveryState] = useState<RecoveryState>({
    kind: "idle",
  });
  const recoveryOperation = useTargetOperation(
    target,
    `apps-recovery:${userId}`,
  );
  const targetIdentity = targetFingerprint(target);

  useEffect(() => {
    setRecoveryState({ kind: "idle" });
  }, [targetIdentity, userId]);

  const resetRecovery = useCallback(() => {
    setRecoveryState({ kind: "idle" });
  }, []);

  const exportActionBaseline = useCallback(async () => {
    if (
      (actionState.kind !== "confirming" &&
        actionState.kind !== "confirming_batch") ||
      !target
    )
      return;
    const plans =
      actionState.kind === "confirming"
        ? [actionState.plan]
        : actionState.plan.plans;
    const first = plans[0];
    if (!first) return;
    const lease = recoveryOperation.begin();
    setRecoveryState({
      kind: "busy",
      message: t("apps.recoveryExporting"),
    });
    try {
      const selected = await callSelectHostPath(
        "recovery_baseline_save",
        recoveryFileName(plans.length, first.request.package),
      );
      if (!lease.isCurrent()) return;
      if (!selected) {
        setRecoveryState({ kind: "idle" });
        return;
      }
      const artifact = await callExportRecoveryBaseline(
        target,
        first.request.user_id,
        plans.map((plan) => ({
          package: plan.request.package,
          kind: plan.request.kind,
        })),
        first.request.pack_context
          ? {
              id: first.request.pack_context.pack_id,
              revision: first.request.pack_context.revision,
            }
          : null,
        selected.id,
      );
      lease.commit(() =>
        setRecoveryState({
          kind: "saved",
          path: artifact.local_path,
          sha256: artifact.sha256,
        }),
      );
    } catch (error) {
      lease.commit(() =>
        setRecoveryState({
          kind: "error",
          message: errorMessage(error),
        }),
      );
    }
  }, [actionState, recoveryOperation, t, target]);

  /** Open a saved baseline and plan one half of the OTA round trip.
   *
   * The direction is chosen up front rather than switched inside the review,
   * because the two halves happen at different times — restore, update,
   * re-apply — and because the native read grant is one-shot. */
  const inspectRecoveryBaseline = useCallback(
    async (roundTrip: BaselineRoundTrip) => {
      if (!target) return;
      const lease = recoveryOperation.begin();
      setRecoveryState({
        kind: "busy",
        message: t("apps.recoveryInspecting"),
      });
      try {
        const selected = await callSelectHostPath("recovery_baseline_open");
        if (!lease.isCurrent()) return;
        if (!selected) {
          setRecoveryState({ kind: "idle" });
          return;
        }
        const diff = await callInspectRecoveryBaseline(
          target,
          selected.id,
          roundTrip,
        );
        lease.commit(() => setRecoveryState({ kind: "review", diff }));
      } catch (error) {
        lease.commit(() =>
          setRecoveryState({
            kind: "error",
            message: errorMessage(error),
          }),
        );
      }
    },
    [recoveryOperation, t, target],
  );

  const applyRecoveryBaseline = useCallback(async () => {
    if (recoveryState.kind !== "review") return;
    const { diff } = recoveryState;
    const lease = recoveryOperation.begin();
    setRecoveryState({
      kind: "busy",
      message: t("apps.recoveryApplying", { count: diff.plans.length }),
    });
    let applied = 0;
    const failures: string[] = [];
    for (const plan of diff.plans) {
      if (!lease.isCurrent()) return;
      try {
        await callApplyAction(plan);
        if (!lease.isCurrent()) return;
        applied += 1;
      } catch (error) {
        if (!lease.isCurrent()) return;
        failures.push(`${plan.request.package}: ${errorMessage(error)}`);
      }
    }
    if (
      !lease.commit(() =>
        setRecoveryState({ kind: "result", diff, applied, failures }),
      )
    )
      return;
    await Promise.all([loadPackages(), loadJournal()]);
  }, [loadJournal, loadPackages, recoveryOperation, recoveryState, t]);

  return {
    recoveryState,
    exportActionBaseline,
    inspectRecoveryBaseline,
    applyRecoveryBaseline,
    resetRecovery,
  };
}

function recoveryFileName(count: number, packageName: string): string {
  const date = new Date().toISOString().slice(0, 10);
  if (count > 1) {
    return `droidsmith-recovery-${date}-batch-${count}-packages.json`;
  }
  const safePackage = packageName.replace(/[^A-Za-z0-9_.-]/g, "_");
  return `droidsmith-recovery-${date}-${safePackage}.json`;
}
