import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "../lib/cn";
import {
  errorMessage,
  callApplyDeviceControl,
  callApplyRemoteFileMutation,
  callCancelOperation,
  callGetDeviceInfo,
  callListRemoteFiles,
  callPullFile,
  callPushFile,
  callPlanRemoteFileMutation,
  callDisconnectDevice,
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
  type HostPathGrant,
  type ListDevicesResult,
  type OperationEvent,
  type RemoteFileEntry,
  type RemoteFileMutationPlan,
  type RemoteFileMutationRequest,
  type RemoteListing,
  type SerializedDeviceState,
} from "../lib/tauri";
import { useFocusTrap } from "../lib/useFocusTrap";
import { formatDateTime } from "../lib/i18n";
import { useTransportAuthorization } from "../lib/useAuthorizedDevices";
import { restartDeviceLifecycle, useDeviceStore } from "../lib/deviceStore";

import {
  Badge,
  Button,
  Card,
  EmptyState,
  FieldInput,
  FieldSelect,
  FieldTextArea,
  PaneHeader,
  RevealInFolderButton,
  SkeletonLine,
  StatePanel,
  TableCell,
  TableHeaderCell,
  TransportBadge,
  TransportTrustNotice,
} from "./common";
import HostDoctor from "./HostDoctor";
import { ADB_RECOVERY_COMMANDS, formatAdbDiagnostics } from "./adbHealth";
import { NetworkInspector } from "./devices/NetworkInspector";
import { LayoutInspector } from "./devices/LayoutInspector";
import { ProcessManager } from "./devices/ProcessManager";
import {
  formatBytes,
  formatKb,
  statusToneClass,
  type StatusMessage as DeviceStatusMessage,
} from "./devices/common";

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
                <li className="border-l border-white/10 pl-3 first:border-l-0 first:pl-0">
                  {t("devices.noDevicesStep1")}
                </li>
                <li className="border-l border-white/10 pl-3 first:border-l-0 first:pl-0">
                  {t("devices.noDevicesStep2Prefix")}{" "}
                  <em>{t("devices.allowUsbDebugging")}</em>{" "}
                  {t("devices.noDevicesStep2Suffix")}
                </li>
                <li className="border-l border-white/10 pl-3 first:border-l-0 first:pl-0">
                  {t("devices.noDevicesStep3")}
                </li>
              </ol>
            </StatePanel>
          )}

        {detail.kind !== "idle" && (
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
                      <span className="ml-2 text-anvil-400">
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

function DeviceHeaderActions({
  refreshing,
  onRefresh,
  onReviewRecovery,
}: {
  refreshing: boolean;
  onRefresh: () => void;
  onReviewRecovery: () => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [open]);

  return (
    <div className="flex items-center gap-2">
      <Button
        type="button"
        onClick={onRefresh}
        disabled={refreshing}
        variant="primary"
      >
        <RefreshIcon spinning={refreshing} />
        {refreshing ? t("devices.scanning") : t("devices.refresh")}
      </Button>
      <div ref={menuRef} className="relative">
        <Button
          type="button"
          variant="secondary"
          aria-label={t("devices.moreActions")}
          aria-haspopup="menu"
          aria-expanded={open}
          className="w-10 px-0"
          onClick={() => setOpen((value) => !value)}
        >
          <MoreIcon />
        </Button>
        {open && (
          <div
            role="menu"
            className="absolute right-0 z-20 mt-2 min-w-48 rounded-lg border border-white/[0.09] bg-[#181d24] p-1.5 shadow-2xl"
          >
            <button
              type="button"
              role="menuitem"
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm text-anvil-200 transition hover:bg-white/[0.07] hover:text-anvil-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300"
              onClick={() => {
                setOpen(false);
                onReviewRecovery();
              }}
            >
              <RecoveryIcon />
              {t("devices.health.reviewRecovery")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function RefreshIcon({ spinning }: { spinning: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={cn("h-4 w-4", spinning && "animate-spin")}
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      <path d="M20 11a8 8 0 0 0-14.8-4M4 4v5h5M4 13a8 8 0 0 0 14.8 4M20 20v-5h-5" />
    </svg>
  );
}

function MoreIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="currentColor"
      aria-hidden="true"
    >
      <circle cx="12" cy="5" r="1.5" />
      <circle cx="12" cy="12" r="1.5" />
      <circle cx="12" cy="19" r="1.5" />
    </svg>
  );
}

function RecoveryIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      <path d="M5 7v5h5M19 17v-5h-5" />
      <path d="M7.1 17A8 8 0 0 0 19 12M16.9 7A8 8 0 0 0 5 12" />
    </svg>
  );
}

function HealthCheckIcon({ healthy }: { healthy: boolean }) {
  return (
    <span
      className={cn(
        "flex h-5 w-5 shrink-0 items-center justify-center rounded-full border",
        healthy
          ? "border-emerald-300 text-emerald-300"
          : "border-amber-300 text-amber-300",
      )}
      aria-hidden="true"
    >
      {healthy ? (
        <svg
          viewBox="0 0 20 20"
          className="h-3 w-3"
          fill="none"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="2"
        >
          <path d="m5 10 3 3 7-7" />
        </svg>
      ) : (
        <span className="h-1.5 w-1.5 rounded-full bg-current" />
      )}
    </span>
  );
}

function AdbHealthPanel({
  health,
  observedAt,
  watching,
}: {
  health: AdbHealth | null;
  observedAt: string | null;
  watching: boolean;
}) {
  const { t, i18n } = useTranslation();
  return (
    <section
      className="border-b border-white/[0.08] py-5"
      aria-labelledby="adb-health-title"
    >
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-2.5">
          <HealthCheckIcon healthy={watching} />
          <div>
            <h3
              id="adb-health-title"
              className="text-sm font-semibold text-anvil-50"
            >
              {t("devices.health.title")}
            </h3>
            {!watching && (
              <p className="mt-0.5 text-xs text-amber-200">
                {t("devices.health.stopped")}
              </p>
            )}
          </div>
        </div>
        <p className="text-xs text-anvil-500">
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

      {health ? (
        <dl className="mt-4 grid gap-y-4 text-xs sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-8">
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
          className="mt-4 border-l-2 border-amber-300/70 pl-3 text-xs text-amber-100"
        >
          {health.warning}
        </p>
      )}
      {health && (
        <p
          className={cn(
            "mt-3 border-l-2 pl-3 text-xs",
            health.platform_tools.status === "blocked"
              ? "border-red-300/70 text-red-100"
              : health.platform_tools.status === "warn"
                ? "border-amber-300/70 text-amber-100"
                : "border-emerald-300/70 text-anvil-300",
          )}
        >
          {health.platform_tools.rationale}{" "}
          <a
            className="font-medium underline underline-offset-2"
            href={health.platform_tools.source_url}
            target="_blank"
            rel="noreferrer"
          >
            {t("devices.health.policyLink", {
              date: health.platform_tools.policy_reviewed_on,
            })}
          </a>
        </p>
      )}
    </section>
  );
}

function HealthMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 border-l border-white/[0.08] px-3 first:border-l-0 first:pl-0">
      <dt className="text-[11px] leading-4 text-anvil-500">{label}</dt>
      <dd className="mt-1 break-words text-[13px] font-medium leading-5 text-anvil-100">
        {value}
      </dd>
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
            <FieldTextArea
              id="adb-recovery-diagnostics"
              readOnly
              value={diagnostics}
              rows={11}
              className="mt-2 resize-y bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200"
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

function DeviceToolbar({
  devices,
  selectedDeviceKey,
  onSelect,
}: {
  devices: ListDevicesResult["devices"];
  selectedDeviceKey: string | null;
  onSelect: (device: Device) => void;
}) {
  const { t } = useTranslation();
  const selectable = devices.filter(
    (device) => typeof device.state === "string" && device.state === "device",
  );

  return (
    <div className="mt-4 flex flex-col gap-3 border-b border-white/[0.08] pb-4 sm:flex-row sm:items-center">
      <FieldSelect
        aria-label={t("common.selectDevice")}
        value={selectedDeviceKey ?? ""}
        onChange={(event) => {
          const device = selectable.find(
            (candidate) =>
              String(candidate.transport_id ?? candidate.serial) ===
              event.currentTarget.value,
          );
          if (device) onSelect(device);
        }}
        className="min-w-64 font-medium"
      >
        <option value="" disabled>
          {t("common.selectDevice")}
        </option>
        {selectable.map((device) => (
          <option
            key={
              String(device.transport_id ?? device.serial) +
              ":" +
              device.connection_generation
            }
            value={String(device.transport_id ?? device.serial)}
          >
            {device.model ?? device.serial}
          </option>
        ))}
      </FieldSelect>
      <span className="hidden h-7 w-px bg-white/[0.08] sm:block" />
      <Badge tone="success">{t("devices.adbReady")}</Badge>
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
    <section className="border-t border-white/[0.08] pt-5">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold text-anvil-50">
          {t("devices.connected")}
        </h3>
        <span className="text-xs text-anvil-500">
          {t("common.deviceCount", { count: devices.length })}
        </span>
      </div>
      <div className="overflow-x-auto rounded-md border border-white/[0.08]">
        <table className="min-w-full text-sm">
          <thead className="bg-white/[0.025]">
            <tr>
              <TableHeaderCell className="w-14">
                <span className="sr-only">{t("common.selectDevice")}</span>
              </TableHeaderCell>
              <TableHeaderCell>{t("devices.serial")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.identity")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.transport")}</TableHeaderCell>
              <TableHeaderCell className="w-14">
                <span className="sr-only">{t("devices.moreActions")}</span>
              </TableHeaderCell>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/[0.07]">
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
                  ].join(" ")}
                >
                  <TableCell className="pr-0">
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
                      className="flex h-7 w-7 items-center justify-center rounded-full text-anvil-600 transition hover:text-anvil-300 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 focus-visible:ring-offset-2 focus-visible:ring-offset-anvil-900"
                    >
                      <SelectionIcon selected={isSelected} />
                    </button>
                  </TableCell>
                  <TableCell>
                    <div className="flex min-w-[10rem] items-center gap-2">
                      <code className="font-mono text-xs text-anvil-50">
                        {device.serial}
                      </code>
                      <TransportBadge kind={device.transport_kind} />
                    </div>
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
                  <TableCell className="pl-0 text-right">
                    <button
                      type="button"
                      disabled={!isDevice}
                      aria-label={t("devices.moreDeviceActions", {
                        device: device.model ?? device.serial,
                      })}
                      onClick={() => onSelect(device)}
                      className="inline-flex h-8 w-8 items-center justify-center rounded-md text-anvil-500 transition hover:bg-white/[0.07] hover:text-anvil-100 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300"
                    >
                      <MoreIcon />
                    </button>
                  </TableCell>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function SelectionIcon({ selected }: { selected: boolean }) {
  if (!selected) {
    return (
      <span
        className="h-5 w-5 rounded-full border border-white/[0.14]"
        aria-hidden="true"
      />
    );
  }

  return (
    <span
      className="flex h-6 w-6 items-center justify-center rounded-full bg-circuit-300 text-anvil-950 shadow-sm"
      aria-hidden="true"
    >
      <svg
        viewBox="0 0 20 20"
        className="h-3.5 w-3.5"
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2"
      >
        <path d="m5 10 3 3 7-7" />
      </svg>
    </span>
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
  const [disconnectMessage, setDisconnectMessage] = useState<string | null>(
    null,
  );
  const doDisconnect = useCallback(
    async (target: DeviceTarget) => {
      try {
        const result = await callDisconnectDevice(target);
        if (result.disconnected) {
          setDisconnectMessage(t("devices.disconnectSuccess"));
        } else {
          setDisconnectMessage(result.message);
        }
      } catch (error) {
        setDisconnectMessage(
          t("devices.disconnectFailed", {
            message: errorMessage(error),
          }),
        );
      }
    },
    [t],
  );

  if (state.kind === "idle") return null;

  if (state.kind === "loading") {
    return (
      <section className="border-b border-white/[0.08] py-5">
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
      </section>
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
    <section
      className="border-b border-white/[0.08] py-5"
      aria-labelledby="device-details-title"
    >
      <div className="flex items-center justify-between">
        <h3
          id="device-details-title"
          className="text-sm font-semibold text-anvil-50"
        >
          {t("devices.detailTitle")}
        </h3>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => void doDisconnect(state.target)}
        >
          {t("devices.disconnect")}
        </Button>
      </div>
      {disconnectMessage && (
        <p role="status" className="mt-2 text-xs text-anvil-300">
          {disconnectMessage}
        </p>
      )}
      <dl className="mt-4 grid gap-x-10 gap-y-4 sm:grid-cols-2 lg:grid-cols-3">
        <InfoField
          label={t("devices.model")}
          value={info.model ?? t("devices.unknownModel")}
        />
        {info.manufacturer && (
          <InfoField
            label={t("devices.manufacturer")}
            value={info.manufacturer}
          />
        )}
        {info.android_version && (
          <InfoField
            label={t("devices.androidVersion")}
            value={`Android ${info.android_version}`}
          />
        )}
        {info.sdk_level && (
          <InfoField
            label={t("devices.apiLevelLabel")}
            value={t("devices.apiLevel", { level: info.sdk_level })}
          />
        )}
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
            value={formatBattery(info.battery, t("common.unknown"))}
          />
        )}
        {info.storage && (
          <InfoField
            label={t("devices.storageData")}
            value={formatStorage(info.storage, t("common.unknown"))}
          />
        )}
      </dl>
      <DeviceHealthCards info={info} />
    </section>
  );
}

function DeviceHealthCards({ info }: { info: DeviceInfo }) {
  const { t } = useTranslation();
  const battery = info.battery;
  const partitions = info.storage_partitions;
  const zones = info.thermal_zones;

  const hasBatteryHealth =
    battery != null &&
    (battery.health != null ||
      battery.cycle_count != null ||
      battery.voltage_mv != null ||
      battery.charge_counter_uah != null ||
      battery.technology != null);

  if (!hasBatteryHealth && partitions.length === 0 && zones.length === 0) {
    return null;
  }

  return (
    <div className="mt-6">
      <h4 className="text-xs font-semibold uppercase tracking-wide text-anvil-400">
        {t("devices.deviceHealth.sectionTitle")}
      </h4>
      <div className="mt-3 grid gap-4 lg:grid-cols-3">
        {hasBatteryHealth && battery && (
          <Card>
            <h5 className="text-sm font-semibold text-anvil-50">
              {t("devices.deviceHealth.batteryTitle")}
            </h5>
            {battery.level != null && (
              <HealthBar
                label={t("devices.deviceHealth.level")}
                value={`${battery.level}%`}
                fraction={battery.level / 100}
              />
            )}
            <dl className="mt-3 space-y-1.5 text-xs">
              {battery.health && (
                <HealthRow
                  label={t("devices.deviceHealth.healthLabel")}
                  value={battery.health}
                />
              )}
              {battery.cycle_count != null && (
                <HealthRow
                  label={t("devices.deviceHealth.cycleCount")}
                  value={String(battery.cycle_count)}
                />
              )}
              {battery.charge_counter_uah != null && (
                <HealthRow
                  label={t("devices.deviceHealth.capacity")}
                  value={`${Math.round(battery.charge_counter_uah / 1000)} mAh`}
                />
              )}
              {battery.voltage_mv != null && (
                <HealthRow
                  label={t("devices.deviceHealth.voltage")}
                  value={`${(battery.voltage_mv / 1000).toFixed(3)} V`}
                />
              )}
              {battery.technology && (
                <HealthRow
                  label={t("devices.deviceHealth.technology")}
                  value={battery.technology}
                />
              )}
            </dl>
          </Card>
        )}
        {partitions.length > 0 && (
          <Card>
            <h5 className="text-sm font-semibold text-anvil-50">
              {t("devices.deviceHealth.storageTitle")}
            </h5>
            <div className="mt-2 space-y-3">
              {partitions.map((partition) => {
                const fraction = partitionUsedFraction(partition);
                const detail = formatPartitionDetail(partition, t);
                return (
                  <HealthBar
                    key={partition.mount}
                    label={partition.mount}
                    value={detail}
                    fraction={fraction}
                  />
                );
              })}
            </div>
          </Card>
        )}
        {zones.length > 0 && (
          <Card>
            <h5 className="text-sm font-semibold text-anvil-50">
              {t("devices.deviceHealth.thermalTitle")}
            </h5>
            <dl className="mt-2 space-y-1.5 text-xs">
              {zones.map((zone) => (
                <div
                  key={zone.name}
                  className="flex items-center justify-between gap-3"
                >
                  <dt className="min-w-0 truncate text-anvil-300">
                    {zone.name}
                  </dt>
                  <dd className="flex items-center gap-2 text-anvil-100">
                    <span className="font-mono">
                      {zone.temperature_c.toFixed(1)}°C
                    </span>
                    {zone.status && zone.status !== "None" && (
                      <Badge tone={thermalTone(zone.status)}>
                        {zone.status}
                      </Badge>
                    )}
                  </dd>
                </div>
              ))}
            </dl>
          </Card>
        )}
      </div>
    </div>
  );
}

function HealthRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <dt className="text-anvil-400">{label}</dt>
      <dd className="text-anvil-100">{value}</dd>
    </div>
  );
}

