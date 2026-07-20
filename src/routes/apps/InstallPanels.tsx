import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { type InstallPackageResult } from "../../lib/tauri";
import { useFocusTrap } from "../../lib/useFocusTrap";
import { formatBackupSize } from "../appsBackup";
import { Button, RevealInFolderButton, StatePanel } from "../common";
import type { BackupNotice, InstallState } from "./types";

/** APK install progress/result surface with SDK-override review entry point
 *  (IMP-67: extracted verbatim from the former Apps.tsx god-file). */
export function InstallStatePanel({
  state,
  onCancel,
  onDismiss,
  onReviewOverride,
}: {
  state: InstallState;
  onCancel: () => void;
  onDismiss: () => void;
  onReviewOverride: () => void;
}) {
  const { t } = useTranslation();
  if (
    state.kind === "idle" ||
    state.kind === "choosing" ||
    state.kind === "confirming_override"
  )
    return null;
  if (state.kind === "running") {
    return (
      <StatePanel
        title={t("apps.installRunning")}
        tone="info"
        actions={
          <Button type="button" size="sm" variant="danger" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
        }
      >
        <p>{state.progress}</p>
        {state.output.trim() && (
          <pre className="mt-3 max-h-40 overflow-auto rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
            {state.output.trim()}
          </pre>
        )}
      </StatePanel>
    );
  }
  if (state.kind === "error") {
    return (
      <StatePanel
        title={t("apps.installFailed")}
        tone="danger"
        actions={
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  const { result } = state;
  const failure = result.failure;
  return (
    <StatePanel
      title={
        result.succeeded ? t("apps.installSucceeded") : t("apps.installFailed")
      }
      tone={result.succeeded ? "success" : "danger"}
      actions={
        <div className="flex flex-wrap gap-2">
          {failure?.suggested_override && (
            <Button
              type="button"
              size="sm"
              variant="danger"
              onClick={onReviewOverride}
            >
              {t("apps.installReviewOverride")}
            </Button>
          )}
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        </div>
      }
    >
      <p>
        {result.succeeded
          ? t("apps.installSucceededBody", {
              count: result.file_count,
              size: formatBackupSize(result.total_bytes),
            })
          : failure?.cause}
      </p>
      {result.succeeded && result.install_mode === "incremental" && (
        <p className="mt-2 text-xs text-circuit-200/80">
          {t("apps.installModeIncremental")}
        </p>
      )}
      {result.succeeded &&
        result.install_mode === "incremental_unsupported" && (
          <p className="mt-2 text-xs text-amber-200/80">
            {t("apps.installModeIncrementalUnsupported")}
          </p>
        )}
      {!result.succeeded && failure && (
        <>
          <dl className="mt-3 grid gap-2 text-xs sm:grid-cols-[7rem_minmax(0,1fr)]">
            <dt className="font-medium text-anvil-400">
              {t("apps.installCode")}
            </dt>
            <dd className="break-words font-mono text-anvil-100">
              {failure.code}
            </dd>
            <dt className="font-medium text-anvil-400">
              {t("apps.installRemedy")}
            </dt>
            <dd className="text-anvil-100">{failure.remedy}</dd>
          </dl>
          <p className="mt-3 text-xs text-anvil-400">
            {t("apps.installNoAutomaticOverride")}
          </p>
          <pre className="mt-3 max-h-48 overflow-auto whitespace-pre-wrap rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
            {failure.raw_output}
          </pre>
        </>
      )}
      <p className="mt-3 text-xs text-anvil-500">
        {t("apps.installAudit", { id: result.audit_id })}
      </p>
    </StatePanel>
  );
}

/** SDK-override confirmation dialog for a rejected APK install (IMP-67:
 *  extracted verbatim from the former Apps.tsx god-file). */
export function InstallOverrideDialog({
  result,
  onCancel,
  onConfirm,
}: {
  result: InstallPackageResult;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const trapRef = useFocusTrap<HTMLDivElement>();
  const override = result.failure?.suggested_override;
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);
  if (!override || !result.failure) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 p-4 backdrop-blur-sm">
      <div
        ref={trapRef}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="install-override-title"
        aria-describedby="install-override-description"
        tabIndex={-1}
        className="w-full max-w-xl rounded-lg border border-red-300/20 bg-anvil-950 p-5 shadow-2xl outline-none"
      >
        <h3
          id="install-override-title"
          className="text-lg font-semibold text-anvil-50"
        >
          {t("apps.installOverrideTitle")}
        </h3>
        <p
          id="install-override-description"
          className="mt-2 text-sm leading-6 text-anvil-300"
        >
          {override === "allow_downgrade"
            ? t("apps.installDowngradeWarning")
            : t("apps.installLowTargetWarning")}
        </p>
        <div className="mt-4 rounded-md border border-red-300/20 bg-red-300/10 p-3 text-sm text-red-100">
          <p>{result.failure.cause}</p>
          <p className="mt-2">{result.failure.remedy}</p>
        </div>
        <p className="mt-3 text-xs leading-5 text-anvil-400">
          {t("apps.installOverrideAudit")}
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button type="button" variant="danger" onClick={onConfirm}>
            {t("apps.installConfirmOverride")}
          </Button>
        </div>
      </div>
    </div>
  );
}

/** Backup/legacy-export progress + evidence surface (IMP-67: extracted
 *  verbatim from the former Apps.tsx god-file). */
export function BackupStatePanel({
  notice,
  onDismiss,
  onCancel,
  onContinueLegacy,
}: {
  notice: BackupNotice;
  onDismiss: () => void;
  onCancel: () => void;
  onContinueLegacy: (
    pending: NonNullable<BackupNotice["pendingLegacy"]>,
  ) => void;
}) {
  const { t } = useTranslation();
  const formattedSize =
    notice.sizeBytes === undefined
      ? undefined
      : formatBackupSize(notice.sizeBytes);

  return (
    <StatePanel
      title={notice.title}
      tone={notice.tone}
      actions={
        notice.operationId ? (
          <Button type="button" size="sm" variant="danger" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
        ) : notice.pendingLegacy ? (
          <div className="flex flex-wrap gap-2">
            <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="primary"
              onClick={() => onContinueLegacy(notice.pendingLegacy!)}
            >
              {t("apps.continueLegacyExport")}
            </Button>
          </div>
        ) : (
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        )
      }
    >
      <p>{notice.message}</p>
      {notice.progress && (
        <p className="mt-2 text-xs font-medium text-circuit-200">
          {notice.progress}
        </p>
      )}
      {notice.showLimitations && (
        <p className="mt-2 text-xs leading-5 text-anvil-400">
          {t("apps.legacyLimitations")}
        </p>
      )}
      {notice.evidence && (
        <dl className="mt-3 grid gap-2 text-xs sm:grid-cols-[8rem_minmax(0,1fr)]">
          <dt className="font-medium text-anvil-400">{t("apps.deviceApi")}</dt>
          <dd className="font-mono text-anvil-100">
            {notice.evidence.device_sdk ?? t("common.notReported")}
          </dd>
          <dt className="font-medium text-anvil-400">{t("apps.targetApi")}</dt>
          <dd className="font-mono text-anvil-100">
            {notice.evidence.target_sdk ?? t("common.notReported")}
          </dd>
          <dt className="font-medium text-anvil-400">
            {t("apps.allowBackup")}
          </dt>
          <dd className="font-mono text-anvil-100">
            {notice.evidence.allow_backup === null
              ? t("common.notReported")
              : String(notice.evidence.allow_backup)}
          </dd>
          <dt className="font-medium text-anvil-400">{t("apps.debuggable")}</dt>
          <dd className="font-mono text-anvil-100">
            {notice.evidence.debuggable === null
              ? t("common.notReported")
              : String(notice.evidence.debuggable)}
          </dd>
        </dl>
      )}
      {(notice.path || formattedSize !== undefined) && (
        <dl className="mt-3 grid gap-2 text-xs sm:grid-cols-[8rem_minmax(0,1fr)]">
          {notice.path && (
            <>
              <dt className="font-medium text-anvil-400">
                {t("apps.backupPath")}
              </dt>
              <dd className="min-w-0 break-words font-mono text-anvil-100">
                {notice.path}
              </dd>
            </>
          )}
          {formattedSize !== undefined && (
            <>
              <dt className="font-medium text-anvil-400">
                {t("apps.backupSize")}
              </dt>
              <dd className="font-mono text-anvil-100">
                {formattedSize ?? t("common.notReported")}
              </dd>
            </>
          )}
        </dl>
      )}
      {notice.output !== undefined && (
        <div className="mt-4">
          <p className="text-xs font-medium text-anvil-400">
            {t("apps.backupOutput")}
          </p>
          <pre className="mt-2 max-h-48 overflow-auto rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
            {notice.output.trim() || t("apps.backupNoOutput")}
          </pre>
        </div>
      )}
      {notice.path && !notice.operationId && (
        <div className="mt-3">
          <RevealInFolderButton path={notice.path} />
        </div>
      )}
    </StatePanel>
  );
}
