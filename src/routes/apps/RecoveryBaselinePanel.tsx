import { useTranslation } from "react-i18next";

import {
  Badge,
  Button,
  Card,
  RevealInFolderButton,
  StatePanel,
  TableCell,
  TableHeaderCell,
} from "../common";
import type { RecoveryState } from "./types";

/** Recovery-baseline drift review + apply surface (IMP-67: extracted verbatim
 *  from the former Apps.tsx god-file). */
export function RecoveryBaselinePanel({
  state,
  onApply,
  onDismiss,
}: {
  state: RecoveryState;
  onApply: () => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  if (state.kind === "idle") return null;
  if (state.kind === "busy") {
    return (
      <StatePanel title={t("apps.recoveryWorking")} tone="info">
        <p>{state.message}</p>
      </StatePanel>
    );
  }
  if (state.kind === "saved") {
    return (
      <StatePanel
        title={t("apps.recoverySavedTitle")}
        tone="success"
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            {t("apps.recoveryDismiss")}
          </Button>
        }
      >
        <p>{t("apps.recoverySaved", { path: state.path })}</p>
        <code className="mt-2 block break-all font-mono text-xs">
          sha256 {state.sha256}
        </code>
        <div className="mt-3">
          <RevealInFolderButton path={state.path} />
        </div>
      </StatePanel>
    );
  }
  if (state.kind === "error") {
    return (
      <StatePanel
        title={t("apps.recoveryFailed")}
        tone="danger"
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            {t("apps.recoveryDismiss")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  const { diff } = state;
  const ready = diff.rows.filter((row) => row.status === "ready").length;
  const drifted = diff.rows.filter((row) => row.status === "drifted").length;
  const skipped = diff.rows.filter((row) => row.status === "skipped").length;
  const matching = diff.rows.filter(
    (row) => row.status === "already_matches",
  ).length;
  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-wrap items-start justify-between gap-3 border-b border-white/10 p-5">
        <div>
          <h3 className="font-semibold text-anvil-50">
            {t("apps.recoveryReviewTitle")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("apps.recoveryReviewBody", {
              user: diff.baseline.android_user,
              date: diff.baseline.exported_at,
            })}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Badge
            tone={
              diff.compatibility.device_identity_matches ? "success" : "danger"
            }
          >
            {diff.compatibility.device_identity_matches
              ? t("apps.recoveryDeviceMatch")
              : t("apps.recoveryDeviceMismatch")}
          </Badge>
          <Badge
            tone={
              diff.compatibility.build_fingerprint_matches
                ? "success"
                : "warning"
            }
          >
            {diff.compatibility.build_fingerprint_matches
              ? t("apps.recoveryBuildMatch")
              : t("apps.recoveryBuildChanged")}
          </Badge>
          <Badge tone="info">{t("apps.recoveryReady", { count: ready })}</Badge>
          {drifted > 0 && (
            <Badge tone="danger">
              {t("apps.recoveryDrifted", { count: drifted })}
            </Badge>
          )}
          <Badge tone="neutral">
            {t("apps.recoveryMatching", { count: matching })}
          </Badge>
          <Badge tone={skipped ? "warning" : "neutral"}>
            {t("apps.recoverySkipped", { count: skipped })}
          </Badge>
        </div>
      </div>
      <div className="max-h-72 overflow-auto">
        <table className="w-full text-start text-xs">
          <thead className="sticky top-0 bg-anvil-900">
            <tr>
              <TableHeaderCell>{t("apps.package")}</TableHeaderCell>
              <TableHeaderCell>
                {t("apps.recoveryBaselineState")}
              </TableHeaderCell>
              <TableHeaderCell>{t("apps.recoveryLiveState")}</TableHeaderCell>
              <TableHeaderCell>{t("apps.recoveryDecision")}</TableHeaderCell>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/10">
            {diff.rows.map((row) => (
              <tr key={row.package}>
                <TableCell>
                  <code className="font-mono">{row.package}</code>
                </TableCell>
                <TableCell>
                  {recoveryPackageState(row.baseline_enabled, t)}
                </TableCell>
                <TableCell>
                  {recoveryPackageState(row.live_enabled, t)}
                </TableCell>
                <TableCell>
                  <Badge
                    tone={
                      row.status === "ready"
                        ? "info"
                        : row.status === "drifted"
                          ? "danger"
                          : row.status === "skipped"
                            ? "warning"
                            : "success"
                    }
                  >
                    {t(`apps.recoveryStatus.${row.status}`)}
                  </Badge>
                  <p className="mt-1 text-anvil-400">{row.reason}</p>
                </TableCell>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="flex flex-wrap justify-end gap-2 border-t border-white/10 p-4">
        {state.kind === "result" && (
          <p className="me-auto text-xs text-anvil-300">
            {t("apps.recoveryResult", {
              applied: state.applied,
              failed: state.failures.length,
            })}
            {state.failures.length > 0 && ` ${state.failures.join("; ")}`}
          </p>
        )}
        <Button type="button" onClick={onDismiss}>
          {t("apps.recoveryDismiss")}
        </Button>
        {state.kind === "review" && (
          <Button
            type="button"
            variant="primary"
            onClick={onApply}
            disabled={diff.plans.length === 0}
          >
            {t("apps.recoveryApply", { count: diff.plans.length })}
          </Button>
        )}
      </div>
    </Card>
  );
}

function recoveryPackageState(
  enabled: boolean | null,
  t: (key: string) => string,
): string {
  if (enabled === null) return t("apps.recoveryAbsent");
  return enabled ? t("apps.recoveryEnabled") : t("apps.recoveryDisabled");
}
