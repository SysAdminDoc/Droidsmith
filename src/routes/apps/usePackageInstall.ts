import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callCancelOperation,
  callGrantDroppedPath,
  callInstallApk,
  callSelectHostPath,
  errorMessage,
  inTauri,
  newOperationId,
  type Device,
  type DeviceTarget,
  type InstallOptions,
  type OperationEvent,
} from "../../lib/tauri";
import {
  targetFingerprint,
  useTargetOperation,
} from "../../lib/targetOperation";
import type { InstallState } from "./types";

type PackageInstallOptions = {
  target: DeviceTarget | null;
  device: Device | null;
  incremental: boolean;
  loadPackages: () => Promise<void>;
};

/** Own the APK picker, drag/drop listener, progress state, cancellation, and
 * reviewed install override. The route only wires controls to this workflow. */
export function usePackageInstall({
  target,
  device,
  incremental,
  loadPackages,
}: PackageInstallOptions) {
  const { t } = useTranslation();
  const [installState, setInstallState] = useState<InstallState>({
    kind: "idle",
  });
  const activeInstallRef = useRef<string | null>(null);
  const installGenerationRef = useRef(0);
  const installStateRef = useRef(installState);
  installStateRef.current = installState;
  const installOperation = useTargetOperation(target, "apps-install");
  const targetIdentity = targetFingerprint(target);

  useEffect(() => {
    installGenerationRef.current += 1;
    activeInstallRef.current = null;
    setInstallState({ kind: "idle" });
    return () => {
      installGenerationRef.current += 1;
      const operationId = activeInstallRef.current;
      activeInstallRef.current = null;
      if (operationId) void callCancelOperation(operationId);
    };
  }, [targetIdentity]);

  const runInstall = useCallback(
    async (
      pathGrant: string,
      localPath: string,
      installOptions: InstallOptions,
    ) => {
      if (!device || !target) return;
      const lease = installOperation.begin();
      const operationId = newOperationId("install");
      const generation = installGenerationRef.current + 1;
      installGenerationRef.current = generation;
      activeInstallRef.current = operationId;
      lease.registerCancellation(operationId);
      setInstallState({
        kind: "running",
        operationId,
        progress: t("apps.installStarting"),
        output: "",
      });
      try {
        const result = await callInstallApk(target, pathGrant, installOptions, {
          operationId,
          onEvent: (event: OperationEvent) => {
            if (
              activeInstallRef.current !== operationId ||
              installGenerationRef.current !== generation ||
              !lease.isCurrent()
            )
              return;
            setInstallState((previous) => {
              if (
                previous.kind !== "running" ||
                previous.operationId !== operationId
              )
                return previous;
              if (event.kind === "output" && event.chunk) {
                return {
                  ...previous,
                  output: `${previous.output}${event.chunk}`.slice(-64 * 1024),
                };
              }
              if (event.kind === "progress" && event.message) {
                return { ...previous, progress: event.message };
              }
              return previous;
            });
          },
        });
        if (installGenerationRef.current !== generation || !lease.isCurrent())
          return;
        activeInstallRef.current = null;
        setInstallState({ kind: "result", localPath, result });
        if (result.succeeded) await loadPackages();
      } catch (error) {
        if (
          activeInstallRef.current !== operationId ||
          installGenerationRef.current !== generation ||
          !lease.isCurrent()
        )
          return;
        activeInstallRef.current = null;
        setInstallState({
          kind: "error",
          message: errorMessage(error),
        });
      }
    },
    [device, installOperation, loadPackages, t, target],
  );

  const startInstall = useCallback(async () => {
    if (!device) return;
    const lease = installOperation.begin();
    setInstallState({ kind: "choosing" });
    try {
      const selected = await callSelectHostPath("install_open");
      if (!lease.isCurrent()) return;
      if (!selected) {
        setInstallState({ kind: "idle" });
        return;
      }
      await runInstall(selected.id, selected.local_path, {
        incremental,
      });
    } catch (error) {
      lease.commit(() =>
        setInstallState({
          kind: "error",
          message: errorMessage(error),
        }),
      );
    }
  }, [device, incremental, installOperation, runInstall]);

  useEffect(() => {
    if (!inTauri() || !device) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) => {
      if (cancelled) return;
      void getCurrentWebview()
        .onDragDropEvent(async (event) => {
          if (event.payload.type !== "drop" || cancelled) return;
          // The install button is disabled while an install runs; a drop must
          // not start a second concurrent install that silently orphans the
          // first (read via ref — the listener is not re-subscribed on state
          // changes).
          if (
            installStateRef.current.kind === "running" ||
            installStateRef.current.kind === "choosing"
          )
            return;
          const apkPaths = event.payload.paths.filter((path) => {
            const ext = path.split(".").pop()?.toLowerCase() ?? "";
            return ["apk", "apks", "xapk", "apkm"].includes(ext);
          });
          if (apkPaths.length === 0) return;
          const path = apkPaths[0];
          const lease = installOperation.begin();
          try {
            const grant = await callGrantDroppedPath(path);
            if (cancelled || !lease.isCurrent()) return;
            await runInstall(grant.id, grant.local_path, {
              incremental,
            });
          } catch (error) {
            if (cancelled || !lease.isCurrent()) return;
            setInstallState({
              kind: "error",
              message: errorMessage(error),
            });
          }
        })
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        });
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [device, incremental, installOperation, runInstall]);

  const cancelInstall = useCallback(async () => {
    const operationId = activeInstallRef.current;
    if (!operationId) return;
    setInstallState((previous) =>
      previous.kind === "running"
        ? { ...previous, progress: t("apps.installCancelling") }
        : previous,
    );
    await callCancelOperation(operationId);
  }, [t]);

  const confirmInstallOverride = useCallback(async () => {
    if (
      installState.kind !== "confirming_override" ||
      !installState.result.failure?.suggested_override
    )
      return;
    const installOptions: InstallOptions = {
      override_confirmed: true,
      allow_downgrade:
        installState.result.failure.suggested_override === "allow_downgrade",
      bypass_low_target_sdk_block:
        installState.result.failure.suggested_override ===
        "bypass_low_target_sdk_block",
    };
    const retryGrant = installState.result.retry_path_grant;
    if (!retryGrant) {
      setInstallState({
        kind: "error",
        message: t("apps.installGrantExpired"),
      });
      return;
    }
    await runInstall(retryGrant, installState.localPath, installOptions);
  }, [installState, runInstall, t]);

  return {
    installState,
    setInstallState,
    startInstall,
    cancelInstall,
    confirmInstallOverride,
  };
}
