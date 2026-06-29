import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callLaunchScrcpy,
  callListDevices,
  callLocateScrcpy,
  callScrcpySessionStatus,
  callStopScrcpy,
  inTauri,
  type ListDevicesResult,
  type ScrcpySession,
} from "../lib/tauri";

import {
  DEFAULT_MIRROR_PRESET,
  normalizePreset,
  presetStorageKey,
  type KeyboardMode,
  type MirrorPreset,
} from "./mirrorPresets";

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

type ScrcpyState =
  | { kind: "checking" }
  | { kind: "found"; path: string }
  | { kind: "not_found" };

type SessionState =
  | { kind: "idle" }
  | { kind: "launching" }
  | { kind: "running"; session: ScrcpySession }
  | { kind: "ended"; session: ScrcpySession }
  | { kind: "error"; message: string };

const KEYBOARD_MODES: { value: KeyboardMode; labelKey: string }[] = [
  { value: "default", labelKey: "mirror.keyboardDefault" },
  { value: "sdk", labelKey: "mirror.keyboardSdk" },
  { value: "uhid", labelKey: "mirror.keyboardUhid" },
  { value: "aoa", labelKey: "mirror.keyboardAoa" },
  { value: "disabled", labelKey: "mirror.keyboardDisabled" },
];

export default function MirrorRoute() {
  const { t } = useTranslation();
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [scrcpyState, setScrcpyState] = useState<ScrcpyState>({
    kind: "checking",
  });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [session, setSession] = useState<SessionState>({ kind: "idle" });
  const [preset, setPreset] = useState<MirrorPreset>(DEFAULT_MIRROR_PRESET);
  const [presetMessage, setPresetMessage] = useState<string | null>(null);

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

  const checkScrcpy = useCallback(async () => {
    if (!inTauri()) return;
    setScrcpyState({ kind: "checking" });
    try {
      const path = await callLocateScrcpy();
      if (path) {
        setScrcpyState({ kind: "found", path });
      } else {
        setScrcpyState({ kind: "not_found" });
      }
    } catch {
      setScrcpyState({ kind: "not_found" });
    }
  }, []);

  useEffect(() => {
    void loadDevices();
    void checkScrcpy();
  }, [loadDevices, checkScrcpy]);

  useEffect(() => {
    if (!selectedSerial) return;
    setPresetMessage(null);
    try {
      const raw = window.localStorage.getItem(presetStorageKey(selectedSerial));
      setPreset(raw ? normalizePreset(JSON.parse(raw)) : DEFAULT_MIRROR_PRESET);
    } catch {
      setPreset(DEFAULT_MIRROR_PRESET);
    }
  }, [selectedSerial]);

  const runningSessionId =
    session.kind === "running" ? session.session.id : null;

  useEffect(() => {
    if (runningSessionId === null) return;
    const timer = window.setInterval(() => {
      void callScrcpySessionStatus(runningSessionId)
        .then((next) => {
          setSession((current) => {
            if (
              current.kind !== "running" ||
              current.session.id !== runningSessionId
            ) {
              return current;
            }
            return next.state === "running"
              ? { kind: "running", session: next }
              : { kind: "ended", session: next };
          });
        })
        .catch((e) => {
          setSession({
            kind: "error",
            message: e instanceof Error ? e.message : String(e),
          });
        });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [runningSessionId]);

  const savePreset = useCallback(() => {
    if (!selectedSerial) return;
    window.localStorage.setItem(
      presetStorageKey(selectedSerial),
      JSON.stringify(preset),
    );
    setPresetMessage(t("mirror.presetSaved", { serial: selectedSerial }));
  }, [preset, selectedSerial, t]);

  const resetPreset = useCallback(() => {
    setPreset(DEFAULT_MIRROR_PRESET);
    if (selectedSerial) {
      window.localStorage.removeItem(presetStorageKey(selectedSerial));
    }
    setPresetMessage(t("mirror.presetReset"));
  }, [selectedSerial, t]);

  const launchMirror = useCallback(async () => {
    if (!selectedSerial || scrcpyState.kind !== "found") return;
    setSession({ kind: "launching" });
    try {
      const next = await callLaunchScrcpy({
        serial: selectedSerial,
        max_size: parsePositiveInt(preset.maxSize),
        bit_rate: preset.bitRate.trim() || null,
        no_audio: preset.noAudio,
        record_path:
          preset.recording && preset.recordPath.trim()
            ? preset.recordPath.trim()
            : null,
        keyboard_mode: preset.keyboardMode,
        turn_screen_off: preset.turnScreenOff,
        stay_awake: preset.stayAwake,
        show_touches: preset.showTouches,
      });
      setSession({ kind: "running", session: next });
    } catch (e) {
      setSession({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedSerial, scrcpyState, preset]);

  const stopMirror = useCallback(async () => {
    if (session.kind !== "running") return;
    try {
      const stopped = await callStopScrcpy(session.session.id);
      setSession({ kind: "ended", session: stopped });
    } catch (e) {
      setSession({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [session]);

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];

  return (
    <>
      <PaneHeader
        title={t("mirror.title")}
        milestone="R-040"
        description={t("mirror.description")}
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {scrcpyState.kind === "found" && (
              <Badge tone="success">{t("mirror.scrcpyFound")}</Badge>
            )}
            {scrcpyState.kind === "not_found" && (
              <Badge tone="warning">{t("mirror.scrcpyMissingShort")}</Badge>
            )}
            {scrcpyState.kind === "checking" && (
              <Badge tone="info">{t("common.checking")}</Badge>
            )}
            {selectedSerial && (
              <Badge tone="info">
                <code className="font-mono">{selectedSerial}</code>
              </Badge>
            )}
          </div>
        }
      />

      <section className="mt-6 max-w-5xl space-y-4">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title={t("common.desktopRequired")} tone="info">
            <p>{t("mirror.desktopRequiredBody")}</p>
          </StatePanel>
        )}

        {scrcpyState.kind === "not_found" && (
          <StatePanel title={t("mirror.scrcpyMissing")} tone="warning">
            <p>
              {t("mirror.scrcpyInstallPrefix")} <code>scrcpy.org</code> or your
              package manager:
            </p>
            <pre className="mt-2 rounded-md border border-white/10 bg-white/[0.04] p-3 font-mono text-xs text-anvil-200">
              {`# Windows (scoop)\nscoop install scrcpy\n\n# macOS (brew)\nbrew install scrcpy\n\n# Linux (apt)\nsudo apt install scrcpy`}
            </pre>
            <div className="mt-3">
              <Button
                type="button"
                size="sm"
                onClick={() => void checkScrcpy()}
              >
                {t("common.checkAgain")}
              </Button>
            </div>
          </StatePanel>
        )}

        {scrcpyState.kind === "found" && (
          <>
            {authorizedDevices.length === 0 && (
              <StatePanel title={t("common.noAuthorized")} tone="warning">
                <p>{t("mirror.noAuthorizedBody")}</p>
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
              <Card className="p-5">
                <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div>
                    <h3 className="text-sm font-semibold text-anvil-50">
                      {t("mirror.options")}
                    </h3>
                    <p className="mt-1 text-xs text-anvil-400">
                      {t("mirror.scrcpyAt")}{" "}
                      <code className="text-anvil-200">{scrcpyState.path}</code>
                    </p>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button type="button" size="sm" onClick={savePreset}>
                      {t("mirror.savePreset")}
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={resetPreset}
                    >
                      {t("mirror.resetPreset")}
                    </Button>
                  </div>
                </div>
                {presetMessage && (
                  <p className="mt-3 text-xs text-circuit-100">
                    {presetMessage}
                  </p>
                )}

                <div className="mt-4 grid gap-4 sm:grid-cols-3">
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.maxSize")}
                    </span>
                    <FieldInput
                      type="text"
                      value={preset.maxSize}
                      onChange={(e) =>
                        setPreset((prev) => ({
                          ...prev,
                          maxSize: e.target.value,
                        }))
                      }
                      placeholder="1024"
                      inputMode="numeric"
                      className="font-mono"
                    />
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.bitRate")}
                    </span>
                    <FieldInput
                      type="text"
                      value={preset.bitRate}
                      onChange={(e) =>
                        setPreset((prev) => ({
                          ...prev,
                          bitRate: e.target.value,
                        }))
                      }
                      placeholder="8M"
                      className="font-mono"
                    />
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.keyboardMode")}
                    </span>
                    <select
                      value={preset.keyboardMode}
                      onChange={(e) =>
                        setPreset((prev) => ({
                          ...prev,
                          keyboardMode: e.target.value as KeyboardMode,
                        }))
                      }
                      className="h-9 rounded-md border border-white/10 bg-white/[0.06] px-3 text-sm text-anvil-50 outline-none transition focus:border-circuit-300/60 focus:ring-2 focus:ring-circuit-300/20"
                    >
                      {KEYBOARD_MODES.map((mode) => (
                        <option key={mode.value} value={mode.value}>
                          {t(mode.labelKey)}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>

                <div className="mt-4 grid gap-3 sm:grid-cols-2">
                  <Toggle
                    checked={preset.noAudio}
                    onChange={(checked) =>
                      setPreset((prev) => ({ ...prev, noAudio: checked }))
                    }
                    label={t("mirror.disableAudio")}
                  />
                  <Toggle
                    checked={preset.recording}
                    onChange={(checked) =>
                      setPreset((prev) => ({ ...prev, recording: checked }))
                    }
                    label={t("mirror.recordSession")}
                  />
                  <Toggle
                    checked={preset.turnScreenOff}
                    onChange={(checked) =>
                      setPreset((prev) => ({
                        ...prev,
                        turnScreenOff: checked,
                      }))
                    }
                    label={t("mirror.turnScreenOff")}
                  />
                  <Toggle
                    checked={preset.stayAwake}
                    onChange={(checked) =>
                      setPreset((prev) => ({ ...prev, stayAwake: checked }))
                    }
                    label={t("mirror.stayAwake")}
                  />
                  <Toggle
                    checked={preset.showTouches}
                    onChange={(checked) =>
                      setPreset((prev) => ({ ...prev, showTouches: checked }))
                    }
                    label={t("mirror.showTouches")}
                  />
                </div>

                {preset.recording && (
                  <label className="mt-4 grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.outputFile")}
                    </span>
                    <FieldInput
                      type="text"
                      value={preset.recordPath}
                      onChange={(e) =>
                        setPreset((prev) => ({
                          ...prev,
                          recordPath: e.target.value,
                        }))
                      }
                      placeholder="recording.mp4"
                      className="font-mono"
                    />
                  </label>
                )}

                <div className="mt-5 flex flex-wrap items-center gap-3">
                  <Button
                    type="button"
                    variant="primary"
                    onClick={() => void launchMirror()}
                    disabled={
                      session.kind === "launching" || session.kind === "running"
                    }
                  >
                    {session.kind === "launching"
                      ? t("mirror.launching")
                      : session.kind === "running"
                        ? t("mirror.sessionActive")
                        : t("mirror.launch")}
                  </Button>
                  {session.kind === "running" && (
                    <Button
                      type="button"
                      variant="danger"
                      onClick={() => void stopMirror()}
                    >
                      {t("mirror.stopSession")}
                    </Button>
                  )}
                  {session.kind === "running" && (
                    <Badge tone="success">PID {session.session.pid}</Badge>
                  )}
                </div>
              </Card>
            )}

            {session.kind === "error" && (
              <StatePanel title={t("mirror.launchFailed")} tone="danger">
                <p>{session.message}</p>
              </StatePanel>
            )}

            {session.kind === "running" && (
              <SessionPanel session={session.session} tone="success" />
            )}

            {session.kind === "ended" && (
              <SessionPanel session={session.session} tone="warning" />
            )}
          </>
        )}
      </section>
    </>
  );
}

function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
}) {
  return (
    <label className="flex items-center gap-2 rounded-md border border-white/10 bg-white/[0.03] px-3 py-2">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4 rounded border-white/20 bg-white/[0.06] text-circuit-300 focus:ring-2 focus:ring-circuit-300/30"
      />
      <span className="text-sm text-anvil-200">{label}</span>
    </label>
  );
}

function SessionPanel({
  session,
  tone,
}: {
  session: ScrcpySession;
  tone: "success" | "warning";
}) {
  const { t } = useTranslation();
  const running = session.state === "running";
  return (
    <StatePanel
      title={
        running ? t("mirror.sessionRunning") : t("mirror.sessionEndedTitle")
      }
      tone={tone}
    >
      <p>
        {running
          ? t("mirror.sessionRunningBody", { pid: session.pid })
          : t("mirror.sessionEndedBody", {
              code: session.exit_code ?? t("common.notReported"),
            })}
      </p>
      <p className="mt-2 text-xs text-anvil-400">
        {t("mirror.sessionStarted", {
          time: formatSessionTime(session.started_at),
        })}
      </p>
      <pre className="mt-3 overflow-auto rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
        scrcpy {session.args.join(" ")}
      </pre>
    </StatePanel>
  );
}

function parsePositiveInt(value: string): number | null {
  const parsed = Number.parseInt(value.trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

function formatSessionTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(date);
}
