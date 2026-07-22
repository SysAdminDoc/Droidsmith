import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { formatBytes } from "../lib/format";
import { formatDateTime, formatNumber } from "../lib/i18n";
import {
  callAnalyzeApk,
  callSelectHostPath,
  errorMessage,
  type ApkAnalysis,
  type ApkSignerCertificate,
  type ApkSigningLineageEntry,
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
        milestone="R-097 · R-107"
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

      <section className="mt-4 max-w-none space-y-3">
        {state.kind === "idle" && (
          <StatePanel title={t("apk.emptyTitle")} tone="info">
            <p>{t("apk.emptyBody")}</p>
          </StatePanel>
        )}

        {state.kind === "error" && (
          <StatePanel title={t("apk.failed")} tone="danger" live="assertive">
            <p className="break-all">{state.message}</p>
          </StatePanel>
        )}

        {state.kind === "ok" && <AnalysisReport analysis={state.analysis} />}
      </section>
    </>
  );
}

function AnalysisReport({ analysis }: { analysis: ApkAnalysis }) {
  const { t, i18n } = useTranslation();
  const { components, dex, signing, signature_verification } = analysis;
  const language = i18n.resolvedLanguage ?? i18n.language;
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
              {analysis.file_name} · {formatBytes(analysis.file_size, language)}
            </p>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {dex.exceeds_64k && (
              <Badge tone="warning">{t("apk.multidex")}</Badge>
            )}
            {schemes.length === 0 ? (
              <Badge tone="neutral">{t("apk.noSignatureArtifacts")}</Badge>
            ) : (
              <Badge tone="neutral">
                {t("apk.signatureArtifacts", {
                  schemes: schemes.join(", "),
                })}
              </Badge>
            )}
            <Badge
              tone={
                signature_verification.status === "verified"
                  ? "success"
                  : signature_verification.status === "failed"
                    ? "danger"
                    : "warning"
              }
            >
              {t(`apk.verificationBadges.${signature_verification.status}`)}
            </Badge>
          </div>
        </div>
        <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-2 text-sm sm:grid-cols-3">
          <Fact label={t("apk.versionName")} value={analysis.version_name} />
          <Fact
            label={t("apk.versionCode")}
            value={formatOptionalNumber(analysis.version_code, language)}
          />
          <Fact
            label={t("apk.minSdk")}
            value={formatOptionalNumber(analysis.min_sdk, language)}
          />
          <Fact
            label={t("apk.targetSdk")}
            value={formatOptionalNumber(analysis.target_sdk, language)}
          />
          <Fact
            label={t("apk.compileSdk")}
            value={formatOptionalNumber(analysis.compile_sdk, language)}
          />
          <Fact
            label={t("apk.entries")}
            value={formatNumber(analysis.total_entries, language)}
          />
        </dl>
        <p className="mt-3 break-all font-mono text-[11px] text-anvil-500">
          SHA-256: {analysis.sha256}
        </p>
      </Card>

      <SignatureVerificationReport
        verification={signature_verification}
        staticSchemes={schemes}
        language={language}
      />

      <div className="grid gap-4 sm:grid-cols-2">
        <Card className="p-5">
          <h4 className="text-sm font-semibold text-anvil-50">
            {t("apk.components")}
          </h4>
          <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
            <Fact
              label={t("apk.activities")}
              value={formatNumber(components.activities, language)}
            />
            <Fact
              label={t("apk.services")}
              value={formatNumber(components.services, language)}
            />
            <Fact
              label={t("apk.receivers")}
              value={formatNumber(components.receivers, language)}
            />
            <Fact
              label={t("apk.providers")}
              value={formatNumber(components.providers, language)}
            />
          </dl>
        </Card>

        <Card className="p-5">
          <h4 className="text-sm font-semibold text-anvil-50">
            {t("apk.dex")}
          </h4>
          <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
            <Fact
              label={t("apk.dexFiles")}
              value={formatNumber(dex.files, language)}
            />
            <Fact
              label={t("apk.classes")}
              value={formatNumber(dex.defined_classes, language)}
            />
            <Fact
              label={t("apk.methodRefs")}
              value={formatNumber(dex.method_refs, language)}
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
                    {formatBytes(entry.uncompressed, language)}
                  </TableCell>
                  <TableCell className="text-end tabular-nums">
                    {formatBytes(entry.compressed, language)}
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

function SignatureVerificationReport({
  verification,
  staticSchemes,
  language,
}: {
  verification: ApkAnalysis["signature_verification"];
  staticSchemes: string[];
  language: string;
}) {
  const { t } = useTranslation();
  const toolLabel = verification.tool
    ? t("apk.verification.toolSummary", {
        version: verification.tool.version,
        buildTools:
          verification.tool.build_tools_version ?? t("apk.unknownValue"),
        source: t(`apk.verification.sources.${verification.tool.source}`),
      })
    : null;

  return (
    <Card className="p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h4 className="text-sm font-semibold text-anvil-50">
            {t("apk.verification.title")}
          </h4>
          <p className="mt-1 max-w-3xl text-xs leading-5 text-anvil-400">
            {t("apk.verification.body")}
          </p>
        </div>
        <Badge
          tone={
            verification.status === "verified"
              ? "success"
              : verification.status === "failed"
                ? "danger"
                : "warning"
          }
        >
          {t(`apk.verificationBadges.${verification.status}`)}
        </Badge>
      </div>

      {verification.status === "not_verified" && (
        <div className="mt-4 rounded-md border border-amber-300/20 bg-amber-400/[0.06] p-3">
          <p className="text-xs font-medium text-amber-100">
            {t(
              `apk.verification.unavailable.${verification.unavailable_reason ?? "not_found"}`,
            )}
          </p>
          <p className="mt-1 text-xs leading-5 text-amber-100/75">
            {t("apk.verification.staticOnly", {
              schemes:
                staticSchemes.length > 0
                  ? staticSchemes.join(", ")
                  : t("apk.verification.noneDetected"),
            })}
          </p>
          {toolLabel && (
            <p className="mt-2 text-[11px] text-anvil-400">{toolLabel}</p>
          )}
        </div>
      )}

      {verification.status === "failed" && (
        <div className="mt-4 rounded-md border border-red-300/20 bg-red-400/[0.06] p-3">
          <p className="text-xs font-medium text-red-100">
            {t("apk.verification.failedBody")}
          </p>
          {verification.errors.length > 0 && (
            <ul className="mt-2 space-y-1 font-mono text-[11px] text-red-100/80">
              {verification.errors.map((error, index) => (
                <li key={`${index}-${error}`} className="break-all">
                  {error}
                </li>
              ))}
            </ul>
          )}
          {toolLabel && (
            <p className="mt-2 text-[11px] text-anvil-400">{toolLabel}</p>
          )}
        </div>
      )}

      {verification.status === "verified" && (
        <>
          <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-2 text-sm sm:grid-cols-3">
            <Fact
              label={t("apk.verification.schemes")}
              value={verification.verified_schemes.join(", ")}
            />
            <Fact
              label={t("apk.verification.signerCount")}
              value={formatNumber(verification.signer_count, language)}
            />
            <Fact
              label={t("apk.verification.sourceStamp")}
              value={
                verification.source_stamp_verified
                  ? t("apk.verification.verified")
                  : t("apk.verification.notPresent")
              }
            />
          </dl>
          {toolLabel && (
            <p className="mt-3 text-[11px] text-anvil-400">{toolLabel}</p>
          )}

          <div className="mt-4 grid gap-3 lg:grid-cols-2">
            {verification.signers.map((certificate) => (
              <CertificateCard
                key={`${certificate.label}-${certificate.sha256}`}
                certificate={certificate}
                language={language}
              />
            ))}
          </div>

          {verification.source_stamp && (
            <div className="mt-4">
              <h5 className="text-xs font-semibold text-anvil-200">
                {t("apk.verification.sourceStampCertificate")}
              </h5>
              <div className="mt-2 max-w-2xl">
                <CertificateCard
                  certificate={verification.source_stamp}
                  language={language}
                />
              </div>
            </div>
          )}

          {verification.proof_of_rotation && (
            <div className="mt-5 border-t border-white/10 pt-4">
              <h5 className="text-xs font-semibold text-anvil-100">
                {t("apk.verification.rotationTitle")}
              </h5>
              <p className="mt-1 text-xs leading-5 text-anvil-400">
                {t("apk.verification.rotationBody", {
                  count: verification.proof_of_rotation.entries.length,
                })}
              </p>
              <ol className="mt-3 space-y-3">
                {verification.proof_of_rotation.entries.map((entry) => (
                  <LineageEntry
                    key={`${entry.position}-${entry.certificate.sha256}`}
                    entry={entry}
                    language={language}
                  />
                ))}
              </ol>
            </div>
          )}

          {verification.warnings.length > 0 && (
            <div className="mt-4 rounded-md border border-amber-300/20 bg-amber-400/[0.05] p-3">
              <h5 className="text-xs font-semibold text-amber-100">
                {t("apk.verification.warnings")}
              </h5>
              <ul className="mt-2 space-y-1 font-mono text-xs text-amber-100/75">
                {verification.warnings.map((warning, index) => (
                  <li key={`${index}-${warning}`} className="break-all">
                    {warning}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </Card>
  );
}

function CertificateCard({
  certificate,
  language,
}: {
  certificate: ApkSignerCertificate;
  language: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="rounded-md border border-white/10 bg-black/10 p-3">
      <h5 className="text-xs font-semibold text-anvil-100">
        {certificate.label}
      </h5>
      <dl className="mt-2 space-y-2 text-xs">
        <CertificateFact
          label={t("apk.verification.subject")}
          value={certificate.subject}
        />
        <CertificateFact
          label={t("apk.verification.issuer")}
          value={certificate.issuer}
        />
        <CertificateFact
          label={t("apk.verification.validity")}
          value={t("apk.verification.validityRange", {
            from: formatUnixDate(certificate.valid_from_unix, language),
            until: formatUnixDate(certificate.valid_until_unix, language),
          })}
        />
        <CertificateFact
          label={t("apk.verification.fingerprint")}
          value={formatDigest(certificate.sha256)}
          mono
        />
      </dl>
    </div>
  );
}

function LineageEntry({
  entry,
  language,
}: {
  entry: ApkSigningLineageEntry;
  language: string;
}) {
  const { t } = useTranslation();
  const capabilities = (
    [
      ["installed_data", "installedData"],
      ["shared_uid", "sharedUid"],
      ["permission", "permissionCapability"],
      ["rollback", "rollback"],
      ["auth", "auth"],
    ] as const
  )
    .filter(([field]) => entry.capabilities[field])
    .map(([, key]) => t(`apk.verification.capabilities.${key}`));
  return (
    <li className="rounded-md border border-white/10 bg-white/[0.025] p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-xs font-semibold text-anvil-100">
          {t("apk.verification.lineagePosition", { position: entry.position })}
        </span>
        <span className="text-xs text-anvil-400">
          {capabilities.length > 0
            ? capabilities.join(" · ")
            : t("apk.verification.noCapabilities")}
        </span>
      </div>
      <div className="mt-2">
        <CertificateCard certificate={entry.certificate} language={language} />
      </div>
    </li>
  );
}

function CertificateFact({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div>
      <dt className="text-anvil-500">{label}</dt>
      <dd
        className={`mt-0.5 break-all text-anvil-200 ${mono ? "font-mono text-[11px]" : ""}`}
      >
        {value}
      </dd>
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

function formatOptionalNumber(
  value: number | null | undefined,
  language: string,
): string | undefined {
  return value == null ? undefined : formatNumber(value, language);
}

function formatUnixDate(seconds: number, language: string): string {
  return formatDateTime(new Date(seconds * 1000).toISOString(), language);
}

function formatDigest(digest: string): string {
  return (
    digest
      .toUpperCase()
      .match(/.{1,2}/gu)
      ?.join(":") ?? digest
  );
}
