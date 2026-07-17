import { useState } from "react";
import { useTranslation } from "react-i18next";

import { normalizeLanguage } from "../lib/i18n";
import { exportStoredSettings, resetStoredSettings } from "../lib/settings";
import type { SettingsScope } from "../lib/tauri";
import { Button, Card } from "./common";

const SCOPES: SettingsScope[] = ["all", "language", "mirror_presets"];

export default function SettingsDataControls({
  embedded = false,
}: {
  embedded?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const [scope, setScope] = useState<SettingsScope>("all");
  const [busy, setBusy] = useState<"export" | "reset" | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const exportSettings = async () => {
    setBusy("export");
    setMessage(null);
    try {
      const result = await exportStoredSettings(scope);
      if (result) {
        setMessage(t("settings.exportSaved", { path: result.path }));
      }
    } catch (error) {
      setMessage(
        t("settings.operationFailed", {
          message: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setBusy(null);
    }
  };

  const resetSettings = async () => {
    setBusy("reset");
    setMessage(null);
    try {
      const snapshot = await resetStoredSettings(scope);
      if ((scope === "all" || scope === "language") && !snapshot?.language) {
        // Keep i18next on a supported code; an unsupported browser locale
        // (e.g. "fr-FR") would otherwise desync from the language selector.
        await i18n.changeLanguage(normalizeLanguage(navigator.language) ?? "en");
      }
      setConfirmReset(false);
      setMessage(t("settings.resetComplete", { scope: t(scopeKey(scope)) }));
    } catch (error) {
      setMessage(
        t("settings.operationFailed", {
          message: error instanceof Error ? error.message : String(error),
        }),
      );
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card
      className={embedded ? "border-0 bg-transparent p-0 shadow-none" : "mt-5"}
    >
      <h3 className="text-base font-semibold text-anvil-50">
        {t("settings.title")}
      </h3>
      <p className="mt-1 text-sm leading-6 text-anvil-300">
        {t("settings.description")}
      </p>
      <label className="mt-4 block max-w-sm">
        <span className="text-xs font-medium text-anvil-300">
          {t("settings.scope")}
        </span>
        <select
          value={scope}
          onChange={(event) => setScope(event.target.value as SettingsScope)}
          disabled={busy !== null}
          className="mt-2 h-9 w-full rounded-md border border-white/10 bg-anvil-900 px-3 text-sm text-anvil-50 outline-none focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
        >
          {SCOPES.map((candidate) => (
            <option key={candidate} value={candidate}>
              {t(scopeKey(candidate))}
            </option>
          ))}
        </select>
      </label>

      {confirmReset ? (
        <div className="mt-4 rounded-md border border-red-300/20 bg-red-300/10 p-4">
          <p className="text-sm font-medium text-red-100">
            {t("settings.resetConfirmTitle")}
          </p>
          <p className="mt-1 text-xs leading-5 text-anvil-300">
            {t("settings.resetConfirmBody", { scope: t(scopeKey(scope)) })}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <Button
              type="button"
              variant="danger"
              size="sm"
              onClick={() => void resetSettings()}
              disabled={busy !== null}
            >
              {busy === "reset"
                ? t("settings.resetting")
                : t("settings.confirmReset")}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => setConfirmReset(false)}
              disabled={busy !== null}
            >
              {t("common.cancel")}
            </Button>
          </div>
        </div>
      ) : (
        <div className="mt-4 flex flex-wrap gap-2">
          <Button
            type="button"
            variant="primary"
            size="sm"
            onClick={() => void exportSettings()}
            disabled={busy !== null}
          >
            {busy === "export" ? t("settings.exporting") : t("settings.export")}
          </Button>
          <Button
            type="button"
            variant="danger"
            size="sm"
            onClick={() => setConfirmReset(true)}
            disabled={busy !== null}
          >
            {t("settings.reset")}
          </Button>
        </div>
      )}
      {message && (
        <p role="status" className="mt-3 break-words text-sm text-circuit-100">
          {message}
        </p>
      )}
    </Card>
  );
}

function scopeKey(scope: SettingsScope): string {
  switch (scope) {
    case "all":
      return "settings.scopes.all";
    case "language":
      return "settings.scopes.language";
    case "mirror_presets":
      return "settings.scopes.mirrorPresets";
  }
}
