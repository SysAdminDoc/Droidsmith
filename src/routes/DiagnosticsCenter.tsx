import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callPreviewDiagnostics,
  callSaveDiagnostics,
  callSelectHostPath,
  callWipeDiagnostics,
  type SupportPreview,
  type WipeDiagnosticsResult,
} from "../lib/tauri";
import { useFocusTrap } from "../lib/useFocusTrap";
import { Badge, Button, SkeletonLine, StatePanel } from "./common";
import BugreportCapture from "./BugreportCapture";
import HostDoctor from "./HostDoctor";

type PreviewState =
  | { kind: "loading" }
  | { kind: "ok"; preview: SupportPreview }
  | { kind: "error"; message: string };

export default function DiagnosticsCenter({
  onDismiss,
}: {
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const [state, setState] = useState<PreviewState>({ kind: "loading" });
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [wipeConfirm, setWipeConfirm] = useState(false);
  const [wiping, setWiping] = useState(false);
  const trapRef = useFocusTrap<HTMLDivElement>(!wipeConfirm);

  const loadPreview = useCallback(async () => {
    setState({ kind: "loading" });
    setMessage(null);
    try {
      setState({ kind: "ok", preview: await callPreviewDiagnostics() });
    } catch (error) {
      setState({
        kind: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }, []);

  useEffect(() => {
    void loadPreview();
  }, [loadPreview]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !wipeConfirm && !wiping) onDismiss();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onDismiss, wipeConfirm, wiping]);

  const saveBundle = useCallback(async () => {
    if (state.kind !== "ok") return;
    setSaving(true);
    setMessage(null);
    try {
      const date = state.preview.generated_at.slice(0, 10);
      const pathGrant = await callSelectHostPath(
        "diagnostics_save",
        `droidsmith-support-${date}.json`,
      );
      if (!pathGrant) return;
      const result = await callSaveDiagnostics(pathGrant.id);
      setMessage(
        t("diagnostics.saved", {
          path: result.path,
          size: formatBytes(result.byte_size),
        }),
      );
    } catch (error) {
      setMessage(
        t("diagnostics.saveFailed", {
          message: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setSaving(false);
    }
  }, [state, t]);

  const wipe = useCallback(async () => {
    setWiping(true);
    setMessage(null);
    try {
      const result = await callWipeDiagnostics(true);
      setWipeConfirm(false);
      setMessage(wipeMessage(result, t));
      const preview = await callPreviewDiagnostics();
      setState({ kind: "ok", preview });
    } catch (error) {
      setMessage(
        t("diagnostics.wipeFailed", {
          message: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setWiping(false);
    }
  }, [t]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-3 backdrop-blur-sm sm:p-5">
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="diagnostics-title"
        tabIndex={-1}
        className="max-h-[94vh] w-full max-w-5xl overflow-y-auto rounded-lg border border-white/10 bg-anvil-950 p-5 shadow-2xl outline-none sm:p-6"
      >
        <div className="flex flex-col gap-4 border-b border-white/10 pb-5 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <div className="flex flex-wrap items-center gap-2">
              <h2
                id="diagnostics-title"
                className="text-xl font-semibold text-anvil-50"
              >
                {t("diagnostics.title")}
              </h2>
              <Badge tone="success">{t("diagnostics.localOnly")}</Badge>
            </div>
            <p className="mt-2 max-w-3xl text-sm leading-6 text-anvil-300">
              {t("diagnostics.description")}
            </p>
          </div>
          <Button
            type="button"
            variant="ghost"
            onClick={onDismiss}
            disabled={wiping}
          >
            {t("common.close")}
          </Button>
        </div>

        <div className="mt-5 rounded-md border border-circuit-300/20 bg-circuit-300/10 p-4">
          <p className="text-sm font-medium text-circuit-100">
            {t("diagnostics.noUploadTitle")}
          </p>
          <p className="mt-1 text-xs leading-5 text-anvil-300">
            {t("diagnostics.noUploadBody")}
          </p>
        </div>

        <div className="mt-5">
          <HostDoctor />
        </div>

        <div className="mt-5">
          <BugreportCapture />
        </div>

        {state.kind === "loading" && (
          <div className="mt-5 space-y-3" aria-label={t("diagnostics.loading")}>
            <SkeletonLine className="w-1/3" />
            <SkeletonLine />
            <SkeletonLine className="w-4/5" />
            <SkeletonLine className="w-2/3" />
          </div>
        )}

        {state.kind === "error" && (
          <div className="mt-5">
            <StatePanel
              title={t("diagnostics.loadFailed")}
              tone="danger"
              actions={
                <Button
                  type="button"
                  variant="danger"
                  size="sm"
                  onClick={() => void loadPreview()}
                >
                  {t("common.checkAgain")}
                </Button>
              }
            >
              <p>{state.message}</p>
            </StatePanel>
          </div>
        )}

        {state.kind === "ok" && (
          <>
            <dl className="mt-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              <SummaryMetric
                label={t("diagnostics.bundleSize")}
                value={formatBytes(state.preview.byte_size)}
              />
              <SummaryMetric
                label={t("diagnostics.devices")}
                value={String(state.preview.device_count)}
              />
              <SummaryMetric
                label={t("diagnostics.failures")}
                value={String(state.preview.failed_operation_count)}
              />
              <SummaryMetric
                label={t("diagnostics.crashLines")}
                value={String(state.preview.crash_line_count)}
              />
            </dl>

            <label className="mt-5 block">
              <span className="text-xs font-medium text-anvil-300">
                {t("diagnostics.preview")}
              </span>
              <textarea
                readOnly
                value={state.preview.content}
                rows={20}
                aria-label={t("diagnostics.preview")}
                className="mt-2 w-full resize-y rounded-md border border-white/10 bg-black/35 p-3 font-mono text-xs leading-5 text-anvil-200 outline-none focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
              />
            </label>
          </>
        )}

        {message && (
          <p
            role="status"
            className="mt-4 break-words text-sm text-circuit-100"
          >
            {message}
          </p>
        )}

        <div className="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-white/10 pt-5">
          <Button
            type="button"
            variant="danger"
            onClick={() => setWipeConfirm(true)}
            disabled={wiping}
          >
            {t("diagnostics.wipe")}
          </Button>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="ghost"
              onClick={() => void loadPreview()}
              disabled={state.kind === "loading" || wiping}
            >
              {t("common.refresh")}
            </Button>
            <Button
              type="button"
              variant="primary"
              onClick={() => void saveBundle()}
              disabled={state.kind !== "ok" || saving || wiping}
            >
              {saving ? t("diagnostics.saving") : t("diagnostics.save")}
            </Button>
          </div>
        </div>
      </div>

      {wipeConfirm && (
        <WipeConfirmation
          wiping={wiping}
          onCancel={() => setWipeConfirm(false)}
          onConfirm={() => void wipe()}
        />
      )}
    </div>
  );
}

function WipeConfirmation({
  wiping,
  onCancel,
  onConfirm,
}: {
  wiping: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const trapRef = useFocusTrap<HTMLDivElement>();
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !wiping) onCancel();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onCancel, wiping]);
  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/75 p-4">
      <div
        ref={trapRef}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="wipe-diagnostics-title"
        aria-describedby="wipe-diagnostics-description"
        tabIndex={-1}
        className="w-full max-w-lg rounded-lg border border-red-300/20 bg-anvil-950 p-5 shadow-2xl outline-none"
      >
        <h3
          id="wipe-diagnostics-title"
          className="text-lg font-semibold text-anvil-50"
        >
          {t("diagnostics.wipeTitle")}
        </h3>
        <p
          id="wipe-diagnostics-description"
          className="mt-2 text-sm leading-6 text-anvil-300"
        >
          {t("diagnostics.wipeBody")}
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            onClick={onCancel}
            disabled={wiping}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant="danger"
            onClick={onConfirm}
            disabled={wiping}
          >
            {wiping ? t("diagnostics.wiping") : t("diagnostics.confirmWipe")}
          </Button>
        </div>
      </div>
    </div>
  );
}

function SummaryMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-white/10 bg-white/[0.03] p-3">
      <dt className="text-xs text-anvil-500">{label}</dt>
      <dd className="mt-1 font-mono text-sm text-anvil-100">{value}</dd>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function wipeMessage(
  result: WipeDiagnosticsResult,
  t: ReturnType<typeof useTranslation>["t"],
): string {
  return t("diagnostics.wiped", {
    count: result.files_removed,
    size: formatBytes(result.bytes_removed),
  });
}
