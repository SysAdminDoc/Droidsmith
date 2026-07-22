import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callFindGnirehtetSession,
  callGnirehtetSessionStatus,
  callLocateGnirehtet,
  callStartGnirehtet,
  callStopGnirehtet,
  type DeviceTarget,
  type GnirehtetSession,
} from "../../lib/tauri";
import { useTargetOperation } from "../../lib/targetOperation";
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
 *  scrcpy start/poll/stop lifecycle. The supervised session persists across
 *  navigation: on (re)mount the panel re-attaches to an already-running session
 *  for the device instead of stopping it, so tethering survives leaving the
 *  route (e.g. to install something that needs the shared connection). */
export function InternetSharing({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const [located, setLocated] = useState<boolean | null>(null);
  const [state, setState] = useState<SharingState>({ kind: "idle" });
  const sessionOperation = useTargetOperation(target);

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
  // On (re)mount and device change, re-attach to a session already running for
  // this device rather than stopping it — so tethering persists across
  // navigation. The supervisor reaps dead sessions, so a stale one resolves to
  // null and shows "start". A running session is only torn down when the user
  // explicitly clicks Stop (or the app exits).
  useEffect(() => {
    const lease = sessionOperation.begin();
    setState({ kind: "idle" });
    void callFindGnirehtetSession(target)
      .then((existing) => {
        lease.commit(() => {
          if (existing && existing.state === "running") {
            setState({ kind: "running", session: existing });
          }
        });
      })
      .catch(() => {
        // Lookup is best-effort; on failure the panel simply offers "start".
      })
      .finally(() => {
        lease.finish();
      });
    // Re-attach only when the device identity changes, not on every render
    // (target is a fresh object each render).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    sessionOperation,
    target.serial,
    target.transport_id,
    target.connection_generation,
  ]);

  const runningSessionId = state.kind === "running" ? state.session.id : null;

  useEffect(() => {
    if (runningSessionId === null) return;
    const timer = window.setInterval(() => {
      const lease = sessionOperation.begin();
      void callGnirehtetSessionStatus(runningSessionId)
        .then((next) => {
          if (!lease.isCurrent()) return;
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
          if (!lease.isCurrent()) return;
          setState((current) => {
            if (
              current.kind !== "running" ||
              current.session.id !== runningSessionId
            ) {
              return current;
            }
            return { kind: "error", message: errorMessage(e) };
          });
        })
        .finally(() => {
          lease.finish();
        });
    }, 2000);
    return () => window.clearInterval(timer);
  }, [runningSessionId, sessionOperation]);

  const start = useCallback(async () => {
    const lease = sessionOperation.begin();
    setState({ kind: "starting" });
    try {
      const session = await callStartGnirehtet(target);
      lease.commit(() => setState({ kind: "running", session }));
    } catch (e) {
      lease.commit(() => setState({ kind: "error", message: errorMessage(e) }));
    } finally {
      lease.finish();
    }
  }, [sessionOperation, target]);

  const stop = useCallback(async () => {
    if (state.kind !== "running") return;
    const lease = sessionOperation.begin();
    try {
      const session = await callStopGnirehtet(state.session.id);
      lease.commit(() => setState({ kind: "ended", session }));
    } catch (e) {
      lease.commit(() => setState({ kind: "error", message: errorMessage(e) }));
    } finally {
      lease.finish();
    }
  }, [sessionOperation, state]);

  // Still probing PATH — render nothing to avoid a flash of the hint.
  if (located === null) return null;

  // Binary missing: surface a discovery hint so the feature is not silently
  // invisible (mirrors how scrcpy/fastboot report a locate failure).
  if (!located) {
    return (
      <Card className="p-5">
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.tethering.title")}
          </h3>
          <Badge tone="neutral">{t("devices.tethering.notInstalled")}</Badge>
        </div>
        <p className="mt-2 text-xs text-anvil-400">
          {t("devices.tethering.notFoundBody")}
        </p>
      </Card>
    );
  }

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
