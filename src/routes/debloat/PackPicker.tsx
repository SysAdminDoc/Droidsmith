import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callExportDevicePack,
  callImportPack,
  callSelectHostPath,
  type DeviceTarget,
  type PackCandidate,
  type PackLoadError,
} from "../../lib/tauri";
import {
  Badge,
  Button,
  Card,
  FieldInput,
  RevealInFolderButton,
  SkeletonLine,
  StatePanel,
} from "../common";
import { compatibilityTone } from "./tones";

export type PacksState =
  | { kind: "loading" }
  | { kind: "ok"; packs: PackCandidate[]; errors: PackLoadError[] }
  | { kind: "error"; message: string };

/// Surfaces bundled packs that failed to load so a packaging defect is
/// visible (with a copyable message) instead of silently vanishing from
/// the list. Healthy packs still render alongside.
function PackErrors({ errors }: { errors: PackLoadError[] }) {
  const { t } = useTranslation();
  if (errors.length === 0) return null;
  return (
    <StatePanel
      title={t("debloat.packErrorsTitle", { count: errors.length })}
      tone="warning"
    >
      <p className="text-xs text-anvil-300">{t("debloat.packErrorsBody")}</p>
      <ul className="mt-2 space-y-2">
        {errors.map((err) => (
          <li key={err.file}>
            <pre className="select-text whitespace-pre-wrap rounded-md border border-white/10 bg-white/[0.04] p-2 font-mono text-xs text-anvil-200">
              {`${err.file} [${err.code}]\n${err.message}`}
            </pre>
          </li>
        ))}
      </ul>
    </StatePanel>
  );
}

