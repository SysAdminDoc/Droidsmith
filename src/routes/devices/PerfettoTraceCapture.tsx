import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callCapturePerfettoTrace,
  callOpenArtifactWith,
  callPerfettoCapabilities,
  callSelectHostPath,
  errorMessage,
  newOperationId,
  type DeviceTarget,
  type OperationEvent,
  type PerfettoCapabilities,
  type PerfettoCaptureResult,
  type PerfettoPreset,
} from "../../lib/tauri";
import { useTargetOperation } from "../../lib/targetOperation";
import {
  Badge,
  Button,
  Card,
  RevealInFolderButton,
  StatePanel,
} from "../common";

type CapabilityState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; value: PerfettoCapabilities };

type CaptureState =
  | { kind: "idle" }
  | { kind: "selecting" }
  | { kind: "running" | "cancelling"; progress: string }
  | { kind: "success"; result: PerfettoCaptureResult }
  | { kind: "error"; message: string };

export function PerfettoTraceCapture({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [capabilities, setCapabilities] = useState<CapabilityState>({
    kind: "loading",
  });
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(null);
  const [privacyAccepted, setPrivacyAccepted] = useState(false);
  const [state, setState] = useState<CaptureState>({ kind: "idle" });
  const [openError, setOpenError] = useState<string | null>(null);
  const operation = useTargetOperation(target, "perfetto");
  const busy = ["selecting", "running", "cancelling"].includes(state.kind);

  const loadCapabilities = useCallback(async () => {
    setCapabilities({ kind: "loading" });
    try {
      const value = await callPerfettoCapabilities(target);
      setCapabilities({ kind: "ready", value });
      setSelectedPresetId((current) =>
        value.presets.some((preset) => preset.id === current)
          ? current
          : (value.presets[0]?.id ?? null),
      );
    } catch (error) {
      setCapabilities({ kind: "error", message: errorMessage(error) });
    }
  }, [target]);

  useEffect(() => {
    void loadCapabilities();
  }, [loadCapabilities]);

  const selectedPreset =
    capabilities.kind === "ready"
      ? (capabilities.value.presets.find(
          (preset) => preset.id === selectedPresetId,
        ) ?? null)
      : null;

  const startCapture = useCallback(async () => {
    if (
      busy ||
      !privacyAccepted ||
      !selectedPreset ||
      capabilities.kind !== "ready" ||
      !capabilities.value.supported
    )
      return;

    const targetSnapshot = { ...target };
    const lease = operation.begin();
    setOpenError(null);
    setState({ kind: "selecting" });
    try {
      const date = new Date().toISOString().slice(0, 10);
      const grant = await callSelectHostPath(
        "perfetto_trace_save",
        `droidsmith-${selectedPreset.id}-${safeFilePart(target.serial)}-${date}.perfetto-trace`,
      );
      if (!grant) {
        lease.commit(() => setState({ kind: "idle" }));
        lease.finish();
        return;
      }
      if (!lease.isCurrent()) return;

      const operationId = newOperationId("perfetto");
      if (!lease.registerCancellation(operationId)) return;
      setState({ kind: "running", progress: t("devices.perfetto.starting") });
      const result = await callCapturePerfettoTrace(
        targetSnapshot,
        grant.id,
        selectedPreset.id,
        true,
        {
          operationId,
          onEvent: (event: OperationEvent) => {
            if (!lease.isCurrent()) return;
            if (event.kind === "started" || event.kind === "progress") {
              setState({
                kind: "running",
                progress: event.message ?? t("devices.perfetto.running"),
              });
            }
          },
        },
      );
      if (!lease.isCurrent()) return;
      setPrivacyAccepted(false);
      setState({ kind: "success", result });
      lease.finish();
    } catch (error) {
      if (!lease.isCurrent()) return;
      setState({ kind: "error", message: errorMessage(error) });
      lease.finish();
    }
  }, [
    busy,
    capabilities,
    operation,
    privacyAccepted,
    selectedPreset,
    t,
    target,
  ]);

  const cancelCapture = useCallback(async () => {
    if (!operation.hasActiveLease()) return;
    setState({
      kind: "cancelling",
      progress: t("devices.perfetto.cancelling"),
    });
    await operation.requestActiveCancellation();
  }, [operation, t]);

  return (
    <Card className="p-5">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.perfetto.title")}
            </h3>
            <Badge tone="warning">{t("devices.perfetto.sensitive")}</Badge>
          </div>
          <p className="mt-1 max-w-3xl text-xs leading-5 text-anvil-400">
            {t("devices.perfetto.body")}
          </p>
        </div>
        {capabilities.kind === "ready" && (
          <Badge tone={capabilities.value.supported ? "success" : "warning"}>
            {capabilities.value.supported
              ? t("devices.perfetto.available")
              : t("devices.perfetto.unavailable")}
          </Badge>
        )}
      </div>

      {capabilities.kind === "loading" && (
        <div className="mt-4">
          <StatePanel
            title={t("devices.perfetto.checking")}
            tone="info"
            live="polite"
          >
            <p>{t("devices.perfetto.body")}</p>
          </StatePanel>
        </div>
      )}

      {capabilities.kind === "error" && (
        <div className="mt-4">
          <StatePanel
            title={t("devices.perfetto.checkFailed")}
            tone="danger"
            actions={
              <Button
                type="button"
                size="sm"
                variant="danger"
                onClick={() => void loadCapabilities()}
              >
                {t("common.refresh")}
              </Button>
            }
          >
            <p>{capabilities.message}</p>
          </StatePanel>
        </div>
      )}

      {capabilities.kind === "ready" && !capabilities.value.supported && (
        <div className="mt-4">
          <StatePanel
            title={t("devices.perfetto.unsupportedTitle")}
            tone="warning"
          >
            <p>
              {capabilities.value.unavailable_reason === "android_version"
                ? t("devices.perfetto.unsupportedAndroid", {
                    sdk: capabilities.value.sdk_level ?? "—",
                  })
                : t("devices.perfetto.toolUnavailable")}
            </p>
          </StatePanel>
        </div>
      )}

      {capabilities.kind === "ready" && capabilities.value.supported && (
        <>
          <fieldset className="mt-4" disabled={busy}>
            <legend className="text-xs font-medium text-anvil-300">
              {t("devices.perfetto.choosePreset")}
            </legend>
            <div className="mt-2 grid gap-2 lg:grid-cols-3">
              {capabilities.value.presets.map((preset) => (
                <PresetCard
                  key={preset.id}
                  preset={preset}
                  selected={preset.id === selectedPresetId}
                  onSelect={() => {
                    setSelectedPresetId(preset.id);
                    setState({ kind: "idle" });
                    setOpenError(null);
                  }}
                />
              ))}
            </div>
          </fieldset>

          <div className="mt-4 rounded-md border border-amber-300/20 bg-amber-400/[0.06] p-3">
            <h4 className="text-xs font-semibold text-amber-100">
              {t("devices.perfetto.privacyTitle")}
            </h4>
            <p className="mt-1 text-xs leading-5 text-amber-100/80">
              {t("devices.perfetto.privacyBody")}
            </p>
            <label className="mt-3 flex cursor-pointer items-start gap-3 text-xs text-anvil-100">
              <input
                type="checkbox"
                className="mt-0.5 h-4 w-4 accent-amber-300"
                checked={privacyAccepted}
                disabled={busy}
                onChange={(event) =>
                  setPrivacyAccepted(event.currentTarget.checked)
                }
              />
              <span>{t("devices.perfetto.acknowledge")}</span>
            </label>
          </div>

          {!busy && (
            <Button
              type="button"
              size="sm"
              variant="primary"
              className="mt-4"
              disabled={!privacyAccepted || !selectedPreset}
              onClick={() => void startCapture()}
            >
              {t("devices.perfetto.chooseDestination")}
            </Button>
          )}
        </>
      )}

      {(state.kind === "selecting" ||
        state.kind === "running" ||
        state.kind === "cancelling") && (
        <div className="mt-4">
          <StatePanel
            title={t("devices.perfetto.captureInProgress")}
            tone="warning"
            live="polite"
          >
            <p>
              {state.kind === "selecting"
                ? t("devices.perfetto.selecting")
                : state.progress}
            </p>
            {state.kind !== "selecting" && (
              <Button
                type="button"
                size="sm"
                variant="danger"
                className="mt-3"
                disabled={state.kind === "cancelling"}
                onClick={() => void cancelCapture()}
              >
                {state.kind === "cancelling"
                  ? t("devices.perfetto.cancelling")
                  : t("devices.perfetto.cancel")}
              </Button>
            )}
          </StatePanel>
        </div>
      )}

      {state.kind === "error" && (
        <div className="mt-4">
          <StatePanel title={t("devices.perfetto.failed")} tone="danger">
            <p>{state.message}</p>
          </StatePanel>
        </div>
      )}

      {state.kind === "success" && (
        <div className="mt-4">
          <StatePanel title={t("devices.perfetto.saved")} tone="success">
            <p className="break-all font-mono text-xs text-anvil-200">
              {state.result.artifact.local_path}
            </p>
            <p className="mt-2 text-xs text-anvil-400">
              {t("devices.perfetto.artifactSummary", {
                size: formatBytes(state.result.artifact.size_bytes),
                sha256: state.result.artifact.sha256,
              })}
            </p>
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <RevealInFolderButton path={state.result.artifact.local_path} />
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => {
                  setOpenError(null);
                  void callOpenArtifactWith(
                    state.result.artifact.local_path,
                  ).catch((error) => setOpenError(errorMessage(error)));
                }}
              >
                {t("devices.perfetto.openWith")}
              </Button>
            </div>
            {openError && (
              <p role="alert" className="mt-2 text-xs text-red-300">
                {openError}
              </p>
            )}
            <p className="mt-3 text-xs text-anvil-400">
              {t("devices.perfetto.localOnly")}
            </p>
          </StatePanel>
        </div>
      )}
    </Card>
  );
}

