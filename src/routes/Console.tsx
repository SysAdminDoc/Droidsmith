import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useFocusTrap } from "../lib/useFocusTrap";

import {
  errorMessage,
  callApplyAction,
  callCancelOperation,
  callPlanShellAction,
  callShellRun,
  deviceTarget,
  newOperationId,
  type DeviceTarget,
  type OperationEvent,
  type PlannedAction,
} from "../lib/tauri";
import {
  resolveAuthorizedTarget,
  sameDeviceTarget,
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";
import {
  appendConsoleHistory,
  parseConsoleCommand,
  type ConsoleHistoryEntry,
} from "./consoleCommand";

import {
  Badge,
  Button,
  Card,
  PaneHeader,
  StatePanel,
  TransportBadge,
  TransportTrustNotice,
} from "./common";

type PendingShellAction = {
  command: string;
  plan: PlannedAction;
  dangerous: boolean;
  generation: number;
};

let nextEntryId = 1;

export default function ConsoleRoute() {
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
  const [command, setCommand] = useState("");
  const [running, setRunning] = useState(false);
  const [operationStatus, setOperationStatus] = useState<string | null>(null);
  const [liveOutput, setLiveOutput] = useState("");
  const [history, setHistory] = useState<ConsoleHistoryEntry[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const [pendingAction, setPendingAction] = useState<PendingShellAction | null>(
    null,
  );
  const outputRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const activeOperationRef = useRef<string | null>(null);
  const commandGenerationRef = useRef(0);
  const stickToOutputBottomRef = useRef(true);
  const reviewTrapRef = useFocusTrap<HTMLDivElement>(pendingAction !== null);

  useEffect(() => {
    if (!pendingAction) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setPendingAction(null);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [pendingAction]);

  useEffect(() => {
    const next = resolveAuthorizedTarget(selectedTarget, authorizedDevices);
    if (sameDeviceTarget(selectedTarget, next)) return;
    commandGenerationRef.current += 1;
    const operationId = activeOperationRef.current;
    activeOperationRef.current = null;
    if (operationId) void callCancelOperation(operationId);
    setSelectedTarget(next);
    setHistory([]);
    setHistoryIndex(-1);
    setLiveOutput("");
    setOperationStatus(null);
    setPendingAction(null);
    setRunning(false);
  }, [authorizedDevices, selectedTarget]);

  useEffect(() => {
    return () => {
      commandGenerationRef.current += 1;
      const operationId = activeOperationRef.current;
      activeOperationRef.current = null;
      if (operationId) void callCancelOperation(operationId);
    };
  }, []);

  useEffect(() => {
    if (outputRef.current && stickToOutputBottomRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [history, liveOutput, operationStatus]);

  const runCommand = useCallback(async () => {
    const trimmed = command.trim();
    if (!trimmed || !authorizedTarget || running) return;

    const parsed = parseConsoleCommand(trimmed);
    if (parsed.error) {
      setHistory((previous) =>
        appendConsoleHistory(
          previous,
          {
            command: trimmed,
            output: t(`console.parseErrors.${parsed.error}`),
            error: true,
            timestamp: Date.now(),
            id: nextEntryId++,
          },
          t("console.earlierOutputOmitted"),
        ),
      );
      return;
    }

    setRunning(true);
    setOperationStatus(null);
    setLiveOutput("");
    setHistoryIndex(-1);
    const generation = commandGenerationRef.current + 1;
    commandGenerationRef.current = generation;
    stickToOutputBottomRef.current = true;

    try {
      const assessment = await callPlanShellAction(
        authorizedTarget,
        parsed.argv,
      );
      if (commandGenerationRef.current !== generation) return;
      if (assessment.mutating) {
        if (!assessment.plan) {
          throw new Error("Audited shell planner returned no mutation plan");
        }
        setPendingAction({
          command: trimmed,
          plan: assessment.plan,
          dangerous: assessment.dangerous,
          generation,
        });
        setCommand("");
        return;
      }
      const operationId = newOperationId("console");
      activeOperationRef.current = operationId;
      setOperationStatus(t("console.starting"));
      const output = await callShellRun(authorizedTarget, parsed.argv, {
        operationId,
        onEvent: (event: OperationEvent) => {
          if (
            activeOperationRef.current !== operationId ||
            commandGenerationRef.current !== generation
          )
            return;
          if (event.kind === "output" && event.chunk) {
            setLiveOutput((previous) =>
              `${previous}${event.chunk}`.slice(-64 * 1024),
            );
          } else if (event.kind === "progress") {
            setOperationStatus(
              t("console.runningFor", {
                seconds: Math.max(
                  1,
                  Math.round((event.elapsed_ms ?? 0) / 1000),
                ),
              }),
            );
          }
        },
      });
      if (commandGenerationRef.current !== generation) return;
      setCommand("");
      setHistory((previous) =>
        appendConsoleHistory(
          previous,
          {
            command: trimmed,
            output,
            error: false,
            timestamp: Date.now(),
            id: nextEntryId++,
          },
          t("console.earlierOutputOmitted"),
        ),
      );
    } catch (e) {
      if (commandGenerationRef.current !== generation) return;
      setHistory((previous) =>
        appendConsoleHistory(
          previous,
          {
            command: trimmed,
            output: errorMessage(e),
            error: true,
            timestamp: Date.now(),
            id: nextEntryId++,
          },
          t("console.earlierOutputOmitted"),
        ),
      );
    } finally {
      if (commandGenerationRef.current === generation) {
        activeOperationRef.current = null;
        setRunning(false);
        setOperationStatus(null);
        setLiveOutput("");
        inputRef.current?.focus();
      }
    }
  }, [authorizedTarget, command, running, t]);

  const cancelRunning = useCallback(async () => {
    const operationId = activeOperationRef.current;
    if (!operationId) return;
    setOperationStatus(t("console.cancelling"));
    await callCancelOperation(operationId);
  }, [t]);

  const confirmPendingAction = useCallback(async () => {
    if (!pendingAction || running) return;
    const pending = pendingAction;
    if (pending.generation !== commandGenerationRef.current) {
      setPendingAction(null);
      return;
    }
    const generation = commandGenerationRef.current + 1;
    commandGenerationRef.current = generation;
    setPendingAction(null);
    setRunning(true);
    stickToOutputBottomRef.current = true;
    try {
      const result = await callApplyAction(pending.plan);
      if (commandGenerationRef.current !== generation) return;
      setHistory((previous) =>
        appendConsoleHistory(
          previous,
          {
            command: pending.command,
            output: result.stdout,
            error: false,
            timestamp: Date.now(),
            id: nextEntryId++,
          },
          t("console.earlierOutputOmitted"),
        ),
      );
    } catch (error) {
      if (commandGenerationRef.current !== generation) return;
      setHistory((previous) =>
        appendConsoleHistory(
          previous,
          {
            command: pending.command,
            output: errorMessage(error),
            error: true,
            timestamp: Date.now(),
            id: nextEntryId++,
          },
          t("console.earlierOutputOmitted"),
        ),
      );
    } finally {
      if (commandGenerationRef.current === generation) {
        setRunning(false);
        inputRef.current?.focus();
      }
    }
  }, [pendingAction, running, t]);

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

  return (
    <>
      {pendingAction && (
        <div
          ref={reviewTrapRef}
          tabIndex={-1}
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 px-4 outline-none backdrop-blur-sm"
          role="alertdialog"
          aria-modal="true"
          aria-labelledby="console-review-title"
          aria-describedby="console-review-description"
        >
          <Card className="w-full max-w-xl p-6">
            <div className="flex items-center gap-2">
              <Badge tone={pendingAction.dangerous ? "danger" : "warning"}>
                {pendingAction.dangerous
                  ? t("console.dangerousMutation")
                  : t("console.mutation")}
              </Badge>
              <code className="font-mono text-xs text-anvil-500">
                {pendingAction.plan.incident_id}
              </code>
            </div>
            <h2
              id="console-review-title"
              className="mt-4 text-lg font-semibold text-anvil-50"
            >
              {t("console.reviewMutation")}
            </h2>
            <p
              id="console-review-description"
              className="mt-2 text-sm leading-6 text-anvil-300"
            >
              {t("console.reviewMutationBody")}
            </p>
            <pre className="mt-4 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs text-anvil-100">
              {pendingAction.plan.args.join(" ")}
            </pre>
            <div className="mt-5 flex justify-end gap-2">
              <Button
                type="button"
                onClick={() => {
                  setPendingAction(null);
                  inputRef.current?.focus();
                }}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant="danger"
                onClick={() => void confirmPendingAction()}
              >
                {t("console.runReviewedMutation")}
              </Button>
            </div>
          </Card>
        </div>
      )}
      <PaneHeader
        title={t("console.title")}
        milestone="R-050"
        description={t("console.description")}
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

      <section className="mt-6 max-w-7xl space-y-4">
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
                    if (
                      d.transport_id === selectedTarget?.transport_id &&
                      d.connection_generation ===
                        selectedTarget?.connection_generation
                    )
                      return;
                    // Scrollback and recall history belong to the previous
                    // device's shell — reset them on switch.
                    commandGenerationRef.current += 1;
                    const operationId = activeOperationRef.current;
                    activeOperationRef.current = null;
                    if (operationId) void callCancelOperation(operationId);
                    setSelectedTarget(deviceTarget(d));
                    setHistory([]);
                    setHistoryIndex(-1);
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
          <Card className="overflow-hidden p-0">
            <div
              ref={outputRef}
              onScroll={() => {
                const output = outputRef.current;
                if (!output) return;
                stickToOutputBottomRef.current =
                  output.scrollHeight - output.scrollTop - output.clientHeight <
                  32;
              }}
              aria-busy={running}
              className="h-[28rem] overflow-y-auto bg-[#0c0d12] p-4 font-mono text-xs leading-6"
            >
              {history.length === 0 && (
                <p className="text-anvil-600">{t("console.hint")}</p>
              )}
              {history.map((entry) => (
                <div key={entry.id} className="mb-3">
                  <div className="flex items-center gap-2">
                    <span className="text-circuit-300">$</span>
                    <span className="break-all text-anvil-100">
                      {entry.command}
                    </span>
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
                <div className="text-anvil-500">
                  <div className="flex items-center gap-2">
                    <span className="animate-pulse">
                      {operationStatus ?? t("console.running")}
                    </span>
                    {activeOperationRef.current && (
                      <Button
                        type="button"
                        size="sm"
                        variant="danger"
                        onClick={() => void cancelRunning()}
                      >
                        {t("common.cancel")}
                      </Button>
                    )}
                  </div>
                  {liveOutput && (
                    <pre className="mt-1 whitespace-pre-wrap text-anvil-400">
                      {liveOutput}
                    </pre>
                  )}
                </div>
              )}
              <span className="sr-only" role="status" aria-live="polite">
                {operationStatus ?? (running ? t("console.running") : "")}
              </span>
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
