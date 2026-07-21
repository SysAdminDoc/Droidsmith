import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callGnirehtetSessionStatus,
  callLocateGnirehtet,
  callStartGnirehtet,
  callStopGnirehtet,
  type DeviceTarget,
  type GnirehtetSession,
} from "../../lib/tauri";
import { Badge, Button, Card } from "../common";

type SharingState =
  | { kind: "idle" }
  | { kind: "starting" }
  | { kind: "running"; session: GnirehtetSession }
  | { kind: "ended"; session: GnirehtetSession }
  | { kind: "error"; message: string };

/** Gnirehtet reverse-tethering ("Share Internet") toggle for the selected
 *  device (R-084). Rendered only when the `gnirehtet` binary is on PATH; the
 *  session is supervised in `src-tauri/src/gnirehtet.rs` and mirrors the
 *  scrcpy start/poll/stop lifecycle. */
export function InternetSharing({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [located, setLocated] = useState<boolean | null>(null);
  const [state, setState] = useState<SharingState>({ kind: "idle" });
  const sessionRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    void callLocateGnirehtet()
      .then((path) => {
        if (!cancelled) setLocated(path != null);
      })
      .catch(() => {
        if (!cancelled) setLocated(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // A sharing session belongs to a specific device and this toggle is its only
  // control surface (gnirehtet is a headless background service, not a window).
  // Reset the UI for the new target, and in the cleanup stop the supervised
  // session when the target changes (device switch / reconnect) or the
  // component unmounts (leaving the route) — otherwise it is orphaned as an
  // uncontrollable process and a later remount would show "start" and spawn a
  // duplicate that fails on the busy relay port.
  useEffect(() => {
    setState({ kind: "idle" });
    return () => {
      const id = sessionRef.current;
      sessionRef.current = null;
      // The session may already be gone (device disconnected and reaped by a
      // status poll), in which case stop rejects with "not tracked" — swallow it.
      if (id !== null) void callStopGnirehtet(id).catch(() => {});
    };
  }, [target.serial, target.transport_id, target.connection_generation]);

  const runningSessionId = state.kind === "running" ? state.session.id : null;

  useEffect(() => {
    if (runningSessionId === null) return;
    const timer = window.setInterval(() => {
      void callGnirehtetSessionStatus(runningSessionId)
        .then((next) => {
          setState((current) => {
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
          setState((current) => {
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

  const start = useCallback(async () => {
    setState({ kind: "starting" });
    try {
      const session = await callStartGnirehtet(target);
      sessionRef.current = session.id;
      setState({ kind: "running", session });
    } catch (e) {
      setState({ kind: "error", message: errorMessage(e) });
    }
  }, [target]);

  const stop = useCallback(async () => {
    const id = sessionRef.current;
    if (id === null) return;
    try {
      const session = await callStopGnirehtet(id);
      sessionRef.current = null;
      setState({ kind: "ended", session });
    } catch (e) {
      setState({ kind: "error", message: errorMessage(e) });
    }
  }, []);

  // Only advertise the feature when the binary is actually installed.
  if (located !== true) return null;

  const running = state.kind === "running";
  const busy = state.kind === "starting";

  return (
    <Card className="p-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.tethering.title")}
            </h3>
            {running && (
              <Badge tone="success">{t("devices.tethering.active")}</Badge>
            )}
          </div>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.tethering.body")}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant={running ? "danger" : "primary"}
          onClick={() => (running ? void stop() : void start())}
          disabled={busy}
        >
          {busy
            ? t("devices.tethering.starting")
            : running
              ? t("devices.tethering.stop")
              : t("devices.tethering.start")}
        </Button>
      </div>
      {state.kind === "ended" && (
        <p role="status" className="mt-3 text-xs text-anvil-400">
          {t("devices.tethering.stopped")}
        </p>
      )}
      {state.kind === "error" && (
        <p role="alert" className="mt-3 text-xs text-red-200">
          {t("devices.tethering.failed", { message: state.message })}
        </p>
      )}
    </Card>
  );
}
