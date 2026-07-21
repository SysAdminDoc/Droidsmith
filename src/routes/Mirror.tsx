import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callLaunchScrcpy,
  callLocateScrcpy,
  callScrcpyCapabilities,
  callScrcpySessionStatus,
  callSelectHostPath,
  callStopScrcpy,
  deviceTarget,
  inTauri,
  type DeviceTarget,
  type ScrcpyCapabilities,
  type ScrcpySession,
} from "../lib/tauri";
import {
  resolveAuthorizedTarget,
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";
import { formatDateTime } from "../lib/i18n";
import {
  loadStoredMirrorPreset,
  resetStoredMirrorPreset,
  saveStoredMirrorPreset,
} from "../lib/settings";

import {
  DEFAULT_MIRROR_PRESET,
  type KeyboardMode,
  type MirrorPreset,
} from "./mirrorPresets";

import {
  Badge,
  Button,
  Card,
  FieldInput,
  FieldSelect,
  PaneHeader,
  RevealInFolderButton,
  StatePanel,
  TransportBadge,
  TransportTrustNotice,
} from "./common";

type ScrcpyState =
  | { kind: "checking" }
  | { kind: "found"; path: string }
  | { kind: "not_found" };

type CapabilityState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "ready"; value: ScrcpyCapabilities }
  | { kind: "error"; message: string };

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
  const { devicesState, authorizedDevices } = useAuthorizedDevices();
  const [scrcpyState, setScrcpyState] = useState<ScrcpyState>({
    kind: "checking",
  });
  const [selectedTarget, setSelectedTarget] = useState<DeviceTarget | null>(
    null,
  );
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(selectedTarget);
  const [session, setSession] = useState<SessionState>({ kind: "idle" });
  const [recordingPath, setRecordingPath] = useState<string | null>(null);
  const [capabilityState, setCapabilityState] = useState<CapabilityState>({
    kind: "idle",
  });
  const [preset, setPreset] = useState<MirrorPreset>(DEFAULT_MIRROR_PRESET);
  const [presetMessage, setPresetMessage] = useState<string | null>(null);
  // Tracks the live selection so a launch that awaited the recording save
  // dialog can detect a mid-dialog device change (e.g. an automatic reconnect).
  const selectedTargetRef = useRef<DeviceTarget | null>(null);

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
    void checkScrcpy();
  }, [checkScrcpy]);

  useEffect(() => {
    setSelectedTarget((previous) =>
      resolveAuthorizedTarget(previous, authorizedDevices),
    );
  }, [authorizedDevices]);

  useEffect(() => {
    selectedTargetRef.current = selectedTarget;
  }, [selectedTarget]);

  useEffect(() => {
    if (!selectedTarget) return;
    let cancelled = false;
    setPresetMessage(null);
    // A mirror session belongs to a specific device — reset tracking when
    // the target changes so the UI doesn't show device A's running session
    // (and disabled launch button) while device B is selected. The A scrcpy
    // window keeps running independently.
    setSession({ kind: "idle" });
    setRecordingPath(null);
    setPreset(DEFAULT_MIRROR_PRESET);
    void loadStoredMirrorPreset(selectedTarget.serial)
      .then((stored) => {
        if (!cancelled) setPreset(stored);
      })
      .catch((error) => {
        if (!cancelled) {
          setPreset(DEFAULT_MIRROR_PRESET);
          setPresetMessage(
            t("mirror.presetLoadFailed", {
              message: errorMessage(error),
            }),
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedTarget, t]);

  const probeCapabilities = useCallback(async (target: DeviceTarget) => {
    setCapabilityState({ kind: "checking" });
    try {
      const value = await callScrcpyCapabilities(target);
      setCapabilityState({ kind: "ready", value });
    } catch (error) {
      setCapabilityState({
        kind: "error",
        message: errorMessage(error),
      });
    }
  }, []);

  useEffect(() => {
    if (scrcpyState.kind !== "found" || !selectedTarget) {
      setCapabilityState({ kind: "idle" });
      return;
    }
    void probeCapabilities(selectedTarget);
  }, [probeCapabilities, scrcpyState.kind, selectedTarget]);

  useEffect(() => {
    if (capabilityState.kind !== "ready") return;
    setPreset((previous) => {
      const videoCodec = capabilityState.value.available_video_codecs.includes(
        previous.videoCodec,
      )
        ? previous.videoCodec
        : (capabilityState.value.available_video_codecs[0] ?? "h264");
      const videoEncoder = capabilityState.value.video_encoders.some(
        (encoder) =>
          encoder.codec === videoCodec &&
          encoder.name === previous.videoEncoder,
      )
        ? previous.videoEncoder
        : "";
      return { ...previous, videoCodec, videoEncoder };
    });
  }, [capabilityState]);

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
          // Guard like the success path: a status poll for a superseded
          // session must not clobber a freshly-launched one.
          setSession((current) => {
            if (
              current.kind !== "running" ||
              current.session.id !== runningSessionId
            ) {
              return current;
            }
            return { kind: "error", message: errorMessage(e) };
          });
        });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [runningSessionId]);

  const savePreset = useCallback(async () => {
    if (!selectedTarget) return;
    try {
      await saveStoredMirrorPreset(selectedTarget.serial, preset);
      setPresetMessage(
        t("mirror.presetSaved", { serial: selectedTarget.serial }),
      );
    } catch (error) {
      setPresetMessage(
        t("mirror.presetSaveFailed", {
          message: errorMessage(error),
        }),
      );
    }
  }, [preset, selectedTarget, t]);

  const resetPreset = useCallback(async () => {
    setPreset(DEFAULT_MIRROR_PRESET);
    try {
      if (selectedTarget) {
        await resetStoredMirrorPreset(selectedTarget.serial);
      }
      setPresetMessage(t("mirror.presetReset"));
    } catch (error) {
      setPresetMessage(
        t("mirror.presetSaveFailed", {
          message: errorMessage(error),
        }),
      );
    }
  }, [selectedTarget, t]);

  const launchMirror = useCallback(async () => {
    if (
      !selectedTarget ||
      !authorizedTarget ||
      scrcpyState.kind !== "found" ||
      capabilityState.kind !== "ready"
    )
      return;
    setSession({ kind: "launching" });
    try {
      const recordingGrant = preset.recording
        ? await callSelectHostPath(
            "scrcpy_record_save",
            `droidsmith-recording-${new Date().toISOString().slice(0, 10)}.mp4`,
          )
        : null;
      if (preset.recording && !recordingGrant) {
        setSession({ kind: "idle" });
        return;
      }
      // If the selection changed while the save dialog was open, abandon this
      // launch so we don't start (and record) a session against a stale device.
      if (selectedTargetRef.current?.serial !== selectedTarget.serial) {
        setSession((current) =>
          current.kind === "launching" ? { kind: "idle" } : current,
        );
        return;
      }
      setRecordingPath(recordingGrant?.local_path ?? null);
      const next = await callLaunchScrcpy(
        {
          serial: selectedTarget.serial,
          target: authorizedTarget,
          max_size: parsePositiveInt(preset.maxSize),
          bit_rate: preset.bitRate.trim() || null,
          no_audio: preset.noAudio,
          keyboard_mode: preset.keyboardMode,
          video_codec: preset.videoCodec,
          video_encoder: preset.videoEncoder || null,
          turn_screen_off: preset.turnScreenOff,
          stay_awake: preset.stayAwake,
          show_touches: preset.showTouches,
          flex_display: preset.flexDisplay,
          keep_active: preset.keepActive,
          max_fps: parsePositiveInt(preset.maxFps),
          fullscreen: preset.fullscreen,
          always_on_top: preset.alwaysOnTop,
          no_control: preset.noControl,
          crop: preset.crop.trim() || null,
          display_orientation: preset.displayOrientation || null,
          screen_off_timeout: parsePositiveInt(preset.screenOffTimeout),
          audio_codec:
            preset.audioCodec === "default" ? null : preset.audioCodec,
          new_display: preset.newDisplay.trim() || null,
          audio_source:
            preset.audioSource === "output" ? null : preset.audioSource,
        },
        recordingGrant?.id,
      );
      setSession({ kind: "running", session: next });
    } catch (e) {
      setSession({
        kind: "error",
        message: errorMessage(e),
      });
    }
  }, [
    authorizedTarget,
    capabilityState.kind,
    selectedTarget,
    scrcpyState,
    preset,
  ]);

  const stopMirror = useCallback(async () => {
    if (session.kind !== "running") return;
    try {
      const stopped = await callStopScrcpy(session.session.id);
      setSession({ kind: "ended", session: stopped });
    } catch (e) {
      setSession({
        kind: "error",
        message: errorMessage(e),
      });
    }
  }, [session]);

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
            {capabilityState.kind === "ready" && (
              <Badge tone="info">scrcpy {capabilityState.value.version}</Badge>
            )}
            {scrcpyState.kind === "not_found" && (
              <Badge tone="warning">{t("mirror.scrcpyMissingShort")}</Badge>
            )}
            {scrcpyState.kind === "checking" && (
              <Badge tone="info">{t("common.checking")}</Badge>
            )}
            {selectedTarget && (
              <>
                <Badge tone="info">
                  <code className="font-mono">{selectedTarget.serial}</code>
                </Badge>
                <TransportBadge kind={selectedTarget.transport_kind} />
              </>
            )}
          </div>
        }
      />

      <section className="mt-4 max-w-7xl space-y-4">
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
                      disabled={session.kind === "launching"}
                      onClick={() => setSelectedTarget(deviceTarget(d))}
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

            {selectedTarget && capabilityState.kind === "checking" && (
              <StatePanel title={t("mirror.probingCapabilities")} tone="info">
                <p>{t("mirror.probingCapabilitiesBody")}</p>
              </StatePanel>
            )}

            {selectedTarget && capabilityState.kind === "error" && (
              <StatePanel
                title={t("mirror.capabilityProbeFailed")}
                tone="danger"
                actions={
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => void probeCapabilities(selectedTarget)}
                  >
                    {t("common.checkAgain")}
                  </Button>
                }
              >
                <p>{capabilityState.message}</p>
              </StatePanel>
            )}

            {capabilityState.kind === "ready" &&
              capabilityState.value.probe_warning && (
                <StatePanel
                  title={t("mirror.capabilityProbeLimited")}
                  tone="warning"
                >
                  <p>{capabilityState.value.probe_warning}</p>
                </StatePanel>
              )}

            {selectedTarget && (
              <Card className="p-4">
                <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div>
                    <h3 className="text-sm font-semibold text-anvil-50">
                      {t("mirror.options")}
                    </h3>
                    <p className="mt-1 text-xs text-anvil-400">
                      {t("mirror.scrcpyAt")}{" "}
                      <code className="text-anvil-200">{scrcpyState.path}</code>
                    </p>
                    {capabilityState.kind === "ready" && (
                      <p className="mt-1 text-xs text-anvil-400">
                        {t("mirror.negotiatedCapabilities", {
                          version: capabilityState.value.version,
                          count:
                            capabilityState.value.available_video_codecs.length,
                        })}
                      </p>
                    )}
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => void savePreset()}
                    >
                      {t("mirror.savePreset")}
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      onClick={() => void resetPreset()}
                    >
                      {t("mirror.resetPreset")}
                    </Button>
                  </div>
                </div>
                {presetMessage && (
                  <p role="status" className="mt-3 text-xs text-circuit-100">
                    {presetMessage}
                  </p>
                )}

                <div className="mt-4 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
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
                      {t("mirror.maxFps")}
                    </span>
                    <FieldInput
                      type="text"
                      value={preset.maxFps}
                      onChange={(e) =>
                        setPreset((prev) => ({
                          ...prev,
                          maxFps: e.target.value.replace(/\D/g, "").slice(0, 3),
                        }))
                      }
                      placeholder={t("mirror.maxFpsPlaceholder")}
                      inputMode="numeric"
                      className="font-mono"
                    />
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.crop")}
                    </span>
                    <FieldInput
                      type="text"
                      value={preset.crop}
                      onChange={(e) =>
                        setPreset((prev) => ({
                          ...prev,
                          crop: e.target.value
                            .replace(/[^\d:]/g, "")
                            .slice(0, 27),
                        }))
                      }
                      placeholder="1224:1440:0:0"
                      inputMode="numeric"
                      className="font-mono"
                    />
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.screenOffTimeout")}
                    </span>
                    <FieldInput
                      type="text"
                      value={preset.screenOffTimeout}
                      onChange={(e) =>
                        setPreset((prev) => ({
                          ...prev,
                          screenOffTimeout: e.target.value
                            .replace(/\D/g, "")
                            .slice(0, 6),
                        }))
                      }
                      placeholder={t("mirror.screenOffTimeoutPlaceholder")}
                      inputMode="numeric"
                      className="font-mono"
                    />
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.displayOrientation")}
                    </span>
                    <FieldSelect
                      value={preset.displayOrientation}
                      onChange={(event) =>
                        setPreset((previous) => ({
                          ...previous,
                          displayOrientation: event.target
                            .value as MirrorPreset["displayOrientation"],
                        }))
                      }
                    >
                      <option value="">{t("mirror.orientationDefault")}</option>
                      <option value="0">0°</option>
                      <option value="90">90°</option>
                      <option value="180">180°</option>
                      <option value="270">270°</option>
                      <option value="flip0">
                        {t("mirror.orientationFlip")} 0°
                      </option>
                      <option value="flip90">
                        {t("mirror.orientationFlip")} 90°
                      </option>
                      <option value="flip180">
                        {t("mirror.orientationFlip")} 180°
                      </option>
                      <option value="flip270">
                        {t("mirror.orientationFlip")} 270°
                      </option>
                    </FieldSelect>
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.audioCodec")}
                    </span>
                    <FieldSelect
                      value={preset.audioCodec}
                      onChange={(event) =>
                        setPreset((previous) => ({
                          ...previous,
                          audioCodec: event.target
                            .value as MirrorPreset["audioCodec"],
                        }))
                      }
                      disabled={preset.noAudio}
                    >
                      <option value="default">
                        {t("mirror.audioCodecDefault")}
                      </option>
                      <option value="opus">Opus</option>
                      <option value="aac">AAC</option>
                      <option value="flac">FLAC</option>
                      <option value="raw">{t("mirror.audioCodecRaw")}</option>
                    </FieldSelect>
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.audioSource")}
                    </span>
                    <FieldSelect
                      value={preset.audioSource}
                      onChange={(event) =>
                        setPreset((previous) => ({
                          ...previous,
                          audioSource: event.target.value,
                        }))
                      }
                      disabled={preset.noAudio}
                    >
                      <option value="output">
                        {t("mirror.audioSourceOutput")}
                      </option>
                      <option value="mic">{t("mirror.audioSourceMic")}</option>
                      {capabilityState.kind === "ready" &&
                        capabilityState.value
                          .supports_audio_source_expansion && (
                          <>
                            <option value="mic-unprocessed">
                              {t("mirror.audioSourceMicUnprocessed")}
                            </option>
                            <option value="mic-voice-communication">
                              {t("mirror.audioSourceMicVoiceComm")}
                            </option>
                            <option value="mic-voice-recognition">
                              {t("mirror.audioSourceMicVoiceRec")}
                            </option>
                            <option value="voice-call">
                              {t("mirror.audioSourceVoiceCall")}
                            </option>
                            <option value="playback">
                              {t("mirror.audioSourcePlayback")}
                            </option>
                          </>
                        )}
                    </FieldSelect>
                  </label>
                  {capabilityState.kind === "ready" &&
                    capabilityState.value.supports_new_display && (
                      <label className="grid gap-1.5">
                        <span className="text-xs font-medium text-anvil-400">
                          {t("mirror.newDisplay")}
                        </span>
                        <FieldInput
                          type="text"
                          value={preset.newDisplay}
                          onChange={(e) =>
                            setPreset((prev) => ({
                              ...prev,
                              newDisplay: e.target.value
                                .replace(/[^\dx/]/g, "")
                                .slice(0, 15),
                            }))
                          }
                          placeholder="1920x1080/240"
                          inputMode="text"
                          className="font-mono"
                        />
                      </label>
                    )}
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.videoCodec")}
                    </span>
                    <FieldSelect
                      value={preset.videoCodec}
                      onChange={(event) =>
                        setPreset((previous) => ({
                          ...previous,
                          videoCodec: event.target
                            .value as MirrorPreset["videoCodec"],
                          videoEncoder: "",
                        }))
                      }
                      disabled={capabilityState.kind !== "ready"}
                    >
                      {(capabilityState.kind === "ready"
                        ? capabilityState.value.available_video_codecs
                        : ["h264"]
                      ).map((codec) => (
                        <option key={codec} value={codec}>
                          {codec.toUpperCase()}
                        </option>
                      ))}
                    </FieldSelect>
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      {t("mirror.videoEncoder")}
                    </span>
                    <FieldSelect
                      value={preset.videoEncoder}
                      onChange={(event) =>
                        setPreset((previous) => ({
                          ...previous,
                          videoEncoder: event.target.value,
                        }))
                      }
                      disabled={capabilityState.kind !== "ready"}
                      className="font-mono text-xs"
                    >
                      <option value="">{t("mirror.encoderAutomatic")}</option>
                      {(capabilityState.kind === "ready"
                        ? capabilityState.value.video_encoders.filter(
                            (encoder) => encoder.codec === preset.videoCodec,
                          )
                        : []
                      ).map((encoder) => (
                        <option key={encoder.name} value={encoder.name}>
                          {encoder.name}
                          {encoder.software
                            ? ` (${t("mirror.encoderSoftware")})`
                            : ""}
                        </option>
                      ))}
                    </FieldSelect>
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
                    <FieldSelect
                      value={preset.keyboardMode}
                      onChange={(e) =>
                        setPreset((prev) => ({
                          ...prev,
                          keyboardMode: e.target.value as KeyboardMode,
                        }))
                      }
                    >
                      {KEYBOARD_MODES.map((mode) => (
                        <option key={mode.value} value={mode.value}>
                          {t(mode.labelKey)}
                        </option>
                      ))}
                    </FieldSelect>
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
                  <Toggle
                    checked={preset.fullscreen}
                    onChange={(checked) =>
                      setPreset((prev) => ({ ...prev, fullscreen: checked }))
                    }
                    label={t("mirror.fullscreen")}
                  />
                  <Toggle
                    checked={preset.alwaysOnTop}
                    onChange={(checked) =>
                      setPreset((prev) => ({ ...prev, alwaysOnTop: checked }))
                    }
                    label={t("mirror.alwaysOnTop")}
                  />
                  <Toggle
                    checked={preset.noControl}
                    onChange={(checked) =>
                      setPreset((prev) => ({ ...prev, noControl: checked }))
                    }
                    label={t("mirror.noControl")}
                  />
                  {capabilityState.kind === "ready" &&
                    capabilityState.value.supports_flex_display && (
                      <Toggle
                        checked={preset.flexDisplay}
                        onChange={(checked) =>
                          setPreset((prev) => ({
                            ...prev,
                            flexDisplay: checked,
                          }))
                        }
                        label={t("mirror.flexDisplay")}
                      />
                    )}
                  {capabilityState.kind === "ready" &&
                    capabilityState.value.supports_keep_active && (
                      <Toggle
                        checked={preset.keepActive}
                        onChange={(checked) =>
                          setPreset((prev) => ({
                            ...prev,
                            keepActive: checked,
                          }))
                        }
                        label={t("mirror.keepActive")}
                      />
                    )}
                </div>

                {preset.recording && (
                  <p className="mt-4 rounded-md border border-circuit-300/20 bg-circuit-300/10 p-3 text-xs leading-5 text-anvil-300">
                    {t("mirror.recordDestinationHint")}
                  </p>
                )}

                <div className="mt-5 flex flex-wrap items-center gap-3">
                  <Button
                    type="button"
                    variant="primary"
                    onClick={() => void launchMirror()}
                    disabled={
                      capabilityState.kind !== "ready" ||
                      session.kind === "launching" ||
                      session.kind === "running"
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
              <SessionPanel
                session={session.session}
                tone={
                  session.session.state === "exited" &&
                  session.session.exit_code !== 0
                    ? "danger"
                    : "warning"
                }
              />
            )}

            {session.kind === "ended" && recordingPath && (
              <div className="flex flex-wrap items-center gap-3">
                <p className="text-xs text-anvil-300">
                  {t("mirror.recordingSavedTo", { path: recordingPath })}
                </p>
                <RevealInFolderButton path={recordingPath} />
              </div>
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
    <label className="flex min-h-10 items-center gap-2 border-b border-white/[0.07] px-1 py-2">
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
  tone: "success" | "warning" | "danger";
}) {
  const { t, i18n } = useTranslation();
  const running = session.state === "running";
  const failed =
    session.state === "exited" &&
    session.exit_code !== 0 &&
    session.exit_reason !== null;
  return (
    <StatePanel
      title={
        running
          ? t("mirror.sessionRunning")
          : failed
            ? t(`mirror.failureReasons.${session.exit_reason}.title`)
            : t("mirror.sessionEndedTitle")
      }
      tone={tone}
    >
      <p>
        {running
          ? t("mirror.sessionRunningBody", { pid: session.pid })
          : failed
            ? t(`mirror.failureReasons.${session.exit_reason}.body`)
            : t("mirror.sessionEndedBody", {
                code: session.exit_code ?? t("common.notReported"),
              })}
      </p>
      <p className="mt-2 text-xs text-anvil-400">
        {t("mirror.sessionStarted", {
          time: formatDateTime(
            session.started_at,
            i18n.resolvedLanguage ?? i18n.language,
          ),
        })}
      </p>
      <pre className="mt-3 overflow-auto rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
        scrcpy {session.args.join(" ")}
      </pre>
      {session.stderr_tail && (
        <div className="mt-3">
          <p className="text-xs font-medium text-anvil-400">
            {t("mirror.stderrTail")}
          </p>
          <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
            {session.stderr_tail}
          </pre>
        </div>
      )}
    </StatePanel>
  );
}

function parsePositiveInt(value: string): number | null {
  const parsed = Number.parseInt(value.trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}
