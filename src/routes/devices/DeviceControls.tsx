import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callApplyDeviceControl,
  callJournalUndo,
  callSelectHostPath,
  callTakeScreenshot,
  type DeviceTarget,
} from "../../lib/tauri";
import { useTransportAuthorization } from "../../lib/useAuthorizedDevices";
import {
  Button,
  Card,
  FieldInput,
  RevealInFolderButton,
  TransportTrustNotice,
} from "../common";
import { statusToneClass, type StatusMessage } from "./common";
import { FileManager } from "./FileManager";
import { InternetSharing } from "./InternetSharing";
import { NetworkInspector } from "./NetworkInspector";
import { LayoutInspector } from "./LayoutInspector";
import { ProcessManager } from "./ProcessManager";

// Inline device-control results confirm real mutations, so a success and a
// failure must not read as the same faint line.
const REMOTE_BUTTONS: { id: string; labelKey: string; keycode: number }[] = [
  { id: "Home", labelKey: "devices.controls.remoteHome", keycode: 3 },
  { id: "Back", labelKey: "devices.controls.remoteBack", keycode: 4 },
  { id: "Recents", labelKey: "devices.controls.remoteRecents", keycode: 187 },
  { id: "Up", labelKey: "devices.controls.remoteUp", keycode: 19 },
  { id: "Down", labelKey: "devices.controls.remoteDown", keycode: 20 },
  { id: "Left", labelKey: "devices.controls.remoteLeft", keycode: 21 },
  { id: "Right", labelKey: "devices.controls.remoteRight", keycode: 22 },
  { id: "OK", labelKey: "devices.controls.remoteOk", keycode: 23 },
  { id: "Vol +", labelKey: "devices.controls.remoteVolUp", keycode: 24 },
  { id: "Vol -", labelKey: "devices.controls.remoteVolDown", keycode: 25 },
  { id: "Power", labelKey: "devices.controls.remotePower", keycode: 26 },
  { id: "Menu", labelKey: "devices.controls.remoteMenu", keycode: 82 },
];

/** Virtual remote, screenshot, display tuning, and the file/process/network/
 *  layout inspectors for the selected device (IMP-67: extracted verbatim from
 *  the former Devices.tsx god-file). */
