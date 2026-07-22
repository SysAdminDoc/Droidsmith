import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callCaptureLayout,
  callSaveLayoutExport,
  callSelectHostPath,
  type DeviceTarget,
  type LayoutAuditFinding,
  type LayoutNode,
} from "../../lib/tauri";
import { Badge, Button, Card, EmptyState, FieldInput } from "../common";
import { statusToneClass, type StatusMessage } from "./common";

type ExportFormat = "xml" | "json" | "txt";

/** Read-only `uiautomator dump` hierarchy viewer and deterministic
 * accessibility audit (IMP-67, R-105). */
export function LayoutInspector({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [nodes, setNodes] = useState<LayoutNode[]>([]);
  const [rawXml, setRawXml] = useState("");
  const [densityDpi, setDensityDpi] = useState<number | null>(null);
  const [findings, setFindings] = useState<LayoutAuditFinding[]>([]);
  const [selectedNode, setSelectedNode] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exportMsg, setExportMsg] = useState<StatusMessage>(null);
  const [search, setSearch] = useState("");

  const capture = useCallback(async () => {
    setLoading(true);
    setError(null);
    setExportMsg(null);
    setSelectedNode(null);
    try {
      const snapshot = await callCaptureLayout(target);
      setNodes(snapshot.nodes);
      setRawXml(snapshot.raw_xml);
      setDensityDpi(snapshot.density_dpi);
      setFindings(snapshot.audit_findings);
    } catch (e) {
      setNodes([]);
      setRawXml("");
      setDensityDpi(null);
      setFindings([]);
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [target]);

  const exportCapture = useCallback(
    async (format: ExportFormat) => {
      if (!rawXml) return;
      setExportMsg(null);
      try {
        const generatedAt = new Date().toISOString();
        const contents =
          format === "xml"
            ? rawXml
            : format === "json"
              ? buildAuditJson({
                  generatedAt,
                  serial: target.serial,
                  densityDpi,
                  findings,
                  nodes,
                })
              : buildAuditText({
                  generatedAt,
                  serial: target.serial,
                  densityDpi,
                  findings,
                  nodes,
                });
        const prefix = format === "xml" ? "layout" : "layout-audit";
        const grant = await callSelectHostPath(
          "layout_export_save",
          `${prefix}-${safeFilePart(target.serial)}-${Date.now()}.${format}`,
        );
        if (!grant) return;
        const savedPath = await callSaveLayoutExport(grant.id, contents);
        setExportMsg({
          text: t("devices.layout.exported", { path: savedPath }),
          tone: "success",
        });
      } catch (e) {
        setExportMsg({
          tone: "danger",
          text: t("devices.layout.exportFailed", {
            message: errorMessage(e),
          }),
        });
      }
    },
    [densityDpi, findings, nodes, rawXml, target.serial, t],
  );

  const goToNode = useCallback((index: number) => {
    setSearch("");
    setSelectedNode(index);
    requestAnimationFrame(() => {
      const element = document.getElementById(`layout-node-${index}`);
      element?.scrollIntoView({ block: "center" });
      element?.focus();
    });
  }, []);

  const term = search.trim().toLowerCase();
  const indexedNodes = nodes.map((node, index) => ({ node, index }));
  const filtered = term
    ? indexedNodes.filter(
        ({ node }) =>
          node.class.toLowerCase().includes(term) ||
          node.text.toLowerCase().includes(term) ||
          node.content_desc.toLowerCase().includes(term) ||
          node.resource_id.toLowerCase().includes(term) ||
          Boolean(node.parse_error),
      )
    : indexedNodes;

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.layout.title")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.layout.body")} <code>uiautomator dump</code>.
          </p>
        </div>
        <div className="flex flex-col gap-1.5 sm:items-end">
          <div className="flex flex-wrap items-center gap-2">
            <FieldInput
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("devices.controls.filter")}
              aria-label={t("devices.layout.filterNodes")}
              className="h-8 w-40 px-2 font-mono text-xs"
            />
            <Button
              type="button"
              size="sm"
              variant="primary"
              onClick={() => void capture()}
              disabled={loading}
            >
              {loading
                ? t("devices.controls.loading")
                : nodes.length > 0
                  ? t("devices.layout.recapture")
                  : t("devices.layout.capture")}
            </Button>
          </div>
          <div className="flex flex-wrap items-center gap-1">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => void exportCapture("xml")}
              disabled={!rawXml || loading}
            >
              {t("devices.layout.export")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => void exportCapture("json")}
              disabled={!rawXml || loading}
            >
              {t("devices.layout.exportAuditJson")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={() => void exportCapture("txt")}
              disabled={!rawXml || loading}
            >
              {t("devices.layout.exportAuditText")}
            </Button>
          </div>
        </div>
      </div>
      {error && (
        <div
          role="alert"
          className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200"
        >
          {t("devices.layout.captureFailed", { message: error })}
        </div>
      )}
      {exportMsg && (
        <p
          role={exportMsg.tone === "danger" ? "alert" : "status"}
          className={`px-4 py-2 text-xs ${statusToneClass(exportMsg.tone)}`}
        >
          {exportMsg.text}
        </p>
      )}
      {nodes.length === 0 && !loading && !error && (
        <EmptyState title={t("devices.layout.empty")}>
          <p>{t("devices.layout.emptyBody")}</p>
        </EmptyState>
      )}
      {nodes.length > 0 && (
        <section
          aria-labelledby="layout-audit-title"
          className="space-y-3 border-b border-white/10 bg-white/[0.02] p-4"
        >
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <div>
              <h4
                id="layout-audit-title"
                className="text-sm font-semibold text-anvil-100"
              >
                {t("devices.layout.auditTitle")}
              </h4>
              <p className="mt-1 text-xs text-anvil-400">
                {findings.length === 0
                  ? t("devices.layout.auditClean")
                  : t("devices.layout.auditSummary", {
                      count: findings.length,
                    })}
              </p>
            </div>
            <Badge tone={findings.length === 0 ? "success" : "warning"}>
              {t("devices.layout.densityLabel")}:{" "}
              {densityDpi ?? t("devices.layout.densityUnavailable")}
              {densityDpi != null && " dpi"}
            </Badge>
          </div>
          <div className="rounded-md border border-amber-300/20 bg-amber-400/[0.06] p-3 text-xs text-amber-100">
            <p className="font-medium">
              {t("devices.layout.privacyWarningTitle")}
            </p>
            <p className="mt-1 text-amber-100/80">
              {t("devices.layout.privacyWarningBody")}
            </p>
          </div>
          <p className="text-xs text-anvil-400">
            {t("devices.layout.contrastNotEvaluated")}
          </p>
          {findings.length > 0 && (
            <ol className="space-y-2">
              {findings.map((finding) => (
                <AuditFindingCard
                  key={finding.id}
                  finding={finding}
                  node={nodes[finding.node_index]}
                  onGoToNode={goToNode}
                />
              ))}
            </ol>
          )}
        </section>
      )}
      {filtered.length > 0 && (
        <div className="max-h-96 overflow-auto py-1">
          {filtered.slice(0, 500).map(({ node, index }) => (
            <div
              id={`layout-node-${index}`}
              key={`${node.resource_id}-${node.bounds}-${index}`}
              tabIndex={-1}
              className={`flex items-baseline gap-2 px-4 py-1 font-mono text-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-circuit-300 ${
                selectedNode === index
                  ? "bg-circuit-300/10"
                  : "hover:bg-white/[0.03]"
              }`}
              style={{ paddingLeft: `${16 + node.depth * 14}px` }}
            >
              {node.parse_error ? (
                <span className="text-red-300">
                  ⚠ {t("devices.layout.nodeParseError")}: {node.parse_error}
                </span>
              ) : (
                <>
                  <span className="text-circuit-200/80">
                    {shortClass(node.class)}
                  </span>
                  {node.resource_id && (
                    <span className="text-anvil-400">#{node.resource_id}</span>
                  )}
                  {node.text && (
                    <span className="text-anvil-100">“{node.text}”</span>
                  )}
                  {node.content_desc && (
                    <span className="text-anvil-300">
                      [{node.content_desc}]
                    </span>
                  )}
                  {node.clickable && (
                    <Badge tone="info">{t("devices.layout.clickable")}</Badge>
                  )}
                  <span className="ms-auto ps-3 text-anvil-600">
                    {node.bounds}
                  </span>
                </>
              )}
            </div>
          ))}
          {filtered.length > 500 && (
            <p className="px-4 py-2 text-xs text-anvil-500">
              {t("devices.layout.truncated", { count: filtered.length })}
            </p>
          )}
        </div>
      )}
      {nodes.length > 0 && filtered.length === 0 && !loading && (
        <EmptyState title={t("devices.layout.noMatches")}>
          <p>{t("devices.layout.noMatchesBody")}</p>
        </EmptyState>
      )}
    </Card>
  );
}

