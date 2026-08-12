import { useTranslation } from "react-i18next";

import { formatBackupSize } from "../appsBackup";
import { Button, RevealInFolderButton, StatePanel } from "../common";
import type { BackupNotice } from "./types";

/** Package export and legacy-backup progress, evidence, and result surface. */
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
