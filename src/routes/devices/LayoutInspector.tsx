import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callCaptureLayout,
  callSaveLayoutExport,
  callSelectHostPath,
  type DeviceTarget,
  type LayoutNode,
} from "../../lib/tauri";
import { Badge, Button, Card, EmptyState, FieldInput } from "../common";
import { statusToneClass, type StatusMessage } from "./common";

/** Read-only `uiautomator dump` hierarchy viewer with raw-XML export (IMP-67:
 *  extracted verbatim from the former Devices.tsx god-file). */
export function LayoutInspector({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [nodes, setNodes] = useState<LayoutNode[]>([]);
  const [rawXml, setRawXml] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exportMsg, setExportMsg] = useState<StatusMessage>(null);
  const [search, setSearch] = useState("");

  const capture = useCallback(async () => {
    setLoading(true);
    setError(null);
    setExportMsg(null);
    try {
      const snapshot = await callCaptureLayout(target);
      setNodes(snapshot.nodes);
      setRawXml(snapshot.raw_xml);
    } catch (e) {
      setNodes([]);
      setRawXml("");
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [target]);

  const exportXml = useCallback(async () => {
    if (!rawXml) return;
    setExportMsg(null);
    try {
      const grant = await callSelectHostPath(
        "layout_export_save",
        `layout-${target.serial.replace(/[<>:"/\\|?*]/gu, "_")}-${Date.now()}.xml`,
      );
      if (!grant) return;
      const savedPath = await callSaveLayoutExport(grant.id, rawXml);
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
  }, [rawXml, target.serial, t]);

  const term = search.trim().toLowerCase();
  const filtered = term
    ? nodes.filter(
        (node) =>
          node.class.toLowerCase().includes(term) ||
          node.text.toLowerCase().includes(term) ||
          node.content_desc.toLowerCase().includes(term) ||
          node.resource_id.toLowerCase().includes(term) ||
          Boolean(node.parse_error),
      )
    : nodes;

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
        <div className="flex items-center gap-2">
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
            variant="ghost"
            onClick={() => void exportXml()}
            disabled={!rawXml || loading}
          >
            {t("devices.layout.export")}
          </Button>
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
      </div>
      {error && (
        <div className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200">
          {t("devices.layout.captureFailed", { message: error })}
        </div>
      )}
      {exportMsg && (
        <p
          role="status"
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
      {filtered.length > 0 && (
        <div className="max-h-96 overflow-auto py-1">
          {filtered.slice(0, 500).map((node, i) => (
            <div
              key={`${node.resource_id}-${node.bounds}-${i}`}
              className="flex items-baseline gap-2 px-4 py-1 font-mono text-xs hover:bg-white/[0.03]"
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
    </Card>
  );
}

function shortClass(value: string): string {
  const parts = value.split(".");
  return parts.length > 1 ? parts.slice(-2).join(".") : value;
}
