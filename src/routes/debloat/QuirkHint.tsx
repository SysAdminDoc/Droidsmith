import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { callExplainFailure, errorMessage, type Quirk } from "../../lib/tauri";
import type { QuirkDeviceContext } from "./queue";

type QuirkHintState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "none" }
  | { kind: "quirk"; quirk: Quirk }
  | { kind: "error"; message: string };

/**
 * IMP-68: on a failed debloat row, offer to match the raw error against the
 * bundled vendor-quirk rules and surface a human explanation + mitigation.
 */
export function QuirkHint({
  packageId,
  rawError,
  deviceContext,
}: {
  packageId: string;
  rawError: string;
  deviceContext: QuirkDeviceContext;
}) {
  const { t } = useTranslation();
  const [state, setState] = useState<QuirkHintState>({ kind: "idle" });

  const explain = useCallback(async () => {
    setState({ kind: "loading" });
    try {
      const quirk = await callExplainFailure({
        manufacturer: deviceContext.manufacturer,
        rom: deviceContext.rom,
        package_id: packageId,
        raw_error: rawError,
      });
      setState(quirk ? { kind: "quirk", quirk } : { kind: "none" });
    } catch (e) {
      setState({ kind: "error", message: errorMessage(e) });
    }
  }, [deviceContext.manufacturer, deviceContext.rom, packageId, rawError]);

  if (state.kind === "idle") {
    return (
      <button
        type="button"
        onClick={() => void explain()}
        className="mt-1 text-xs font-medium text-circuit-200 underline underline-offset-2 hover:text-circuit-100"
      >
        {t("debloat.explainFailure")}
      </button>
    );
  }

  if (state.kind === "loading") {
    return (
      <p className="mt-1 text-xs text-anvil-400">
        {t("debloat.explainLoading")}
      </p>
    );
  }

  if (state.kind === "error") {
    return (
      <p className="mt-1 text-xs text-red-200/80">
        {t("debloat.explainError")}
      </p>
    );
  }

  if (state.kind === "none") {
    return (
      <p className="mt-1 text-xs text-anvil-400">
        {t("debloat.explainNoMatch")}
      </p>
    );
  }

  return (
    <div className="mt-2 max-w-xl rounded-md border border-circuit-300/25 bg-circuit-300/[0.06] p-3">
      <h5 className="text-xs font-semibold text-circuit-100">
        {state.quirk.title}
      </h5>
      <p className="mt-1 whitespace-pre-wrap text-xs leading-5 text-anvil-200">
        {state.quirk.explanation}
      </p>
      {state.quirk.mitigation && (
        <p className="mt-2 text-[11px] font-medium text-circuit-200">
          {t("debloat.explainMitigation", {
            kind: state.quirk.mitigation.kind.replace(/_/g, " "),
          })}
        </p>
      )}
    </div>
  );
}
