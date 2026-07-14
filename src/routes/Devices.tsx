import { useCallback, useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";

import {
  callGetDeviceInfo,
  callListDevices,
  callListNetworkConnections,
  callListProcesses,
  callListRemoteFiles,
  callPullFile,
  callShellRun,
  callTakeScreenshot,
  inTauri,
  summarizeState,
  type DeviceInfo,
  type ListDevicesResult,
  type NetworkConnection,
  type ProcessInfo,
  type RemoteFileEntry,
  type RemoteListing,
  type SerializedDeviceState,
} from "../lib/tauri";

import {
  Badge,
  Button,
  Card,
  EmptyState,
  FieldInput,
  PaneHeader,
  SkeletonLine,
  StatePanel,
  TableCell,
  TableHeaderCell,
} from "./common";

type State =
  | { kind: "loading" }
  | { kind: "no_tauri" }
  | { kind: "ok"; value: ListDevicesResult }
  | { kind: "error"; message: string };

type DetailState =
  | { kind: "idle" }
  | { kind: "loading"; serial: string }
  | { kind: "ok"; info: DeviceInfo }
  | { kind: "error"; serial: string; message: string };

export default function DevicesRoute() {
  const { t } = useTranslation();
  const [state, setState] = useState<State>({ kind: "loading" });
  const [detail, setDetail] = useState<DetailState>({ kind: "idle" });

  const refresh = useCallback(async () => {
    if (!inTauri()) {
      setState({ kind: "no_tauri" });
      return;
    }
    setState({ kind: "loading" });
    try {
      const value = await callListDevices();
      setState({ kind: "ok", value });
    } catch (e) {
      setState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  const selectDevice = useCallback(async (serial: string) => {
    setDetail({ kind: "loading", serial });
    try {
      const info = await callGetDeviceInfo(serial);
      setDetail({ kind: "ok", info });
    } catch (e) {
      setDetail({
        kind: "error",
        serial,
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <>
      <PaneHeader
        title={t("devices.title")}
        milestone="R-012"
        description={t("devices.description")}
        actions={
          <Button
            type="button"
            onClick={() => void refresh()}
            disabled={state.kind === "loading"}
            variant="primary"
          >
            {state.kind === "loading"
              ? t("devices.scanning")
              : t("devices.refresh")}
          </Button>
        }
        meta={<DeviceHeaderMeta state={state} />}
      />

      <section className="mt-6 max-w-6xl" aria-live="polite">
        {state.kind === "no_tauri" && (
          <StatePanel
            title={t("devices.launchDesktopTitle")}
            tone="info"
            actions={
              <Button type="button" onClick={() => void refresh()} size="sm">
                {t("common.checkAgain")}
              </Button>
            }
          >
            <p>
              {t("devices.launchDesktopBodyPrefix")}{" "}
              <code>npm run tauri:dev</code>{" "}
              {t("devices.launchDesktopBodySuffix")}
            </p>
          </StatePanel>
        )}

        {state.kind === "loading" && <DeviceTableSkeleton />}

        {state.kind === "error" && (
          <StatePanel
            title={t("devices.scanFailedTitle")}
            tone="danger"
            actions={
              <Button
                type="button"
                onClick={() => void refresh()}
                variant="danger"
                size="sm"
              >
                {t("common.retryScan")}
              </Button>
            }
          >
            <p>{state.message}</p>
          </StatePanel>
        )}

        {state.kind === "ok" && !state.value.adb_resolved && (
          <StatePanel title={t("devices.noAdb")} tone="warning">
            <p>
              {t("devices.noAdbBodyPrefix")} <code>$PATH</code>,{" "}
              <code>$ANDROID_HOME</code>, {t("devices.noAdbBodyMiddle")}{" "}
              <code>scripts/fetch-platform-tools.*</code>{" "}
              {t("devices.noAdbBodySuffix")}
            </p>
          </StatePanel>
        )}

        {state.kind === "ok" &&
          state.value.adb_resolved &&
          state.value.devices.length === 0 && (
            <StatePanel
              title={t("devices.noDevices")}
              tone="info"
              actions={
                <Button
                  type="button"
                  onClick={() => void refresh()}
                  variant="secondary"
                  size="sm"
                >
                  {t("common.scanAgain")}
                </Button>
              }
            >
              <ol className="grid gap-2 text-sm sm:grid-cols-3">
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  {t("devices.noDevicesStep1")}
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  {t("devices.noDevicesStep2Prefix")}{" "}
                  <em>{t("devices.allowUsbDebugging")}</em>{" "}
                  {t("devices.noDevicesStep2Suffix")}
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  {t("devices.noDevicesStep3")}
                </li>
              </ol>
            </StatePanel>
          )}

        {state.kind === "ok" && state.value.devices.length > 0 && (
          <DeviceTable
            devices={state.value.devices}
            selectedSerial={
              detail.kind === "ok"
                ? detail.info.serial
                : detail.kind === "loading"
                  ? detail.serial
                  : undefined
            }
            onSelect={(serial) => void selectDevice(serial)}
          />
        )}

        {state.kind === "ok" &&
          state.value.devices.some(
            (d) => typeof d.state === "string" && d.state === "unauthorized",
          ) && (
            <AuthorizePrompt
              devices={state.value.devices.filter(
                (d) =>
                  typeof d.state === "string" && d.state === "unauthorized",
              )}
              onRefresh={() => void refresh()}
            />
          )}

        {state.kind === "ok" &&
          state.value.devices.some(
            (d) => typeof d.state === "string" && d.state === "no_permissions",
          ) && (
            <StatePanel title={t("devices.linuxPerms")} tone="danger">
              <p>{t("devices.linuxPermsBody")}</p>
              <pre className="mt-2 rounded-md border border-white/10 bg-white/[0.04] p-3 font-mono text-xs text-anvil-200">
                {`echo 'SUBSYSTEM=="usb", ATTR{idVendor}=="XXXX", MODE="0666"' | sudo tee /etc/udev/rules.d/51-android.rules\nsudo udevadm control --reload-rules && sudo udevadm trigger`}
              </pre>
              <p className="mt-2">
                {t("devices.linuxPermsReplace")} <code>XXXX</code>{" "}
                {t("devices.linuxPermsExamples")} <code>18d1</code>{" "}
                {t("devices.forGoogle")} <code>04e8</code>{" "}
                {t("devices.forSamsung")} {t("devices.reconnect")}
              </p>
            </StatePanel>
          )}
      </section>

      {detail.kind !== "idle" && (
        <section className="mt-4 max-w-6xl space-y-4" aria-live="polite">
          <DeviceDetail
            state={detail}
            onRetry={(serial) => void selectDevice(serial)}
          />
          {detail.kind === "ok" && (
            <DeviceControls serial={detail.info.serial} />
          )}
        </section>
      )}
    </>
  );
}

function DeviceHeaderMeta({ state }: { state: State }) {
  const { t } = useTranslation();

  if (state.kind === "loading") {
    return (
      <div className="flex flex-wrap gap-2">
        <Badge tone="info">{t("devices.scanningBridge")}</Badge>
        <Badge tone="neutral">{t("devices.waitingForAdb")}</Badge>
      </div>
    );
  }

  if (state.kind === "no_tauri") {
    return (
      <div className="flex flex-wrap gap-2">
        <Badge tone="neutral">{t("runtime.browserPreview")}</Badge>
        <Badge tone="info">{t("common.tauriIpcRequired")}</Badge>
      </div>
    );
  }

  if (state.kind === "error") {
    return <Badge tone="danger">{t("devices.scanFailed")}</Badge>;
  }

  if (!state.value.adb_resolved) {
    return <Badge tone="warning">{t("devices.adbMissing")}</Badge>;
  }

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-2">
      <Badge tone="success">{t("devices.adbResolved")}</Badge>
      {state.value.adb_path && (
        <code className="max-w-full truncate font-mono text-xs">
          {state.value.adb_path}
        </code>
      )}
    </div>
  );
}

function DeviceTable({
  devices,
  selectedSerial,
  onSelect,
}: {
  devices: ListDevicesResult["devices"];
  selectedSerial?: string;
  onSelect: (serial: string) => void;
}) {
  const { t } = useTranslation();

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("devices.connected")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("devices.selectHint")}
          </p>
        </div>
        <Badge tone="success">
          {t("common.deviceCount", { count: devices.length })}
        </Badge>
      </div>
      <div className="overflow-x-auto">
        <table className="min-w-full text-sm">
          <thead className="bg-white/[0.04]">
            <tr>
              <TableHeaderCell>{t("devices.serial")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.identity")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.transport")}</TableHeaderCell>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/10">
            {devices.map((device) => {
              const isDevice =
                typeof device.state === "string" && device.state === "device";
              const isSelected = device.serial === selectedSerial;
              return (
                <tr
                  key={device.serial}
                  role={isDevice ? "button" : undefined}
                  tabIndex={isDevice ? 0 : undefined}
                  onClick={isDevice ? () => onSelect(device.serial) : undefined}
                  onKeyDown={
                    isDevice
                      ? (e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            onSelect(device.serial);
                          }
                        }
                      : undefined
                  }
                  title={
                    !isDevice
                      ? "Device must be authorized before it can be selected"
                      : undefined
                  }
                  className={[
                    "transition",
                    isDevice
                      ? "cursor-pointer hover:bg-white/[0.055] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-circuit-300 focus-visible:outline-none"
                      : "bg-anvil-950/20 opacity-75",
                    isSelected ? "bg-circuit-300/[0.06]" : "",
                  ].join(" ")}
                >
                  <TableCell>
                    <div className="flex min-w-[13rem] items-center gap-2">
                      <code className="font-mono text-xs text-anvil-50">
                        {device.serial}
                      </code>
                      {device.wireless && <Badge tone="info">Wi-Fi</Badge>}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge tone={deviceStateTone(device.state)}>
                      {formatStateLabel(device.state)}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <div className="min-w-[13rem]">
                      <p className="font-medium text-anvil-100">
                        {device.model ?? t("devices.unknownModel")}
                      </p>
                      <p className="mt-1 text-xs text-anvil-400">
                        {[device.product, device.device]
                          .filter(Boolean)
                          .join(" / ") || t("devices.noProductMetadata")}
                      </p>
                    </div>
                  </TableCell>
                  <TableCell>
                    {device.transport_id != null ? (
                      <code className="font-mono text-xs">
                        {t("devices.transportId", {
                          id: device.transport_id,
                        })}
                      </code>
                    ) : (
                      <span className="text-anvil-500">
                        {t("common.notReported")}
                      </span>
                    )}
                  </TableCell>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </Card>
  );
}

function DeviceDetail({
  state,
  onRetry,
}: {
  state: DetailState;
  onRetry: (serial: string) => void;
}) {
  const { t } = useTranslation();

  if (state.kind === "idle") return null;

  if (state.kind === "loading") {
    return (
      <Card className="p-5">
        <h3 className="text-sm font-semibold text-anvil-50">
          {t("devices.loadingDeviceInfo")}
        </h3>
        <p className="mt-1 text-xs text-anvil-400">
          {t("devices.queryingSerial", { serial: state.serial })}
        </p>
        <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <div key={i}>
              <SkeletonLine className="w-20" />
              <SkeletonLine className="mt-2 w-36" />
            </div>
          ))}
        </div>
      </Card>
    );
  }

  if (state.kind === "error") {
    return (
      <StatePanel
        title={t("devices.deviceInfoFailed")}
        tone="danger"
        actions={
          <Button
            type="button"
            onClick={() => onRetry(state.serial)}
            variant="danger"
            size="sm"
          >
            {t("runtime.retry")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  const info = state.info;
  return (
    <Card className="p-5">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h3 className="text-lg font-semibold text-anvil-50">
            {info.model ?? info.serial}
          </h3>
          {info.manufacturer && (
            <p className="mt-1 text-sm text-anvil-400">{info.manufacturer}</p>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          {info.android_version && (
            <Badge tone="info">Android {info.android_version}</Badge>
          )}
          {info.sdk_level && (
            <Badge tone="neutral">
              {t("devices.apiLevel", { level: info.sdk_level })}
            </Badge>
          )}
        </div>
      </div>

      <dl className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <InfoField label={t("devices.serial")} value={info.serial} mono />
        {info.hardware_serial && (
          <InfoField
            label={t("devices.hwSerial")}
            value={info.hardware_serial}
            mono
          />
        )}
        {info.build_fingerprint && (
          <InfoField
            label={t("devices.buildFingerprint")}
            value={info.build_fingerprint}
            mono
            wrap
          />
        )}
        {info.security_patch && (
          <InfoField
            label={t("devices.securityPatch")}
            value={info.security_patch}
          />
        )}
        {info.wifi_ip && (
          <InfoField label={t("devices.wifiIp")} value={info.wifi_ip} mono />
        )}
        {info.battery && (
          <InfoField
            label={t("devices.battery")}
            value={formatBattery(info.battery)}
          />
        )}
        {info.storage && (
          <InfoField
            label={t("devices.storageData")}
            value={formatStorage(info.storage)}
          />
        )}
      </dl>
    </Card>
  );
}

function InfoField({
  label,
  value,
  mono = false,
  wrap = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
  wrap?: boolean;
}) {
  return (
    <div className="min-w-0">
      <dt className="text-xs font-medium text-anvil-500">{label}</dt>
      <dd
        className={[
          "mt-1 text-sm text-anvil-100",
          mono ? "font-mono text-xs" : "",
          wrap ? "break-all" : "truncate",
        ].join(" ")}
      >
        {value}
      </dd>
    </div>
  );
}

function formatBattery(b: NonNullable<DeviceInfo["battery"]>): string {
  const parts: string[] = [];
  if (b.level != null) parts.push(`${b.level}%`);
  if (b.status) parts.push(b.status);
  if (b.temperature != null) parts.push(`${b.temperature}°C`);
  return parts.join(" · ") || "Unknown";
}

function formatStorage(s: NonNullable<DeviceInfo["storage"]>): string {
  if (s.total_kb == null || s.available_kb == null) return "Unknown";
  const totalGb = (s.total_kb / 1048576).toFixed(1);
  const availGb = (s.available_kb / 1048576).toFixed(1);
  return `${availGb} GB free / ${totalGb} GB`;
}

function DeviceTableSkeleton() {
  const { t } = useTranslation();

  return (
    <Card
      className="overflow-hidden p-0"
      aria-label={t("devices.loadingDevices")}
    >
      <div className="border-b border-white/10 p-4">
        <SkeletonLine className="w-40" />
        <SkeletonLine className="mt-3 w-80 max-w-full" />
      </div>
      <div className="divide-y divide-white/10">
        {Array.from({ length: 4 }).map((_, index) => (
          <div
            key={index}
            className="grid gap-4 p-4 sm:grid-cols-[1.2fr_0.7fr_1.2fr_0.8fr]"
          >
            <SkeletonLine className="w-44" />
            <SkeletonLine className="w-24" />
            <div>
              <SkeletonLine className="w-36" />
              <SkeletonLine className="mt-2 w-48" />
            </div>
            <SkeletonLine className="w-28" />
          </div>
        ))}
      </div>
    </Card>
  );
}

function formatStateLabel(state: SerializedDeviceState): string {
  const label = summarizeState(state);
  return label.charAt(0).toUpperCase() + label.slice(1);
}

const REMOTE_BUTTONS: { label: string; keycode: number }[] = [
  { label: "Home", keycode: 3 },
  { label: "Back", keycode: 4 },
  { label: "Recents", keycode: 187 },
  { label: "Up", keycode: 19 },
  { label: "Down", keycode: 20 },
  { label: "Left", keycode: 21 },
  { label: "Right", keycode: 22 },
  { label: "OK", keycode: 23 },
  { label: "Vol +", keycode: 24 },
  { label: "Vol -", keycode: 25 },
  { label: "Power", keycode: 26 },
  { label: "Menu", keycode: 82 },
];

function DeviceControls({ serial }: { serial: string }) {
  const [lastKey, setLastKey] = useState<string | null>(null);
  const [screenshotMsg, setScreenshotMsg] = useState<string | null>(null);
  const [density, setDensity] = useState("");
  const [displayMsg, setDisplayMsg] = useState<string | null>(null);

  const sendKey = useCallback(
    async (keycode: number, label: string) => {
      try {
        await callShellRun(serial, ["input", "keyevent", String(keycode)]);
        setLastKey(label);
      } catch {
        setLastKey(`${label} failed`);
      }
    },
    [serial],
  );

  const takeScreenshot = useCallback(async () => {
    try {
      // The host destination comes from the native save dialog, never a
      // renderer-built path; the backend rejects non-absolute paths.
      const localPath = await save({
        title: "Save screenshot as",
        defaultPath: `screenshot-${serial}-${Date.now()}.png`,
        filters: [{ name: "PNG", extensions: ["png"] }],
      });
      if (!localPath) {
        setScreenshotMsg(null);
        return;
      }
      setScreenshotMsg("Capturing...");
      await callTakeScreenshot(serial, localPath);
      setScreenshotMsg(`Saved to ${localPath}`);
    } catch (e) {
      setScreenshotMsg(`Failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }, [serial]);

  const applyDensity = useCallback(async () => {
    if (!density.trim()) return;
    try {
      await callShellRun(serial, ["wm", "density", density.trim()]);
      setDisplayMsg(`Density set to ${density.trim()}`);
    } catch (e) {
      setDisplayMsg(`Failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }, [serial, density]);

  const resetDensity = useCallback(async () => {
    try {
      await callShellRun(serial, ["wm", "density", "reset"]);
      setDisplayMsg("Density reset to default");
    } catch (e) {
      setDisplayMsg(`Failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }, [serial]);

  const toggleForceDark = useCallback(
    async (enable: boolean) => {
      try {
        await callShellRun(serial, [
          "settings",
          "put",
          "secure",
          "ui_night_mode",
          enable ? "2" : "1",
        ]);
        setDisplayMsg(enable ? "Force dark enabled" : "Force dark disabled");
      } catch (e) {
        setDisplayMsg(`Failed: ${e instanceof Error ? e.message : String(e)}`);
      }
    },
    [serial],
  );

  return (
    <div className="space-y-4">
      <div className="grid gap-4 lg:grid-cols-[1fr_1fr]">
        <Card className="p-5">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold text-anvil-50">
              Virtual remote
            </h3>
            {lastKey && (
              <span className="text-xs text-anvil-400">Last: {lastKey}</span>
            )}
          </div>
          <div className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
            {REMOTE_BUTTONS.map((btn) => (
              <Button
                key={btn.label}
                type="button"
                size="sm"
                onClick={() => void sendKey(btn.keycode, btn.label)}
                title={`keyevent ${btn.keycode}`}
                className="justify-start"
              >
                <RemoteGlyph label={btn.label} />
                <span>{btn.label}</span>
              </Button>
            ))}
          </div>
        </Card>

        <div className="space-y-4">
          <Card className="p-5">
            <h3 className="text-sm font-semibold text-anvil-50">Screenshot</h3>
            <p className="mt-1 text-xs text-anvil-400">
              Capture the current screen to a local PNG file.
            </p>
            <div className="mt-3 flex items-center gap-3">
              <Button
                type="button"
                size="sm"
                variant="primary"
                onClick={() => void takeScreenshot()}
              >
                Capture
              </Button>
              {screenshotMsg && (
                <span className="text-xs text-anvil-300">{screenshotMsg}</span>
              )}
            </div>
          </Card>

          <Card className="p-5">
            <h3 className="text-sm font-semibold text-anvil-50">
              Display tuning
            </h3>
            <div className="mt-3 grid gap-3 sm:grid-cols-2">
              <div className="flex items-end gap-2">
                <label className="grid flex-1 gap-1.5">
                  <span className="text-xs font-medium text-anvil-400">
                    Density (DPI)
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
                  Set
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => void resetDensity()}
                >
                  Reset
                </Button>
              </div>
              <div className="flex items-end gap-2">
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void toggleForceDark(true)}
                >
                  Force dark on
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => void toggleForceDark(false)}
                >
                  Force dark off
                </Button>
              </div>
            </div>
            {displayMsg && (
              <p className="mt-3 text-xs text-anvil-300">{displayMsg}</p>
            )}
          </Card>
        </div>
      </div>
      <ProcessManager serial={serial} />
      <FileManager serial={serial} />
      <NetworkInspector serial={serial} />
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

function ProcessManager({ serial }: { serial: string }) {
  const [processes, setProcesses] = useState<ProcessInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [sortBy, setSortBy] = useState<"rss" | "name">("rss");

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const procs = await callListProcesses(serial);
      setProcesses(procs);
    } catch {
      setProcesses([]);
    } finally {
      setLoading(false);
    }
  }, [serial]);

  const filtered = processes
    .filter((p) =>
      search ? p.name.toLowerCase().includes(search.toLowerCase()) : true,
    )
    .sort((a, b) =>
      sortBy === "rss" ? b.rss_kb - a.rss_kb : a.name.localeCompare(b.name),
    );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            Process manager
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            Snapshot of running processes sorted by memory usage.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <FieldInput
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter"
            aria-label="Filter processes"
            className="h-8 w-40 px-2 font-mono text-xs"
          />
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => void refresh()}
            disabled={loading}
          >
            {loading ? "Loading..." : processes.length > 0 ? "Refresh" : "Load"}
          </Button>
        </div>
      </div>
      {processes.length === 0 && !loading && (
        <EmptyState title="No process snapshot loaded">
          <p>Load a process snapshot to inspect memory-heavy apps.</p>
        </EmptyState>
      )}
      {processes.length > 0 && filtered.length === 0 && (
        <EmptyState title="No matching processes">
          <p>Clear the filter or search for another process name.</p>
        </EmptyState>
      )}
      {processes.length > 0 && filtered.length > 0 && (
        <div className="overflow-x-auto" style={{ maxHeight: "24rem" }}>
          <table className="min-w-full text-xs">
            <thead className="sticky top-0 bg-anvil-900">
              <tr>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  PID
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  User
                </th>
                <th
                  className="cursor-pointer px-3 py-2 text-right font-semibold text-anvil-400"
                  onClick={() => setSortBy("rss")}
                >
                  RSS {sortBy === "rss" ? "(sorted)" : ""}
                </th>
                <th
                  className="cursor-pointer px-3 py-2 text-left font-semibold text-anvil-400"
                  onClick={() => setSortBy("name")}
                >
                  Name {sortBy === "name" ? "(sorted)" : ""}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {filtered.slice(0, 100).map((p, i) => (
                <tr
                  key={`${p.pid}-${p.name}-${i}`}
                  className="hover:bg-white/[0.03]"
                >
                  <td className="px-3 py-1.5 font-mono text-anvil-300">
                    {p.pid}
                  </td>
                  <td className="px-3 py-1.5 text-anvil-400">{p.user}</td>
                  <td className="px-3 py-1.5 text-right font-mono text-anvil-200">
                    {formatKb(p.rss_kb)}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    <span>{p.name}</span>
                    {p.parse_error && (
                      <Badge tone="warning" className="ml-2">
                        Parse issue
                      </Badge>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {filtered.length > 100 && (
            <p className="px-3 py-2 text-xs text-anvil-500">
              Showing 100 of {filtered.length} processes
            </p>
          )}
        </div>
      )}
    </Card>
  );
}

function formatKb(kb: number): string {
  if (kb >= 1048576) return `${(kb / 1048576).toFixed(1)} GB`;
  if (kb >= 1024) return `${(kb / 1024).toFixed(1)} MB`;
  return `${kb} KB`;
}

function formatBytes(bytes: number | null): string {
  if (bytes == null) return "Unknown";
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`;
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function FileManager({ serial }: { serial: string }) {
  const [listing, setListing] = useState<RemoteListing | null>(null);
  const [currentPath, setCurrentPath] = useState("/sdcard");
  const [loading, setLoading] = useState(false);
  const [pullMsg, setPullMsg] = useState<string | null>(null);

  const browse = useCallback(
    async (path: string) => {
      setLoading(true);
      setPullMsg(null);
      try {
        const result = await callListRemoteFiles(serial, path);
        setListing(result);
        setCurrentPath(path);
      } catch {
        setListing(null);
      } finally {
        setLoading(false);
      }
    },
    [serial],
  );

  const navigateUp = useCallback(() => {
    const parent = currentPath.replace(/\/[^/]+\/?$/, "") || "/";
    void browse(parent);
  }, [currentPath, browse]);

  const pullRemote = useCallback(
    async (entry: RemoteFileEntry) => {
      try {
        // Destination is chosen through the native save dialog so the
        // renderer never dictates an arbitrary host path.
        const localPath = await save({
          title: `Save ${entry.name} as`,
          defaultPath: entry.name,
        });
        if (!localPath) {
          setPullMsg(null);
          return;
        }
        setPullMsg(`Pulling ${entry.name}...`);
        const remoteFull =
          currentPath === "/"
            ? `/${entry.name}`
            : `${currentPath}/${entry.name}`;
        await callPullFile(serial, remoteFull, localPath);
        setPullMsg(`Saved ${entry.name} to ${localPath}`);
      } catch (e) {
        setPullMsg(`Failed: ${e instanceof Error ? e.message : String(e)}`);
      }
    },
    [serial, currentPath],
  );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">File manager</h3>
          <p className="mt-1 text-xs text-anvil-400">
            Browse, pull, and inspect files on the device.
          </p>
        </div>
        <div className="flex items-center gap-2">
          {listing?.free_space_kb != null && (
            <Badge tone="neutral">{formatKb(listing.free_space_kb)} free</Badge>
          )}
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => void browse(currentPath)}
            disabled={loading}
          >
            {loading ? "Loading..." : listing ? "Refresh" : "Browse"}
          </Button>
        </div>
      </div>

      {!listing && !loading && (
        <EmptyState title="No directory loaded">
          <p>
            Browse <code>/sdcard</code> to inspect device files and pull local
            copies.
          </p>
        </EmptyState>
      )}

      {listing && (
        <>
          <div className="flex items-center gap-2 border-b border-white/10 bg-white/[0.02] px-4 py-2">
            <Button
              type="button"
              size="sm"
              variant="ghost"
              onClick={navigateUp}
              disabled={currentPath === "/"}
            >
              ..
            </Button>
            <code className="flex-1 truncate font-mono text-xs text-anvil-200">
              {currentPath}
            </code>
          </div>
          <div
            className="divide-y divide-white/5 overflow-y-auto"
            style={{ maxHeight: "20rem" }}
          >
            {listing.entries.length === 0 && (
              <EmptyState title="Empty directory" className="border-t-0">
                <p>This path has no visible entries.</p>
              </EmptyState>
            )}
            {listing.entries.map((entry, index) => (
              <div
                key={`${entry.name}-${index}`}
                className="flex items-center gap-3 px-4 py-2 text-xs hover:bg-white/[0.03]"
              >
                <FileGlyph directory={entry.is_dir} />
                {entry.is_dir ? (
                  <button
                    type="button"
                    className="min-w-0 flex-1 truncate text-left font-mono text-circuit-200 hover:underline"
                    onClick={() =>
                      void browse(
                        currentPath === "/"
                          ? `/${entry.name}`
                          : `${currentPath}/${entry.name}`,
                      )
                    }
                  >
                    {entry.name}/
                  </button>
                ) : (
                  <span className="min-w-0 flex-1 truncate font-mono text-anvil-100">
                    {entry.name}
                  </span>
                )}
                <span className="shrink-0 font-mono text-anvil-500">
                  {entry.is_dir ? "" : formatBytes(entry.size)}
                </span>
                <span className="hidden shrink-0 font-mono text-anvil-600 sm:inline">
                  {entry.permissions}
                </span>
                {entry.parse_error && (
                  <Badge tone="warning" className="shrink-0">
                    Parse issue
                  </Badge>
                )}
                {!entry.is_dir && (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    onClick={() => void pullRemote(entry)}
                  >
                    Pull
                  </Button>
                )}
              </div>
            ))}
          </div>
          {pullMsg && (
            <div className="border-t border-white/10 px-4 py-2">
              <p className="text-xs text-anvil-300">{pullMsg}</p>
            </div>
          )}
        </>
      )}
    </Card>
  );
}

function FileGlyph({ directory }: { directory: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4 shrink-0 text-anvil-400"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.8"
      aria-hidden="true"
    >
      {directory ? (
        <>
          <path d="M3.5 6.5h6l2 2H20a1.5 1.5 0 0 1 1.5 1.5v7.5A1.5 1.5 0 0 1 20 19H4a1.5 1.5 0 0 1-1.5-1.5V8A1.5 1.5 0 0 1 4 6.5Z" />
          <path d="M3.5 10h18" />
        </>
      ) : (
        <>
          <path d="M7 3.5h7l3 3V20a.5.5 0 0 1-.5.5h-9A.5.5 0 0 1 7 20V3.5Z" />
          <path d="M14 3.5v3h3" />
          <path d="M9.5 11h5M9.5 14h5" />
        </>
      )}
    </svg>
  );
}

function NetworkInspector({ serial }: { serial: string }) {
  const [connections, setConnections] = useState<NetworkConnection[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const conns = await callListNetworkConnections(serial);
      setConnections(conns);
    } catch {
      setConnections([]);
    } finally {
      setLoading(false);
    }
  }, [serial]);

  const filtered = connections.filter((c) =>
    search
      ? c.local_addr.includes(search) ||
        c.remote_addr.includes(search) ||
        (c.process?.includes(search) ?? false) ||
        c.state.toLowerCase().includes(search.toLowerCase())
      : true,
  );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            Network connections
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            Active TCP/UDP connections via <code>ss -tunp</code>.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <FieldInput
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter"
            aria-label="Filter network connections"
            className="h-8 w-40 px-2 font-mono text-xs"
          />
          <Button
            type="button"
            size="sm"
            variant="primary"
            onClick={() => void refresh()}
            disabled={loading}
          >
            {loading
              ? "Loading..."
              : connections.length > 0
                ? "Refresh"
                : "Load"}
          </Button>
        </div>
      </div>
      {connections.length === 0 && !loading && (
        <EmptyState title="No network snapshot loaded">
          <p>Load active sockets to review endpoints and owning processes.</p>
        </EmptyState>
      )}
      {connections.length > 0 && filtered.length === 0 && (
        <EmptyState title="No matching connections">
          <p>
            Clear the filter or search for another address, process, or state.
          </p>
        </EmptyState>
      )}
      {connections.length > 0 && filtered.length > 0 && (
        <div className="overflow-x-auto" style={{ maxHeight: "20rem" }}>
          <table className="min-w-full text-xs">
            <thead className="sticky top-0 bg-anvil-900">
              <tr>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  Proto
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  State
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  Local
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  Remote
                </th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">
                  Process
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {filtered.slice(0, 100).map((c, i) => (
                <tr key={i} className="hover:bg-white/[0.03]">
                  <td className="px-3 py-1.5 font-mono text-anvil-300">
                    {c.protocol}
                  </td>
                  <td className="px-3 py-1.5 text-anvil-200">{c.state}</td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    {c.local_addr}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    {c.remote_addr}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-400">
                    {c.process ?? "Not reported"}
                    {c.parse_error && (
                      <Badge tone="warning" className="ml-2">
                        Parse issue
                      </Badge>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {filtered.length > 100 && (
            <p className="px-3 py-2 text-xs text-anvil-500">
              Showing 100 of {filtered.length} connections
            </p>
          )}
        </div>
      )}
    </Card>
  );
}

function AuthorizePrompt({
  devices,
  onRefresh,
}: {
  devices: ListDevicesResult["devices"];
  onRefresh: () => void;
}) {
  const { t } = useTranslation();

  return (
    <Card className="mt-4 border-amber-300/20 bg-amber-950/20 p-5">
      <div className="flex gap-4">
        <span
          className="mt-1 h-2.5 w-2.5 shrink-0 rounded-sm bg-amber-300 ring-4 ring-amber-300/10"
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-anvil-50">
            {devices.length === 1
              ? t("devices.authorize")
              : t("devices.authorizeMultiple", { count: devices.length })}
          </h3>
          <div className="mt-3 text-sm leading-6 text-anvil-300">
            <p>
              {devices.length === 1
                ? t("devices.authorizeOneBody", {
                    serial: devices[0]!.serial,
                  })
                : t("devices.authorizeManyBody")}
            </p>
            {devices.length > 1 && (
              <ul className="mt-2 space-y-1">
                {devices.map((d) => (
                  <li key={d.serial}>
                    <code className="font-mono text-xs text-anvil-100">
                      {d.serial}
                    </code>
                    {d.model && (
                      <span className="ml-2 text-xs text-anvil-400">
                        ({d.model})
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
            <div className="mt-4">
              <p className="text-xs font-semibold text-anvil-200">
                {t("devices.authorizeSteps")}
              </p>
              <ol className="mt-2 grid gap-2 text-sm sm:grid-cols-3">
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">1.</span>{" "}
                  {t("devices.step1")}
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">2.</span>{" "}
                  {t("devices.step2")}
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">3.</span>{" "}
                  {t("devices.step3")}
                </li>
              </ol>
            </div>
            <div className="mt-4 rounded-md border border-white/10 bg-white/[0.04] p-3">
              <p className="text-xs font-medium text-anvil-400">
                {t("devices.noDialog")}
              </p>
              <ul className="mt-2 space-y-1 text-xs text-anvil-400">
                <li>
                  {t("devices.revokeAuthorizations")}{" "}
                  <code className="text-anvil-200">
                    Settings → Developer options → Revoke USB debugging
                    authorizations
                  </code>
                </li>
                <li>{t("devices.reconnectUsb")}</li>
                <li>{t("devices.fileTransferMode")}</li>
              </ul>
            </div>
          </div>
          <div className="mt-4">
            <Button
              type="button"
              size="sm"
              variant="primary"
              onClick={onRefresh}
            >
              {t("devices.refresh")}
            </Button>
          </div>
        </div>
      </div>
    </Card>
  );
}

function deviceStateTone(
  state: SerializedDeviceState,
): "neutral" | "info" | "success" | "warning" | "danger" {
  if (typeof state !== "string") {
    return "neutral";
  }

  if (state === "device") {
    return "success";
  }

  if (state === "bootloader" || state === "recovery" || state === "sideload") {
    return "info";
  }

  if (state === "unauthorized" || state === "offline") {
    return "warning";
  }

  if (state === "no_permissions") {
    return "danger";
  }

  return "neutral";
}
