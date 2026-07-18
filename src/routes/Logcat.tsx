import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callCancelOperation,
  callListProcesses,
  callSaveLogcatExport,
  callSelectHostPath,
  callStreamLogcat,
  deviceTarget,
  newOperationId,
  type DeviceTarget,
  type LogcatQueryScope,
  type OperationEvent,
} from "../lib/tauri";
import {
  loadLogcatLibrary,
  saveLogcatQueries,
  type LogcatLibrary,
} from "../lib/logcatQueries";
import {
  resolveAuthorizedTarget,
  sameDeviceTarget,
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";
import {
  BUILTIN_QUERIES,
  DEFAULT_QUERY,
  LOGCAT_LEVELS,
  MAX_LOGCAT_HISTORY,
  matchesLine,
  newQueryId,
  parseLogcatLine,
  serializeQueries,
  parseImportedQueries,
  validateQuery,
  type LogLine,
  type WorkingQuery,
} from "./logcatQueries";

import {
  Badge,
  Button,
  Card,
  FieldInput,
  FieldSelect,
  FieldTextArea,
  PaneHeader,
  StatePanel,
  TransportBadge,
  TransportTrustNotice,
} from "./common";

const MAX_LOG_LINES = 2_000;

export default function LogcatRoute() {
  const { t } = useTranslation();
  const { devicesState, authorizedDevices } = useAuthorizedDevices();
  const [selectedTarget, setSelectedTarget] = useState<DeviceTarget | null>(
    null,
  );
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(selectedTarget);
  const [lines, setLines] = useState<LogLine[]>([]);
  const [tailing, setTailing] = useState(false);
  const [paused, setPaused] = useState(false);
  const [processNames, setProcessNames] = useState<Map<string, string>>(
    () => new Map(),
  );
  const [query, setQuery] = useState<WorkingQuery>({ ...DEFAULT_QUERY });
  const [library, setLibrary] = useState<LogcatLibrary>({
    global: [],
    device: [],
  });
  const [history, setHistory] = useState<WorkingQuery[]>([]);
  const [saveName, setSaveName] = useState("");
  const [saveScope, setSaveScope] = useState<LogcatQueryScope>("global");
  const [queryMessage, setQueryMessage] = useState<string | null>(null);
  const [renameId, setRenameId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [transferOpen, setTransferOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [streamError, setStreamError] = useState<string | null>(null);
  const [reconnecting, setReconnecting] = useState(false);
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [announcementRevision, setAnnouncementRevision] = useState(0);
  const operationRef = useRef<string | null>(null);
  const partialLineRef = useRef("");
  const generationRef = useRef(0);
  const outputRef = useRef<HTMLDivElement>(null);

  const deviceIdentity = selectedTarget?.serial ?? null;

  const queueLogAnnouncement = useCallback(() => {
    setAnnouncement("");
    setAnnouncementRevision((revision) => revision + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void loadLogcatLibrary(deviceIdentity)
      .then((loaded) => {
        if (!cancelled) setLibrary(loaded);
      })
      .catch(() => {
        if (!cancelled) setLibrary({ global: [], device: [] });
      });
    return () => {
      cancelled = true;
    };
  }, [deviceIdentity]);

  // Resolve PIDs to process names for package/process filters, refreshed on a
  // bounded cadence only while such a filter is active. The `ps` snapshot never
  // blocks the log stream — filtering just uses the most recent map it has.
  const needsProcessNames =
    query.packageFilter.length > 0 || query.processFilter.length > 0;
  useEffect(() => {
    if (!authorizedTarget || !needsProcessNames) {
      setProcessNames(new Map());
      return;
    }
    let cancelled = false;
    const refresh = () => {
      void callListProcesses(authorizedTarget)
        .then((processes) => {
          if (cancelled) return;
          setProcessNames(
            new Map(processes.map((p) => [String(p.pid), p.name])),
          );
        })
        .catch(() => {
          // A failed snapshot leaves the previous map in place; unmapped PIDs
          // are surfaced rather than dropped by matchesLine.
        });
    };
    refresh();
    const timer = window.setInterval(refresh, 4000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [authorizedTarget, needsProcessNames]);

  const startTailing = useCallback(() => {
    if (!authorizedTarget || operationRef.current) return;
    const operationId = newOperationId("logcat");
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    operationRef.current = operationId;
    partialLineRef.current = "";
    setTailing(true);
    setPaused(false);
    setStreamError(null);
    setReconnecting(false);
    setExportMessage(null);

    const onEvent = (event: OperationEvent) => {
      if (
        generationRef.current !== generation ||
        operationRef.current !== operationId
      )
        return;
      if (event.kind === "output" && event.stream === "stdout") {
        appendLogcatChunk(event.chunk ?? "", partialLineRef, setLines);
        queueLogAnnouncement();
        setReconnecting(false);
        return;
      }
      if (event.kind === "reconnecting") {
        flushPartialLogcatLine(partialLineRef, setLines);
        appendBoundedLines(setLines, [
          parseLogcatLine(
            `--- ${event.message ?? "Logcat disconnected; reconnecting"} (attempt ${event.attempt ?? "?"}) ---`,
          ),
        ]);
        queueLogAnnouncement();
        setReconnecting(true);
      }
    };

    void callStreamLogcat(authorizedTarget, {
      operationId,
      onEvent,
    })
      .catch((error) => {
        if (
          generationRef.current === generation &&
          operationRef.current === operationId
        ) {
          setStreamError(errorMessage(error));
        }
      })
      .finally(() => {
        if (
          generationRef.current === generation &&
          operationRef.current === operationId
        ) {
          flushPartialLogcatLine(partialLineRef, setLines);
          operationRef.current = null;
          setTailing(false);
          setReconnecting(false);
        }
      });
  }, [authorizedTarget, queueLogAnnouncement]);

  const stopTailing = useCallback(() => {
    const operationId = operationRef.current;
    operationRef.current = null;
    generationRef.current += 1;
    flushPartialLogcatLine(partialLineRef, setLines);
    setTailing(false);
    setReconnecting(false);
    if (operationId) void cancelWithRegistrationRetry(operationId);
  }, []);

  useEffect(() => {
    const next = resolveAuthorizedTarget(selectedTarget, authorizedDevices);
    if (sameDeviceTarget(selectedTarget, next)) return;
    stopTailing();
    setSelectedTarget(next);
    setLines([]);
    setStreamError(null);
  }, [authorizedDevices, selectedTarget, stopTailing]);

  useEffect(() => {
    return () => {
      const operationId = operationRef.current;
      operationRef.current = null;
      generationRef.current += 1;
      if (operationId) void cancelWithRegistrationRetry(operationId);
    };
  }, []);

  useEffect(() => {
    if (outputRef.current && !paused) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [lines, paused]);

  // Only anchor the memo to wall-clock time when an age filter is active;
  // otherwise Date.now() would invalidate it on every render and refilter the
  // whole buffer needlessly. When aging is on, a 1s bucket bounds recomputes.
  const ageActive = query.maxAgeSeconds !== null;
  const timeBucket = ageActive ? Math.floor(Date.now() / 1000) : 0;
  const filteredLines = useMemo(
    () =>
      lines.filter((line) =>
        matchesLine(line, query, timeBucket * 1000, processNames),
      ),
    [lines, query, processNames, timeBucket],
  );

  useEffect(() => {
    if (announcementRevision === 0) return;
    const timer = window.setTimeout(() => {
      setAnnouncement(
        t("logcat.updatedAnnouncement", { count: filteredLines.length }),
      );
    }, 750);
    return () => window.clearTimeout(timer);
  }, [announcementRevision, filteredLines.length, t]);

  const applyQuery = useCallback((preset: WorkingQuery) => {
    setQuery({ ...preset });
    setSaveName(preset.name);
    setHistory((previous) => {
      const next = [
        preset,
        ...previous.filter((item) => item.id !== preset.id),
      ];
      return next.slice(0, MAX_LOGCAT_HISTORY);
    });
  }, []);

  const persistScope = useCallback(
    async (scope: LogcatQueryScope, next: WorkingQuery[], toast: string) => {
      try {
        const updated = await saveLogcatQueries(scope, deviceIdentity, next);
        setLibrary(updated);
        setQueryMessage(toast);
      } catch (error) {
        setQueryMessage(
          t("logcat.queries.saveFailed", {
            message: errorMessage(error),
          }),
        );
      }
    },
    [deviceIdentity, t],
  );

  const scopeList = useCallback(
    (scope: LogcatQueryScope) =>
      scope === "device" ? library.device : library.global,
    [library],
  );

  const saveCurrent = useCallback(async () => {
    const candidate: WorkingQuery = {
      ...query,
      name: saveName.trim(),
      id:
        query.id && !query.id.startsWith("builtin-") ? query.id : newQueryId(),
    };
    const invalid = validateQuery(candidate);
    if (invalid) {
      setQueryMessage(
        t(`logcat.queries.invalid.${invalid.code}`, invalid.code),
      );
      return;
    }
    const current = scopeList(saveScope);
    const existingIndex = current.findIndex((item) => item.id === candidate.id);
    const next =
      existingIndex >= 0
        ? current.map((item, index) =>
            index === existingIndex ? candidate : item,
          )
        : [...current, candidate];
    setQuery(candidate);
    await persistScope(saveScope, next, t("logcat.queries.saved"));
  }, [persistScope, query, saveName, saveScope, scopeList, t]);

  const duplicateQuery = useCallback(
    async (scope: LogcatQueryScope, preset: WorkingQuery) => {
      const target: LogcatQueryScope = preset.id.startsWith("builtin-")
        ? "global"
        : scope;
      const copy: WorkingQuery = {
        ...preset,
        id: newQueryId(),
        name: t("logcat.queries.copyName", { name: preset.name }),
      };
      const next = [...scopeList(target), copy];
      await persistScope(target, next, t("logcat.queries.saved"));
    },
    [persistScope, scopeList, t],
  );

  const deleteQuery = useCallback(
    async (scope: LogcatQueryScope, id: string) => {
      const next = scopeList(scope).filter((item) => item.id !== id);
      await persistScope(scope, next, t("logcat.queries.deleted"));
    },
    [persistScope, scopeList, t],
  );

  const moveQuery = useCallback(
    async (scope: LogcatQueryScope, index: number, delta: number) => {
      const current = scopeList(scope);
      const target = index + delta;
      if (target < 0 || target >= current.length) return;
      const next = [...current];
      const [moved] = next.splice(index, 1);
      next.splice(target, 0, moved!);
      await persistScope(scope, next, t("logcat.queries.saved"));
    },
    [persistScope, scopeList, t],
  );

  const commitRename = useCallback(
    async (scope: LogcatQueryScope, id: string) => {
      const name = renameValue.trim();
      const next = scopeList(scope).map((item) =>
        item.id === id ? { ...item, name } : item,
      );
      const invalid = next.find((item) => validateQuery(item) !== null);
      setRenameId(null);
      if (invalid) {
        setQueryMessage(t("logcat.queries.invalid.name", "name"));
        return;
      }
      await persistScope(scope, next, t("logcat.queries.saved"));
    },
    [persistScope, renameValue, scopeList, t],
  );

  const exportQueries = useCallback(() => {
    const combined = [...library.global, ...library.device];
    setImportText(serializeQueries(combined));
    setTransferOpen(true);
    setQueryMessage(t("logcat.queries.exported", { count: combined.length }));
  }, [library, t]);

  const importQueries = useCallback(async () => {
    const parsed = parseImportedQueries(importText);
    if (!parsed.ok) {
      setQueryMessage(
        t(`logcat.queries.importError.${parsed.error}`, parsed.error),
      );
      return;
    }
    const next = [...scopeList("global"), ...parsed.queries].slice(0, 64);
    await persistScope("global", next, t("logcat.queries.imported"));
    setTransferOpen(false);
  }, [importText, persistScope, scopeList, t]);

  const exportLog = useCallback(async () => {
    if (lines.length === 0) return;
    setExportMessage(null);
    try {
      const pathGrant = await callSelectHostPath(
        "logcat_save",
        `droidsmith-logcat-${Date.now()}.log`,
      );
      if (!pathGrant) return;
      const savedPath = await callSaveLogcatExport(
        pathGrant.id,
        `${lines.map((line) => line.raw).join("\n")}\n`,
      );
      setExportMessage(t("logcat.exportSaved", { path: savedPath }));
    } catch (error) {
      setExportMessage(
        t("logcat.exportFailed", {
          message: errorMessage(error),
        }),
      );
    }
  }, [lines, t]);

  return (
    <>
      <PaneHeader
        title={t("logcat.title")}
        milestone="R-051"
        description={t("logcat.description")}
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {selectedTarget && (
              <>
                <Badge tone="info">
                  <code className="font-mono">{selectedTarget.serial}</code>
                </Badge>
                <TransportBadge kind={selectedTarget.transport_kind} />
              </>
            )}
            {tailing && !streamError && !reconnecting && (
              <Badge tone="success">{t("logcat.tailing")}</Badge>
            )}
            {tailing && reconnecting && (
              <Badge tone="warning">{t("logcat.reconnecting")}</Badge>
            )}
            {streamError && (
              <Badge tone="danger">{t("logcat.streamFailed")}</Badge>
            )}
            <Badge tone="neutral">
              {t("logcat.lineCount", { count: filteredLines.length })}
            </Badge>
          </div>
        }
      />

      <section className="mt-6 max-w-7xl space-y-4">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title={t("common.desktopRequired")} tone="info">
            <p>{t("logcat.desktopRequiredBody")}</p>
          </StatePanel>
        )}

        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title={t("common.noAuthorized")} tone="warning">
            <p>{t("logcat.noAuthorizedBody")}</p>
          </StatePanel>
        )}

        {authorizedDevices.length > 1 && (
          <Card className="p-4">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("common.targetDevice")}
            </h3>
            <div className="mt-3 flex flex-wrap gap-2">
              {authorizedDevices.map((d) => (
                <Button
                  key={`${d.transport_id ?? d.serial}:${d.connection_generation}`}
                  type="button"
                  variant={
                    d.transport_id === selectedTarget?.transport_id &&
                    d.connection_generation ===
                      selectedTarget?.connection_generation
                      ? "primary"
                      : "secondary"
                  }
                  size="sm"
                  onClick={() => {
                    setSelectedTarget(deviceTarget(d));
                    stopTailing();
                    setLines([]);
                  }}
                >
                  {d.model ?? d.serial}
                </Button>
              ))}
            </div>
          </Card>
        )}

        <TransportTrustNotice
          target={selectedTarget}
          accepted={transportOverrideAccepted}
          onAcceptedChange={setTransportOverrideAccepted}
        />

        {selectedTarget && (
          <>
            <div className="flex flex-wrap items-end gap-3">
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.tagFilter")}
                </span>
                <FieldInput
                  type="text"
                  value={query.tagFilter}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, tagFilter: e.target.value }))
                  }
                  placeholder="ActivityManager"
                  className="w-40 font-mono"
                />
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.minLevel")}
                </span>
                <FieldSelect
                  value={query.minLevel}
                  onChange={(e) =>
                    setQuery((q) => ({
                      ...q,
                      minLevel: e.target.value as WorkingQuery["minLevel"],
                    }))
                  }
                  className="h-9"
                >
                  {LOGCAT_LEVELS.map((l) => (
                    <option key={l} value={l}>
                      {t(levelNameKey(l))}
                    </option>
                  ))}
                </FieldSelect>
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.queries.message")}
                </span>
                <FieldInput
                  type="text"
                  value={query.messageFilter}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, messageFilter: e.target.value }))
                  }
                  placeholder="grep"
                  aria-label={t("logcat.textSearchLabel")}
                  className="w-40 font-mono"
                />
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.queries.pid")}
                </span>
                <FieldInput
                  type="text"
                  inputMode="numeric"
                  value={query.pidFilter}
                  onChange={(e) =>
                    setQuery((q) => ({
                      ...q,
                      pidFilter: e.target.value.replace(/[^0-9]/gu, ""),
                    }))
                  }
                  placeholder="1234"
                  className="w-24 font-mono"
                />
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.queries.package")}
                </span>
                <FieldInput
                  type="text"
                  value={query.packageFilter}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, packageFilter: e.target.value }))
                  }
                  placeholder="com.example.app"
                  className="w-44 font-mono"
                />
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.queries.process")}
                </span>
                <FieldInput
                  type="text"
                  value={query.processFilter}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, processFilter: e.target.value }))
                  }
                  placeholder="com.example.app:svc"
                  className="w-44 font-mono"
                />
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.queries.maxAge")}
                </span>
                <FieldInput
                  type="text"
                  inputMode="numeric"
                  value={
                    query.maxAgeSeconds === null
                      ? ""
                      : String(query.maxAgeSeconds)
                  }
                  onChange={(e) => {
                    const digits = e.target.value.replace(/[^0-9]/gu, "");
                    setQuery((q) => ({
                      ...q,
                      maxAgeSeconds: digits ? Number(digits) : null,
                    }));
                  }}
                  placeholder="300"
                  className="w-24 font-mono"
                />
              </label>
            </div>

            <div className="flex flex-wrap items-center gap-4 text-xs text-anvil-300">
              <label className="inline-flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={query.useRegex}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, useRegex: e.target.checked }))
                  }
                />
                {t("logcat.queries.useRegex")}
              </label>
              <label className="inline-flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={query.negateTag}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, negateTag: e.target.checked }))
                  }
                />
                {t("logcat.queries.negateTag")}
              </label>
              <label className="inline-flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={query.negateMessage}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, negateMessage: e.target.checked }))
                  }
                />
                {t("logcat.queries.negateMessage")}
              </label>
              <label className="inline-flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={query.negatePid}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, negatePid: e.target.checked }))
                  }
                />
                {t("logcat.queries.negatePid")}
              </label>
              <label className="inline-flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={query.negatePackage}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, negatePackage: e.target.checked }))
                  }
                />
                {t("logcat.queries.negatePackage")}
              </label>
              <label className="inline-flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={query.negateProcess}
                  onChange={(e) =>
                    setQuery((q) => ({ ...q, negateProcess: e.target.checked }))
                  }
                />
                {t("logcat.queries.negateProcess")}
              </label>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              {!tailing ? (
                <Button
                  type="button"
                  variant="primary"
                  size="sm"
                  onClick={startTailing}
                >
                  {t("logcat.startTail")}
                </Button>
              ) : (
                <>
                  <Button
                    type="button"
                    variant="danger"
                    size="sm"
                    onClick={stopTailing}
                  >
                    {t("logcat.stop")}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => setPaused((p) => !p)}
                  >
                    {paused
                      ? t("logcat.resumeScroll")
                      : t("logcat.pauseScroll")}
                  </Button>
                </>
              )}
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => {
                  partialLineRef.current = "";
                  setLines([]);
                  setAnnouncement(t("logcat.clearedAnnouncement"));
                }}
              >
                {t("logcat.clear")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => void exportLog()}
                disabled={lines.length === 0}
              >
                {t("logcat.export")}
              </Button>
            </div>

            <QueryManager
              library={library}
              history={history}
              saveName={saveName}
              saveScope={saveScope}
              renameId={renameId}
              renameValue={renameValue}
              transferOpen={transferOpen}
              importText={importText}
              message={queryMessage}
              onSaveNameChange={setSaveName}
              onSaveScopeChange={setSaveScope}
              onSaveCurrent={() => void saveCurrent()}
              onApply={applyQuery}
              onDuplicate={(scope, preset) =>
                void duplicateQuery(scope, preset)
              }
              onDelete={(scope, id) => void deleteQuery(scope, id)}
              onMove={(scope, index, delta) =>
                void moveQuery(scope, index, delta)
              }
              onStartRename={(id, name) => {
                setRenameId(id);
                setRenameValue(name);
              }}
              onRenameValueChange={setRenameValue}
              onCommitRename={(scope, id) => void commitRename(scope, id)}
              onCancelRename={() => setRenameId(null)}
              onToggleTransfer={() =>
                transferOpen ? setTransferOpen(false) : exportQueries()
              }
              onImportTextChange={setImportText}
              onImport={() => void importQueries()}
            />

            {streamError && (
              <StatePanel title={t("logcat.streamFailed")} tone="danger">
                <p>{streamError}</p>
              </StatePanel>
            )}
            {exportMessage && (
              <p role="status" className="text-xs text-anvil-300">
                {exportMessage}
              </p>
            )}

            <Card className="overflow-hidden p-0">
              <div
                ref={outputRef}
                className="h-[32rem] overflow-y-auto bg-[#0c0d12] p-3 font-mono text-[11px] leading-5"
                role="log"
                aria-live="off"
                aria-label={t("logcat.outputLabel")}
              >
                {filteredLines.length === 0 && !tailing && (
                  <p className="text-anvil-600">{t("logcat.startHint")}</p>
                )}
                {filteredLines.length === 0 && tailing && (
                  <p className="text-anvil-600 animate-pulse">
                    {t("logcat.waiting")}
                  </p>
                )}
                {filteredLines.map((line, i) => (
                  <div key={i} className={logLineColor(line.level)}>
                    {line.raw}
                  </div>
                ))}
              </div>
              <p
                className="sr-only"
                role="status"
                aria-live="polite"
                aria-atomic="true"
              >
                {announcement}
              </p>
            </Card>
          </>
        )}
      </section>
    </>
  );
}

