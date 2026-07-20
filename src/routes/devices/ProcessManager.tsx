import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callListProcesses,
  type DeviceTarget,
  type ProcessInfo,
} from "../../lib/tauri";
import { Badge, Button, Card, EmptyState, FieldInput } from "../common";
import { formatKb } from "./common";

/** Read-only process list (`ps`) with search and RSS/name sort (IMP-67:
 *  extracted verbatim from the former Devices.tsx god-file). */
export function ProcessManager({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [processes, setProcesses] = useState<ProcessInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<"rss" | "name">("rss");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const procs = await callListProcesses(target);
      setProcesses(procs);
    } catch (e) {
      setProcesses([]);
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [target]);

  const filtered = processes
    .filter((p) =>
      search ? p.name.toLowerCase().includes(search.toLowerCase()) : true,
    )
    .sort((a, b) =>
      sortBy === "rss" ? b.rss_kb - a.rss_kb : a.name.localeCompare(b.name),
    );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.controls.processManager")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.controls.processManagerBody")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <FieldInput
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("devices.controls.filter")}
            aria-label={t("devices.controls.filterProcesses")}
            className="h-8 w-40 px-2 font-mono text-xs"
          />
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => void refresh()}
            disabled={loading}
          >
            {loading
              ? t("devices.controls.loading")
              : processes.length > 0
                ? t("devices.controls.refresh")
                : t("devices.controls.load")}
          </Button>
        </div>
      </div>
      {error && (
        <div className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200">
          {t("devices.controls.processReadFailed", { message: error })}
        </div>
      )}
      {processes.length === 0 && !loading && !error && (
        <EmptyState title={t("devices.controls.noProcesses")}>
          <p>{t("devices.controls.noProcessesBody")}</p>
        </EmptyState>
      )}
      {processes.length > 0 && filtered.length === 0 && (
        <EmptyState title={t("devices.controls.noMatchingProcesses")}>
          <p>{t("devices.controls.noMatchingProcessesBody")}</p>
        </EmptyState>
      )}
      {processes.length > 0 && filtered.length > 0 && (
        <div className="max-h-96 overflow-x-auto">
          <table className="min-w-full text-xs">
            <thead className="sticky top-0 bg-anvil-900">
              <tr>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colPid")}
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  {t("devices.controls.colUser")}
                </th>
                <th
                  className="px-3 py-2 text-right font-semibold text-anvil-400"
                  aria-sort={sortBy === "rss" ? "descending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("rss")}
                    className="ml-auto flex items-center gap-1 hover:text-anvil-200"
                  >
                    {t("devices.controls.colRss")}
                    {sortBy === "rss" && <span aria-hidden="true">&darr;</span>}
                  </button>
                </th>
                <th
                  className="px-3 py-2 text-left font-semibold text-anvil-400"
                  aria-sort={sortBy === "name" ? "ascending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("name")}
                    className="flex items-center gap-1 hover:text-anvil-200"
                  >
                    {t("devices.controls.colName")}
                    {sortBy === "name" && (
                      <span aria-hidden="true">&uarr;</span>
                    )}
                  </button>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {filtered.slice(0, 100).map((p, i) => (
                <tr
                  key={`${p.pid}-${p.name}-${i}`}
                  className="hover:bg-white/[0.03]"
                >
                  <td className="px-3 py-1.5 font-mono text-anvil-300">
                    {p.pid}
                  </td>
                  <td className="px-3 py-1.5 text-anvil-400">{p.user}</td>
                  <td className="px-3 py-1.5 text-right font-mono text-anvil-200">
                    {formatKb(p.rss_kb)}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    <span>{p.name}</span>
                    {p.parse_error && (
                      <Badge tone="warning" className="ml-2">
                        {t("devices.controls.parseIssue")}
                      </Badge>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {filtered.length > 100 && (
            <p className="px-3 py-2 text-xs text-anvil-500">
              {t("devices.controls.showingProcesses", {
                count: filtered.length,
              })}
            </p>
          )}
        </div>
      )}
    </Card>
  );
}