function AuditFindingCard({
  finding,
  node,
  onGoToNode,
}: {
  finding: LayoutAuditFinding;
  node: LayoutNode | undefined;
  onGoToNode: (index: number) => void;
}) {
  const { t } = useTranslation();
  const keys = {
    missing_accessible_label: [
      "devices.layout.missingAccessibleLabelTitle",
      "devices.layout.missingAccessibleLabelBody",
    ],
    duplicate_resource_id: [
      "devices.layout.duplicateResourceIdTitle",
      "devices.layout.duplicateResourceIdBody",
    ],
    small_click_target: [
      "devices.layout.smallClickTargetTitle",
      "devices.layout.smallClickTargetBody",
    ],
  } as const;
  const [titleKey, bodyKey] = keys[finding.kind];
  const related = Array.from(new Set(finding.related_node_indices));

  return (
    <li className="rounded-md border border-white/10 bg-anvil-950/30 p-3">
      <h5 className="text-xs font-semibold text-anvil-100">{t(titleKey)}</h5>
      <p className="mt-1 text-xs text-anvil-400">{t(bodyKey)}</p>
      <div className="mt-2 flex flex-wrap gap-2">
        {related.map((index) => (
          <Button
            key={index}
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => onGoToNode(index)}
          >
            {t("devices.layout.goToNode", { index })}
          </Button>
        ))}
      </div>
      <dl className="mt-2 grid gap-1 text-xs sm:grid-cols-[max-content_1fr]">
        {finding.resource_id && (
          <>
            <dt className="text-anvil-500">ID</dt>
            <dd className="break-all font-mono text-anvil-300">
              {finding.resource_id}
            </dd>
          </>
        )}
        <dt className="text-anvil-500">{t("devices.layout.bounds")}</dt>
        <dd className="font-mono text-anvil-300">{finding.bounds}</dd>
        {finding.width_px != null && finding.height_px != null && (
          <>
            <dt className="text-anvil-500">
              {t("devices.layout.measuredTarget")}
            </dt>
            <dd className="font-mono text-anvil-300">
              {finding.width_px}×{finding.height_px}px
              {finding.width_dp_tenths != null &&
                finding.height_dp_tenths != null &&
                ` (${formatTenths(finding.width_dp_tenths)}×${formatTenths(finding.height_dp_tenths)}dp)`}
            </dd>
          </>
        )}
        <dt className="text-anvil-500">{t("devices.layout.rawAttributes")}</dt>
        <dd className="break-all font-mono text-anvil-300">
          {node?.raw_attributes ?? "—"}
        </dd>
      </dl>
    </li>
  );
}

