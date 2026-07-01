import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

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

type HistoryEntry = {
  command: string;
  output: string;
  error: boolean;
  timestamp: number;
  id: number;
};

let nextEntryId = 1;

export default function ConsoleRoute() {
  const { t } = useTranslation();
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [command, setCommand] = useState("");
  const [running, setRunning] = useState(false);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const outputRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

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
        setSelectedSerial((prev) => prev ?? authorized[0]!.serial);
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

  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [history]);

  const runCommand = useCallback(async () => {
    const trimmed = command.trim();
    if (!trimmed || !selectedSerial || running) return;

    setRunning(true);
    setCommand("");
    setHistoryIndex(-1);

    const argv = trimmed.split(/\s+/);

    try {
      const output = await callShellRun(selectedSerial, argv);
      setHistory((prev) => [
        ...prev,
        { command: trimmed, output, error: false, timestamp: Date.now(), id: nextEntryId++ },
      ]);
    } catch (e) {
      setHistory((prev) => [
        ...prev,
        {
          command: trimmed,
          output: e instanceof Error ? e.message : String(e),
          error: true,
          timestamp: Date.now(),
          id: nextEntryId++,
        },
      ]);
    } finally {
      setRunning(false);
      inputRef.current?.focus();
    }
  }, [command, selectedSerial, running]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        void runCommand();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        const cmds = history.filter((h) => !h.error);
        if (cmds.length === 0) return;
        const next =
          historyIndex < cmds.length - 1 ? historyIndex + 1 : historyIndex;
        setHistoryIndex(next);
        setCommand(cmds[cmds.length - 1 - next]?.command ?? "");
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        if (historyIndex <= 0) {
          setHistoryIndex(-1);
          setCommand("");
        } else {
          const next = historyIndex - 1;
          const cmds = history.filter((h) => !h.error);
          setHistoryIndex(next);
          setCommand(cmds[cmds.length - 1 - next]?.command ?? "");
        }
      }
    },
    [history, historyIndex, runCommand],
  );

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];

  return (
    <>
      <PaneHeader
        title={t("console.title")}
        milestone="R-050"
        description={t("console.description")}
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {selectedSerial && (
              <Badge tone="info">
                <code className="font-mono">{selectedSerial}</code>
              </Badge>
            )}
            <Badge tone="neutral">
              {t("console.commandCount", { count: history.length })}
            </Badge>
          </div>
        }
        actions={
          history.length > 0 ? (
            <Button
              type="button"
              onClick={() => setHistory([])}
              variant="ghost"
              size="sm"
            >
              {t("console.clearHistory")}
            </Button>
          ) : undefined
        }
      />

      <section className="mt-6 max-w-5xl space-y-4">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title={t("common.desktopRequired")} tone="info">
            <p>{t("console.desktopRequiredBody")}</p>
          </StatePanel>
        )}

        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title={t("common.noAuthorized")} tone="warning">
            <p>{t("console.noAuthorizedBody")}</p>
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
                  key={d.serial}
                  type="button"
                  variant={
                    d.serial === selectedSerial ? "primary" : "secondary"
                  }
                  size="sm"
                  onClick={() => setSelectedSerial(d.serial)}
                >
                  {d.model ?? d.serial}
                </Button>
              ))}
            </div>
          </Card>
        )}

        {selectedSerial && (
          <Card className="overflow-hidden p-0">
            <div
              ref={outputRef}
              className="h-[28rem] overflow-y-auto bg-[#0c0d12] p-4 font-mono text-xs leading-6"
            >
              {history.length === 0 && (
                <p className="text-anvil-600">{t("console.hint")}</p>
              )}
              {history.map((entry) => (
                <div key={entry.id} className="mb-3">
                  <div className="flex items-center gap-2">
                    <span className="text-circuit-300">$</span>
                    <span className="text-anvil-100">{entry.command}</span>
                  </div>
                  <pre
                    className={[
                      "mt-1 whitespace-pre-wrap",
                      entry.error ? "text-red-300/80" : "text-anvil-400",
                    ].join(" ")}
                  >
                    {entry.output || t("console.noOutput")}
                  </pre>
                </div>
              ))}
              {running && (
                <div className="flex items-center gap-2 text-anvil-500">
                  <span className="animate-pulse">{t("console.running")}</span>
                </div>
              )}
            </div>
            <div className="flex items-center gap-2 border-t border-white/10 bg-white/[0.02] px-4 py-3">
              <span className="font-mono text-sm text-circuit-300">$</span>
              <input
                ref={inputRef}
                type="text"
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={t("console.placeholder")}
                disabled={running}
                autoFocus
                aria-label={t("console.commandLabel")}
                className="h-8 flex-1 bg-transparent font-mono text-sm text-anvil-50 outline-none placeholder:text-anvil-600"
              />
              <Button
                type="button"
                size="sm"
                variant="primary"
                onClick={() => void runCommand()}
                disabled={running || !command.trim()}
              >
                {t("console.run")}
              </Button>
            </div>
          </Card>
        )}
      </section>
    </>
  );
}
