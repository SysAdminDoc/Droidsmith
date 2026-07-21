import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callAnalyzeApk,
  callSelectHostPath,
  errorMessage,
  type ApkAnalysis,
} from "../lib/tauri";
import {
  Badge,
  Button,
  Card,
  PaneHeader,
  StatePanel,
  TableCell,
  TableHeaderCell,
} from "./common";

type AnalyzerState =
  | { kind: "idle" }
  | { kind: "analyzing" }
  | { kind: "ok"; analysis: ApkAnalysis }
  | { kind: "error"; message: string };

/// Offline static APK inspector (R-097). Selects a local .apk/.apks via the
/// backend-owned grant dialog and renders manifest, dex, signing, and size
/// facts — no device required.
export default function ApkAnalyzerRoute() {
  const { t } = useTranslation();
  const [state, setState] = useState<AnalyzerState>({ kind: "idle" });

  const analyze = useCallback(async () => {
    setState({ kind: "analyzing" });
    try {
      const grant = await callSelectHostPath("apk_analyze_open");
      if (!grant) {
        setState({ kind: "idle" });
        return;
      }
      const analysis = await callAnalyzeApk(grant.id);
      setState({ kind: "ok", analysis });
    } catch (e) {
      setState({ kind: "error", message: errorMessage(e) });
    }
  }, []);

  return (
    <>
      <PaneHeader
        title={t("apk.title")}
        milestone="R-097"
        description={t("apk.description")}
        actions={
          <Button
            type="button"
            variant="primary"
            onClick={() => void analyze()}
            disabled={state.kind === "analyzing"}
          >
            {state.kind === "analyzing" ? t("apk.analyzing") : t("apk.choose")}
          </Button>
        }
      />

      <section className="mt-6 max-w-5xl space-y-4">
        {state.kind === "idle" && (
          <StatePanel title={t("apk.emptyTitle")} tone="info">
            <p>{t("apk.emptyBody")}</p>
          </StatePanel>
        )}

        {state.kind === "error" && (
          <StatePanel title={t("apk.failed")} tone="danger">
            <p className="break-all">{state.message}</p>
          </StatePanel>
        )}

        {state.kind === "ok" && <AnalysisReport analysis={state.analysis} />}
      </section>
    </>
  );
}

function AnalysisReport({ analysis }: { analysis: ApkAnalysis }) {
  const { t } = useTranslation();
  const { components, dex, signing } = analysis;
  const schemes = [
    signing.v1 && "v1",
    signing.v2 && "v2",
    signing.v3 && "v3",
    signing.v31 && "v3.1",
  ].filter(Boolean) as string[];

  return (
    <div className="space-y-4">
      <Card className="p-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <h3 className="truncate text-sm font-semibold text-anvil-50">
              {analysis.package ?? analysis.file_name}
            </h3>
            <p className="mt-0.5 break-all font-mono text-xs text-anvil-400">
              {analysis.file_name} · {formatBytes(analysis.file_size)}
            </p>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {dex.exceeds_64k && (
              <Badge tone="warning">{t("apk.multidex")}</Badge>
            )}
            {schemes.length === 0 ? (
              <Badge tone="danger">{t("apk.unsigned")}</Badge>
            ) : (
              <Badge tone="success">
                {t("apk.signed", { schemes: schemes.join(", ") })}
              </Badge>
            )}
          </div>
        </div>
        <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-2 text-sm sm:grid-cols-3">
          <Fact label={t("apk.versionName")} value={analysis.version_name} />
          <Fact
            label={t("apk.versionCode")}
            value={analysis.version_code?.toString()}
          />
          <Fact label={t("apk.minSdk")} value={analysis.min_sdk?.toString()} />
          <Fact
            label={t("apk.targetSdk")}
            value={analysis.target_sdk?.toString()}
          />
          <Fact
            label={t("apk.compileSdk")}
            value={analysis.compile_sdk?.toString()}
          />
          <Fact
            label={t("apk.entries")}
            value={analysis.total_entries.toString()}
          />
        </dl>
        <p className="mt-3 break-all font-mono text-[11px] text-anvil-500">
          SHA-256: {analysis.sha256}
        </p>
      </Card>

      <div className="grid gap-4 sm:grid-cols-2">
        <Card className="p-5">
          <h4 className="text-sm font-semibold text-anvil-50">
            {t("apk.components")}
          </h4>
          <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
            <Fact
              label={t("apk.activities")}
              value={components.activities.toString()}
            />
            <Fact
              label={t("apk.services")}
              value={components.services.toString()}
            />
            <Fact
              label={t("apk.receivers")}
              value={components.receivers.toString()}
            />
            <Fact
              label={t("apk.providers")}
              value={components.providers.toString()}
            />
          </dl>
        </Card>

        <Card className="p-5">
          <h4 className="text-sm font-semibold text-anvil-50">
            {t("apk.dex")}
          </h4>
          <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
            <Fact label={t("apk.dexFiles")} value={dex.files.toString()} />
            <Fact
              label={t("apk.classes")}
              value={dex.defined_classes.toLocaleString()}
            />
            <Fact
              label={t("apk.methodRefs")}
              value={dex.method_refs.toLocaleString()}
            />
          </dl>
        </Card>
      </div>

      <Card className="p-5">
        <h4 className="text-sm font-semibold text-anvil-50">
          {t("apk.permissions", { count: analysis.permissions.length })}
        </h4>
        {analysis.permissions.length === 0 ? (
          <p className="mt-2 text-xs text-anvil-400">
            {t("apk.noPermissions")}
          </p>
        ) : (
          <ul className="mt-3 flex flex-wrap gap-1.5">
            {analysis.permissions.map((permission) => (
              <li key={permission}>
                <code className="rounded-sm border border-white/10 bg-white/[0.04] px-1.5 py-0.5 font-mono text-[11px] text-anvil-200">
                  {permission}
                </code>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card className="p-5">
        <h4 className="text-sm font-semibold text-anvil-50">
          {t("apk.largestEntries")}
        </h4>
        <div className="mt-3 overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr>
                <TableHeaderCell>{t("apk.entryName")}</TableHeaderCell>
                <TableHeaderCell className="text-end">
                  {t("apk.entryUncompressed")}
                </TableHeaderCell>
                <TableHeaderCell className="text-end">
                  {t("apk.entryCompressed")}
                </TableHeaderCell>
              </tr>
            </thead>
            <tbody>
              {analysis.largest_entries.map((entry) => (
                <tr key={entry.name}>
                  <TableCell className="break-all font-mono text-xs">
                    {entry.name}
                  </TableCell>
                  <TableCell className="text-end tabular-nums">
                    {formatBytes(entry.uncompressed)}
                  </TableCell>
                  <TableCell className="text-end tabular-nums">
                    {formatBytes(entry.compressed)}
                  </TableCell>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}

function Fact({ label, value }: { label: string; value?: string | null }) {
  const { t } = useTranslation();
  return (
    <div className="min-w-0">
      <dt className="text-xs text-anvil-500">{label}</dt>
      <dd className="truncate text-anvil-100">
        {value && value.length > 0 ? value : t("apk.unknownValue")}
      </dd>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}
