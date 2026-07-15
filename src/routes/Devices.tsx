import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callApplyDeviceControl,
  callCancelOperation,
  callGetDeviceInfo,
  callListNetworkConnections,
  callListProcesses,
  callListRemoteFiles,
  callPullFile,
  callRecoverAdb,
  callSelectHostPath,
  callTakeScreenshot,
  deviceTarget,
  newOperationId,
  summarizeState,
  type Device,
  type AdbHealth,
  type AdbRecoveryResult,
  type DeviceInfo,
  type DeviceTarget,
  type ListDevicesResult,
  type NetworkConnection,
  type OperationEvent,
  type ProcessInfo,
  type RemoteFileEntry,
  type RemoteListing,
  type SerializedDeviceState,
} from "../lib/tauri";
import { useFocusTrap } from "../lib/useFocusTrap";
import { formatDateTime } from "../lib/i18n";
import { useTransportAuthorization } from "../lib/useAuthorizedDevices";
import {
  restartDeviceLifecycle,
  useDeviceStore,
  type SharedDevicesState,
} from "../lib/deviceStore";

import {
  Badge,
  Button,
  Card,
  EmptyState,
  FieldInput,
  PaneHeader,
  SkeletonLine,
  StatePanel,
  TableCell,
  TableHeaderCell,
  TransportBadge,
  TransportTrustNotice,
} from "./common";
import HostDoctor from "./HostDoctor";
import { ADB_RECOVERY_COMMANDS, formatAdbDiagnostics } from "./adbHealth";

type State = SharedDevicesState;

type DetailState =
  | { kind: "idle" }
  | { kind: "loading"; target: DeviceTarget }
  | { kind: "ok"; info: DeviceInfo; target: DeviceTarget }
  | { kind: "error"; target: DeviceTarget; message: string };

