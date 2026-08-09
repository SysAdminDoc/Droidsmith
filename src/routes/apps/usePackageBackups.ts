import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callBackupPackage,
  callCancelOperation,
  callExportPackageApks,
  callPreflightPackageBackup,
  callSelectHostPath,
  newOperationId,
  type Device,
  type DeviceTarget,
  type PackageBackupPreflight,
} from "../../lib/tauri";
import {
  targetFingerprint,
  useTargetOperation,
} from "../../lib/targetOperation";
import {
  canRunLegacyExport,
  packageExportDefaultFileName,
  packageExportDisplayState,
} from "../appsBackup";
import type { BackupNotice } from "./types";

type PackageBackupOptions = {
  target: DeviceTarget | null;
  device: Device | null;
  userId: number;
  usersReady: boolean;
};

/** Own the package export and legacy-backup workflow, including its
 * cancellation generation. AppsRoute only supplies device identity and
 * renders the extracted BackupStatePanel. */
export function usePackageBackups({
  target,
  device,
  userId,
  usersReady,
}: PackageBackupOptions) {
  const { t } = useTranslation();
  const [backupNotice, setBackupNotice] = useState<BackupNotice | null>(null);
  const activeBackupRef = useRef<string | null>(null);
  const backupGenerationRef = useRef(0);
  const backupOperation = useTargetOperation(target, `apps-backup:${userId}`);
  const targetIdentity = targetFingerprint(target);

  useEffect(() => {
    backupGenerationRef.current += 1;
    activeBackupRef.current = null;
    setBackupNotice(null);
    return () => {
      backupGenerationRef.current += 1;
      const operationId = activeBackupRef.current;
      activeBackupRef.current = null;
      if (operationId) void callCancelOperation(operationId);
    };
  }, [targetIdentity, userId]);

  const runPackageExport = useCallback(
    async (
      pkg: string,
      mode: "apk_export" | "legacy_data",
      inspected?: PackageBackupPreflight,
    ) => {
      if (!device || !target || !usersReady) return;
      // One export at a time: claiming a fresh generation while another export
      // runs would silently orphan its completion/failure handling and make it
      // uncancellable.
      if (activeBackupRef.current) return;
      const lease = backupOperation.begin();
      // Claim the generation before the first await so a device switch during
      // the preflight/save dialog invalidates this export before it starts.
      const generation = backupGenerationRef.current + 1;
      backupGenerationRef.current = generation;
      let startedOperationId: string | null = null;
      try {
        const preflight =
          inspected ?? (await callPreflightPackageBackup(target, pkg, userId));
        if (backupGenerationRef.current !== generation || !lease.isCurrent())
          return;
        if (mode === "legacy_data" && !canRunLegacyExport(preflight)) {
          setBackupNotice({
            title: t("apps.legacyBlockedTitle"),
            message: preflight.evidence.reason,
            tone: "warning",
            evidence: preflight.evidence,
            showLimitations: true,
          });
          return;
        }

        const pathGrant = await callSelectHostPath(
          mode === "apk_export" ? "package_export_save" : "backup_save",
          packageExportDefaultFileName(pkg, mode),
        );
        if (backupGenerationRef.current !== generation || !lease.isCurrent())
          return;
        if (!pathGrant) {
          setBackupNotice({
            title: t("apps.exportCancelledTitle"),
            message: t("apps.exportCancelled"),
            tone: "neutral",
          });
          return;
        }

        const operationId = newOperationId(
          mode === "apk_export" ? "package-export" : "legacy-backup",
        );
        startedOperationId = operationId;
        activeBackupRef.current = operationId;
        lease.registerCancellation(operationId);
        setBackupNotice({
          title:
            mode === "apk_export"
              ? t("apps.apkExportRunningTitle", { package: pkg })
              : t("apps.legacyRunningTitle", { package: pkg }),
          message:
            mode === "apk_export"
              ? t("apps.apkExportRunningBody", {
                  count: preflight.apk_paths.length,
                })
              : t("apps.legacyLimitations"),
          tone: "info",
          path: pathGrant.local_path,
          operationId,
          progress: t("apps.exportStarting"),
          evidence: preflight.evidence,
          showLimitations: mode === "legacy_data",
        });

        const result =
          mode === "apk_export"
            ? await callExportPackageApks(target, pkg, userId, pathGrant.id, {
                operationId,
                onEvent: (event) => {
                  if (
                    activeBackupRef.current !== operationId ||
                    backupGenerationRef.current !== generation ||
                    !lease.isCurrent()
                  )
                    return;
                  setBackupNotice((previous) => {
                    if (!previous || previous.operationId !== operationId)
                      return previous;
                    if (event.kind === "progress") {
                      return {
                        ...previous,
                        progress:
                          event.message ??
                          t("apps.exportProgress", {
                            seconds: Math.max(
                              1,
                              Math.round((event.elapsed_ms ?? 0) / 1000),
                            ),
                          }),
                      };
                    }
                    if (event.kind === "output" && event.chunk) {
                      return {
                        ...previous,
                        output: `${previous.output ?? ""}${event.chunk}`.slice(
                          -64 * 1024,
                        ),
                      };
                    }
                    return previous;
                  });
                },
              })
            : await callBackupPackage(target, pkg, userId, pathGrant.id, {
                operationId,
                onEvent: (event) => {
                  if (
                    activeBackupRef.current !== operationId ||
                    backupGenerationRef.current !== generation ||
                    !lease.isCurrent()
                  )
                    return;
                  setBackupNotice((previous) => {
                    if (!previous || previous.operationId !== operationId)
                      return previous;
                    if (event.kind === "progress") {
                      return {
                        ...previous,
                        progress:
                          event.message ??
                          t("apps.exportProgress", {
                            seconds: Math.max(
                              1,
                              Math.round((event.elapsed_ms ?? 0) / 1000),
                            ),
                          }),
                      };
                    }
                    if (event.kind === "output" && event.chunk) {
                      return {
                        ...previous,
                        output: `${previous.output ?? ""}${event.chunk}`.slice(
                          -64 * 1024,
                        ),
                      };
                    }
                    return previous;
                  });
                },
              });
        if (backupGenerationRef.current !== generation || !lease.isCurrent())
          return;
        activeBackupRef.current = null;
        const displayState = packageExportDisplayState(result);
        const titleByState: Record<typeof displayState, string> = {
          apk_exported: t("apps.apkExportSavedTitle"),
          legacy_entries_detected: t("apps.legacyInspectedTitle"),
          legacy_no_data: t("apps.legacyNoDataTitle"),
        };
        const messageByState: Record<typeof displayState, string> = {
          apk_exported: t("apps.apkExportSaved", {
            file: result.artifact.local_path,
            count: result.manifest.artifacts.length,
          }),
          legacy_entries_detected: t("apps.legacyEntriesDetected", {
            file: result.artifact.local_path,
          }),
          legacy_no_data: t("apps.legacyNoDataBody", {
            file: result.artifact.local_path,
          }),
        };
        setBackupNotice({
          title: titleByState[displayState],
          message: messageByState[displayState],
          tone: displayState === "apk_exported" ? "success" : "warning",
          path: result.artifact.local_path,
          sizeBytes: result.artifact.size_bytes,
          showLimitations: mode === "legacy_data",
          evidence: result.manifest.eligibility,
        });
      } catch (e) {
        if (backupGenerationRef.current !== generation || !lease.isCurrent())
          return;
        if (
          startedOperationId &&
          activeBackupRef.current !== startedOperationId
        )
          return;
        activeBackupRef.current = null;
        setBackupNotice({
          title: t("apps.exportFailedTitle"),
          message: t("apps.exportFailed", {
            message: errorMessage(e),
          }),
          tone: "danger",
        });
      }
    },
    [backupOperation, device, target, t, userId, usersReady],
  );

  const inspectLegacyExport = useCallback(
    async (pkg: string) => {
      if (!target || !usersReady) return;
      const lease = backupOperation.begin();
      try {
        const preflight = await callPreflightPackageBackup(target, pkg, userId);
        const runnable = canRunLegacyExport(preflight);
        lease.commit(() =>
          setBackupNotice({
            title: runnable
              ? t("apps.legacyReviewTitle")
              : t("apps.legacyBlockedTitle"),
            message: preflight.evidence.reason,
            tone:
              preflight.legacy_capability === "legacy_data_eligible"
                ? "info"
                : "warning",
            evidence: preflight.evidence,
            showLimitations: true,
            pendingLegacy: runnable ? { package: pkg, preflight } : undefined,
          }),
        );
      } catch (error) {
        lease.commit(() =>
          setBackupNotice({
            title: t("apps.exportFailedTitle"),
            message: t("apps.exportFailed", {
              message: errorMessage(error),
            }),
            tone: "danger",
          }),
        );
      }
    },
    [backupOperation, target, t, userId, usersReady],
  );

  const cancelBackup = useCallback(async () => {
    const operationId = activeBackupRef.current;
    if (!operationId) return;
    setBackupNotice((previous) =>
      previous ? { ...previous, progress: t("apps.backupCancelling") } : null,
    );
    await callCancelOperation(operationId);
  }, [t]);

  return {
    backupNotice,
    setBackupNotice,
    runPackageExport,
    inspectLegacyExport,
    cancelBackup,
  };
}