function HealthBar({
  label,
  value,
  fraction,
}: {
  label: string;
  value: string;
  fraction: number | null;
}) {
  const pct =
    fraction == null ? null : Math.min(100, Math.max(0, fraction * 100));
  return (
    <div className="mt-2">
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="min-w-0 truncate font-medium text-anvil-200">
          {label}
        </span>
        <span className="text-anvil-400">{value}</span>
      </div>
      <div
        className="mt-1 h-1.5 overflow-hidden rounded-full bg-white/[0.08]"
        role="progressbar"
        aria-label={label}
        aria-valuenow={pct == null ? undefined : Math.round(pct)}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div
          className={cn(
            "h-full rounded-full transition-[width]",
            pct != null && pct >= 90 ? "bg-red-400/70" : "bg-circuit-300/70",
          )}
          style={{ width: `${pct ?? 0}%` }}
        />
      </div>
    </div>
  );
}

function partitionUsedFraction(
  p: DeviceInfo["storage_partitions"][number],
): number | null {
  if (p.total_kb == null || p.total_kb === 0) return null;
  if (p.used_kb != null) return p.used_kb / p.total_kb;
  if (p.available_kb != null) return 1 - p.available_kb / p.total_kb;
  return null;
}

function formatPartitionDetail(
  p: DeviceInfo["storage_partitions"][number],
  t: (key: string, opts?: Record<string, unknown>) => string,
): string {
  const total = p.total_kb != null ? formatKb(p.total_kb) : null;
  if (p.used_kb != null && total) {
    return t("devices.deviceHealth.usedOfTotal", {
      used: formatKb(p.used_kb),
      total,
    });
  }
  if (p.available_kb != null) {
    return t("devices.deviceHealth.freeSpace", {
      free: formatKb(p.available_kb),
    });
  }
  return total ?? "";
}

