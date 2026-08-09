import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import type { ActionKind } from "../../lib/tauri";
import { useFocusTrap } from "../../lib/useFocusTrap";
import { presentRecovery } from "./uninstallRecovery";
import type { ActionState } from "./types";
import { Badge, Button, Card, StatePanel } from "../common";
import type { UnarchiveOutlook } from "./unarchive";

/** Review, progress, and result surface for single and batch package actions.
 * Keeping this focus-trapped export/review surface outside AppsRoute leaves the
 * route responsible for state orchestration rather than dialog markup. */
export function ActionOverlay({
  state,
  archiveWarnings,
  onConfirm,
  onExportBaseline,
  exportingBaseline,
  baselineFeedback,
  onCancel,
  onDismiss,
}: {
  state: ActionState;
  archiveWarnings: { package: string; outlook: UnarchiveOutlook }[];
  onConfirm: () => void;
  onExportBaseline: () => void;
  exportingBaseline: boolean;
  baselineFeedback: string | null;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const confirming =
    state.kind === "confirming" || state.kind === "confirming_batch";
  const applying = state.kind === "applying" || state.kind === "applying_batch";
  const trapRef = useFocusTrap<HTMLDivElement>(confirming || applying);

  useEffect(() => {
    if (!confirming) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [confirming, onCancel]);

  useEffect(() => {
    if (applying) trapRef.current?.focus();
  }, [applying, trapRef]);

  if (state.kind === "idle") return null;

  if (state.kind === "confirming" || state.kind === "confirming_batch") {
    const plans = state.kind === "confirming" ? [state.plan] : state.plan.plans;
    const description = state.plan.description;
    const portableBaselineSupported = plans.every(
      (plan) =>
        ![
          "suspend",
          "unsuspend",
          "unstop",
          "hide",
          "unhide",
          "disable_until_used",
          "default_state",
          "suspend_quarantine",
          "archive",
          "request_unarchive",
        ].includes(plan.request.kind),
    );
    const tier = actionTier(plans[0]?.request.kind);
    const recovery =
      state.kind === "confirming" ? presentRecovery(state.recovery) : null;
    return (
      <div
        ref={trapRef}
        tabIndex={-1}
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 px-4 outline-none backdrop-blur-sm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        aria-describedby="confirm-dialog-description"
      >
        <Card surface="dialog" className="w-full max-w-lg p-6">
          <Badge tone="warning">{t("apps.reviewBeforeApplying")}</Badge>
          <h3
            id="confirm-dialog-title"
            className="mt-4 text-lg font-semibold text-anvil-50"
          >
            {t("apps.applyPackageAction")}
          </h3>
          <p
            id="confirm-dialog-description"
            className="mt-3 text-sm leading-6 text-anvil-200"
          >
            {description}
          </p>
          <div className="mt-3 rounded-md border border-circuit-300/25 bg-circuit-950/20 p-3">
            <p className="text-xs font-semibold text-circuit-100">
              {t(`apps.actionTier.${tier}.title`)}
            </p>
            <p className="mt-1 text-xs leading-5 text-anvil-200">
              {t(`apps.actionTier.${tier}.body`)}
            </p>
          </div>
          {recovery && (
            <div
              className="mt-3 rounded-md border border-white/10 bg-white/[0.04] p-3"
              data-testid="uninstall-recovery"
              data-recovery-tone={recovery.tone}
            >
              <Badge tone={recovery.tone}>{t(recovery.titleKey)}</Badge>
              <p className="mt-2 text-xs leading-5 text-anvil-200">
                {t(recovery.detailKey)}
              </p>
            </div>
          )}
          <div className="mt-3 max-h-56 space-y-2 overflow-y-auto rounded-md border border-white/10 bg-white/[0.04] p-3">
            <p className="text-xs font-medium text-anvil-400">
              {plans.length === 1
                ? t("apps.commandPreview")
                : t("apps.batchCommandPreview", { count: plans.length })}
            </p>
            {plans.map((plan) => (
              <code
                key={plan.incident_id}
                className="block break-all font-mono text-xs text-anvil-100"
              >
                adb -s {plan.request.serial} shell {plan.args.join(" ")}
              </code>
            ))}
          </div>
          {archiveWarnings.length > 0 && (
            <div
              className="mt-3 rounded-md border border-amber-300/30 bg-amber-950/20 p-3"
              role="alert"
            >
              <p className="text-xs font-semibold text-amber-200">
                {t("apps.archiveNotReversibleTitle")}
              </p>
              <p className="mt-1 text-xs text-amber-100/90">
                {t("apps.archiveNotReversibleBody")}
              </p>
              <ul className="mt-2 space-y-1">
                {archiveWarnings.map((entry) => (
                  <li
                    key={entry.package}
                    className="break-all font-mono text-xs text-amber-100"
                  >
                    {entry.package} —{" "}
                    <span className="font-sans">
                      {t(`apps.unarchiveOutlook.${entry.outlook}`)}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {baselineFeedback && (
            <p
              className="mt-3 text-xs leading-5 text-circuit-100"
              role="status"
            >
              {baselineFeedback}
            </p>
          )}
          <div className="mt-5 flex justify-end gap-3">
            {portableBaselineSupported && (
              <Button
                type="button"
                variant="ghost"
                onClick={onExportBaseline}
                disabled={exportingBaseline}
              >
                {exportingBaseline
                  ? t("apps.recoveryExporting")
                  : t("apps.recoveryExportBeforeApply")}
              </Button>
            )}
            <Button type="button" onClick={onCancel}>
              {t("apps.cancel")}
            </Button>
            <Button type="button" variant="primary" onClick={onConfirm}>
              {t("apps.applyChange")}
            </Button>
          </div>
        </Card>
      </div>
    );
  }

  if (state.kind === "applying" || state.kind === "applying_batch") {
    return (
      <div
        ref={trapRef}
        tabIndex={-1}
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 px-4 outline-none backdrop-blur-sm"
        role="dialog"
        aria-modal="true"
        aria-busy="true"
        aria-labelledby="applying-dialog-title"
        aria-describedby="applying-dialog-description"
      >
        <Card surface="dialog" className="w-full max-w-lg p-6">
          <h3
            id="applying-dialog-title"
            className="text-sm font-semibold text-anvil-50"
          >
            {t("apps.applyingChange")}
          </h3>
          <p
            id="applying-dialog-description"
            className="mt-2 text-xs text-anvil-400"
          >
            {state.plan.description}
          </p>
          <div className="mt-4 h-2 overflow-hidden rounded-sm bg-white/[0.08]">
            <div className="h-full w-2/3 animate-pulse rounded-sm bg-circuit-300" />
          </div>
        </Card>
      </div>
    );
  }

  if (state.kind === "success") {
    return (
      <StatePanel
        title={t("apps.actionCompleted")}
        tone="success"
        live="polite"
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        }
      >
        <p>{state.message}</p>
        {state.details && state.details.length > 0 && (
          <ul className="mt-3 list-disc space-y-1 ps-5 text-sm text-anvil-200">
            {state.details.map((detail) => (
              <li key={detail} className="break-words font-mono text-xs">
                {detail}
              </li>
            ))}
          </ul>
        )}
      </StatePanel>
    );
  }

  return (
    <StatePanel
      title={t("apps.actionFailed")}
      tone="danger"
      live="assertive"
      actions={
        <Button type="button" size="sm" variant="danger" onClick={onDismiss}>
          {t("common.dismiss")}
        </Button>
      }
    >
      <p>{state.message}</p>
    </StatePanel>
  );
}

function actionTier(kind: ActionKind | undefined): string {
  if (kind === "suspend" || kind === "unsuspend") return "suspend";
  if (kind === "disable" || kind === "enable") return "disable";
  if (kind === "archive" || kind === "request_unarchive") return "archive";
  if (kind === "uninstall_for_user" || kind === "clear_data") {
    return "destructive";
  }
  return "utility";
}