type QueryManagerProps = {
  library: LogcatLibrary;
  history: WorkingQuery[];
  saveName: string;
  saveScope: LogcatQueryScope;
  renameId: string | null;
  renameValue: string;
  transferOpen: boolean;
  importText: string;
  message: string | null;
  onSaveNameChange: (value: string) => void;
  onSaveScopeChange: (scope: LogcatQueryScope) => void;
  onSaveCurrent: () => void;
  onApply: (preset: WorkingQuery) => void;
  onDuplicate: (scope: LogcatQueryScope, preset: WorkingQuery) => void;
  onDelete: (scope: LogcatQueryScope, id: string) => void;
  onMove: (scope: LogcatQueryScope, index: number, delta: number) => void;
  onStartRename: (id: string, name: string) => void;
  onRenameValueChange: (value: string) => void;
  onCommitRename: (scope: LogcatQueryScope, id: string) => void;
  onCancelRename: () => void;
  onToggleTransfer: () => void;
  onImportTextChange: (value: string) => void;
  onImport: () => void;
};

function QueryManager(props: QueryManagerProps) {
  const { t } = useTranslation();
  return (
    <Card className="p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("logcat.queries.title")}
          </h3>
          <p className="mt-1 text-xs leading-5 text-anvil-400">
            {t("logcat.queries.description")}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={props.onToggleTransfer}
        >
          {props.transferOpen
            ? t("logcat.queries.close")
            : t("logcat.queries.transfer")}
        </Button>
      </div>

      <div className="mt-4 flex flex-wrap items-end gap-2">
        <label className="grid gap-1.5">
          <span className="text-xs font-medium text-anvil-400">
            {t("logcat.queries.name")}
          </span>
          <FieldInput
            type="text"
            value={props.saveName}
            onChange={(e) => props.onSaveNameChange(e.target.value)}
            placeholder={t("logcat.queries.namePlaceholder")}
            className="w-56"
          />
        </label>
        <label className="grid gap-1.5">
          <span className="text-xs font-medium text-anvil-400">
            {t("logcat.queries.scope")}
          </span>
          <FieldSelect
            value={props.saveScope}
            onChange={(e) =>
              props.onSaveScopeChange(e.target.value as LogcatQueryScope)
            }
            className="h-9"
          >
            <option value="global">{t("logcat.queries.scopeGlobal")}</option>
            <option value="device">{t("logcat.queries.scopeDevice")}</option>
          </FieldSelect>
        </label>
        <Button type="button" size="sm" onClick={props.onSaveCurrent}>
          {t("logcat.queries.save")}
        </Button>
      </div>

      {props.transferOpen && (
        <div className="mt-4 rounded-md border border-white/10 bg-anvil-900/60 p-3">
          <FieldTextArea
            value={props.importText}
            onChange={(e) => props.onImportTextChange(e.target.value)}
            spellCheck={false}
            aria-label={t("logcat.queries.transfer")}
            className="h-40 bg-[#0c0d12] p-2 font-mono text-[11px] text-anvil-100"
            placeholder={t("logcat.queries.importPlaceholder")}
          />
          <div className="mt-2 flex justify-end">
            <Button type="button" size="sm" onClick={props.onImport}>
              {t("logcat.queries.import")}
            </Button>
          </div>
        </div>
      )}

      {props.message && (
        <p role="status" className="mt-3 text-xs text-circuit-100">
          {props.message}
        </p>
      )}

      {props.history.length > 0 && (
        <div className="mt-4">
          <p className="text-xs font-medium text-anvil-400">
            {t("logcat.queries.history")}
          </p>
          <div className="mt-2 flex flex-wrap gap-2">
            {props.history.map((preset) => (
              <button
                key={`history-${preset.id}`}
                type="button"
                onClick={() => props.onApply(preset)}
                className="rounded-md border border-white/10 bg-white/[0.04] px-2 py-1 text-xs text-anvil-200 hover:border-white/20"
              >
                {preset.name || t("logcat.queries.unnamed")}
              </button>
            ))}
          </div>
        </div>
      )}

      <QueryList
        heading={t("logcat.queries.builtinHeading")}
        scope="global"
        presets={BUILTIN_QUERIES as WorkingQuery[]}
        builtin
        {...props}
      />
      <QueryList
        heading={t("logcat.queries.globalHeading")}
        scope="global"
        presets={props.library.global}
        {...props}
      />
      <QueryList
        heading={t("logcat.queries.deviceHeading")}
        scope="device"
        presets={props.library.device}
        {...props}
      />
    </Card>
  );
}

