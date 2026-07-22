import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { changeDroidsmithLanguage, normalizeLanguage } from "../lib/i18n";
import {
  applyStoredSettingsImport,
  exportStoredSettings,
  hasStoredSettingsImportBackup,
  previewStoredSettingsImport,
  resetStoredSettings,
  restoreStoredSettingsImportBackup,
} from "../lib/settings";
import {
  errorMessage,
  type SettingsChangeCounts,
  type SettingsImportMode,
  type SettingsImportPreview,
  type SettingsScope,
  type SettingsSnapshot,
} from "../lib/tauri";
import { Button, Card, FieldSelect } from "./common";

const SCOPES: SettingsScope[] = ["all", "language", "mirror_presets"];

export default function SettingsDataControls({
  embedded = false,
}: {
  embedded?: boolean;
}) {
  const { t } = useTranslation();
  const [scope, setScope] = useState<SettingsScope>("all");
  const [busy, setBusy] = useState<
    "export" | "preview" | "apply" | "restore" | "reset" | null
  >(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [importPreview, setImportPreview] =
    useState<SettingsImportPreview | null>(null);
  const [importMode, setImportMode] = useState<SettingsImportMode>("merge");
  const [backupAvailable, setBackupAvailable] = useState(false);

  useEffect(() => {
    let active = true;
    void hasStoredSettingsImportBackup()
      .then((available) => {
        if (active) setBackupAvailable(available);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  const applySnapshotLanguage = async (snapshot: SettingsSnapshot) => {
    await changeDroidsmithLanguage(
      snapshot.language ?? normalizeLanguage(navigator.language) ?? "en",
    );
  };

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
          message: errorMessage(error),
        }),
      );
    } finally {
      setBusy(null);
    }
  };

  const previewImport = async () => {
    setBusy("preview");
    setMessage(null);
    try {
      const preview = await previewStoredSettingsImport();
      if (preview) {
        setImportPreview(preview);
        setImportMode("merge");
        setBackupAvailable(preview.backupAvailable);
      }
    } catch (error) {
      setMessage(
        t("settings.operationFailed", { message: errorMessage(error) }),
      );
    } finally {
      setBusy(null);
    }
  };

  const applyImport = async () => {
    if (!importPreview) return;
    setBusy("apply");
    setMessage(null);
    try {
      const result = await applyStoredSettingsImport(
        importPreview.importId,
        importMode,
      );
      await applySnapshotLanguage(result.settings);
      setBackupAvailable(result.backupAvailable);
      setImportPreview(null);
      setMessage(
        t("settings.importComplete", { mode: t(modeKey(importMode)) }),
      );
    } catch (error) {
      setMessage(
        t("settings.operationFailed", { message: errorMessage(error) }),
      );
    } finally {
      setBusy(null);
    }
  };

  const restoreImportBackup = async () => {
    setBusy("restore");
    setMessage(null);
    try {
      const snapshot = await restoreStoredSettingsImportBackup();
      await applySnapshotLanguage(snapshot);
      setMessage(t("settings.importBackupRestored"));
    } catch (error) {
      setMessage(
        t("settings.operationFailed", { message: errorMessage(error) }),
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
        await applySnapshotLanguage(
          snapshot ?? {
            version: "1",
            language: null,
            mirrorPresetCount: 0,
          },
        );
      }
      setConfirmReset(false);
      setMessage(t("settings.resetComplete", { scope: t(scopeKey(scope)) }));
    } catch (error) {
      setMessage(
        t("settings.operationFailed", {
          message: errorMessage(error),
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
        <FieldSelect
          value={scope}
          onChange={(event) => setScope(event.target.value as SettingsScope)}
          disabled={busy !== null}
          className="mt-2 h-9 w-full"
        >
          {SCOPES.map((candidate) => (
            <option key={candidate} value={candidate}>
              {t(scopeKey(candidate))}
            </option>
          ))}
        </FieldSelect>
      </label>

      {importPreview ? (
        <div className="mt-4 rounded-md border border-circuit-300/20 bg-circuit-300/[0.06] p-4">
          <h4 className="text-sm font-medium text-anvil-50">
            {t("settings.importPreviewTitle")}
          </h4>
          <p className="mt-1 text-xs leading-5 text-anvil-300">
            {t("settings.importPreviewBody", {
              version: importPreview.version,
              scope: t(scopeKey(importPreview.scope)),
            })}
          </p>
          <label className="mt-3 block max-w-sm">
            <span className="text-xs font-medium text-anvil-300">
              {t("settings.importMode")}
            </span>
            <FieldSelect
              value={importMode}
              onChange={(event) =>
                setImportMode(event.target.value as SettingsImportMode)
              }
              disabled={busy !== null}
              className="mt-2 h-9 w-full"
            >
              <option value="merge">{t("settings.importModes.merge")}</option>
              <option value="replace">
                {t("settings.importModes.replace")}
              </option>
            </FieldSelect>
          </label>
          <ImportDiff preview={importPreview} mode={importMode} t={t} />
          <p className="mt-3 text-xs leading-5 text-anvil-300">
            {t("settings.importMachineLocal", {
              fields: importPreview.excludedMachineLocal.join(", "),
            })}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <Button
              type="button"
              variant="primary"
              size="sm"
              onClick={() => void applyImport()}
              disabled={busy !== null}
            >
              {busy === "apply"
                ? t("settings.importApplying")
                : t("settings.importApply")}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => setImportPreview(null)}
              disabled={busy !== null}
            >
              {t("common.cancel")}
            </Button>
          </div>
        </div>
      ) : confirmReset ? (
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
            variant="secondary"
            size="sm"
            onClick={() => void previewImport()}
            disabled={busy !== null}
          >
            {busy === "preview"
              ? t("settings.importReading")
              : t("settings.import")}
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => void restoreImportBackup()}
            disabled={busy !== null || !backupAvailable}
          >
            {busy === "restore"
              ? t("settings.importRestoring")
              : t("settings.importRestore")}
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

function ImportDiff({
  preview,
  mode,
  t,
}: {
  preview: SettingsImportPreview;
  mode: SettingsImportMode;
  t: ReturnType<typeof useTranslation>["t"];
}) {
  const diff = mode === "merge" ? preview.merge : preview.replace;
  return (
    <dl className="mt-3 grid gap-2 text-xs text-anvil-300 sm:grid-cols-2">
      <DiffRow
        label={t("settings.importDiff.language")}
        value={t(
          diff.languageChanged
            ? "settings.importDiff.changed"
            : "settings.importDiff.unchanged",
        )}
      />
      <DiffRow
        label={t("settings.importDiff.mirrorPresets")}
        value={formatCounts(diff.mirrorPresets, t)}
      />
      <DiffRow
        label={t("settings.importDiff.logcatQueries")}
        value={formatCounts(diff.logcatQueries, t)}
      />
      <DiffRow
        label={t("settings.importDiff.wirelessEndpoints")}
        value={formatCounts(diff.wirelessEndpoints, t)}
      />
      <DiffRow
        label={t("settings.importDiff.autoReconnect")}
        value={t(
          diff.autoReconnectChanged
            ? "settings.importDiff.changed"
            : "settings.importDiff.unchanged",
        )}
      />
    </dl>
  );
}

function DiffRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border border-white/10 bg-black/10 px-3 py-2">
      <dt className="font-medium text-anvil-100">{label}</dt>
      <dd className="mt-1">{value}</dd>
    </div>
  );
}

function formatCounts(
  counts: SettingsChangeCounts,
  t: ReturnType<typeof useTranslation>["t"],
): string {
  return t("settings.importDiff.counts", counts);
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

function modeKey(mode: SettingsImportMode): string {
  return mode === "merge"
    ? "settings.importModes.merge"
    : "settings.importModes.replace";
}