function PresetCard({
  preset,
  selected,
  onSelect,
}: {
  preset: PerfettoPreset;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  return (
    <label
      className={`cursor-pointer rounded-md border p-3 text-xs transition ${
        selected
          ? "border-circuit-300/60 bg-circuit-300/[0.08]"
          : "border-white/10 bg-black/10 hover:border-white/20"
      }`}
    >
      <span className="flex items-center gap-2">
        <input
          type="radio"
          name="perfetto-preset"
          checked={selected}
          onChange={onSelect}
          className="h-4 w-4 accent-circuit-300"
        />
        <span className="font-semibold text-anvil-100">
          {t(`devices.perfetto.presets.${preset.id}.title`)}
        </span>
      </span>
      <span className="mt-2 block leading-5 text-anvil-400">
        {t(`devices.perfetto.presets.${preset.id}.body`)}
      </span>
      <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-2 gap-y-1 text-anvil-400">
        <dt>{t("devices.perfetto.duration")}</dt>
        <dd>
          {t("devices.perfetto.seconds", { count: preset.duration_secs })}
        </dd>
        <dt>{t("devices.perfetto.buffer")}</dt>
        <dd>{preset.buffer_size_mb} MB</dd>
        <dt>{t("devices.perfetto.maximum")}</dt>
        <dd>{formatBytes(preset.max_output_bytes)}</dd>
        <dt>{t("devices.perfetto.sources")}</dt>
        <dd className="break-words font-mono text-[11px]">
          {[...preset.data_sources, ...preset.atrace_categories].join(", ")}
        </dd>
      </dl>
    </label>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function safeFilePart(value: string): string {
  return value.replace(/[<>:"/\\|?*]/gu, "_");
}