function QueryList(
  props: QueryManagerProps & {
    heading: string;
    scope: LogcatQueryScope;
    presets: WorkingQuery[];
    builtin?: boolean;
  },
) {
  const { t } = useTranslation();
  return (
    <div className="mt-4">
      <p className="text-xs font-medium text-anvil-400">{props.heading}</p>
      {props.presets.length === 0 ? (
        <p className="mt-2 text-xs text-anvil-600">
          {t("logcat.queries.empty")}
        </p>
      ) : (
        <ul className="mt-2 space-y-1.5">
          {props.presets.map((preset, index) => (
            <li
              key={preset.id}
              className="flex flex-wrap items-center gap-2 rounded-md border border-white/10 bg-white/[0.03] px-3 py-2"
            >
              {props.renameId === preset.id ? (
                <>
                  <FieldInput
                    type="text"
                    value={props.renameValue}
                    onChange={(e) => props.onRenameValueChange(e.target.value)}
                    className="w-56"
                    aria-label={t("logcat.queries.rename")}
                  />
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => props.onCommitRename(props.scope, preset.id)}
                  >
                    {t("logcat.queries.save")}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    onClick={props.onCancelRename}
                  >
                    {t("common.cancel")}
                  </Button>
                </>
              ) : (
                <>
                  <span className="min-w-0 flex-1 truncate text-sm text-anvil-100">
                    {preset.name || t("logcat.queries.unnamed")}
                  </span>
                  {props.builtin && (
                    <Badge tone="neutral">
                      {t("logcat.queries.builtinBadge")}
                    </Badge>
                  )}
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => props.onApply(preset)}
                  >
                    {t("logcat.queries.apply")}
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    onClick={() => props.onDuplicate(props.scope, preset)}
                  >
                    {t("logcat.queries.duplicate")}
                  </Button>
                  {!props.builtin && (
                    <>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={() =>
                          props.onStartRename(preset.id, preset.name)
                        }
                      >
                        {t("logcat.queries.rename")}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        disabled={index === 0}
                        aria-label={t("logcat.queries.moveUp")}
                        onClick={() => props.onMove(props.scope, index, -1)}
                      >
                        ↑
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        disabled={index === props.presets.length - 1}
                        aria-label={t("logcat.queries.moveDown")}
                        onClick={() => props.onMove(props.scope, index, 1)}
                      >
                        ↓
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="danger"
                        onClick={() => props.onDelete(props.scope, preset.id)}
                      >
                        {t("logcat.queries.delete")}
                      </Button>
                    </>
                  )}
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

type PartialLineRef = { current: string };
type LineSetter = (update: (previous: LogLine[]) => LogLine[]) => void;

function appendLogcatChunk(
  chunk: string,
  partialRef: PartialLineRef,
  setLines: LineSetter,
) {
  const combined = `${partialRef.current}${chunk}`.replace(/\r\n?/g, "\n");
  const rows = combined.split("\n");
  partialRef.current = rows.pop() ?? "";
  const parsed = rows
    .filter((row) => row.trim())
    .map((row) => parseLogcatLine(row));
  appendBoundedLines(setLines, parsed);
}

function flushPartialLogcatLine(
  partialRef: PartialLineRef,
  setLines: LineSetter,
) {
  const pending = partialRef.current.trimEnd();
  partialRef.current = "";
  if (pending) appendBoundedLines(setLines, [parseLogcatLine(pending)]);
}

function appendBoundedLines(setLines: LineSetter, incoming: LogLine[]) {
  if (incoming.length === 0) return;
  setLines((previous) => [...previous, ...incoming].slice(-MAX_LOG_LINES));
}

async function cancelWithRegistrationRetry(operationId: string) {
  await callCancelOperation(operationId);
}

function logLineColor(level: string): string {
  switch (level) {
    case "V":
      return "text-anvil-600";
    case "D":
      return "text-anvil-400";
    case "I":
      return "text-circuit-200/80";
    case "W":
      return "text-amber-200/80";
    case "E":
    case "F":
      return "text-red-300/80";
    default:
      return "text-anvil-500";
  }
}

function levelNameKey(l: string): string {
  switch (l) {
    case "V":
      return "logcat.verbose";
    case "D":
      return "logcat.debug";
    case "I":
      return "logcat.info";
    case "W":
      return "logcat.warning";
    case "E":
      return "logcat.error";
    case "F":
      return "logcat.fatal";
    default:
      return l;
  }
}