/// Import a debloat pack from a local YAML file through the audited host-path
/// grant model. Network-free alternative to remote-pack fetching (R-095): the
/// backend schema-validates the bytes, optionally checks a SHA-256 pin, and
/// stores the pack under the app-data `packs/` directory.
function PackImportControl({ onImported }: { onImported: () => void }) {
  const { t } = useTranslation();
  const [sha256, setSha256] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<
    | { kind: "success"; message: string; sha: string }
    | { kind: "error"; message: string }
    | null
  >(null);

  const runImport = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setStatus(null);
    try {
      const grant = await callSelectHostPath("pack_import_open");
      if (!grant) {
        setBusy(false);
        return;
      }
      const result = await callImportPack(grant.id, sha256);
      setSha256("");
      setStatus({
        kind: "success",
        message: t("debloat.import.success", { name: result.name }),
        sha: result.sha256,
      });
      onImported();
    } catch (e) {
      setStatus({ kind: "error", message: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }, [busy, sha256, onImported, t]);

  return (
    <Card className="p-5">
      <h3 className="text-sm font-semibold text-anvil-50">
        {t("debloat.import.title")}
      </h3>
      <p className="mt-1 text-xs text-anvil-400">{t("debloat.import.body")}</p>
      <div className="mt-3 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-xs text-anvil-300">
          <span>{t("debloat.import.sha256Label")}</span>
          <FieldInput
            value={sha256}
            onChange={(e) => setSha256(e.target.value)}
            placeholder={t("debloat.import.sha256Placeholder")}
            spellCheck={false}
            autoComplete="off"
            className="w-72 font-mono"
            aria-label={t("debloat.import.sha256Label")}
          />
        </label>
        <Button
          type="button"
          size="sm"
          onClick={() => void runImport()}
          disabled={busy}
        >
          {busy ? t("debloat.import.importing") : t("debloat.import.button")}
        </Button>
      </div>
      {status?.kind === "success" && (
        <div className="mt-3">
          <StatePanel title={t("debloat.import.title")} tone="success">
            <p>{status.message}</p>
            <p className="mt-1 break-all font-mono text-xs text-anvil-300">
              {t("debloat.import.shaLine", { sha: status.sha })}
            </p>
          </StatePanel>
        </div>
      )}
      {status?.kind === "error" && (
        <div className="mt-3">
          <StatePanel title={t("debloat.import.failed")} tone="danger">
            <p className="break-all">{status.message}</p>
          </StatePanel>
        </div>
      )}
    </Card>
  );
}

/// Capture the current device's disabled/archived/uninstalled packages into a
/// shareable debloat pack YAML (R-098), symmetric to the import control. The
/// exported file round-trips through the importer.
function PackExportControl({
  target,
  userId,
}: {
  target: DeviceTarget;
  userId: number;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<
    | { kind: "success"; message: string; path: string }
    | { kind: "error"; message: string }
    | null
  >(null);

  const runExport = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    setStatus(null);
    try {
      const grant = await callSelectHostPath(
        "pack_export_save",
        "device-debloat.yaml",
      );
      if (!grant) {
        setBusy(false);
        return;
      }
      const result = await callExportDevicePack(target, userId, grant.id);
      setStatus({
        kind: "success",
        message: t("debloat.export.success", {
          count: result.packages,
          id: result.pack_id,
        }),
        path: result.artifact.local_path,
      });
    } catch (e) {
      setStatus({ kind: "error", message: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }, [busy, target, userId, t]);

  return (
    <Card className="p-5">
      <h3 className="text-sm font-semibold text-anvil-50">
        {t("debloat.export.title")}
      </h3>
      <p className="mt-1 text-xs text-anvil-400">{t("debloat.export.body")}</p>
      <div className="mt-3">
        <Button
          type="button"
          size="sm"
          onClick={() => void runExport()}
          disabled={busy}
        >
          {busy ? t("debloat.export.exporting") : t("debloat.export.button")}
        </Button>
      </div>
      {status?.kind === "success" && (
        <div className="mt-3">
          <StatePanel
            title={t("debloat.export.title")}
            tone="success"
            actions={<RevealInFolderButton path={status.path} />}
          >
            <p>{status.message}</p>
          </StatePanel>
        </div>
      )}
      {status?.kind === "error" && (
        <div className="mt-3">
          <StatePanel title={t("debloat.export.failed")} tone="danger">
            <p className="break-all">{status.message}</p>
          </StatePanel>
        </div>
      )}
    </Card>
  );
}

/// A single selectable pack card. Imported packs carry a badge and a remove
/// control; the remove button is a sibling of the selectable button so it is
/// never nested inside another button.
function PackCard({
  candidate,
  onSelect,
  onRemove,
}: {
  candidate: PackCandidate;
  onSelect: (candidate: PackCandidate) => void;
  onRemove: (packId: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const { pack, assessment, imported } = candidate;
  const [removing, setRemoving] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);

  const remove = useCallback(async () => {
    if (removing) return;
    setRemoving(true);
    setRemoveError(null);
    try {
      await onRemove(pack.id);
    } catch (e) {
      setRemoveError(errorMessage(e));
      setRemoving(false);
    }
  }, [removing, onRemove, pack.id]);

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => onSelect(candidate)}
        className="group h-full w-full rounded-lg border border-white/10 bg-white/[0.02] p-4 text-start transition hover:border-circuit-300/30 hover:bg-circuit-300/[0.04] focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300"
      >
        <div className="flex items-start justify-between gap-3">
          <h4 className="text-sm font-semibold text-anvil-50">{pack.name}</h4>
          <Badge tone="neutral">
            {t("debloat.packageShortCount", {
              count: pack.packages.length,
            })}
          </Badge>
          <Badge tone={compatibilityTone(assessment.status)}>
            {t(`debloat.compatibility.${assessment.status}`)}
          </Badge>
        </div>
        <p className="mt-2 line-clamp-3 text-xs leading-5 text-anvil-400">
          {pack.description}
        </p>
        <div className="mt-3 flex flex-wrap gap-1.5">
          {imported && <Badge tone="info">{t("debloat.import.badge")}</Badge>}
          {pack.targets.manufacturer.map((m) => (
            <Badge key={m} tone="info">
              {m}
            </Badge>
          ))}
          {pack.targets.rom.map((r) => (
            <Badge key={r} tone="neutral">
              {r}
            </Badge>
          ))}
        </div>
      </button>
      {imported && (
        <Button
          type="button"
          size="sm"
          variant="danger"
          onClick={() => void remove()}
          disabled={removing}
          aria-label={t("debloat.import.remove")}
          className="absolute end-2 top-2"
        >
          {removing ? t("debloat.import.importing") : "×"}
        </Button>
      )}
      {removeError && (
        <p className="mt-1 break-all text-xs text-red-300">
          {t("debloat.import.removeFailed")}: {removeError}
        </p>
      )}
    </div>
  );
}

export function PackPicker({
  state,
  target,
  userId,
  onSelect,
  onRefresh,
  onRemove,
}: {
  state: PacksState;
  target: DeviceTarget | null;
  userId: number;
  onSelect: (candidate: PackCandidate) => void;
  onRefresh: () => void;
  onRemove: (packId: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const tools =
    target !== null ? (
      <div className="grid gap-4 sm:grid-cols-2">
        <PackImportControl onImported={onRefresh} />
        <PackExportControl target={target} userId={userId} />
      </div>
    ) : (
      <PackImportControl onImported={onRefresh} />
    );

  if (state.kind === "loading") {
    return (
      <Card className="p-5">
        <SkeletonLine className="w-40" />
        <div className="mt-4 grid gap-3 sm:grid-cols-2">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="rounded-lg border border-white/10 p-4">
              <SkeletonLine className="w-32" />
              <SkeletonLine className="mt-3 w-full" />
            </div>
          ))}
        </div>
      </Card>
    );
  }

  if (state.kind === "error") {
    return (
      <StatePanel
        title={t("debloat.loadPacksFailed")}
        tone="danger"
        actions={
          <Button type="button" size="sm" variant="danger" onClick={onRefresh}>
            {t("runtime.retry")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  if (state.packs.length === 0) {
    return (
      <div className="space-y-4">
        <PackErrors errors={state.errors} />
        <StatePanel title={t("debloat.noPacksTitle")} tone="info">
          <p>
            {t("debloat.noPacksBodyPrefix")} <code>.yaml</code>{" "}
            {t("debloat.noPacksBodyMiddle")} <code>packs/</code>{" "}
            {t("debloat.noPacksBodySuffix")} <code>packs/_example.yaml</code>.
          </p>
        </StatePanel>
        {tools}
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {tools}
      <Card className="p-5">
        <PackErrors errors={state.errors} />
        <h3 className="text-sm font-semibold text-anvil-50">
          {t("debloat.choosePack")}
        </h3>
        <p className="mt-1 text-xs text-anvil-400">
          {t("debloat.choosePackBody")}
        </p>
        <div className="mt-4 grid gap-3 sm:grid-cols-2">
          {state.packs.map((candidate) => (
            <PackCard
              key={candidate.pack.id}
              candidate={candidate}
              onSelect={onSelect}
              onRemove={onRemove}
            />
          ))}
        </div>
      </Card>
    </div>
  );
}