type RecoveryState =
  | { kind: "idle" }
  | { kind: "running"; status: string }
  | { kind: "done"; result: AdbRecoveryResult }
  | { kind: "error"; message: string };

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

  const refresh = useCallback(async () => {
    await restartDeviceLifecycle();
  }, []);

  const selectDevice = useCallback(async (device: Device) => {
    const target = deviceTarget(device);
    setDetail({ kind: "loading", target });
    try {
      const info = await callGetDeviceInfo(target);
      setDetail({ kind: "ok", info, target });
    } catch (e) {
      setDetail({
        kind: "error",
        target,
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

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
        message: error instanceof Error ? error.message : String(error),
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
          <Button
            type="button"
            onClick={() => void refresh()}
            disabled={state.kind === "loading"}
            variant="primary"
          >
            {state.kind === "loading"
              ? t("devices.scanning")
              : t("devices.refresh")}
          </Button>
        }
        meta={<DeviceHeaderMeta state={state} />}
      />

      <section className="mt-6 max-w-6xl" aria-live="polite">
        {state.kind === "ok" && state.value.adb_resolved && (
          <AdbHealthPanel
            health={health}
            observedAt={observedAt}
            watching={watching}
            onReviewRecovery={() => {
              setRecovery({ kind: "idle" });
              setRecoveryOpen(true);
            }}
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
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  {t("devices.noDevicesStep1")}
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  {t("devices.noDevicesStep2Prefix")}{" "}
                  <em>{t("devices.allowUsbDebugging")}</em>{" "}
                  {t("devices.noDevicesStep2Suffix")}
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  {t("devices.noDevicesStep3")}
                </li>
              </ol>
            </StatePanel>
          )}

        {state.kind === "ok" && state.value.devices.length > 0 && (
          <DeviceTable
            devices={state.value.devices}
            selectedSerial={
              detail.kind === "ok"
                ? detail.target.transport_id
                : detail.kind === "loading"
                  ? detail.target.transport_id
                  : undefined
            }
            onSelect={(device) => void selectDevice(device)}
          />
        )}

        <div className="mt-4">
          <HostDoctor />
        </div>

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
      </section>

      {detail.kind !== "idle" && (
        <section className="mt-4 max-w-6xl space-y-4" aria-live="polite">
          <DeviceDetail
            state={detail}
            onRetry={(target) => {
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
          {detail.kind === "ok" && (
            // Key by serial so every sub-panel's internal state (process
            // list, file listing, network sockets, screenshot/density
            // messages) resets on device switch instead of showing device
            // A's data while device B is selected.
            <DeviceControls
              key={`${detail.target.transport_id ?? detail.target.serial}:${detail.target.connection_generation}`}
              target={detail.target}
            />
          )}
        </section>
      )}
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

function AdbHealthPanel({
  health,
  observedAt,
  watching,
  onReviewRecovery,
}: {
  health: AdbHealth | null;
  observedAt: string | null;
  watching: boolean;
  onReviewRecovery: () => void;
}) {
  const { t, i18n } = useTranslation();
  return (
    <Card className="mb-4 p-5" aria-labelledby="adb-health-title">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h3
              id="adb-health-title"
              className="text-sm font-semibold text-anvil-50"
            >
              {t("devices.health.title")}
            </h3>
            <Badge tone={watching ? "success" : "warning"}>
              {watching
                ? t("devices.health.live")
                : t("devices.health.stopped")}
            </Badge>
            {health?.wifi_v2_state === "supported" && (
              <Badge tone="success">
                {t("devices.health.wifiTwoDetected")}
              </Badge>
            )}
          </div>
          <p className="mt-1 text-xs leading-5 text-anvil-400">
            {observedAt
              ? t("devices.health.observed", {
                  time: formatDateTime(
                    observedAt,
                    i18n.resolvedLanguage ?? i18n.language,
                  ),
                })
              : t("devices.health.probing")}
          </p>
        </div>
        <Button type="button" size="sm" onClick={onReviewRecovery}>
          {t("devices.health.reviewRecovery")}
        </Button>
      </div>

      {health ? (
        <dl className="mt-4 grid gap-3 text-xs sm:grid-cols-2 lg:grid-cols-4">
          <HealthMetric
            label={t("devices.health.client")}
            value={health.client_version ?? t("common.notReported")}
          />
          <HealthMetric
            label={t("devices.health.server")}
            value={health.server_version ?? t("common.notReported")}
          />
          <HealthMetric
            label={t("devices.health.usbBackend")}
            value={health.usb_backend ?? t("common.notReported")}
          />
          <HealthMetric
            label={t("devices.health.mdnsBackend")}
            value={health.mdns_backend ?? t("common.notReported")}
          />
          <HealthMetric
            label={t("devices.health.mdns")}
            value={
              health.mdns_enabled == null
                ? t("common.notReported")
                : health.mdns_enabled
                  ? t("devices.health.enabled")
                  : t("devices.health.disabled")
            }
          />
          <HealthMetric
            label={t("devices.health.wifiTwo")}
            value={t(`devices.health.wifiTwoState.${health.wifi_v2_state}`)}
          />
          <HealthMetric
            label={t("devices.health.wifiTwoDevices")}
            value={
              health.wifi_v2_devices.join(", ") ||
              t("devices.health.noneDetected")
            }
          />
          <HealthMetric
            label={t("devices.health.mdnsCheck")}
            value={health.mdns_check ?? t("common.notReported")}
          />
        </dl>
      ) : (
        <div className="mt-4 grid gap-2 sm:grid-cols-3" aria-hidden="true">
          <SkeletonLine />
          <SkeletonLine />
          <SkeletonLine />
        </div>
      )}
      {health?.warning && (
        <p
          role="status"
          className="mt-4 rounded-md border border-amber-300/20 bg-amber-400/10 px-3 py-2 text-xs leading-5 text-amber-100"
        >
          {health.warning}
        </p>
      )}
    </Card>
  );
}

function HealthMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-white/10 bg-white/[0.03] p-3">
      <dt className="text-anvil-500">{label}</dt>
      <dd className="mt-1 break-words font-mono text-anvil-100">{value}</dd>
    </div>
  );
}

function RecoveryDialog({
  health,
  observedAt,
  state,
  onConfirm,
  onCancel,
  onDismiss,
}: {
  health: AdbHealth | null;
  observedAt: string | null;
  state: RecoveryState;
  onConfirm: () => void;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const trapRef = useFocusTrap<HTMLDivElement>();
  const [copyStatus, setCopyStatus] = useState<string | null>(null);
  const running = state.kind === "running";
  const diagnostics = formatAdbDiagnostics({
    health:
      state.kind === "done"
        ? (state.result.record.health_after ?? health)
        : health,
    observedAt,
    recovery: state.kind === "done" ? state.result : null,
  });

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !running) onDismiss();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onDismiss, running]);

  const copyDiagnostics = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(diagnostics);
      setCopyStatus(t("devices.health.copied"));
    } catch {
      setCopyStatus(t("devices.health.copyFallback"));
    }
  }, [diagnostics, t]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="adb-recovery-title"
        tabIndex={-1}
        className="max-h-[90vh] w-full max-w-2xl overflow-y-auto rounded-lg border border-white/10 bg-anvil-950 p-5 shadow-2xl outline-none"
      >
        <h3
          id="adb-recovery-title"
          className="text-lg font-semibold text-anvil-50"
        >
          {t("devices.health.recoveryTitle")}
        </h3>
        <p className="mt-2 text-sm leading-6 text-anvil-300">
          {t("devices.health.recoveryBody")}
        </p>
        <ol className="mt-4 space-y-2">
          {ADB_RECOVERY_COMMANDS.map((command, index) => (
            <li
              key={command}
              className="flex items-center gap-3 rounded-md border border-white/10 bg-black/30 px-3 py-2"
            >
              <span className="font-mono text-xs text-anvil-500">
                {index + 1}
              </span>
              <code className="font-mono text-xs text-anvil-100">
                {command}
              </code>
            </li>
          ))}
        </ol>
        <p className="mt-4 rounded-md border border-amber-300/20 bg-amber-400/10 p-3 text-xs leading-5 text-amber-100">
          {t("devices.health.recoveryWarning")}
        </p>

        {state.kind === "running" && (
          <p role="status" className="mt-4 text-sm text-circuit-100">
            {state.status}
          </p>
        )}
        {state.kind === "error" && (
          <p role="alert" className="mt-4 text-sm text-red-100">
            {state.message}
          </p>
        )}
        {state.kind === "done" && (
          <p
            role="status"
            className={
              state.result.record.outcome === "succeeded"
                ? "mt-4 text-sm text-emerald-200"
                : "mt-4 text-sm text-amber-100"
            }
          >
            {t(`devices.health.recoveryOutcome.${state.result.record.outcome}`)}
          </p>
        )}

        {(state.kind === "done" || state.kind === "error") && (
          <div className="mt-4">
            <label
              htmlFor="adb-recovery-diagnostics"
              className="text-xs font-medium text-anvil-300"
            >
              {t("devices.health.copyableDiagnostics")}
            </label>
            <textarea
              id="adb-recovery-diagnostics"
              readOnly
              value={diagnostics}
              rows={11}
              className="mt-2 w-full resize-y rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200 outline-none focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
            />
            {copyStatus && (
              <p className="mt-2 text-xs text-anvil-400">{copyStatus}</p>
            )}
          </div>
        )}

        <div className="mt-5 flex flex-wrap justify-end gap-2">
          {(state.kind === "done" || state.kind === "error") && (
            <Button type="button" onClick={() => void copyDiagnostics()}>
              {t("devices.health.copyDiagnostics")}
            </Button>
          )}
          {running ? (
            <Button type="button" variant="danger" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
          ) : (
            <Button type="button" variant="ghost" onClick={onDismiss}>
              {state.kind === "idle" ? t("common.cancel") : t("common.close")}
            </Button>
          )}
          {state.kind === "idle" && (
            <Button type="button" variant="danger" onClick={onConfirm}>
              {t("devices.health.confirmRecovery")}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

function DeviceHeaderMeta({ state }: { state: State }) {
  const { t } = useTranslation();

  if (state.kind === "loading") {
    return (
      <div className="flex flex-wrap gap-2">
        <Badge tone="info">{t("devices.scanningBridge")}</Badge>
        <Badge tone="neutral">{t("devices.waitingForAdb")}</Badge>
      </div>
    );
  }

  if (state.kind === "no_tauri") {
    return (
      <div className="flex flex-wrap gap-2">
        <Badge tone="neutral">{t("runtime.browserPreview")}</Badge>
        <Badge tone="info">{t("common.tauriIpcRequired")}</Badge>
      </div>
    );
  }

  if (state.kind === "error") {
    return <Badge tone="danger">{t("devices.scanFailed")}</Badge>;
  }

  if (!state.value.adb_resolved) {
    return <Badge tone="warning">{t("devices.adbMissing")}</Badge>;
  }

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-2">
      <Badge tone="success">{t("devices.adbResolved")}</Badge>
      {state.value.adb_path && (
        <code className="max-w-full truncate font-mono text-xs">
          {state.value.adb_path}
        </code>
      )}
    </div>
  );
}

function DeviceTable({
  devices,
  selectedSerial,
  onSelect,
}: {
  devices: ListDevicesResult["devices"];
  selectedSerial?: number | null;
  onSelect: (device: Device) => void;
}) {
  const { t } = useTranslation();

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.connected")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.selectHint")}
          </p>
        </div>
        <Badge tone="success">
          {t("common.deviceCount", { count: devices.length })}
        </Badge>
      </div>
      <div className="overflow-x-auto">
        <table className="min-w-full text-sm">
          <thead className="bg-white/[0.04]">
            <tr>
              <TableHeaderCell>{t("devices.serial")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.identity")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.transport")}</TableHeaderCell>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/10">
            {devices.map((device) => {
              const isDevice =
                typeof device.state === "string" && device.state === "device";
              const isSelected = device.transport_id === selectedSerial;
              return (
                <tr
                  key={`${device.transport_id ?? device.serial}:${device.connection_generation}`}
                  title={!isDevice ? t("devices.mustAuthorize") : undefined}
                  className={[
                    "transition",
                    isDevice
                      ? "hover:bg-white/[0.055]"
                      : "bg-anvil-950/20 opacity-75",
                    isSelected ? "bg-circuit-300/[0.06]" : "",
                  ].join(" ")}
                >
                  <TableCell>
                    <button
                      type="button"
                      disabled={!isDevice}
                      aria-pressed={isDevice ? isSelected : undefined}
                      aria-label={
                        isDevice
                          ? t("devices.selectDeviceLabel", {
                              device: device.model ?? device.serial,
                            })
                          : undefined
                      }
                      onClick={() => onSelect(device)}
                      className="flex min-w-[13rem] items-center gap-2 rounded-sm text-left disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 focus-visible:ring-offset-2 focus-visible:ring-offset-anvil-900"
                    >
                      <code className="font-mono text-xs text-anvil-50">
                        {device.serial}
                      </code>
                      <TransportBadge kind={device.transport_kind} />
                    </button>
                  </TableCell>
                  <TableCell>
                    <Badge tone={deviceStateTone(device.state)}>
                      {formatStateLabel(device.state)}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <div className="min-w-[13rem]">
                      <p className="font-medium text-anvil-100">
                        {device.model ?? t("devices.unknownModel")}
                      </p>
                      <p className="mt-1 text-xs text-anvil-400">
                        {[device.product, device.device]
                          .filter(Boolean)
                          .join(" / ") || t("devices.noProductMetadata")}
                      </p>
                    </div>
                  </TableCell>
                  <TableCell>
                    {device.transport_id != null ? (
                      <code className="font-mono text-xs">
                        {t("devices.transportId", {
                          id: device.transport_id,
                        })}
                      </code>
                    ) : (
                      <span className="text-anvil-500">
                        {t("common.notReported")}
                      </span>
                    )}
                  </TableCell>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </Card>
  );
}

function DeviceDetail({
  state,
  onRetry,
}: {
  state: DetailState;
  onRetry: (target: DeviceTarget) => void;
}) {
  const { t } = useTranslation();

  if (state.kind === "idle") return null;

  if (state.kind === "loading") {
    return (
      <Card className="p-5">
        <h3 className="text-sm font-semibold text-anvil-50">
          {t("devices.loadingDeviceInfo")}
        </h3>
        <p className="mt-1 text-xs text-anvil-400">
          {t("devices.queryingSerial", { serial: state.target.serial })}
        </p>
        <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <div key={i}>
              <SkeletonLine className="w-20" />
              <SkeletonLine className="mt-2 w-36" />
            </div>
          ))}
        </div>
      </Card>
    );
  }

  if (state.kind === "error") {
    return (
      <StatePanel
        title={t("devices.deviceInfoFailed")}
        tone="danger"
        actions={
          <Button
            type="button"
            onClick={() => onRetry(state.target)}
            variant="danger"
            size="sm"
          >
            {t("runtime.retry")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  const info = state.info;
  return (
    <Card className="p-5">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h3 className="text-lg font-semibold text-anvil-50">
            {info.model ?? info.serial}
          </h3>
          {info.manufacturer && (
            <p className="mt-1 text-sm text-anvil-400">{info.manufacturer}</p>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          {info.android_version && (
            <Badge tone="info">Android {info.android_version}</Badge>
          )}
          {info.sdk_level && (
            <Badge tone="neutral">
              {t("devices.apiLevel", { level: info.sdk_level })}
            </Badge>
          )}
        </div>
      </div>

      <dl className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <InfoField label={t("devices.serial")} value={info.serial} mono />
        {info.hardware_serial && (
          <InfoField
            label={t("devices.hwSerial")}
            value={info.hardware_serial}
            mono
          />
        )}
        {info.build_fingerprint && (
          <InfoField
            label={t("devices.buildFingerprint")}
            value={info.build_fingerprint}
            mono
            wrap
          />
        )}
        {info.security_patch && (
          <InfoField
            label={t("devices.securityPatch")}
            value={info.security_patch}
          />
        )}
        {info.wifi_ip && (
          <InfoField label={t("devices.wifiIp")} value={info.wifi_ip} mono />
        )}
        {info.battery && (
          <InfoField
            label={t("devices.battery")}
            value={formatBattery(info.battery)}
          />
        )}
        {info.storage && (
          <InfoField
            label={t("devices.storageData")}
            value={formatStorage(info.storage)}
          />
        )}
      </dl>
    </Card>
  );
}

function InfoField({
  label,
  value,
  mono = false,
  wrap = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
  wrap?: boolean;
}) {
  return (
    <div className="min-w-0">
      <dt className="text-xs font-medium text-anvil-500">{label}</dt>
      <dd
        className={[
          "mt-1 text-sm text-anvil-100",
          mono ? "font-mono text-xs" : "",
          wrap ? "break-all" : "truncate",
        ].join(" ")}
      >
        {value}
      </dd>
    </div>
  );
}

function formatBattery(b: NonNullable<DeviceInfo["battery"]>): string {
  const parts: string[] = [];
  if (b.level != null) parts.push(`${b.level}%`);
  if (b.status) parts.push(b.status);
  if (b.temperature != null) parts.push(`${b.temperature}°C`);
  return parts.join(" · ") || "Unknown";
}

function formatStorage(s: NonNullable<DeviceInfo["storage"]>): string {
  if (s.total_kb == null || s.available_kb == null) return "Unknown";
  const totalGb = (s.total_kb / 1048576).toFixed(1);
  const availGb = (s.available_kb / 1048576).toFixed(1);
  return `${availGb} GB free / ${totalGb} GB`;
}

function DeviceTableSkeleton() {
  const { t } = useTranslation();

  return (
    <Card
      className="overflow-hidden p-0"
      aria-label={t("devices.loadingDevices")}
    >
      <div className="border-b border-white/10 p-4">
        <SkeletonLine className="w-40" />
        <SkeletonLine className="mt-3 w-80 max-w-full" />
      </div>
      <div className="divide-y divide-white/10">
        {Array.from({ length: 4 }).map((_, index) => (
          <div
            key={index}
            className="grid gap-4 p-4 sm:grid-cols-[1.2fr_0.7fr_1.2fr_0.8fr]"
          >
            <SkeletonLine className="w-44" />
            <SkeletonLine className="w-24" />
            <div>
              <SkeletonLine className="w-36" />
              <SkeletonLine className="mt-2 w-48" />
            </div>
            <SkeletonLine className="w-28" />
          </div>
        ))}
      </div>
    </Card>
  );
}

function formatStateLabel(state: SerializedDeviceState): string {
  const label = summarizeState(state);
  return label.charAt(0).toUpperCase() + label.slice(1);
}

const REMOTE_BUTTONS: { id: string; labelKey: string; keycode: number }[] = [
  { id: "Home", labelKey: "devices.controls.remoteHome", keycode: 3 },
  { id: "Back", labelKey: "devices.controls.remoteBack", keycode: 4 },
  { id: "Recents", labelKey: "devices.controls.remoteRecents", keycode: 187 },
  { id: "Up", labelKey: "devices.controls.remoteUp", keycode: 19 },
  { id: "Down", labelKey: "devices.controls.remoteDown", keycode: 20 },
  { id: "Left", labelKey: "devices.controls.remoteLeft", keycode: 21 },
  { id: "Right", labelKey: "devices.controls.remoteRight", keycode: 22 },
  { id: "OK", labelKey: "devices.controls.remoteOk", keycode: 23 },
  { id: "Vol +", labelKey: "devices.controls.remoteVolUp", keycode: 24 },
  { id: "Vol -", labelKey: "devices.controls.remoteVolDown", keycode: 25 },
  { id: "Power", labelKey: "devices.controls.remotePower", keycode: 26 },
  { id: "Menu", labelKey: "devices.controls.remoteMenu", keycode: 82 },
];

function DeviceControls({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(target);
  const operationTarget = authorizedTarget ?? target;
  const serial = target.serial;
  const [lastKey, setLastKey] = useState<string | null>(null);
  const [screenshotMsg, setScreenshotMsg] = useState<string | null>(null);
  const [density, setDensity] = useState("");
  const [displayMsg, setDisplayMsg] = useState<string | null>(null);

  const sendKey = useCallback(
    async (keycode: number, label: string) => {
      try {
        await callApplyDeviceControl(operationTarget, [
          "input",
          "keyevent",
          String(keycode),
        ]);
        setLastKey(label);
      } catch {
        setLastKey(t("devices.controls.keyFailed", { label }));
      }
    },
    [operationTarget, t],
  );

  const takeScreenshot = useCallback(async () => {
    try {
      const pathGrant = await callSelectHostPath(
        "screenshot_save",
        `screenshot-${serial.replace(/[<>:"/\\|?*]/gu, "_")}-${Date.now()}.png`,
      );
      if (!pathGrant) {
        setScreenshotMsg(null);
        return;
      }
      setScreenshotMsg(t("devices.controls.capturing"));
      const artifact = await callTakeScreenshot(operationTarget, pathGrant.id);
      setScreenshotMsg(
        t("devices.controls.savedTo", { path: artifact.local_path }),
      );
    } catch (e) {
      setScreenshotMsg(
        t("devices.controls.failed", {
          message: e instanceof Error ? e.message : String(e),
        }),
      );
    }
  }, [operationTarget, serial, t]);

  const applyDensity = useCallback(async () => {
    if (!density.trim()) return;
    try {
      await callApplyDeviceControl(operationTarget, [
        "wm",
        "density",
        density.trim(),
      ]);
      setDisplayMsg(
        t("devices.controls.densitySet", { value: density.trim() }),
      );
    } catch (e) {
      setDisplayMsg(
        t("devices.controls.failed", {
          message: e instanceof Error ? e.message : String(e),
        }),
      );
    }
  }, [operationTarget, density, t]);

  const resetDensity = useCallback(async () => {
    try {
      await callApplyDeviceControl(operationTarget, ["wm", "density", "reset"]);
      setDisplayMsg(t("devices.controls.densityReset"));
    } catch (e) {
      setDisplayMsg(
        t("devices.controls.failed", {
          message: e instanceof Error ? e.message : String(e),
        }),
      );
    }
  }, [operationTarget, t]);

  const toggleForceDark = useCallback(
    async (enable: boolean) => {
      try {
        await callApplyDeviceControl(operationTarget, [
          "settings",
          "put",
          "secure",
          "ui_night_mode",
          enable ? "2" : "1",
        ]);
        setDisplayMsg(
          enable
            ? t("devices.controls.forceDarkEnabled")
            : t("devices.controls.forceDarkDisabled"),
        );
      } catch (e) {
        setDisplayMsg(
          t("devices.controls.failed", {
            message: e instanceof Error ? e.message : String(e),
          }),
        );
      }
    },
    [operationTarget, t],
  );

  return (
    <div className="space-y-4">
      <TransportTrustNotice
        target={target}
        accepted={transportOverrideAccepted}
        onAcceptedChange={setTransportOverrideAccepted}
      />
      <div className="grid gap-4 lg:grid-cols-[1fr_1fr]">
        <Card className="p-5">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.controls.virtualRemote")}
            </h3>
            {lastKey && (
              <span className="text-xs text-anvil-400">
                {t("devices.controls.lastKey", { key: lastKey })}
              </span>
            )}
          </div>
          <div className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
            {REMOTE_BUTTONS.map((btn) => {
              const label = t(btn.labelKey);
              return (
                <Button
                  key={btn.id}
                  type="button"
                  size="sm"
                  onClick={() => void sendKey(btn.keycode, label)}
                  title={`keyevent ${btn.keycode}`}
                  className="justify-start"
                >
                  <RemoteGlyph label={btn.id} />
                  <span>{label}</span>
                </Button>
              );
            })}
          </div>
        </Card>

        <div className="space-y-4">
          <Card className="p-5">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.controls.screenshot")}
            </h3>
            <p className="mt-1 text-xs text-anvil-400">
              {t("devices.controls.screenshotBody")}
            </p>
            <div className="mt-3 flex items-center gap-3">
              <Button
                type="button"
                size="sm"
                variant="primary"
                onClick={() => void takeScreenshot()}
              >
                {t("devices.controls.capture")}
              </Button>
              {screenshotMsg && (
                <span className="text-xs text-anvil-300">{screenshotMsg}</span>
              )}
            </div>
          </Card>

          <Card className="p-5">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.controls.displayTuning")}
            </h3>
            <div className="mt-3 grid gap-3 sm:grid-cols-2">
              <div className="flex items-end gap-2">
                <label className="grid flex-1 gap-1.5">
                  <span className="text-xs font-medium text-anvil-400">
                    {t("devices.controls.densityLabel")}
                  </span>
                  <FieldInput
                    type="text"
                    value={density}
                    onChange={(e) => setDensity(e.target.value)}
                    placeholder="420"
                    inputMode="numeric"
                    className="font-mono"
                  />
                </label>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void applyDensity()}
                >
                  {t("devices.controls.set")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => void resetDensity()}
                >
                  {t("devices.controls.reset")}
                </Button>
              </div>
              <div className="flex items-end gap-2">
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void toggleForceDark(true)}
                >
                  {t("devices.controls.forceDarkOn")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => void toggleForceDark(false)}
                >
                  {t("devices.controls.forceDarkOff")}
                </Button>
              </div>
            </div>
            {displayMsg && (
              <p className="mt-3 text-xs text-anvil-300">{displayMsg}</p>
            )}
          </Card>
        </div>
      </div>
      <ProcessManager target={operationTarget} />
      <FileManager target={operationTarget} />
      <NetworkInspector target={operationTarget} />
    </div>
  );
}

function RemoteGlyph({ label }: { label: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4 shrink-0"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      {label === "Home" && (
        <path d="m4 11 8-7 8 7v8a1 1 0 0 1-1 1h-5v-6h-4v6H5a1 1 0 0 1-1-1v-8Z" />
      )}
      {label === "Back" && <path d="M15 6 9 12l6 6" />}
      {label === "Recents" && (
        <>
          <path d="M8 7h9v9" />
          <path d="M5 10h9v9H5z" />
        </>
      )}
      {label === "Up" && <path d="m7 14 5-5 5 5" />}
      {label === "Down" && <path d="m7 10 5 5 5-5" />}
      {label === "Left" && <path d="m14 7-5 5 5 5" />}
      {label === "Right" && <path d="m10 7 5 5-5 5" />}
      {label === "OK" && <circle cx="12" cy="12" r="4" />}
      {label === "Vol +" && (
        <>
          <path d="M5 10v4h3l4 3V7l-4 3H5Z" />
          <path d="M17 9v6M14 12h6" />
        </>
      )}
      {label === "Vol -" && (
        <>
          <path d="M5 10v4h3l4 3V7l-4 3H5Z" />
          <path d="M15 12h5" />
        </>
      )}
      {label === "Power" && (
        <>
          <path d="M12 4v8" />
          <path d="M7.5 7.5a7 7 0 1 0 9 0" />
        </>
      )}
      {label === "Menu" && (
        <>
          <path d="M6 8h12" />
          <path d="M6 12h12" />
          <path d="M6 16h12" />
        </>
      )}
    </svg>
  );
}

function ProcessManager({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [processes, setProcesses] = useState<ProcessInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<"rss" | "name">("rss");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const procs = await callListProcesses(target);
      setProcesses(procs);
    } catch (e) {
      setProcesses([]);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [target]);

  const filtered = processes
    .filter((p) =>
      search ? p.name.toLowerCase().includes(search.toLowerCase()) : true,
    )
    .sort((a, b) =>
      sortBy === "rss" ? b.rss_kb - a.rss_kb : a.name.localeCompare(b.name),
    );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.controls.processManager")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.controls.processManagerBody")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <FieldInput
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("devices.controls.filter")}
            aria-label={t("devices.controls.filterProcesses")}
            className="h-8 w-40 px-2 font-mono text-xs"
          />
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => void refresh()}
            disabled={loading}
          >
            {loading
              ? t("devices.controls.loading")
              : processes.length > 0
                ? t("devices.controls.refresh")
                : t("devices.controls.load")}
          </Button>
        </div>
      </div>
      {error && (
        <div className="border-b border-rose-500/20 bg-rose-500/10 px-4 py-3 text-xs text-rose-200">
          {t("devices.controls.processReadFailed", { message: error })}
        </div>
      )}
      {processes.length === 0 && !loading && !error && (
        <EmptyState title={t("devices.controls.noProcesses")}>
          <p>{t("devices.controls.noProcessesBody")}</p>
        </EmptyState>
      )}
      {processes.length > 0 && filtered.length === 0 && (
        <EmptyState title={t("devices.controls.noMatchingProcesses")}>
          <p>{t("devices.controls.noMatchingProcessesBody")}</p>
        </EmptyState>
      )}
      {processes.length > 0 && filtered.length > 0 && (
        <div className="max-h-96 overflow-x-auto">
          <table className="min-w-full text-xs">
            <thead className="sticky top-0 bg-anvil-900">
              <tr>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colPid")}
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colUser")}
                </th>
                <th
                  className="px-3 py-2 text-right font-semibold text-anvil-400"
                  aria-sort={sortBy === "rss" ? "descending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("rss")}
                    className="ml-auto flex items-center gap-1 hover:text-anvil-200"
                  >
                    {t("devices.controls.colRss")}
                    {sortBy === "rss" && <span aria-hidden="true">&darr;</span>}
                  </button>
                </th>
                <th
                  className="px-3 py-2 text-left font-semibold text-anvil-400"
                  aria-sort={sortBy === "name" ? "ascending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("name")}
                    className="flex items-center gap-1 hover:text-anvil-200"
                  >
                    {t("devices.controls.colName")}
                    {sortBy === "name" && (
                      <span aria-hidden="true">&uarr;</span>
                    )}
                  </button>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {filtered.slice(0, 100).map((p, i) => (
                <tr
                  key={`${p.pid}-${p.name}-${i}`}
                  className="hover:bg-white/[0.03]"
                >
                  <td className="px-3 py-1.5 font-mono text-anvil-300">
                    {p.pid}
                  </td>
                  <td className="px-3 py-1.5 text-anvil-400">{p.user}</td>
                  <td className="px-3 py-1.5 text-right font-mono text-anvil-200">
                    {formatKb(p.rss_kb)}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    <span>{p.name}</span>
                    {p.parse_error && (
                      <Badge tone="warning" className="ml-2">
                        {t("devices.controls.parseIssue")}
                      </Badge>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {filtered.length > 100 && (
            <p className="px-3 py-2 text-xs text-anvil-500">
              {t("devices.controls.showingProcesses", {
                count: filtered.length,
              })}
            </p>
          )}
        </div>
      )}
    </Card>
  );
}

function formatKb(kb: number): string {
  if (kb >= 1048576) return `${(kb / 1048576).toFixed(1)} GB`;
  if (kb >= 1024) return `${(kb / 1024).toFixed(1)} MB`;
  return `${kb} KB`;
}

function formatBytes(bytes: number | null): string {
  if (bytes == null) return "Unknown";
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`;
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function FileManager({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [listing, setListing] = useState<RemoteListing | null>(null);
  const [currentPath, setCurrentPath] = useState("/sdcard");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pullMsg, setPullMsg] = useState<string | null>(null);
  const [pullOperationId, setPullOperationId] = useState<string | null>(null);
  const pullOperationRef = useRef<string | null>(null);
  const pullGenerationRef = useRef(0);

  useEffect(() => {
    return () => {
      pullGenerationRef.current += 1;
      const operationId = pullOperationRef.current;
      pullOperationRef.current = null;
      if (operationId) void callCancelOperation(operationId);
    };
  }, [target.serial, target.transport_id, target.connection_generation]);

  const browse = useCallback(
    async (path: string) => {
      setLoading(true);
      setPullMsg(null);
      setError(null);
      try {
        const result = await callListRemoteFiles(target, path);
        setListing(result);
        setCurrentPath(path);
      } catch (e) {
        setListing(null);
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    },
    [target],
  );

  const navigateUp = useCallback(() => {
    const parent = currentPath.replace(/\/[^/]+\/?$/, "") || "/";
    void browse(parent);
  }, [currentPath, browse]);

  const pullRemote = useCallback(
    async (entry: RemoteFileEntry) => {
      let operationId: string | null = null;
      let generation: number | null = null;
      try {
        const pathGrant = await callSelectHostPath(
          "pull_save",
          entry.name.replace(/[<>:"/\\|?*]/gu, "_"),
        );
        if (!pathGrant) {
          setPullMsg(null);
          return;
        }
        setPullMsg(t("devices.controls.pulling", { name: entry.name }));
        const remoteFull =
          currentPath === "/"
            ? `/${entry.name}`
            : `${currentPath}/${entry.name}`;
        operationId = newOperationId("pull");
        generation = pullGenerationRef.current + 1;
        pullGenerationRef.current = generation;
        pullOperationRef.current = operationId;
        setPullOperationId(operationId);
        const artifact = await callPullFile(target, remoteFull, pathGrant.id, {
          operationId,
          onEvent: (event: OperationEvent) => {
            if (
              pullOperationRef.current !== operationId ||
              pullGenerationRef.current !== generation
            )
              return;
            if (event.kind === "progress") {
              setPullMsg(
                t("devices.controls.pullProgress", {
                  name: entry.name,
                  seconds: Math.max(
                    1,
                    Math.round((event.elapsed_ms ?? 0) / 1000),
                  ),
                }),
              );
            }
          },
        });
        if (pullGenerationRef.current !== generation) return;
        pullOperationRef.current = null;
        setPullOperationId(null);
        setPullMsg(
          t("devices.controls.savedName", {
            name: entry.name,
            path: artifact.local_path,
          }),
        );
      } catch (e) {
        if (
          operationId &&
          (pullGenerationRef.current !== generation ||
            pullOperationRef.current !== operationId)
        )
          return;
        pullOperationRef.current = null;
        setPullOperationId(null);
        setPullMsg(
          t("devices.controls.failed", {
            message: e instanceof Error ? e.message : String(e),
          }),
        );
      }
    },
    [target, currentPath, t],
  );

  const cancelPull = useCallback(async () => {
    const operationId = pullOperationRef.current;
    if (!operationId) return;
    setPullMsg(t("devices.controls.pullCancelling"));
    await callCancelOperation(operationId);
  }, [t]);

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.controls.fileManager")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.controls.fileManagerBody")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {listing?.free_space_kb != null && (
            <Badge tone="neutral">
              {t("devices.controls.freeSpace", {
                size: formatKb(listing.free_space_kb),
              })}
            </Badge>
          )}
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => void browse(currentPath)}
            disabled={loading}
          >
            {loading
              ? t("devices.controls.loading")
              : listing
                ? t("devices.controls.refresh")
                : t("devices.controls.browse")}
          </Button>
        </div>
      </div>

      {error && (
        <div className="border-b border-rose-500/20 bg-rose-500/10 px-4 py-3 text-xs text-rose-200">
          {t("devices.controls.fileListFailed", {
            path: currentPath,
            message: error,
          })}
        </div>
      )}

      {!listing && !loading && !error && (
        <EmptyState title={t("devices.controls.noDirectory")}>
          <p>
            {t("devices.controls.noDirectoryBodyPrefix")} <code>/sdcard</code>{" "}
            {t("devices.controls.noDirectoryBodySuffix")}
          </p>
        </EmptyState>
      )}

      {listing && (
        <>
          <div className="flex items-center gap-2 border-b border-white/10 bg-white/[0.02] px-4 py-2">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={navigateUp}
              disabled={currentPath === "/"}
            >
              ..
            </Button>
            <code className="flex-1 truncate font-mono text-xs text-anvil-200">
              {currentPath}
            </code>
          </div>
          <div className="max-h-80 divide-y divide-white/5 overflow-y-auto">
            {listing.entries.length === 0 && (
              <EmptyState
                title={t("devices.controls.emptyDirectory")}
                className="border-t-0"
              >
                <p>{t("devices.controls.emptyDirectoryBody")}</p>
              </EmptyState>
            )}
            {listing.entries.map((entry, index) => (
              <div
                key={`${entry.name}-${index}`}
                className="flex items-center gap-3 px-4 py-2 text-xs hover:bg-white/[0.03]"
              >
                <FileGlyph directory={entry.is_dir} />
                {entry.is_dir ? (
                  <button
                    type="button"
                    className="min-w-0 flex-1 truncate text-left font-mono text-circuit-200 hover:underline"
                    onClick={() =>
                      void browse(
                        currentPath === "/"
                          ? `/${entry.name}`
                          : `${currentPath}/${entry.name}`,
                      )
                    }
                  >
                    {entry.name}/
                  </button>
                ) : (
                  <span className="min-w-0 flex-1 truncate font-mono text-anvil-100">
                    {entry.name}
                  </span>
                )}
                <span className="shrink-0 font-mono text-anvil-500">
                  {entry.is_dir ? "" : formatBytes(entry.size)}
                </span>
                <span className="hidden shrink-0 font-mono text-anvil-600 sm:inline">
                  {entry.permissions}
                </span>
                {entry.parse_error && (
                  <Badge tone="warning" className="shrink-0">
                    {t("devices.controls.parseIssue")}
                  </Badge>
                )}
                {!entry.is_dir && (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    onClick={() => void pullRemote(entry)}
                  >
                    {t("devices.controls.pull")}
                  </Button>
                )}
              </div>
            ))}
          </div>
          {pullMsg && (
            <div className="flex items-center justify-between gap-3 border-t border-white/10 px-4 py-2">
              <p className="text-xs text-anvil-300">{pullMsg}</p>
              {pullOperationId && (
                <Button
                  type="button"
                  size="sm"
                  variant="danger"
                  onClick={() => void cancelPull()}
                >
                  {t("common.cancel")}
                </Button>
              )}
            </div>
          )}
        </>
      )}
    </Card>
  );
}

function FileGlyph({ directory }: { directory: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4 shrink-0 text-anvil-400"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      {directory ? (
        <>
          <path d="M3.5 6.5h6l2 2H20a1.5 1.5 0 0 1 1.5 1.5v7.5A1.5 1.5 0 0 1 20 19H4a1.5 1.5 0 0 1-1.5-1.5V8A1.5 1.5 0 0 1 4 6.5Z" />
          <path d="M3.5 10h18" />
        </>
      ) : (
        <>
          <path d="M7 3.5h7l3 3V20a.5.5 0 0 1-.5.5h-9A.5.5 0 0 1 7 20V3.5Z" />
          <path d="M14 3.5v3h3" />
          <path d="M9.5 11h5M9.5 14h5" />
        </>
      )}
    </svg>
  );
}

function NetworkInspector({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [connections, setConnections] = useState<NetworkConnection[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const conns = await callListNetworkConnections(target);
      setConnections(conns);
    } catch (e) {
      setConnections([]);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [target]);

  const filtered = connections.filter((c) =>
    search
      ? c.local_addr.includes(search) ||
        c.remote_addr.includes(search) ||
        (c.process?.includes(search) ?? false) ||
        c.state.toLowerCase().includes(search.toLowerCase())
      : true,
  );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.controls.networkConnections")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.controls.networkBody")} <code>ss -tunp</code>.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <FieldInput
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("devices.controls.filter")}
            aria-label={t("devices.controls.filterConnections")}
            className="h-8 w-40 px-2 font-mono text-xs"
          />
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => void refresh()}
            disabled={loading}
          >
            {loading
              ? t("devices.controls.loading")
              : connections.length > 0
                ? t("devices.controls.refresh")
                : t("devices.controls.load")}
          </Button>
        </div>
      </div>
      {error && (
        <div className="border-b border-rose-500/20 bg-rose-500/10 px-4 py-3 text-xs text-rose-200">
          {t("devices.controls.connectionsReadFailed", { message: error })}
        </div>
      )}
      {connections.length === 0 && !loading && !error && (
        <EmptyState title={t("devices.controls.noConnections")}>
          <p>{t("devices.controls.noConnectionsBody")}</p>
        </EmptyState>
      )}
      {connections.length > 0 && filtered.length === 0 && (
        <EmptyState title={t("devices.controls.noMatchingConnections")}>
          <p>{t("devices.controls.noMatchingConnectionsBody")}</p>
        </EmptyState>
      )}
      {connections.length > 0 && filtered.length > 0 && (
        <div className="max-h-80 overflow-x-auto">
          <table className="min-w-full text-xs">
            <thead className="sticky top-0 bg-anvil-900">
              <tr>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colProto")}
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colState")}
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colLocal")}
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colRemote")}
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colProcess")}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {filtered.slice(0, 100).map((c, i) => (
                <tr
                  key={`${c.protocol}-${c.local_addr}-${c.remote_addr}-${i}`}
                  className="hover:bg-white/[0.03]"
                >
                  <td className="px-3 py-1.5 font-mono text-anvil-300">
                    {c.protocol}
                  </td>
                  <td className="px-3 py-1.5 text-anvil-200">{c.state}</td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    {c.local_addr}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    {c.remote_addr}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-400">
                    {c.process ?? t("devices.controls.notReported")}
                    {c.parse_error && (
                      <Badge tone="warning" className="ml-2">
                        {t("devices.controls.parseIssue")}
                      </Badge>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {filtered.length > 100 && (
            <p className="px-3 py-2 text-xs text-anvil-500">
              {t("devices.controls.showingConnections", {
                count: filtered.length,
              })}
            </p>
          )}
        </div>
      )}
    </Card>
  );
}

function AuthorizePrompt({
  devices,
  onRefresh,
}: {
  devices: ListDevicesResult["devices"];
  onRefresh: () => void;
}) {
  const { t } = useTranslation();

  return (
    <Card className="mt-4 border-amber-300/20 bg-amber-950/20 p-5">
      <div className="flex gap-4">
        <span
          className="mt-1 h-2.5 w-2.5 shrink-0 rounded-sm bg-amber-300 ring-4 ring-amber-300/10"
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-anvil-50">
            {devices.length === 1
              ? t("devices.authorize")
              : t("devices.authorizeMultiple", { count: devices.length })}
          </h3>
          <div className="mt-3 text-sm leading-6 text-anvil-300">
            <p>
              {devices.length === 1
                ? t("devices.authorizeOneBody", {
                    serial: devices[0]!.serial,
                  })
                : t("devices.authorizeManyBody")}
            </p>
            {devices.length > 1 && (
              <ul className="mt-2 space-y-1">
                {devices.map((d) => (
                  <li key={d.serial}>
                    <code className="font-mono text-xs text-anvil-100">
                      {d.serial}
                    </code>
                    {d.model && (
                      <span className="ml-2 text-xs text-anvil-400">
                        ({d.model})
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
            <div className="mt-4">
              <p className="text-xs font-semibold text-anvil-200">
                {t("devices.authorizeSteps")}
              </p>
              <ol className="mt-2 grid gap-2 text-sm sm:grid-cols-3">
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">1.</span>{" "}
                  {t("devices.step1")}
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">2.</span>{" "}
                  {t("devices.step2")}
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">3.</span>{" "}
                  {t("devices.step3")}
                </li>
              </ol>
            </div>
            <div className="mt-4 rounded-md border border-white/10 bg-white/[0.04] p-3">
              <p className="text-xs font-medium text-anvil-400">
                {t("devices.noDialog")}
              </p>
              <ul className="mt-2 space-y-1 text-xs text-anvil-400">
                <li>
                  {t("devices.revokeAuthorizations")}{" "}
                  <code className="text-anvil-200">
                    Settings → Developer options → Revoke USB debugging
                    authorizations
                  </code>
                </li>
                <li>{t("devices.reconnectUsb")}</li>
                <li>{t("devices.fileTransferMode")}</li>
              </ul>
            </div>
          </div>
          <div className="mt-4">
            <Button
              type="button"
              size="sm"
              variant="primary"
              onClick={onRefresh}
            >
              {t("devices.refresh")}
            </Button>
          </div>
        </div>
      </div>
    </Card>
  );
}

function deviceStateTone(
  state: SerializedDeviceState,
): "neutral" | "info" | "success" | "warning" | "danger" {
  if (typeof state !== "string") {
    return "neutral";
  }

  if (state === "device") {
    return "success";
  }

  if (state === "bootloader" || state === "recovery" || state === "sideload") {
    return "info";
  }

  if (state === "unauthorized" || state === "offline") {
    return "warning";
  }

  if (state === "no_permissions") {
    return "danger";
  }

  return "neutral";
}
