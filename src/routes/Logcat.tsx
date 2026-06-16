import { useCallback, useEffect, useRef, useState } from "react";

import {
  callListDevices,
  callShellRun,
  inTauri,
  type ListDevicesResult,
} from "../lib/tauri";

import { Badge, Button, Card, PaneHeader, StatePanel } from "./common";

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
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [lines, setLines] = useState<LogLine[]>([]);
  const [tailing, setTailing] = useState(false);
  const [paused, setPaused] = useState(false);
  const [tagFilter, setTagFilter] = useState("");
  const [levelFilter, setLevelFilter] = useState<string>("V");
  const [textFilter, setTextFilter] = useState("");
  const tailRef = useRef(false);
  const outputRef = useRef<HTMLDivElement>(null);

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
      if (authorized.length === 1 && !selectedSerial) {
        setSelectedSerial(authorized[0]!.serial);
      }
    } catch (e) {
      setDevicesState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedSerial]);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  const fetchLogcat = useCallback(async () => {
    if (!selectedSerial || !tailRef.current) return;

    try {
      const argv = ["logcat", "-v", "brief", "-d", "-t", "200"];
      if (tagFilter.trim()) {
        argv.push("-s", tagFilter.trim());
      }
      const output = await callShellRun(selectedSerial, argv);
      const parsed = output
        .split("\n")
        .filter((l) => l.trim())
        .map(parseLogcatLine);
      setLines(parsed);
    } catch {
      // Silently handle — logcat may fail during device transitions
    }

    if (tailRef.current) {
      setTimeout(() => void fetchLogcat(), 2000);
    }
  }, [selectedSerial, tagFilter]);

  const startTailing = useCallback(() => {
    tailRef.current = true;
    setTailing(true);
    setPaused(false);
    void fetchLogcat();
  }, [fetchLogcat]);

  const stopTailing = useCallback(() => {
    tailRef.current = false;
    setTailing(false);
  }, []);

  useEffect(() => {
    return () => {
      tailRef.current = false;
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
        title="Logcat"
        milestone="R-051"
        description="Tail device logs with level and text filters. Polling-based capture refreshes every 2 seconds."
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {selectedSerial && (
              <Badge tone="info">
                <code className="font-mono">{selectedSerial}</code>
              </Badge>
            )}
            {tailing && <Badge tone="success">Tailing</Badge>}
            <Badge tone="neutral">{filteredLines.length} lines</Badge>
          </div>
        }
      />

      <section className="mt-6 max-w-6xl space-y-4">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title="Desktop shell required" tone="info">
            <p>Logcat runs inside the Tauri runtime.</p>
          </StatePanel>
        )}

        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title="No authorized devices" tone="warning">
            <p>Connect and authorize a device to view logs.</p>
          </StatePanel>
        )}

        {authorizedDevices.length > 1 && (
          <Card className="p-4">
            <h3 className="text-sm font-semibold text-anvil-50">Target device</h3>
            <div className="mt-3 flex flex-wrap gap-2">
              {authorizedDevices.map((d) => (
                <Button
                  key={d.serial}
                  type="button"
                  variant={d.serial === selectedSerial ? "primary" : "secondary"}
                  size="sm"
                  onClick={() => {
                    setSelectedSerial(d.serial);
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

        {selectedSerial && (
          <>
            <div className="flex flex-wrap items-end gap-3">
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">Tag filter</span>
                <input
                  type="text"
                  value={tagFilter}
                  onChange={(e) => setTagFilter(e.target.value)}
                  placeholder="ActivityManager"
                  className="h-9 w-44 rounded-md border border-white/10 bg-white/[0.06] px-3 font-mono text-sm text-anvil-50 outline-none transition placeholder:text-anvil-600 focus:border-circuit-300/50 focus:ring-2 focus:ring-circuit-300/20"
                />
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">Min level</span>
                <select
                  value={levelFilter}
                  onChange={(e) => setLevelFilter(e.target.value)}
                  className="h-9 rounded-md border border-white/10 bg-white/[0.06] px-3 text-sm text-anvil-50 outline-none focus:border-circuit-300/50 focus:ring-2 focus:ring-circuit-300/20"
                >
                  {LEVELS.map((l) => (
                    <option key={l} value={l}>
                      {levelName(l)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="grid gap-1.5">
                <span className="text-xs font-medium text-anvil-400">Text search</span>
                <input
                  type="text"
                  value={textFilter}
                  onChange={(e) => setTextFilter(e.target.value)}
                  placeholder="grep..."
                  aria-label="Filter log lines by text"
                  className="h-9 w-44 rounded-md border border-white/10 bg-white/[0.06] px-3 font-mono text-sm text-anvil-50 outline-none transition placeholder:text-anvil-600 focus:border-circuit-300/50 focus:ring-2 focus:ring-circuit-300/20"
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
                    Start tail
                  </Button>
                ) : (
                  <>
                    <Button
                      type="button"
                      variant="danger"
                      size="sm"
                      onClick={stopTailing}
                    >
                      Stop
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => setPaused((p) => !p)}
                    >
                      {paused ? "Resume scroll" : "Pause scroll"}
                    </Button>
                  </>
                )}
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setLines([])}
                >
                  Clear
                </Button>
              </div>
            </div>

            <Card className="overflow-hidden p-0">
              <div
                ref={outputRef}
                className="h-[32rem] overflow-y-auto bg-[#0c0d12] p-3 font-mono text-[11px] leading-5"
                role="log"
                aria-live="polite"
                aria-label="Logcat output"
              >
                {filteredLines.length === 0 && !tailing && (
                  <p className="text-anvil-600">
                    Press "Start tail" to begin capturing logs.
                  </p>
                )}
                {filteredLines.length === 0 && tailing && (
                  <p className="text-anvil-600 animate-pulse">
                    Waiting for log output...
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

function levelName(l: string): string {
  switch (l) {
    case "V":
      return "Verbose";
    case "D":
      return "Debug";
    case "I":
      return "Info";
    case "W":
      return "Warning";
    case "E":
      return "Error";
    case "F":
      return "Fatal";
    default:
      return l;
  }
}
