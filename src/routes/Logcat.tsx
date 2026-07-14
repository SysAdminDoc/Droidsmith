import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callListDevices,
  callShellRun,
  deviceTarget,
  inTauri,
  type DeviceTarget,
  type ListDevicesResult,
} from "../lib/tauri";

import {
  Badge,
  Button,
  Card,
  FieldInput,
  PaneHeader,
  StatePanel,
} from "./common";

type DevicesState =
  | { kind: "loading" }
  | { kind: "no_tauri" }
  | { kind: "ok"; value: ListDevicesResult }
  | { kind: "error"; message: string };

type LogLine = {
  raw: string;
  level: string;
  tag: string;
  pid: string;
  message: string;
};

const LEVELS = ["V", "D", "I", "W", "E", "F"] as const;

export default function LogcatRoute() {
  const { t } = useTranslation();
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [selectedTarget, setSelectedTarget] = useState<DeviceTarget | null>(
    null,
  );
  const [lines, setLines] = useState<LogLine[]>([]);
  const [tailing, setTailing] = useState(false);
  const [paused, setPaused] = useState(false);
  const [tagFilter, setTagFilter] = useState("");
  const [levelFilter, setLevelFilter] = useState<string>("V");
  const [textFilter, setTextFilter] = useState("");
  const [fetchError, setFetchError] = useState(false);
  const tailRef = useRef(false);
  const targetRef = useRef(selectedTarget);
  const tagFilterRef = useRef(tagFilter);
  const timerRef = useRef<number | null>(null);
  const outputRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    targetRef.current = selectedTarget;
  }, [selectedTarget]);
  useEffect(() => {
    tagFilterRef.current = tagFilter;
  }, [tagFilter]);

  const loadDevices = useCallback(async () => {
    if (!inTauri()) {
      setDevicesState({ kind: "no_tauri" });
      return;
    }
    setDevicesState({ kind: "loading" });
    try {
      const value = await callListDevices();
      setDevicesState({ kind: "ok", value });
      const authorized = value.devices.filter(
        (d) => typeof d.state === "string" && d.state === "device",
      );
      if (authorized.length === 1) {
        setSelectedTarget((prev) => prev ?? deviceTarget(authorized[0]!));
      }
    } catch (e) {
      setDevicesState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  const fetchLogcat = useCallback(async () => {
    const target = targetRef.current;
    if (!target || !tailRef.current) return;

    try {
      const argv = ["logcat", "-v", "brief", "-d", "-t", "200"];
      const currentTag = tagFilterRef.current.trim();
      if (currentTag) {
        argv.push("-s", currentTag);
      }
      const output = await callShellRun(target, argv);
      const parsed = output
        .split("\n")
        .filter((l) => l.trim())
        .map(parseLogcatLine);
      setLines(parsed);
      setFetchError(false);
    } catch {
      setFetchError(true);
    }

    if (tailRef.current) {
      timerRef.current = window.setTimeout(() => void fetchLogcat(), 2000);
    }
  }, []);

  const startTailing = useCallback(() => {
    tailRef.current = true;
    setTailing(true);
    setPaused(false);
    setFetchError(false);
    void fetchLogcat();
  }, [fetchLogcat]);

  const stopTailing = useCallback(() => {
    tailRef.current = false;
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setTailing(false);
    setFetchError(false);
  }, []);

  useEffect(() => {
    return () => {
      tailRef.current = false;
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
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
    if (textFilter && !l.raw.toLowerCase().includes(textFilter.toLowerCase()))
      return false;
    return true;
  });

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];

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
            {tailing && !fetchError && (
              <Badge tone="success">{t("logcat.tailing")}</Badge>
            )}
            {tailing && fetchError && (
              <Badge tone="danger">{t("logcat.fetchFailed")}</Badge>
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
                  onClick={() => setLines([])}
                >
                  {t("logcat.clear")}
                </Button>
              </div>
            </div>

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