interface AuditExportInput {
  generatedAt: string;
  serial: string;
  densityDpi: number | null;
  findings: LayoutAuditFinding[];
  nodes: LayoutNode[];
}

function buildAuditJson(input: AuditExportInput): string {
  return `${JSON.stringify(
    {
      schema_version: 1,
      generated_at: input.generatedAt,
      device_serial: input.serial,
      density_dpi: input.densityDpi,
      contrast_evaluated: false,
      privacy_notice:
        "This local report may contain sensitive on-screen text, descriptions, and resource IDs.",
      findings: input.findings.map((finding) => ({
        ...finding,
        node: input.nodes[finding.node_index] ?? null,
        related_nodes: finding.related_node_indices.map(
          (index) => input.nodes[index] ?? null,
        ),
      })),
    },
    null,
    2,
  )}\n`;
}

function buildAuditText(input: AuditExportInput): string {
  const lines = [
    "Droidsmith Layout Accessibility Audit",
    `Generated: ${input.generatedAt}`,
    `Device: ${input.serial}`,
    `Density: ${input.densityDpi == null ? "unavailable" : `${input.densityDpi} dpi`}`,
    "Contrast evaluated: no (UIAutomator XML has no rendered-color evidence)",
    "Privacy: this local report may contain sensitive on-screen text, descriptions, and resource IDs.",
    `Findings: ${input.findings.length}`,
  ];
  for (const finding of input.findings) {
    const node = input.nodes[finding.node_index];
    lines.push(
      "",
      `[${finding.kind}] Node ${finding.node_index}`,
      `Related nodes: ${finding.related_node_indices.join(", ")}`,
      `Resource ID: ${finding.resource_id || "(empty)"}`,
      `Bounds: ${finding.bounds || "(unavailable)"}`,
      `Measured target: ${formatFindingDimensions(finding)}`,
      `Raw attributes: ${node?.raw_attributes ?? "(unavailable)"}`,
    );
  }
  return `${lines.join("\n")}\n`;
}

function formatFindingDimensions(finding: LayoutAuditFinding): string {
  if (finding.width_px == null || finding.height_px == null)
    return "unavailable";
  const px = `${finding.width_px}x${finding.height_px}px`;
  if (finding.width_dp_tenths == null || finding.height_dp_tenths == null) {
    return px;
  }
  return `${px} (${formatTenths(finding.width_dp_tenths)}x${formatTenths(finding.height_dp_tenths)}dp)`;
}

function formatTenths(value: number): string {
  return (value / 10).toFixed(1);
}

function safeFilePart(value: string): string {
  return value.replace(/[<>:"/\\|?*]/gu, "_");
}

function shortClass(value: string): string {
  const parts = value.split(".");
  return parts.length > 1 ? parts.slice(-2).join(".") : value;
}