export function DeviceControls({ target }: { target: DeviceTarget }) {
  const { t } = useTranslation();
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(target);
  const operationTarget = authorizedTarget ?? target;
  const serial = target.serial;
  const [lastKey, setLastKey] = useState<string | null>(null);
  const [screenshotMsg, setScreenshotMsg] = useState<StatusMessage>(null);
  const [screenshotPath, setScreenshotPath] = useState<string | null>(null);
  const [density, setDensity] = useState("");
  const [displayMsg, setDisplayMsg] = useState<StatusMessage>(null);
  const [displayRestoreEntryId, setDisplayRestoreEntryId] = useState<
    number | null
  >(null);

  useEffect(() => {
    setDisplayMsg(null);
    setDisplayRestoreEntryId(null);
  }, [target.connection_generation, target.serial, target.transport_id]);

  const sendKey = useCallback(
    async (keycode: number, label: string) => {
      try {
        await callApplyDeviceControl(operationTarget, [
          "input",
          "keyevent",
          String(keycode),
        ]);
        setLastKey(label);
      } catch {
        setLastKey(t("devices.controls.keyFailed", { label }));
      }
    },
    [operationTarget, t],
  );

  const takeScreenshot = useCallback(async () => {
    try {
      const pathGrant = await callSelectHostPath(
        "screenshot_save",
        `screenshot-${serial.replace(/[<>:"/\\|?*]/gu, "_")}-${Date.now()}.png`,
      );
      if (!pathGrant) {
        setScreenshotMsg(null);
        return;
      }
      setScreenshotPath(null);
      setScreenshotMsg({
        text: t("devices.controls.capturing"),
        tone: "neutral",
      });
      const artifact = await callTakeScreenshot(operationTarget, pathGrant.id);
      setScreenshotMsg({
        text: t("devices.controls.savedTo", { path: artifact.local_path }),
        tone: "success",
      });
      setScreenshotPath(artifact.local_path);
    } catch (e) {
      setScreenshotPath(null);
      setScreenshotMsg({
        tone: "danger",
        text: t("devices.controls.failed", {
          message: errorMessage(e),
        }),
      });
    }
  }, [operationTarget, serial, t]);

  const applyDensity = useCallback(async () => {
    if (!density.trim()) return;
    try {
      const result = await callApplyDeviceControl(operationTarget, [
        "wm",
        "density",
        density.trim(),
      ]);
      setDisplayRestoreEntryId(result.entry.id);
      setDisplayMsg({
        text: t("devices.controls.densitySet", { value: density.trim() }),
        tone: "success",
      });
    } catch (e) {
      setDisplayMsg({
        tone: "danger",
        text: t("devices.controls.failed", {
          message: errorMessage(e),
        }),
      });
    }
  }, [operationTarget, density, t]);

  const resetDensity = useCallback(async () => {
    try {
      const result = await callApplyDeviceControl(operationTarget, [
        "wm",
        "density",
        "reset",
      ]);
      setDisplayRestoreEntryId(result.entry.id);
      setDisplayMsg({
        text: t("devices.controls.densityReset"),
        tone: "success",
      });
    } catch (e) {
      setDisplayMsg({
        tone: "danger",
        text: t("devices.controls.failed", {
          message: errorMessage(e),
        }),
      });
    }
  }, [operationTarget, t]);

  const toggleForceDark = useCallback(
    async (enable: boolean) => {
      try {
        const result = await callApplyDeviceControl(operationTarget, [
          "settings",
          "put",
          "secure",
          "ui_night_mode",
          enable ? "2" : "1",
        ]);
        setDisplayRestoreEntryId(result.entry.id);
        setDisplayMsg({
          text: enable
            ? t("devices.controls.forceDarkEnabled")
            : t("devices.controls.forceDarkDisabled"),
          tone: "success",
        });
      } catch (e) {
        setDisplayMsg({
          tone: "danger",
          text: t("devices.controls.failed", {
            message: errorMessage(e),
          }),
        });
      }
    },
    [operationTarget, t],
  );

  const restoreDisplayState = useCallback(async () => {
    if (displayRestoreEntryId === null) return;
    try {
      await callJournalUndo(operationTarget, displayRestoreEntryId);
      setDisplayRestoreEntryId(null);
      setDisplayMsg({
        text: t("devices.controls.displayRestored"),
        tone: "success",
      });
    } catch (error) {
      setDisplayMsg({
        tone: "danger",
        text: t("devices.controls.restoreFailed", {
          message: errorMessage(error),
        }),
      });
    }
  }, [displayRestoreEntryId, operationTarget, t]);

  return (
    <div className="space-y-4">
      <TransportTrustNotice
        target={target}
        accepted={transportOverrideAccepted}
        onAcceptedChange={setTransportOverrideAccepted}
      />
      <div className="grid gap-4 lg:grid-cols-[1fr_1fr]">
        <Card className="p-5">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.controls.virtualRemote")}
            </h3>
            {lastKey && (
              <span className="text-xs text-anvil-400">
                {t("devices.controls.lastKey", { key: lastKey })}
              </span>
            )}
          </div>
          <div className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
            {REMOTE_BUTTONS.map((btn) => {
              const label = t(btn.labelKey);
              return (
                <Button
                  key={btn.id}
                  type="button"
                  size="sm"
                  onClick={() => void sendKey(btn.keycode, label)}
                  title={`keyevent ${btn.keycode}`}
                  className="justify-start"
                >
                  <RemoteGlyph label={btn.id} />
                  <span>{label}</span>
                </Button>
              );
            })}
          </div>
        </Card>

        <div className="space-y-4">
          <Card className="p-5">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.controls.screenshot")}
            </h3>
            <p className="mt-1 text-xs text-anvil-400">
              {t("devices.controls.screenshotBody")}
            </p>
            <div className="mt-3 flex items-center gap-3">
              <Button
                type="button"
                size="sm"
                variant="primary"
                onClick={() => void takeScreenshot()}
              >
                {t("devices.controls.capture")}
              </Button>
              {screenshotMsg && (
                <span
                  role="status"
                  className={`text-xs ${statusToneClass(screenshotMsg.tone)}`}
                >
                  {screenshotMsg.text}
                </span>
              )}
              {screenshotPath && <RevealInFolderButton path={screenshotPath} />}
            </div>
          </Card>

          <Card className="p-5">
            <h3 className="text-sm font-semibold text-anvil-50">
              {t("devices.controls.displayTuning")}
            </h3>
            <div className="mt-3 space-y-3">
              <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto] sm:items-end">
                <label className="grid min-w-0 gap-1.5">
                  <span className="text-xs font-medium text-anvil-400">
                    {t("devices.controls.densityLabel")}
                  </span>
                  <FieldInput
                    type="text"
                    value={density}
                    onChange={(e) => setDensity(e.target.value)}
                    placeholder="420"
                    inputMode="numeric"
                    className="font-mono"
                  />
                </label>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void applyDensity()}
                >
                  {t("devices.controls.set")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => void resetDensity()}
                >
                  {t("devices.controls.reset")}
                </Button>
              </div>
              <div className="flex flex-wrap items-end gap-2">
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void toggleForceDark(true)}
                >
                  {t("devices.controls.forceDarkOn")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => void toggleForceDark(false)}
                >
                  {t("devices.controls.forceDarkOff")}
                </Button>
              </div>
            </div>
            {displayMsg && (
              <p
                role="status"
                className={`mt-3 text-xs ${statusToneClass(displayMsg.tone)}`}
              >
                {displayMsg.text}
              </p>
            )}
            {displayRestoreEntryId !== null && (
              <Button
                type="button"
                size="sm"
                variant="primary"
                className="mt-3"
                onClick={() => void restoreDisplayState()}
              >
                {t("devices.controls.restoreDisplay")}
              </Button>
            )}
          </Card>
        </div>
      </div>
      <InternetSharing target={operationTarget} />
      <ProcessManager target={operationTarget} />
      <FileManager target={operationTarget} />
      <NetworkInspector target={operationTarget} />
      <LayoutInspector target={operationTarget} />
    </div>
  );
}