function thermalTone(status: string): "warning" | "danger" {
  return status === "Light" || status === "Moderate" ? "warning" : "danger";
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

function formatBattery(
  b: NonNullable<DeviceInfo["battery"]>,
  unknown: string,
): string {
  const parts: string[] = [];
  if (b.level != null) parts.push(`${b.level}%`);
  if (b.status) parts.push(b.status);
  if (b.temperature != null) parts.push(`${b.temperature}°C`);
  return parts.join(" · ") || unknown;
}

function formatStorage(
  s: NonNullable<DeviceInfo["storage"]>,
  unknown: string,
): string {
  if (s.total_kb == null || s.available_kb == null) return unknown;
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

type StatusMessage = DeviceStatusMessage;

// Inline device-control results confirm real mutations, so a success and a
// failure must not read as the same faint line.
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
  const [screenshotMsg, setScreenshotMsg] = useState<StatusMessage>(null);
  const [screenshotPath, setScreenshotPath] = useState<string | null>(null);
  const [density, setDensity] = useState("");
  const [displayMsg, setDisplayMsg] = useState<StatusMessage>(null);

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
      setScreenshotPath(null);
      setScreenshotMsg({
        text: t("devices.controls.capturing"),
        tone: "neutral",
      });
      const artifact = await callTakeScreenshot(operationTarget, pathGrant.id);
      setScreenshotMsg({
        text: t("devices.controls.savedTo", { path: artifact.local_path }),
        tone: "success",
      });
      setScreenshotPath(artifact.local_path);
    } catch (e) {
      setScreenshotPath(null);
      setScreenshotMsg({
        tone: "danger",
        text: t("devices.controls.failed", {
          message: errorMessage(e),
        }),
      });
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
      setDisplayMsg({
        text: t("devices.controls.densitySet", { value: density.trim() }),
        tone: "success",
      });
    } catch (e) {
      setDisplayMsg({
        tone: "danger",
        text: t("devices.controls.failed", {
          message: errorMessage(e),
        }),
      });
    }
  }, [operationTarget, density, t]);

  const resetDensity = useCallback(async () => {
    try {
      await callApplyDeviceControl(operationTarget, ["wm", "density", "reset"]);
      setDisplayMsg({
        text: t("devices.controls.densityReset"),
        tone: "success",
      });
    } catch (e) {
      setDisplayMsg({
        tone: "danger",
        text: t("devices.controls.failed", {
          message: errorMessage(e),
        }),
      });
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
        setDisplayMsg({
          text: enable
            ? t("devices.controls.forceDarkEnabled")
            : t("devices.controls.forceDarkDisabled"),
          tone: "success",
        });
      } catch (e) {
        setDisplayMsg({
          tone: "danger",
          text: t("devices.controls.failed", {
            message: errorMessage(e),
          }),
        });
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
                <span
                  role="status"
                  className={`text-xs ${statusToneClass(screenshotMsg.tone)}`}
                >
                  {screenshotMsg.text}
                </span>
              )}
              {screenshotPath && <RevealInFolderButton path={screenshotPath} />}
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
              <p
                role="status"
                className={`mt-3 text-xs ${statusToneClass(displayMsg.tone)}`}
              >
                {displayMsg.text}
              </p>
            )}
          </Card>
        </div>
      </div>
      <ProcessManager target={operationTarget} />
      <FileManager target={operationTarget} />
      <NetworkInspector target={operationTarget} />
      <LayoutInspector target={operationTarget} />
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

