import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callRunHostDoctor,
  type HostDoctorReport,
  type HostFinding,
} from "../lib/tauri";
import { formatDateTime } from "../lib/i18n";
import { Badge, Button, Card, StatePanel } from "./common";

type DoctorState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "result"; report: HostDoctorReport }
  | { kind: "error"; message: string };

export default function HostDoctor() {
  const { t, i18n } = useTranslation();
  const titleId = useId();
  const [state, setState] = useState<DoctorState>({ kind: "idle" });

  const run = async () => {
    setState({ kind: "loading" });
    try {
      setState({ kind: "result", report: await callRunHostDoctor() });
    } catch (error) {
      setState({
        kind: "error",
        message: errorMessage(error),
      });
    }
  };

  return (
    <Card
      className="rounded-none border-t border-white/10 bg-transparent px-0 py-4 shadow-none"
      aria-labelledby={titleId}
    >
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <h3 id={titleId} className="text-sm font-semibold text-anvil-50">
              {t("hostDoctor.title")}
            </h3>
            <Badge tone="success">{t("hostDoctor.readOnly")}</Badge>
          </div>
          <p className="mt-1 max-w-3xl text-sm text-anvil-400">
            {t("hostDoctor.description")}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="primary"
          disabled={state.kind === "loading"}
          onClick={() => void run()}
        >
          {state.kind === "loading"
            ? t("hostDoctor.scanning")
            : t("hostDoctor.run")}
        </Button>
      </div>

      {state.kind === "idle" && (
        <p className="mt-2 text-xs text-anvil-500">{t("hostDoctor.idle")}</p>
      )}
      {state.kind === "error" && (
        <div className="mt-4">
          <StatePanel title={t("hostDoctor.failed")} tone="danger">
            <p>{state.message}</p>
          </StatePanel>
        </div>
      )}
      {state.kind === "result" && (
        <div className="mt-4 space-y-3">
          <div className="flex flex-wrap gap-2 text-xs text-anvil-400">
            <Badge
              tone={
                state.report.adb.compatibility.status === "blocked"
                  ? "danger"
                  : state.report.adb.compatibility.status === "warn"
                    ? "warning"
                    : state.report.adb.query_succeeded
                      ? "success"
                      : "warning"
              }
            >
              {t(
                `hostDoctor.compatibility.${state.report.adb.compatibility.status}`,
              )}
            </Badge>
            <Badge tone="neutral">
              {state.report.adb.version
                ? t("hostDoctor.adbVersion", {
                    version: state.report.adb.version,
                  })
                : t("hostDoctor.adbUnverified")}
            </Badge>
            <Badge tone="neutral">
              {t(`hostDoctor.platforms.${state.report.platform}`, {
                defaultValue: state.report.platform,
              })}
            </Badge>
            <span>
              {t("hostDoctor.scanned", {
                date: formatDateTime(state.report.scanned_at, i18n.language),
              })}
            </span>
          </div>

          {state.report.findings.map((finding) => (
            <Finding key={finding.code} finding={finding} />
          ))}

          <details className="rounded-md border border-white/10 bg-white/[0.025] p-3 text-xs text-anvil-400">
            <summary className="cursor-pointer font-medium text-anvil-200">
              {t("hostDoctor.privacy")}
            </summary>
            <ul className="mt-2 list-disc space-y-1 ps-5">
              {state.report.privacy.map((item, index) => (
                <li key={item}>
                  {t(`hostDoctor.privacyItems.${index}`, {
                    defaultValue: item,
                  })}
                </li>
              ))}
            </ul>
          </details>
        </div>
      )}
    </Card>
  );
}

function Finding({ finding }: { finding: HostFinding }) {
  const { t } = useTranslation();
  const tone =
    finding.severity === "error"
      ? "danger"
      : finding.severity === "warning"
        ? "warning"
        : "info";
  // Prefer locale overrides keyed by the stable finding code; the
  // Rust-provided English strings remain the fallback for codes a locale
  // does not cover (e.g. findings whose summary is composed dynamically).
  // Evidence is raw probe output and intentionally stays untranslated.
  const title = t(`hostDoctor.findings.${finding.code}.title`, {
    defaultValue: finding.title,
  });
  // A dynamic summary (e.g. the platform-tools version policy) carries a locale
  // key plus version params so the sentence is localized from structured data;
  // only the free-form policy `summary` stays English when no key is provided.
  const summaryParams = Object.fromEntries(
    finding.summary_params.map((param) => [param.key, param.value]),
  );
  const summary = finding.summary_key
    ? t(`hostDoctor.dynamicSummary.${finding.summary_key}`, {
        defaultValue: finding.summary,
        ...summaryParams,
      })
    : t(`hostDoctor.findings.${finding.code}.summary`, {
        defaultValue: finding.summary,
      });

  return (
    <StatePanel title={title} tone={tone}>
      <p>{summary}</p>
      {finding.evidence.length > 0 && (
        <ul className="mt-2 list-disc space-y-1 ps-5 font-mono text-xs text-anvil-300">
          {finding.evidence.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      )}
      <ol className="mt-3 list-decimal space-y-1 ps-5">
        {finding.remediation.map((item, index) => (
          <li key={item}>
            {t(`hostDoctor.findings.${finding.code}.remediation.${index}`, {
              defaultValue: item,
            })}
          </li>
        ))}
      </ol>
      <a
        href={finding.official_url}
        target="_blank"
        rel="noreferrer"
        className="mt-3 inline-flex text-xs font-medium text-circuit-200 underline decoration-circuit-300/40 underline-offset-4 hover:text-circuit-100"
      >
        {t("hostDoctor.officialGuidance")}
      </a>
    </StatePanel>
  );
}