function RemoteGlyph({ label }: { label: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4 shrink-0"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      {label === "Home" && (
        <path d="m4 11 8-7 8 7v8a1 1 0 0 1-1 1h-5v-6h-4v6H5a1 1 0 0 1-1-1v-8Z" />
      )}
      {label === "Back" && <path d="M15 6 9 12l6 6" />}
      {label === "Recents" && (
        <>
          <path d="M8 7h9v9" />
          <path d="M5 10h9v9H5z" />
        </>
      )}
      {label === "Up" && <path d="m7 14 5-5 5 5" />}
      {label === "Down" && <path d="m7 10 5 5 5-5" />}
      {label === "Left" && <path d="m14 7-5 5 5 5" />}
      {label === "Right" && <path d="m10 7 5 5-5 5" />}
      {label === "OK" && <circle cx="12" cy="12" r="4" />}
      {label === "Vol +" && (
        <>
          <path d="M5 10v4h3l4 3V7l-4 3H5Z" />
          <path d="M17 9v6M14 12h6" />
        </>
      )}
      {label === "Vol -" && (
        <>
          <path d="M5 10v4h3l4 3V7l-4 3H5Z" />
          <path d="M15 12h5" />
        </>
      )}
      {label === "Power" && (
        <>
          <path d="M12 4v8" />
          <path d="M7.5 7.5a7 7 0 1 0 9 0" />
        </>
      )}
      {label === "Menu" && (
        <>
          <path d="M6 8h12" />
          <path d="M6 12h12" />
          <path d="M6 16h12" />
        </>
      )}
    </svg>
  );
}
