import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { save } from "@tauri-apps/plugin-dialog";

import {
  callCancelOperation,
  callSaveLogcatExport,
  callStreamLogcat,
  deviceTarget,
  newOperationId,
  type DeviceTarget,
  type OperationEvent,
} from "../lib/tauri";
import {
  resolveAuthorizedTarget,
  sameDeviceTarget,
  useAuthorizedDevices,
} from "../lib/useAuthorizedDevices";

import {
  Badge,
  Button,
  Card,
  FieldInput,
  PaneHeader,
  StatePanel,
} from "./common";

type LogLine = {
  raw: string;
  level: string;
  tag: string;
  pid: string;
  message: string;
};

const LEVELS = ["V", "D", "I", "W", "E", "F"] as const;
const MAX_LOG_LINES = 2_000;

export default function LogcatRoute() {
  const { t } = useTranslation();
  const { devicesState, authorizedDevices } = useAuthorizedDevices();
  const [selectedTarget, setSelectedTarget] = useState<DeviceTarget | null>(
    null,
  );
  const [lines, setLines] = useState<LogLine[]>([]);
  const [tailing, setTailing] = useState(false);
  const [paused, setPaused] = useState(false);
  const [tagFilter, setTagFilter] = useState("");
  const [levelFilter, setLevelFilter] = useState<string>("V");
  const [textFilter, setTextFilter] = useState("");
  const [streamError, setStreamError] = useState<string | null>(null);
  const [reconnecting, setReconnecting] = useState(false);
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const operationRef = useRef<string | null>(null);
  const partialLineRef = useRef("");
  const generationRef = useRef(0);
  const outputRef = useRef<HTMLDivElement>(null);

  const startTailing = useCallback(() => {
    if (!selectedTarget || operationRef.current) return;
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
        setReconnecting(true);
      }
    };

    void callStreamLogcat(selectedTarget, {
      operationId,
      onEvent,
    })
      .catch((error) => {
        if (
          generationRef.current === generation &&
          operationRef.current === operationId
        ) {
          setStreamError(
            error instanceof Error ? error.message : String(error),
          );
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
  }, [selectedTarget]);

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

  const levelIndex = LEVELS.indexOf(levelFilter as (typeof LEVELS)[number]);
  const filteredLines = lines.filter((l) => {
    const li = LEVELS.indexOf(l.level as (typeof LEVELS)[number]);
    if (li >= 0 && levelIndex >= 0 && li < levelIndex) return false;
    if (
      tagFilter &&
      l.tag &&
      !l.tag.toLowerCase().includes(tagFilter.trim().toLowerCase())
    )
      return false;
    if (textFilter && !l.raw.toLowerCase().includes(textFilter.toLowerCase()))
      return false;
    return true;
  });

  const exportLog = useCallback(async () => {
    if (lines.length === 0) return;
    setExportMessage(null);
    try {
      const localPath = await save({
        title: t("logcat.exportTitle"),
        defaultPath: `droidsmith-logcat-${Date.now()}.log`,
        filters: [{ name: "Logcat", extensions: ["log", "txt"] }],
      });
      if (!localPath) return;
      const savedPath = await callSaveLogcatExport(
        localPath,
        `${lines.map((line) => line.raw).join("\n")}\n`,
      );
      setExportMessage(t("logcat.exportSaved", { path: savedPath }));
    } catch (error) {
      setExportMessage(
        t("logcat.exportFailed", {
          message: error instanceof Error ? error.message : String(error),
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
              <Badge tone="info">
                <code className="font-mono">{selectedTarget.serial}</code>
              </Badge>
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

      <section className="mt-6 max-w-6xl space-y-4">
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

        {selectedTarget && (
          <>
            <div className="flex flex-wrap items-end gap-3">
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.tagFilter")}
                </span>
                <FieldInput
                  type="text"
                  value={tagFilter}
                  onChange={(e) => setTagFilter(e.target.value)}
                  placeholder="ActivityManager"
                  className="w-44 font-mono"
                />
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.minLevel")}
                </span>
                <select
                  value={levelFilter}
                  onChange={(e) => setLevelFilter(e.target.value)}
                  className="h-9 rounded-md border border-white/10 bg-white/[0.06] px-3 text-sm text-anvil-50 outline-none transition hover:border-white/20 focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
                >
                  {LEVELS.map((l) => (
                    <option key={l} value={l}>
                      {t(levelNameKey(l))}
                    </option>
                  ))}
                </select>
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  {t("logcat.textSearch")}
                </span>
                <FieldInput
                  type="text"
                  value={textFilter}
                  onChange={(e) => setTextFilter(e.target.value)}
                  placeholder="grep"
                  aria-label={t("logcat.textSearchLabel")}
                  className="w-44 font-mono"
                />
              </label>
              <div className="flex gap-2">
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
            </div>

            {streamError && (
              <StatePanel title={t("logcat.streamFailed")} tone="danger">
                <p>{streamError}</p>
              </StatePanel>
            )}
            {exportMessage && (
              <p className="text-xs text-anvil-300">{exportMessage}</p>
            )}

            <Card className="overflow-hidden p-0">
              <div
                ref={outputRef}
                className="h-[32rem] overflow-y-auto bg-[#0c0d12] p-3 font-mono text-[11px] leading-5"
                role="log"
                aria-live="polite"
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
            </Card>
          </>
        )}
      </section>
    </>
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
  const parsed = rows.filter((row) => row.trim()).map(parseLogcatLine);
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

function parseLogcatLine(raw: string): LogLine {
  // Brief format: L/Tag(PID): message
  const match = raw.match(/^([VDIWEF])\/([^(]+)\(\s*(\d+)\):\s*(.*)$/);
  if (match) {
    return {
      raw,
      level: match[1]!,
      tag: match[2]!.trim(),
      pid: match[3]!,
      message: match[4]!,
    };
  }
  return { raw, level: "", tag: "", pid: "", message: raw };
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
