import { useCallback, useEffect, useState } from "react";

import {
  callLaunchScrcpy,
  callListDevices,
  callLocateScrcpy,
  inTauri,
  type ListDevicesResult,
} from "../lib/tauri";

import { Badge, Button, Card, PaneHeader, StatePanel } from "./common";

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
  | { kind: "active"; pid: number }
  | { kind: "error"; message: string };

export default function MirrorRoute() {
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [scrcpyState, setScrcpyState] = useState<ScrcpyState>({
    kind: "checking",
  });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [session, setSession] = useState<SessionState>({ kind: "idle" });
  const [maxSize, setMaxSize] = useState("1024");
  const [bitRate, setBitRate] = useState("8M");
  const [noAudio, setNoAudio] = useState(false);
  const [recording, setRecording] = useState(false);
  const [recordPath, setRecordPath] = useState("");

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

  const launchMirror = useCallback(async () => {
    if (!selectedSerial || scrcpyState.kind !== "found") return;
    setSession({ kind: "launching" });
    try {
      const pid = await callLaunchScrcpy({
        serial: selectedSerial,
        max_size: maxSize ? Number(maxSize) : null,
        bit_rate: bitRate || null,
        no_audio: noAudio,
        record_path: recording && recordPath.trim() ? recordPath.trim() : null,
      });
      setSession({ kind: "active", pid });
    } catch (e) {
      setSession({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedSerial, scrcpyState, maxSize, bitRate, noAudio]);

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];

  return (
    <>
      <PaneHeader
        title="Mirror"
        milestone="R-040"
        description="Launch scrcpy sessions to mirror and control Android devices. Requires scrcpy installed on your system."
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {scrcpyState.kind === "found" && (
              <Badge tone="success">scrcpy found</Badge>
            )}
            {scrcpyState.kind === "not_found" && (
              <Badge tone="warning">scrcpy missing</Badge>
            )}
            {scrcpyState.kind === "checking" && (
              <Badge tone="info">Checking...</Badge>
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
          <StatePanel title="Desktop shell required" tone="info">
            <p>Mirror sessions run inside the Tauri runtime.</p>
          </StatePanel>
        )}

        {scrcpyState.kind === "not_found" && (
          <StatePanel title="scrcpy is not installed" tone="warning">
            <p>
              Droidsmith uses scrcpy for device mirroring. Install it from{" "}
              <code>scrcpy.org</code> or your package manager:
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
                Check again
              </Button>
            </div>
          </StatePanel>
        )}

        {scrcpyState.kind === "found" && (
          <>
            {authorizedDevices.length === 0 && (
              <StatePanel title="No authorized devices" tone="warning">
                <p>Connect and authorize a device to start mirroring.</p>
              </StatePanel>
            )}

            {authorizedDevices.length > 1 && (
              <Card className="p-4">
                <h3 className="text-sm font-semibold text-anvil-50">
                  Target device
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
                <h3 className="text-sm font-semibold text-anvil-50">
                  Mirror options
                </h3>
                <p className="mt-1 text-xs text-anvil-400">
                  scrcpy at{" "}
                  <code className="text-anvil-200">{scrcpyState.path}</code>
                </p>
                <div className="mt-4 grid gap-4 sm:grid-cols-3">
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      Max size (px)
                    </span>
                    <input
                      type="text"
                      value={maxSize}
                      onChange={(e) => setMaxSize(e.target.value)}
                      placeholder="1024"
                      inputMode="numeric"
                      className="h-9 rounded-md border border-white/10 bg-white/[0.06] px-3 font-mono text-sm text-anvil-50 outline-none transition placeholder:text-anvil-600 focus:border-circuit-300/50 focus:ring-2 focus:ring-circuit-300/20"
                    />
                  </label>
                  <label className="grid gap-1.5">
                    <span className="text-xs font-medium text-anvil-400">
                      Video bit rate
                    </span>
                    <input
                      type="text"
                      value={bitRate}
                      onChange={(e) => setBitRate(e.target.value)}
                      placeholder="8M"
                      className="h-9 rounded-md border border-white/10 bg-white/[0.06] px-3 font-mono text-sm text-anvil-50 outline-none transition placeholder:text-anvil-600 focus:border-circuit-300/50 focus:ring-2 focus:ring-circuit-300/20"
                    />
                  </label>
                  <label className="flex items-center gap-2 self-end">
                    <input
                      type="checkbox"
                      checked={noAudio}
                      onChange={(e) => setNoAudio(e.target.checked)}
                      className="h-4 w-4 rounded border-white/20 bg-white/[0.06] text-circuit-300 focus:ring-2 focus:ring-circuit-300/30"
                    />
                    <span className="text-sm text-anvil-200">Disable audio</span>
                  </label>
                </div>
                <div className="mt-4 flex flex-wrap items-end gap-3">
                  <label className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={recording}
                      onChange={(e) => setRecording(e.target.checked)}
                      className="h-4 w-4 rounded border-white/20 bg-white/[0.06] text-circuit-300 focus:ring-2 focus:ring-circuit-300/30"
                    />
                    <span className="text-sm text-anvil-200">Record session</span>
                  </label>
                  {recording && (
                    <label className="grid flex-1 gap-1.5">
                      <span className="text-xs font-medium text-anvil-400">
                        Output file
                      </span>
                      <input
                        type="text"
                        value={recordPath}
                        onChange={(e) => setRecordPath(e.target.value)}
                        placeholder="recording.mp4"
                        className="h-9 rounded-md border border-white/10 bg-white/[0.06] px-3 font-mono text-sm text-anvil-50 outline-none transition placeholder:text-anvil-600 focus:border-circuit-300/50 focus:ring-2 focus:ring-circuit-300/20"
                      />
                    </label>
                  )}
                </div>
                <div className="mt-5 flex items-center gap-3">
                  <Button
                    type="button"
                    variant="primary"
                    onClick={() => void launchMirror()}
                    disabled={
                      session.kind === "launching" || session.kind === "active"
                    }
                  >
                    {session.kind === "launching"
                      ? "Launching..."
                      : session.kind === "active"
                        ? "Session active"
                        : "Launch mirror"}
                  </Button>
                  {session.kind === "active" && (
                    <Badge tone="success">PID {session.pid}</Badge>
                  )}
                </div>
              </Card>
            )}

            {session.kind === "error" && (
              <StatePanel title="Mirror launch failed" tone="danger">
                <p>{session.message}</p>
              </StatePanel>
            )}

            {session.kind === "active" && (
              <StatePanel title="Mirror session running" tone="success">
                <p>
                  scrcpy is running as PID {session.pid}. Close the scrcpy
                  window to end the session.
                </p>
              </StatePanel>
            )}
          </>
        )}
      </section>
    </>
  );
}
