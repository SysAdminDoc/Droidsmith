import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useFocusTrap } from "../../lib/useFocusTrap";
import {
  errorMessage,
  callApplyAction,
  callGetAppMemoryLimit,
  callListProcessExitHistory,
  callListProcesses,
  callPlanAction,
  type DeviceTarget,
  type ProcessExitHistory,
  type ProcessInfo,
  type AppMemoryLimit,
} from "../../lib/tauri";
import { useTargetOperation } from "../../lib/targetOperation";
import { Badge, Button, Card, EmptyState, FieldInput } from "../common";
import { appProcessPackage, formatKb } from "./common";

/** Read-only process list (`ps`) with search and RSS/name sort (IMP-67:
 *  extracted verbatim from the former Devices.tsx god-file). R-090 adds a
 *  confirmed force-stop on rows whose name resolves to an app package. */
export function ProcessManager({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const refreshOperation = useTargetOperation(target, "process-list");
  const mutationOperation = useTargetOperation(target, "process-mutation");
  const exitHistoryOperation = useTargetOperation(
    target,
    "process-exit-history",
  );
  const [processes, setProcesses] = useState<ProcessInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<"rss" | "cpu" | "name">("rss");
  const [confirmPackage, setConfirmPackage] = useState<string | null>(null);
  const [stopping, setStopping] = useState<string | null>(null);
  const [stopError, setStopError] = useState<string | null>(null);
  const [memoryLimit, setMemoryLimit] = useState<AppMemoryLimit | null>(null);
  const [exitPackage, setExitPackage] = useState("");
  const [exitUser, setExitUser] = useState("0");
  const [exitHistory, setExitHistory] = useState<ProcessExitHistory | null>(
    null,
  );
  const [exitHistoryLoading, setExitHistoryLoading] = useState(false);
  const [exitHistoryError, setExitHistoryError] = useState<string | null>(null);
  const confirmTrapRef = useFocusTrap<HTMLDivElement>(confirmPackage !== null);

  const exitUserNumber = Number(exitUser.trim());
  const validExitUser =
    /^\d+$/.test(exitUser.trim()) &&
    Number.isSafeInteger(exitUserNumber) &&
    exitUserNumber >= 0 &&
    exitUserNumber <= 0xffffffff;

  useEffect(() => {
    setProcesses([]);
    setLoading(false);
    setError(null);
    setConfirmPackage(null);
    setStopping(null);
    setStopError(null);
    setMemoryLimit(null);
    setExitHistory(null);
    setExitHistoryLoading(false);
    setExitHistoryError(null);
  }, [target.connection_generation, target.serial, target.transport_id]);

  useEffect(() => {
    let active = true;
    void callGetAppMemoryLimit(target)
      .then((value) => {
        if (active) setMemoryLimit(value);
      })
      .catch(() => {
        if (active) setMemoryLimit(null);
      });
    return () => {
      active = false;
    };
  }, [target]);

  useEffect(() => {
    if (!confirmPackage) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setConfirmPackage(null);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [confirmPackage]);

  const refresh = useCallback(async () => {
    const lease = refreshOperation.begin();
    setLoading(true);
    setError(null);
    try {
      const procs = await callListProcesses(target);
      lease.commit(() => setProcesses(procs));
    } catch (e) {
      lease.commit(() => {
        setProcesses([]);
        setError(errorMessage(e));
      });
    } finally {
      lease.commit(() => setLoading(false));
    }
  }, [refreshOperation, target]);

  const forceStop = useCallback(
    async (pkg: string) => {
      const lease = mutationOperation.begin();
      setConfirmPackage(null);
      setStopping(pkg);
      setStopError(null);
      try {
        const plan = await callPlanAction({
          serial: target.serial,
          target,
          package: pkg,
          kind: "force_stop",
          user_id: 0,
        });
        await callApplyAction(plan);
        if (!lease.isCurrent()) return;
        await refresh();
      } catch (e) {
        lease.commit(() =>
          setStopError(t("devices.controls.forceStopFailed", { package: pkg })),
        );
        void e;
      } finally {
        lease.commit(() => setStopping(null));
      }
    },
    [mutationOperation, refresh, t, target],
  );

  const loadExitHistory = useCallback(async () => {
    const pkg = exitPackage.trim();
    if (!pkg || !validExitUser) {
      setExitHistoryError(t("devices.controls.exitHistoryInvalid"));
      return;
    }
    const lease = exitHistoryOperation.begin();
    setExitHistoryLoading(true);
    setExitHistoryError(null);
    try {
      const history = await callListProcessExitHistory(
        target,
        pkg,
        exitUserNumber,
      );
      lease.commit(() => setExitHistory(history));
    } catch (e) {
      lease.commit(() => {
        setExitHistory(null);
        setExitHistoryError(
          t("devices.controls.exitHistoryReadFailed", {
            message: errorMessage(e),
          }),
        );
      });
    } finally {
      lease.commit(() => setExitHistoryLoading(false));
    }
  }, [
    exitHistoryOperation,
    exitPackage,
    exitUserNumber,
    t,
    target,
    validExitUser,
  ]);

  const filtered = processes
    .filter((p) =>
      search ? p.name.toLowerCase().includes(search.toLowerCase()) : true,
    )
    .sort((a, b) => {
      if (sortBy === "rss") return b.rss_kb - a.rss_kb;
      if (sortBy === "cpu")
        return (b.cpu_percent ?? -1) - (a.cpu_percent ?? -1);
      return a.name.localeCompare(b.name);
    });

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
        <div
          role="alert"
          className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200"
        >
          {t("devices.controls.processReadFailed", { message: error })}
        </div>
      )}
      {stopError && (
        <div
          role="alert"
          className="border-b border-red-500/20 bg-red-500/10 px-4 py-3 text-xs text-red-200"
        >
          {stopError}
        </div>
      )}
      {memoryLimit && (
        <div className="border-b border-white/10 bg-white/[0.02] px-4 py-3 text-xs text-anvil-300">
          <span className="font-medium text-anvil-100">
            {t("devices.controls.memoryLimitTitle")}
          </span>{" "}
          {memoryLimit.status === "unsupported"
            ? t("devices.controls.memoryLimitUnsupported")
            : memoryLimit.status === "available"
              ? memoryLimit.limit_kb
                ? t("devices.controls.memoryLimitValue", {
                    value: formatKb(memoryLimit.limit_kb),
                  })
                : t("devices.controls.memoryLimitDetected", {
                    detail: memoryLimit.detail ?? "",
                  })
              : t("devices.controls.memoryLimitUnknown")}
        </div>
      )}
      <div className="border-b border-white/10 bg-white/[0.02] px-4 py-4">
        <div className="flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
          <div>
            <h4 className="text-sm font-semibold text-anvil-100">
              {t("devices.controls.exitHistory")}
            </h4>
            <p className="mt-1 max-w-2xl text-xs text-anvil-400">
              {t("devices.controls.exitHistoryBody")}
            </p>
          </div>
          <form
            className="flex flex-wrap items-end gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              void loadExitHistory();
            }}
          >
            <label className="flex min-w-48 flex-col gap-1 text-[11px] text-anvil-400">
              {t("devices.controls.exitHistoryPackage")}
              <FieldInput
                type="text"
                value={exitPackage}
                onChange={(event) => setExitPackage(event.target.value)}
                placeholder={t(
                  "devices.controls.exitHistoryPackagePlaceholder",
                )}
                aria-label={t("devices.controls.exitHistoryPackage")}
                className="h-8 font-mono text-xs"
              />
            </label>
            <label className="flex w-28 flex-col gap-1 text-[11px] text-anvil-400">
              {t("devices.controls.exitHistoryUser")}
              <FieldInput
                type="number"
                min="0"
                step="1"
                value={exitUser}
                onChange={(event) => setExitUser(event.target.value)}
                aria-label={t("devices.controls.exitHistoryUser")}
                className="h-8 font-mono text-xs"
              />
            </label>
            <Button
              type="submit"
              size="sm"
              variant="ghost"
              disabled={
                exitHistoryLoading || !exitPackage.trim() || !validExitUser
              }
            >
              {exitHistoryLoading
                ? t("devices.controls.exitHistoryLoading")
                : t("devices.controls.exitHistoryLoad")}
            </Button>
          </form>
        </div>
        {exitHistoryError && (
          <div
            role="alert"
            className="mt-3 rounded border border-red-500/20 bg-red-500/10 px-3 py-2 text-xs text-red-200"
          >
            {exitHistoryError}
          </div>
        )}
        {exitHistory && (
          <div className="mt-4 overflow-x-auto">
            {exitHistory.truncated && (
              <p className="mb-2 text-xs text-amber-200">
                {t("devices.controls.exitHistoryTruncated", {
                  count: exitHistory.entries.length,
                })}
              </p>
            )}
            {exitHistory.entries.length === 0 ? (
              <EmptyState title={t("devices.controls.exitHistoryEmpty")}>
                <p>{t("devices.controls.exitHistoryEmptyBody")}</p>
              </EmptyState>
            ) : (
              <table className="min-w-full text-xs">
                <thead>
                  <tr>
                    <th className="px-2 py-2 text-start font-semibold text-anvil-500">
                      {t("devices.controls.exitTimestamp")}
                    </th>
                    <th className="px-2 py-2 text-start font-semibold text-anvil-500">
                      {t("devices.controls.exitHistoryUser")}
                    </th>
                    <th className="px-2 py-2 text-start font-semibold text-anvil-500">
                      {t("devices.controls.exitProcess")}
                    </th>
                    <th className="px-2 py-2 text-start font-semibold text-anvil-500">
                      {t("devices.controls.exitReasonLabel")}
                    </th>
                    <th className="px-2 py-2 text-end font-semibold text-anvil-500">
                      {t("devices.controls.exitStatus")}
                    </th>
                    <th className="px-2 py-2 text-end font-semibold text-anvil-500">
                      {t("devices.controls.exitPss")}
                    </th>
                    <th className="px-2 py-2 text-end font-semibold text-anvil-500">
                      {t("devices.controls.exitRss")}
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {exitHistory.entries.map((entry, index) => (
                    <tr
                      key={`${entry.timestamp}-${entry.process}-${index}`}
                      className="hover:bg-white/[0.03]"
                    >
                      <td className="whitespace-nowrap px-2 py-1.5 font-mono text-anvil-300">
                        {entry.timestamp || t("devices.controls.exitUnknown")}
                      </td>
                      <td className="px-2 py-1.5 font-mono text-anvil-300">
                        {entry.user_id ?? t("devices.controls.exitUnknown")}
                      </td>
                      <td className="px-2 py-1.5 font-mono text-anvil-100">
                        {entry.process || t("devices.controls.exitUnknown")}
                      </td>
                      <td className="px-2 py-1.5 text-anvil-200">
                        {t(`devices.controls.exitReasons.${entry.reason}`, {
                          defaultValue: t("devices.controls.exitUnknown"),
                        })}
                        {entry.parse_error && (
                          <Badge tone="warning" className="ms-2">
                            {t("devices.controls.exitParseIssue", {
                              message: entry.parse_error,
                            })}
                          </Badge>
                        )}
                      </td>
                      <td className="px-2 py-1.5 text-end font-mono text-anvil-300">
                        {entry.status ?? t("devices.controls.exitUnknown")}
                      </td>
                      <td className="px-2 py-1.5 text-end font-mono text-anvil-300">
                        {entry.pss_kb != null
                          ? formatKb(entry.pss_kb)
                          : t("devices.controls.exitUnknown")}
                      </td>
                      <td className="px-2 py-1.5 text-end font-mono text-anvil-300">
                        {entry.rss_kb != null
                          ? formatKb(entry.rss_kb)
                          : t("devices.controls.exitUnknown")}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}
      </div>
      {confirmPackage && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
          <div
            ref={confirmTrapRef}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="force-stop-dialog-title"
            aria-describedby="force-stop-dialog-description"
            tabIndex={-1}
            className="w-full max-w-lg rounded-lg border border-amber-300/25 bg-anvil-950 p-5 shadow-2xl outline-none"
          >
            <h4
              id="force-stop-dialog-title"
              className="text-lg font-semibold text-anvil-50"
            >
              {t("devices.controls.forceStopConfirmTitle")}
            </h4>
            <p
              id="force-stop-dialog-description"
              className="mt-2 text-sm leading-6 text-anvil-300"
            >
              {t("devices.controls.forceStopConfirmBody", {
                package: confirmPackage,
              })}
            </p>
            <div className="mt-5 flex justify-end gap-2">
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => setConfirmPackage(null)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="danger"
                onClick={() => void forceStop(confirmPackage)}
              >
                {t("devices.controls.forceStop")}
              </Button>
            </div>
          </div>
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
                <th className="px-3 py-2 text-start font-semibold text-anvil-400">
                  {t("devices.controls.colPid")}
                </th>
                <th className="px-3 py-2 text-start font-semibold text-anvil-400">
                  {t("devices.controls.colUser")}
                </th>
                <th
                  className="px-3 py-2 text-end font-semibold text-anvil-400"
                  aria-sort={sortBy === "rss" ? "descending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("rss")}
                    className="ms-auto flex items-center gap-1 hover:text-anvil-200"
                  >
                    {t("devices.controls.colRss")}
                    {sortBy === "rss" && <span aria-hidden="true">&darr;</span>}
                  </button>
                </th>
                <th
                  className="px-3 py-2 text-end font-semibold text-anvil-400"
                  aria-sort={sortBy === "cpu" ? "descending" : "none"}
                >
                  <button
                    type="button"
                    onClick={() => setSortBy("cpu")}
                    className="ms-auto flex items-center gap-1 hover:text-anvil-200"
                  >
                    {t("devices.controls.colCpu")}
                    {sortBy === "cpu" && <span aria-hidden="true">&darr;</span>}
                  </button>
                </th>
                <th
                  className="px-3 py-2 text-start font-semibold text-anvil-400"
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
                <th className="px-3 py-2 text-end font-semibold text-anvil-400">
                  {t("devices.controls.colActions")}
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
                  <td className="px-3 py-1.5 text-end font-mono text-anvil-200">
                    {formatKb(p.rss_kb)}
                  </td>
                  <td className="px-3 py-1.5 text-end font-mono text-anvil-300 tabular-nums">
                    {p.cpu_percent != null
                      ? `${p.cpu_percent.toFixed(1)}%`
                      : "—"}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    <span>{p.name}</span>
                    {p.parse_error && (
                      <Badge tone="warning" className="ms-2">
                        {t("devices.controls.parseIssue")}
                      </Badge>
                    )}
                  </td>
                  <td className="px-3 py-1.5 text-end">
                    {appProcessPackage(p.name) && (
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        disabled={stopping !== null}
                        onClick={() =>
                          setConfirmPackage(appProcessPackage(p.name))
                        }
                      >
                        {stopping === appProcessPackage(p.name)
                          ? t("devices.controls.forceStopping")
                          : t("devices.controls.forceStop")}
                      </Button>
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
