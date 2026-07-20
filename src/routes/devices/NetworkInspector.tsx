import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callListNetworkConnections,
  type DeviceTarget,
  type NetworkConnection,
} from "../../lib/tauri";
import { Badge, Button, Card, EmptyState, FieldInput } from "../common";

/** Read-only `ss -tunp` socket inspector for the selected device (IMP-67:
 *  extracted verbatim from the former Devices.tsx god-file). */
export function NetworkInspector({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [connections, setConnections] = useState<NetworkConnection[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const conns = await callListNetworkConnections(target);
      setConnections(conns);
    } catch (e) {
      setConnections([]);
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [target]);

  const filtered = connections.filter((c) =>
    search
      ? c.local_addr.includes(search) ||
        c.remote_addr.includes(search) ||
        (c.process?.includes(search) ?? false) ||
        c.state.toLowerCase().includes(search.toLowerCase())
      : true,
  );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.controls.networkConnections")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.controls.networkBody")} <code>ss -tunp</code>.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <FieldInput
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("devices.controls.filter")}
            aria-label={t("devices.controls.filterConnections")}
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
              : connections.length > 0
                ? t("devices.controls.refresh")
                : t("devices.controls.load")}
          </Button>
        </div>
      </div>
      {error && (
        <div className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200">
          {t("devices.controls.connectionsReadFailed", { message: error })}
        </div>
      )}
      {connections.length === 0 && !loading && !error && (
        <EmptyState title={t("devices.controls.noConnections")}>
          <p>{t("devices.controls.noConnectionsBody")}</p>
        </EmptyState>
      )}
      {connections.length > 0 && filtered.length === 0 && (
        <EmptyState title={t("devices.controls.noMatchingConnections")}>
          <p>{t("devices.controls.noMatchingConnectionsBody")}</p>
        </EmptyState>
      )}
      {connections.length > 0 && filtered.length > 0 && (
        <div className="max-h-80 overflow-x-auto">
          <table className="min-w-full text-xs">
            <thead className="sticky top-0 bg-anvil-900">
              <tr>
                <th className="px-3 py-2 text-start font-semibold text-anvil-400">
                  {t("devices.controls.colProto")}
                </th>
                <th className="px-3 py-2 text-start font-semibold text-anvil-400">
                  {t("devices.controls.colState")}
                </th>
                <th className="px-3 py-2 text-start font-semibold text-anvil-400">
                  {t("devices.controls.colLocal")}
                </th>
                <th className="px-3 py-2 text-start font-semibold text-anvil-400">
                  {t("devices.controls.colRemote")}
                </th>
                <th className="px-3 py-2 text-start font-semibold text-anvil-400">
                  {t("devices.controls.colProcess")}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {filtered.slice(0, 100).map((c, i) => (
                <tr
                  key={`${c.protocol}-${c.local_addr}-${c.remote_addr}-${i}`}
                  className="hover:bg-white/[0.03]"
                >
                  <td className="px-3 py-1.5 font-mono text-anvil-300">
                    {c.protocol}
                  </td>
                  <td className="px-3 py-1.5 text-anvil-200">{c.state}</td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    {c.local_addr}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    {c.remote_addr}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-400">
                    {c.process ?? t("devices.controls.notReported")}
                    {c.parse_error && (
                      <Badge tone="warning" className="ms-2">
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
              {t("devices.controls.showingConnections", {
                count: filtered.length,
              })}
            </p>
          )}
        </div>
      )}
    </Card>
  );
}
