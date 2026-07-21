// ADB server recovery dialog (IMP-72: extracted verbatim from the former
// Devices.tsx god-file).

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useFocusTrap } from "../../lib/useFocusTrap";
import type { AdbHealth } from "../../lib/tauri";
import { Button, FieldTextArea } from "../common";
import { ADB_RECOVERY_COMMANDS, formatAdbDiagnostics } from "../adbHealth";
import type { RecoveryState } from "./common";

export function RecoveryDialog({
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
