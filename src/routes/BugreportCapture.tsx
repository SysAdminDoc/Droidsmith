import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callCancelOperation,
  callCaptureBugreport,
  callSelectHostPath,
  deviceTarget,
  newOperationId,
  requiresTransportOverride,
  type BugreportCaptureResult,
  type DeviceTarget,
  type OperationEvent,
} from "../lib/tauri";
import {
  resolveAuthorizedTarget,
  sameDeviceTarget,
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";
import {
  Badge,
  Button,
  Card,
  StatePanel,
  TransportBadge,
  TransportTrustNotice,
} from "./common";

type CaptureState =
  | { kind: "idle" }
  | { kind: "selecting" }
  | { kind: "running" | "cancelling"; progress: string }
  | { kind: "success"; result: BugreportCaptureResult }
  | { kind: "error"; message: string };

export default function BugreportCapture() {
  const { t } = useTranslation();
  const { devicesState, authorizedDevices } = useAuthorizedDevices();
  const [selectedTarget, setSelectedTarget] = useState<DeviceTarget | null>(
    null,
  );
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(selectedTarget);
  const [reviewing, setReviewing] = useState(false);
  const [privacyAccepted, setPrivacyAccepted] = useState(false);
  const [state, setState] = useState<CaptureState>({ kind: "idle" });
  const operationRef = useRef<string | null>(null);

  const busy = ["selecting", "running", "cancelling"].includes(state.kind);

  useEffect(() => {
    if (busy) return;
    const next = resolveAuthorizedTarget(selectedTarget, authorizedDevices);
    if (!sameDeviceTarget(selectedTarget, next)) setSelectedTarget(next);
  }, [authorizedDevices, busy, selectedTarget]);

  useEffect(() => {
    return () => {
      const operationId = operationRef.current;
      operationRef.current = null;
      if (operationId) void callCancelOperation(operationId);
    };
  }, []);

  const startCapture = useCallback(async () => {
    if (!authorizedTarget || !privacyAccepted || busy) return;
    if (
      requiresTransportOverride(authorizedTarget) &&
      !transportOverrideAccepted
    )
      return;

    // Freeze the exact transport generation before opening the native save
    // dialog. A reconnect cannot silently retarget the eventual capture.
    const targetSnapshot = { ...authorizedTarget };
    setState({ kind: "selecting" });
    try {
      const date = new Date().toISOString().slice(0, 10);
      const grant = await callSelectHostPath(
        "bugreport_save",
        `droidsmith-bugreport-${date}.zip`,
      );
      if (!grant) {
        setState({ kind: "idle" });
        return;
      }

      const operationId = newOperationId("bugreport");
      operationRef.current = operationId;
      setState({ kind: "running", progress: t("bugreport.starting") });
      const result = await callCaptureBugreport(
        targetSnapshot,
        grant.id,
        true,
        {
          operationId,
          onEvent: (event: OperationEvent) => {
            if (operationRef.current !== operationId) return;
            if (event.kind === "progress" || event.kind === "started") {
              setState({
                kind: "running",
                progress: event.message ?? t("bugreport.running"),
              });
            }
          },
        },
      );
      if (operationRef.current !== operationId) return;
      operationRef.current = null;
      setReviewing(false);
      setPrivacyAccepted(false);
      setState({ kind: "success", result });
    } catch (error) {
      operationRef.current = null;
      setState({
        kind: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }, [authorizedTarget, busy, privacyAccepted, t, transportOverrideAccepted]);

  const cancelCapture = useCallback(async () => {
    const operationId = operationRef.current;
    if (!operationId) return;
    setState({ kind: "cancelling", progress: t("bugreport.cancelling") });
    await callCancelOperation(operationId);
  }, [t]);

  return (
    <Card className="p-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-base font-semibold text-anvil-50">
              {t("bugreport.title")}
            </h3>
            <Badge tone="warning">{t("bugreport.sensitive")}</Badge>
          </div>
          <p className="mt-2 max-w-3xl text-sm leading-6 text-anvil-300">
            {t("bugreport.description")}
          </p>
        </div>
        {selectedTarget && (
          <div className="flex flex-wrap gap-2">
            <Badge tone="info">
              {selectedTarget.model ?? selectedTarget.serial}
            </Badge>
            <TransportBadge kind={selectedTarget.transport_kind} />
          </div>
        )}
      </div>

      {devicesState.kind === "no_tauri" && (
        <div className="mt-4">
          <StatePanel title={t("common.desktopRequired")} tone="info">
            <p>{t("bugreport.desktopRequiredBody")}</p>
          </StatePanel>
        </div>
      )}

      {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
        <div className="mt-4">
          <StatePanel title={t("common.noAuthorized")} tone="warning">
            <p>{t("bugreport.noAuthorizedBody")}</p>
          </StatePanel>
        </div>
      )}

      {authorizedDevices.length > 1 && (
        <fieldset className="mt-4" disabled={busy}>
          <legend className="text-xs font-medium text-anvil-300">
            {t("common.targetDevice")}
          </legend>
          <div className="mt-2 flex flex-wrap gap-2">
            {authorizedDevices.map((device) => {
              const target = deviceTarget(device);
              const selected = sameDeviceTarget(target, selectedTarget);
              return (
                <Button
                  key={`${device.transport_id ?? device.serial}:${device.connection_generation}`}
                  type="button"
                  variant={selected ? "primary" : "secondary"}
                  size="sm"
                  onClick={() => {
                    setSelectedTarget(target);
                    setReviewing(false);
                    setPrivacyAccepted(false);
                    setState({ kind: "idle" });
                  }}
                >
                  {device.model ?? device.serial}
                </Button>
              );
            })}
          </div>
        </fieldset>
      )}

      <div className="mt-4">
        <TransportTrustNotice
          target={selectedTarget}
          accepted={transportOverrideAccepted}
          onAcceptedChange={setTransportOverrideAccepted}
        />
      </div>

      {selectedTarget && !reviewing && !busy && (
        <div className="mt-4">
          <Button
            type="button"
            variant="secondary"
            onClick={() => {
              setReviewing(true);
              setPrivacyAccepted(false);
              setState({ kind: "idle" });
            }}
          >
            {t("bugreport.review")}
          </Button>
        </div>
      )}

      {selectedTarget && reviewing && !busy && (
        <div
          role="alert"
          className="mt-4 rounded-md border border-amber-300/25 bg-amber-950/25 p-4"
        >
          <h4 className="text-sm font-semibold text-amber-100">
            {t("bugreport.warningTitle")}
          </h4>
          <p className="mt-2 text-sm leading-6 text-anvil-200">
            {t("bugreport.warningBody")}
          </p>
          <p className="mt-2 text-xs leading-5 text-anvil-400">
            {t("bugreport.privacyContract")}
          </p>
          <label className="mt-4 flex cursor-pointer items-start gap-3 rounded-md border border-amber-300/20 bg-black/20 p-3 text-sm text-anvil-100">
            <input
              type="checkbox"
              className="mt-1 h-4 w-4 accent-amber-300"
              checked={privacyAccepted}
              onChange={(event) =>
                setPrivacyAccepted(event.currentTarget.checked)
              }
            />
            <span>{t("bugreport.acknowledge")}</span>
          </label>
          <div className="mt-4 flex flex-wrap gap-2">
            <Button
              type="button"
              variant="primary"
              disabled={
                !privacyAccepted ||
                (requiresTransportOverride(selectedTarget) &&
                  !transportOverrideAccepted)
              }
              onClick={() => void startCapture()}
            >
              {t("bugreport.chooseDestination")}
            </Button>
            <Button
              type="button"
              variant="ghost"
              onClick={() => {
                setReviewing(false);
                setPrivacyAccepted(false);
              }}
            >
              {t("common.cancel")}
            </Button>
          </div>
        </div>
      )}

      {(state.kind === "selecting" ||
        state.kind === "running" ||
        state.kind === "cancelling") && (
        <div className="mt-4">
          <StatePanel title={t("bugreport.captureInProgress")} tone="warning">
            <p>
              {state.kind === "selecting"
                ? t("bugreport.selecting")
                : state.progress}
            </p>
            {state.kind !== "selecting" && (
              <div className="mt-4">
                <Button
                  type="button"
                  variant="danger"
                  size="sm"
                  disabled={state.kind === "cancelling"}
                  onClick={() => void cancelCapture()}
                >
                  {state.kind === "cancelling"
                    ? t("bugreport.cancelling")
                    : t("bugreport.cancelCapture")}
                </Button>
              </div>
            )}
          </StatePanel>
        </div>
      )}

      {state.kind === "error" && (
        <div className="mt-4">
          <StatePanel title={t("bugreport.failed")} tone="danger">
            <p>{state.message}</p>
          </StatePanel>
        </div>
      )}

      {state.kind === "success" && (
        <div className="mt-4">
          <StatePanel title={t("bugreport.saved")} tone="success">
            <dl className="grid gap-3 sm:grid-cols-2">
              <ArtifactResult
                label={t("bugreport.report")}
                path={state.result.report.local_path}
                size={state.result.report.size_bytes}
                sha256={state.result.report.sha256}
              />
              <ArtifactResult
                label={t("bugreport.sidecar")}
                path={state.result.sidecar.local_path}
                size={state.result.sidecar.size_bytes}
                sha256={state.result.sidecar.sha256}
              />
            </dl>
            <p className="mt-3 text-xs text-anvil-400">
              {t("bugreport.savedContract")}
            </p>
          </StatePanel>
        </div>
      )}
    </Card>
  );
}

function ArtifactResult({
  label,
  path,
  size,
  sha256,
}: {
  label: string;
  path: string;
  size: number;
  sha256: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="min-w-0 rounded-md border border-white/10 bg-black/20 p-3">
      <dt className="font-medium text-anvil-100">{label}</dt>
      <dd className="mt-2 break-all font-mono text-xs text-anvil-300">
        {path}
      </dd>
      <dd className="mt-2 text-xs text-anvil-400">
        {t("bugreport.artifactSize", { size: formatBytes(size) })}
      </dd>
      <dd className="mt-1 break-all font-mono text-[11px] text-anvil-500">
        SHA-256 {sha256}
      </dd>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
