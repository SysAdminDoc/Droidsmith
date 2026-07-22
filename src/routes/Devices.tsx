import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callCancelOperation,
  callGetDeviceInfo,
  callRecoverAdb,
  deviceTarget,
  newOperationId,
  type Device,
  type DeviceTarget,
} from "../lib/tauri";
import { restartDeviceLifecycle, useDeviceStore } from "../lib/deviceStore";

import { Button, PaneHeader, StatePanel } from "./common";
import HostDoctor from "./HostDoctor";
import { DeviceControls } from "./devices/DeviceControls";
import {
  formatStateLabel,
  type DetailState,
  type RecoveryState,
} from "./devices/common";
import { DeviceHeaderActions } from "./devices/DeviceHeaderActions";
import { AdbHealthPanel } from "./devices/AdbHealthPanel";
import { RecoveryDialog } from "./devices/RecoveryDialog";
import {
  DeviceTable,
  DeviceTableSkeleton,
  DeviceToolbar,
} from "./devices/DeviceTable";
import { DeviceDetail } from "./devices/DeviceDetail";
import { AuthorizePrompt } from "./devices/AuthorizePrompt";

export default function DevicesRoute() {
  const { t } = useTranslation();
  const state = useDeviceStore((store) => store.devicesState);
  const health = useDeviceStore((store) => store.health);
  const observedAt = useDeviceStore((store) => store.observedAt);
  const watching = useDeviceStore((store) => store.watching);
  const [detail, setDetail] = useState<DetailState>({ kind: "idle" });
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const [recovery, setRecovery] = useState<RecoveryState>({ kind: "idle" });
  const recoveryOperationRef = useRef<string | null>(null);
  const recoveryGenerationRef = useRef(0);
  // Click device A then B: whichever callGetDeviceInfo resolves last must not
  // win the detail panel; only the latest selection may write it.
  const selectGenerationRef = useRef(0);

  const refresh = useCallback(async () => {
    await restartDeviceLifecycle();
  }, []);

  const selectDevice = useCallback(async (device: Device) => {
    const generation = selectGenerationRef.current + 1;
    selectGenerationRef.current = generation;
    const target = deviceTarget(device);
    setDetail({ kind: "loading", target });
    try {
      const info = await callGetDeviceInfo(target);
      if (selectGenerationRef.current !== generation) return;
      setDetail({ kind: "ok", info, target });
    } catch (e) {
      if (selectGenerationRef.current !== generation) return;
      setDetail({
        kind: "error",
        target,
        message: errorMessage(e),
      });
    }
  }, []);

  useEffect(() => {
    if (detail.kind !== "idle" || state.kind !== "ok") return;
    const authorized = state.value.devices.filter(
      (device) => typeof device.state === "string" && device.state === "device",
    );
    if (authorized.length === 1) void selectDevice(authorized[0]!);
  }, [detail.kind, selectDevice, state]);

  const runRecovery = useCallback(async () => {
    const operationId = newOperationId("adb-recovery");
    const generation = recoveryGenerationRef.current + 1;
    recoveryGenerationRef.current = generation;
    recoveryOperationRef.current = operationId;
    setRecovery({ kind: "running", status: t("devices.health.starting") });
    try {
      const result = await callRecoverAdb(true, {
        operationId,
        onEvent: (event) => {
          if (
            recoveryGenerationRef.current !== generation ||
            recoveryOperationRef.current !== operationId
          )
            return;
          if (event.message) {
            setRecovery({ kind: "running", status: event.message });
          }
        },
      });
      if (recoveryGenerationRef.current !== generation) return;
      setRecovery({ kind: "done", result });
      await restartDeviceLifecycle();
    } catch (error) {
      if (recoveryGenerationRef.current !== generation) return;
      setRecovery({
        kind: "error",
        message: errorMessage(error),
      });
    } finally {
      if (recoveryGenerationRef.current === generation) {
        recoveryOperationRef.current = null;
      }
    }
  }, [t]);

  const cancelRecovery = useCallback(() => {
    const operationId = recoveryOperationRef.current;
    if (!operationId) return;
    setRecovery({ kind: "running", status: t("devices.health.cancelling") });
    void callCancelOperation(operationId);
  }, [t]);

  useEffect(() => {
    return () => {
      recoveryGenerationRef.current += 1;
      const operationId = recoveryOperationRef.current;
      recoveryOperationRef.current = null;
      if (operationId) void callCancelOperation(operationId);
    };
  }, []);

  useEffect(() => {
    if (detail.kind === "idle" || state.kind !== "ok") return;
    const stillConnected = state.value.devices.some(
      (device) =>
        device.transport_id === detail.target.transport_id &&
        device.connection_generation === detail.target.connection_generation,
    );
    if (!stillConnected) setDetail({ kind: "idle" });
  }, [detail, state]);

  return (
    <>
      <PaneHeader
        title={t("devices.title")}
        milestone="R-012"
        description={t("devices.description")}
        actions={
          <DeviceHeaderActions
            refreshing={state.kind === "loading"}
            onRefresh={() => void refresh()}
            onReviewRecovery={() => {
              setRecovery({ kind: "idle" });
              setRecoveryOpen(true);
            }}
          />
        }
      />

      {state.kind === "ok" &&
        state.value.adb_resolved &&
        state.value.devices.length > 0 && (
          <DeviceToolbar
            devices={state.value.devices}
            selectedDeviceKey={
              detail.kind === "ok" || detail.kind === "loading"
                ? String(detail.target.transport_id ?? detail.target.serial)
                : null
            }
            onSelect={(device) => void selectDevice(device)}
          />
        )}

      <section className="mt-4 max-w-7xl" aria-live="polite">
        {state.kind === "ok" && state.value.adb_resolved && (
          <AdbHealthPanel
            health={health}
            observedAt={observedAt}
            watching={watching}
          />
        )}
        {state.kind === "no_tauri" && (
          <StatePanel
            title={t("devices.launchDesktopTitle")}
            tone="info"
            actions={
              <Button type="button" onClick={() => void refresh()} size="sm">
                {t("common.checkAgain")}
              </Button>
            }
          >
            <p>
              {t("devices.launchDesktopBodyPrefix")}{" "}
              <code>npm run tauri:dev</code>{" "}
              {t("devices.launchDesktopBodySuffix")}
            </p>
          </StatePanel>
        )}

        {state.kind === "loading" && <DeviceTableSkeleton />}

        {state.kind === "error" && (
          <StatePanel
            title={t("devices.scanFailedTitle")}
            tone="danger"
            actions={
              <Button
                type="button"
                onClick={() => void refresh()}
                variant="danger"
                size="sm"
              >
                {t("common.retryScan")}
              </Button>
            }
          >
            <p>{state.message}</p>
          </StatePanel>
        )}

        {state.kind === "ok" && !state.value.adb_resolved && (
          <StatePanel title={t("devices.noAdb")} tone="warning">
            <p>
              {t("devices.noAdbBodyPrefix")} <code>$PATH</code>,{" "}
              <code>$ANDROID_HOME</code>, {t("devices.noAdbBodyMiddle")}{" "}
              <code>scripts/fetch-platform-tools.*</code>{" "}
              {t("devices.noAdbBodySuffix")}
            </p>
          </StatePanel>
        )}

        {state.kind === "ok" &&
          state.value.adb_resolved &&
          state.value.devices.length === 0 && (
            <StatePanel
              title={t("devices.noDevices")}
              tone="info"
              actions={
                <Button
                  type="button"
                  onClick={() => void refresh()}
                  variant="secondary"
                  size="sm"
                >
                  {t("common.scanAgain")}
                </Button>
              }
            >
              <ol className="grid gap-2 text-sm sm:grid-cols-3">
                <li className="border-s border-white/10 ps-3 first:border-s-0 first:ps-0">
                  {t("devices.noDevicesStep1")}
                </li>
                <li className="border-s border-white/10 ps-3 first:border-s-0 first:ps-0">
                  {t("devices.noDevicesStep2Prefix")}{" "}
                  <em>{t("devices.allowUsbDebugging")}</em>{" "}
                  {t("devices.noDevicesStep2Suffix")}
                </li>
                <li className="border-s border-white/10 ps-3 first:border-s-0 first:ps-0">
                  {t("devices.noDevicesStep3")}
                </li>
              </ol>
            </StatePanel>
          )}

        {detail.kind !== "idle" && (
          <DeviceDetail
            state={detail}
            onRetry={(target: DeviceTarget) => {
              const device =
                state.kind === "ok"
                  ? state.value.devices.find(
                      (candidate) =>
                        candidate.transport_id === target.transport_id &&
                        candidate.connection_generation ===
                          target.connection_generation,
                    )
                  : undefined;
              if (device) void selectDevice(device);
            }}
          />
        )}

        {state.kind === "ok" && state.value.devices.length > 0 && (
          <DeviceTable
            devices={state.value.devices}
            selectedDeviceKey={
              detail.kind === "ok" || detail.kind === "loading"
                ? String(detail.target.transport_id ?? detail.target.serial)
                : null
            }
            onSelect={(device) => void selectDevice(device)}
          />
        )}

        {state.kind === "ok" &&
          state.value.devices.some(
            (d) => typeof d.state === "string" && d.state === "unauthorized",
          ) && (
            <AuthorizePrompt
              devices={state.value.devices.filter(
                (d) =>
                  typeof d.state === "string" && d.state === "unauthorized",
              )}
              onRefresh={() => void refresh()}
            />
          )}

        {state.kind === "ok" &&
          state.value.devices.some(
            (d) => typeof d.state === "string" && d.state === "no_permissions",
          ) && (
            <StatePanel title={t("devices.linuxPerms")} tone="danger">
              <p>{t("devices.linuxPermsBody")}</p>
              <p className="mt-2">{t("devices.linuxPermsDoctor")}</p>
            </StatePanel>
          )}

        {state.kind === "ok" &&
          (() => {
            const unusable = state.value.devices.filter(
              (d) =>
                !(
                  typeof d.state === "string" &&
                  ["device", "unauthorized", "no_permissions"].includes(d.state)
                ),
            );
            if (unusable.length === 0) return null;
            return (
              <StatePanel title={t("devices.unusableState")} tone="warning">
                <p>{t("devices.unusableStateBody")}</p>
                <ul className="mt-3 space-y-1">
                  {unusable.map((device) => (
                    <li
                      key={`${device.transport_id ?? device.serial}:${device.connection_generation}`}
                      className="text-xs"
                    >
                      <code className="font-mono text-anvil-100">
                        {device.serial}
                      </code>
                      <span className="ms-2 text-anvil-400">
                        {formatStateLabel(device.state)}
                      </span>
                    </li>
                  ))}
                </ul>
              </StatePanel>
            );
          })()}
      </section>

      {detail.kind === "ok" && (
        <section className="mt-4 max-w-7xl space-y-4" aria-live="polite">
          {/* Key by serial so every sub-panel's internal state (process list,
              file listing, network sockets, screenshot/density messages)
              resets on device switch. */}
          <DeviceControls
            key={`${detail.target.transport_id ?? detail.target.serial}:${detail.target.connection_generation}`}
            target={detail.target}
          />
        </section>
      )}
      <div className="mt-4 max-w-7xl">
        <HostDoctor />
      </div>
      {recoveryOpen && (
        <RecoveryDialog
          health={health}
          observedAt={observedAt}
          state={recovery}
          onConfirm={() => void runRecovery()}
          onCancel={cancelRecovery}
          onDismiss={() => setRecoveryOpen(false)}
        />
      )}
    </>
  );
}