type FileNameDraft =
  | { kind: "mkdir"; value: string }
  | { kind: "rename"; entry: RemoteFileEntry; value: string };

type FileReview =
  | {
      kind: "mutation";
      request: RemoteFileMutationRequest;
      plan: RemoteFileMutationPlan;
    }
  | { kind: "push"; grant: HostPathGrant; remotePath: string };

function FileManager({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [listing, setListing] = useState<RemoteListing | null>(null);
  const [currentPath, setCurrentPath] = useState("/sdcard");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<FileNameDraft | null>(null);
  const [review, setReview] = useState<FileReview | null>(null);
  const [reviewBusy, setReviewBusy] = useState(false);
  const [operationMessage, setOperationMessage] = useState<string | null>(null);
  const [fileOperationId, setFileOperationId] = useState<string | null>(null);
  const [pullMsg, setPullMsg] = useState<StatusMessage>(null);
  const [pullPath, setPullPath] = useState<string | null>(null);
  const [pullOperationId, setPullOperationId] = useState<string | null>(null);
  const pullOperationRef = useRef<string | null>(null);
  const pullGenerationRef = useRef(0);
  const fileOperationRef = useRef<string | null>(null);
  const draftTrapRef = useFocusTrap<HTMLDivElement>(draft !== null);
  const reviewTrapRef = useFocusTrap<HTMLDivElement>(review !== null);

  useEffect(() => {
    return () => {
      pullGenerationRef.current += 1;
      const operationId = pullOperationRef.current;
      pullOperationRef.current = null;
      if (operationId) void callCancelOperation(operationId);
      const fileOperationId = fileOperationRef.current;
      fileOperationRef.current = null;
      if (fileOperationId) void callCancelOperation(fileOperationId);
    };
  }, [target.serial, target.transport_id, target.connection_generation]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || reviewBusy) return;
      if (review) setReview(null);
      else if (draft) setDraft(null);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [draft, review, reviewBusy]);

  const remotePathFor = useCallback(
    (name: string) =>
      currentPath === "/" ? `/${name}` : `${currentPath}/${name}`,
    [currentPath],
  );

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
        setError(errorMessage(e));
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
        setPullPath(null);
        setPullMsg({
          text: t("devices.controls.pulling", { name: entry.name }),
          tone: "neutral",
        });
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
              setPullMsg({
                tone: "neutral",
                text: t("devices.controls.pullProgress", {
                  name: entry.name,
                  seconds: Math.max(
                    1,
                    Math.round((event.elapsed_ms ?? 0) / 1000),
                  ),
                }),
              });
            }
          },
        });
        if (pullGenerationRef.current !== generation) return;
        pullOperationRef.current = null;
        setPullOperationId(null);
        setPullMsg({
          tone: "success",
          text: t("devices.controls.savedName", {
            name: entry.name,
            path: artifact.local_path,
          }),
        });
        setPullPath(artifact.local_path);
      } catch (e) {
        if (
          operationId &&
          (pullGenerationRef.current !== generation ||
            pullOperationRef.current !== operationId)
        )
          return;
        pullOperationRef.current = null;
        setPullOperationId(null);
        setPullPath(null);
        setPullMsg({
          tone: "danger",
          text: t("devices.controls.failed", {
            message: errorMessage(e),
          }),
        });
      }
    },
    [target, currentPath, t],
  );

  const cancelPull = useCallback(async () => {
    const operationId = pullOperationRef.current;
    if (!operationId) return;
    setPullMsg({
      text: t("devices.controls.pullCancelling"),
      tone: "neutral",
    });
    await callCancelOperation(operationId);
  }, [t]);

  const stageMutation = useCallback(
    async (request: RemoteFileMutationRequest) => {
      setOperationMessage(null);
      try {
        const plan = await callPlanRemoteFileMutation(request);
        setReview({ kind: "mutation", request, plan });
      } catch (e) {
        setOperationMessage(
          t("devices.controls.fileOperationFailed", {
            message: errorMessage(e),
          }),
        );
      }
    },
    [t],
  );

  const submitDraft = useCallback(async () => {
    if (!draft) return;
    const name = draft.value.trim();
    if (
      !name ||
      name === "." ||
      name === ".." ||
      name.includes("/") ||
      name.includes("\\")
    ) {
      setOperationMessage(t("devices.controls.invalidFileName"));
      return;
    }
    const request: RemoteFileMutationRequest =
      draft.kind === "mkdir"
        ? {
            kind: "mkdir",
            source_path: remotePathFor(name),
            destination_path: null,
          }
        : {
            kind: "rename",
            source_path: remotePathFor(draft.entry.name),
            destination_path: remotePathFor(name),
          };
    setDraft(null);
    await stageMutation(request);
  }, [draft, remotePathFor, stageMutation, t]);

  const stagePush = useCallback(async () => {
    setOperationMessage(null);
    try {
      const grant = await callSelectHostPath("push_open");
      if (!grant) return;
      const fileName = grant.local_path.split(/[\\/]/u).pop()?.trim();
      if (!fileName) {
        setOperationMessage(t("devices.controls.invalidFileName"));
        return;
      }
      setReview({
        kind: "push",
        grant,
        remotePath: remotePathFor(fileName),
      });
    } catch (e) {
      setOperationMessage(
        t("devices.controls.fileOperationFailed", {
          message: errorMessage(e),
        }),
      );
    }
  }, [remotePathFor, t]);

  const confirmReview = useCallback(async () => {
    if (!review) return;
    setReviewBusy(true);
    setOperationMessage(t("devices.controls.applyingFileOperation"));
    let operationId: string | null = null;
    try {
      if (review.kind === "mutation") {
        await callApplyRemoteFileMutation(target, review.request, true);
      } else {
        operationId = newOperationId("push");
        fileOperationRef.current = operationId;
        setFileOperationId(operationId);
        await callPushFile(target, review.grant.id, review.remotePath, true, {
          operationId,
          onEvent: (event: OperationEvent) => {
            if (fileOperationRef.current !== operationId) return;
            if (event.kind === "progress") {
              setOperationMessage(
                t("devices.controls.pushProgress", {
                  seconds: Math.max(
                    1,
                    Math.round((event.elapsed_ms ?? 0) / 1000),
                  ),
                }),
              );
            }
          },
        });
      }
      fileOperationRef.current = null;
      setFileOperationId(null);
      setReview(null);
      await browse(currentPath);
      setOperationMessage(t("devices.controls.fileOperationComplete"));
    } catch (e) {
      if (operationId && fileOperationRef.current !== operationId) return;
      fileOperationRef.current = null;
      setFileOperationId(null);
      setOperationMessage(
        t("devices.controls.fileOperationFailed", {
          message: errorMessage(e),
        }),
      );
    } finally {
      setReviewBusy(false);
    }
  }, [browse, currentPath, review, t, target]);

  const cancelFileOperation = useCallback(async () => {
    const operationId = fileOperationRef.current;
    if (!operationId) return;
    setOperationMessage(t("devices.controls.pushCancelling"));
    await callCancelOperation(operationId);
  }, [t]);

  return (
    <>
      {draft && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
          <div
            ref={draftTrapRef}
            role="dialog"
            aria-modal="true"
            aria-labelledby="file-name-dialog-title"
            tabIndex={-1}
            className="w-full max-w-md rounded-lg border border-white/10 bg-anvil-950 p-5 shadow-2xl outline-none"
          >
            <h3
              id="file-name-dialog-title"
              className="text-lg font-semibold text-anvil-50"
            >
              {draft.kind === "mkdir"
                ? t("devices.controls.newFolderTitle")
                : t("devices.controls.renameTitle")}
            </h3>
            <p className="mt-2 text-sm text-anvil-300">
              {t("devices.controls.fileNameBody", { path: currentPath })}
            </p>
            <label className="mt-4 block text-xs font-medium text-anvil-300">
              {t("devices.controls.fileName")}
              <FieldInput
                autoFocus
                className="mt-2 w-full font-mono"
                value={draft.value}
                onChange={(event) =>
                  setDraft({ ...draft, value: event.currentTarget.value })
                }
                onKeyDown={(event) => {
                  if (event.key === "Enter") void submitDraft();
                }}
              />
            </label>
            <div className="mt-5 flex justify-end gap-2">
              <Button type="button" onClick={() => setDraft(null)}>
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant="primary"
                onClick={() => void submitDraft()}
              >
                {t("devices.controls.reviewFileOperation")}
              </Button>
            </div>
          </div>
        </div>
      )}

      {review && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
          <div
            ref={reviewTrapRef}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="file-review-title"
            aria-describedby="file-review-body"
            tabIndex={-1}
            className="max-h-[90vh] w-full max-w-xl overflow-y-auto rounded-lg border border-white/10 bg-anvil-950 p-5 shadow-2xl outline-none"
          >
            <Badge
              tone={
                review.kind === "mutation" && review.plan.destructive
                  ? "danger"
                  : "warning"
              }
            >
              {t("devices.controls.fileChange")}
            </Badge>
            <h3
              id="file-review-title"
              className="mt-4 text-lg font-semibold text-anvil-50"
            >
              {t("devices.controls.reviewFileOperation")}
            </h3>
            <p
              id="file-review-body"
              className="mt-2 text-sm leading-6 text-anvil-300"
            >
              {review.kind === "mutation" && review.plan.destructive
                ? t("devices.controls.destructiveFileWarning")
                : t("devices.controls.fileReviewBody")}
            </p>
            <dl className="mt-4 space-y-3 text-xs">
              <div>
                <dt className="text-anvil-500">
                  {t("devices.controls.source")}
                </dt>
                <dd className="mt-1 break-all font-mono text-anvil-100">
                  {review.kind === "push"
                    ? review.grant.local_path
                    : review.plan.source_path}
                </dd>
              </div>
              {(review.kind === "push" || review.plan.destination_path) && (
                <div>
                  <dt className="text-anvil-500">
                    {t("devices.controls.target")}
                  </dt>
                  <dd className="mt-1 break-all font-mono text-anvil-100">
                    {review.kind === "push"
                      ? review.remotePath
                      : review.plan.destination_path}
                  </dd>
                </div>
              )}
            </dl>
            <pre className="mt-4 overflow-auto whitespace-pre-wrap break-all rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs text-anvil-100">
              {review.kind === "push"
                ? `adb push ${JSON.stringify(review.grant.local_path)} ${JSON.stringify(review.remotePath)}`
                : `adb shell ${review.plan.argv.map((arg) => JSON.stringify(arg)).join(" ")}`}
            </pre>
            <div className="mt-5 flex justify-end gap-2">
              <Button
                type="button"
                onClick={() => setReview(null)}
                disabled={reviewBusy}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant={
                  review.kind === "mutation" && review.plan.destructive
                    ? "danger"
                    : "primary"
                }
                onClick={() => void confirmReview()}
                disabled={reviewBusy}
              >
                {reviewBusy
                  ? t("devices.controls.applyingFileOperation")
                  : t("devices.controls.confirmFileOperation")}
              </Button>
            </div>
          </div>
        </div>
      )}

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
            {listing && (
              <>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void stagePush()}
                >
                  {t("devices.controls.push")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => setDraft({ kind: "mkdir", value: "" })}
                >
                  {t("devices.controls.newFolder")}
                </Button>
              </>
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
          <div className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200">
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
                    {entry.is_dir
                      ? ""
                      : formatBytes(entry.size, t("common.unknown"))}
                  </span>
                  <span className="hidden shrink-0 font-mono text-anvil-600 sm:inline">
                    {entry.permissions}
                  </span>
                  {entry.parse_error && (
                    <Badge tone="warning" className="shrink-0">
                      {t("devices.controls.parseIssue")}
                    </Badge>
                  )}
                  {!entry.is_dir && !entry.parse_error && (
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={() => void pullRemote(entry)}
                    >
                      {t("devices.controls.pull")}
                    </Button>
                  )}
                  {!entry.parse_error && (
                    <>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={() =>
                          setDraft({ kind: "rename", entry, value: entry.name })
                        }
                      >
                        {t("devices.controls.rename")}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="danger"
                        onClick={() =>
                          void stageMutation({
                            kind: entry.is_dir
                              ? "delete_directory"
                              : "delete_file",
                            source_path: remotePathFor(entry.name),
                            destination_path: null,
                          })
                        }
                      >
                        {t("devices.controls.delete")}
                      </Button>
                    </>
                  )}
                </div>
              ))}
            </div>
            {pullMsg && (
              <div className="flex items-center justify-between gap-3 border-t border-white/10 px-4 py-2">
                <p className={`text-xs ${statusToneClass(pullMsg.tone)}`}>
                  {pullMsg.text}
                </p>
                {pullOperationId ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="danger"
                    onClick={() => void cancelPull()}
                  >
                    {t("common.cancel")}
                  </Button>
                ) : (
                  pullPath && <RevealInFolderButton path={pullPath} />
                )}
              </div>
            )}
            {operationMessage && (
              <div
                role="status"
                className="flex items-center justify-between gap-3 border-t border-white/10 px-4 py-2"
              >
                <p className="text-xs text-anvil-300">{operationMessage}</p>
                {fileOperationId && (
                  <Button
                    type="button"
                    size="sm"
                    variant="danger"
                    onClick={() => void cancelFileOperation()}
                  >
                    {t("common.cancel")}
                  </Button>
                )}
              </div>
            )}
          </>
        )}
      </Card>
    </>
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
