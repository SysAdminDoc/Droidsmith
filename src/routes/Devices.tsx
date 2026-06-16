import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";

import {
  callGetDeviceInfo,
  callListDevices,
  callListProcesses,
  callShellRun,
  callTakeScreenshot,
  inTauri,
  summarizeState,
  type DeviceInfo,
  type ListDevicesResult,
  type ProcessInfo,
  type SerializedDeviceState,
} from "../lib/tauri";

import {
  Badge,
  Button,
  Card,
  PaneHeader,
  SkeletonLine,
  StatePanel,
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
        title="Devices"
        milestone="R-012"
        description="Scan USB and TCP/IP targets, confirm ADB readiness, and see enough context to choose the right device before any action runs."
        actions={
          <Button
            type="button"
            onClick={() => void refresh()}
            disabled={state.kind === "loading"}
            variant="primary"
          >
            {state.kind === "loading" ? "Scanning..." : "Refresh devices"}
          </Button>
        }
        meta={<DeviceHeaderMeta state={state} />}
      />

      <section className="mt-6 max-w-6xl" aria-live="polite">
        {state.kind === "no_tauri" && (
          <StatePanel
            title="Launch the desktop shell to scan hardware"
            tone="info"
            actions={
              <Button type="button" onClick={() => void refresh()} size="sm">
                Check again
              </Button>
            }
          >
            <p>
              This browser preview can render the interface, but device IPC only
              exists inside Tauri. Start the desktop shell with{" "}
              <code>npm run tauri:dev</code> when you want Droidsmith to talk to
              ADB.
            </p>
          </StatePanel>
        )}

        {state.kind === "loading" && <DeviceTableSkeleton />}

        {state.kind === "error" && (
          <StatePanel
            title="Device scan did not complete"
            tone="danger"
            actions={
              <Button
                type="button"
                onClick={() => void refresh()}
                variant="danger"
                size="sm"
              >
                Retry scan
              </Button>
            }
          >
            <p>{state.message}</p>
          </StatePanel>
        )}

        {state.kind === "ok" && !state.value.adb_resolved && (
          <StatePanel title="ADB is not available yet" tone="warning">
            <p>
              Droidsmith checked <code>$PATH</code>, <code>$ANDROID_HOME</code>,
              Android Studio defaults, Homebrew, and common Linux
              package-manager locations. Install Android platform tools or run{" "}
              <code>scripts/fetch-platform-tools.*</code> when the bundled
              sidecar lands.
            </p>
          </StatePanel>
        )}

        {state.kind === "ok" &&
          state.value.adb_resolved &&
          state.value.devices.length === 0 && (
            <StatePanel
              title="No Android devices found"
              tone="info"
              actions={
                <Button
                  type="button"
                  onClick={() => void refresh()}
                  variant="secondary"
                  size="sm"
                >
                  Scan again
                </Button>
              }
            >
              <ol className="grid gap-2 text-sm sm:grid-cols-3">
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  Connect USB and enable developer options.
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  Accept the <em>Allow USB debugging</em> prompt on-device.
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  Or use the Wireless tab for Wi-Fi pairing.
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
            (d) =>
              typeof d.state === "string" && d.state === "unauthorized",
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
            (d) =>
              typeof d.state === "string" && d.state === "no_permissions",
          ) && (
            <StatePanel title="Linux permissions issue" tone="danger">
              <p>
                One or more devices report "no permissions". On Linux, add a udev
                rule for your device's USB vendor ID:
              </p>
              <pre className="mt-2 rounded-md border border-white/10 bg-white/[0.04] p-3 font-mono text-xs text-anvil-200">
                {`echo 'SUBSYSTEM=="usb", ATTR{idVendor}=="XXXX", MODE="0666"' | sudo tee /etc/udev/rules.d/51-android.rules\nsudo udevadm control --reload-rules && sudo udevadm trigger`}
              </pre>
              <p className="mt-2">
                Replace <code>XXXX</code> with your device's vendor ID (e.g.{" "}
                <code>18d1</code> for Google,{" "}
                <code>04e8</code> for Samsung). Then unplug and reconnect.
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
  if (state.kind === "loading") {
    return (
      <div className="flex flex-wrap gap-2">
        <Badge tone="info">Scanning bridge</Badge>
        <Badge tone="neutral">Waiting for ADB</Badge>
      </div>
    );
  }

  if (state.kind === "no_tauri") {
    return (
      <div className="flex flex-wrap gap-2">
        <Badge tone="neutral">Browser preview</Badge>
        <Badge tone="info">Tauri IPC required</Badge>
      </div>
    );
  }

  if (state.kind === "error") {
    return <Badge tone="danger">Scan failed</Badge>;
  }

  if (!state.value.adb_resolved) {
    return <Badge tone="warning">ADB missing</Badge>;
  }

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-2">
      <Badge tone="success">ADB resolved</Badge>
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
  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            Connected devices
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            Select a device to view its dashboard. Review state and transport
            before launching any action.
          </p>
        </div>
        <Badge tone="success">
          {devices.length} {devices.length === 1 ? "device" : "devices"}
        </Badge>
      </div>
      <div className="overflow-x-auto">
        <table className="min-w-full text-sm">
          <thead className="bg-white/[0.04]">
            <tr>
              <Th>Serial</Th>
              <Th>State</Th>
              <Th>Identity</Th>
              <Th>Transport</Th>
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
                  onClick={isDevice ? () => onSelect(device.serial) : undefined}
                  className={[
                    "transition",
                    isDevice
                      ? "cursor-pointer hover:bg-white/[0.055]"
                      : "bg-anvil-950/20",
                    isSelected ? "bg-circuit-300/[0.06]" : "",
                  ].join(" ")}
                >
                  <Td>
                    <div className="flex min-w-[13rem] items-center gap-2">
                      <code className="font-mono text-xs text-anvil-50">
                        {device.serial}
                      </code>
                      {device.wireless && <Badge tone="info">Wi-Fi</Badge>}
                    </div>
                  </Td>
                  <Td>
                    <Badge tone={deviceStateTone(device.state)}>
                      {formatStateLabel(device.state)}
                    </Badge>
                  </Td>
                  <Td>
                    <div className="min-w-[13rem]">
                      <p className="font-medium text-anvil-100">
                        {device.model ?? "Unknown model"}
                      </p>
                      <p className="mt-1 text-xs text-anvil-400">
                        {[device.product, device.device]
                          .filter(Boolean)
                          .join(" / ") || "No product metadata"}
                      </p>
                    </div>
                  </Td>
                  <Td>
                    {device.transport_id != null ? (
                      <code className="font-mono text-xs">
                        transport {device.transport_id}
                      </code>
                    ) : (
                      <span className="text-anvil-500">Not reported</span>
                    )}
                  </Td>
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
  if (state.kind === "idle") return null;

  if (state.kind === "loading") {
    return (
      <Card className="p-5">
        <h3 className="text-sm font-semibold text-anvil-50">
          Loading device info
        </h3>
        <p className="mt-1 text-xs text-anvil-400">
          Querying {state.serial}...
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
        title="Could not load device info"
        tone="danger"
        actions={
          <Button
            type="button"
            onClick={() => onRetry(state.serial)}
            variant="danger"
            size="sm"
          >
            Retry
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
            <Badge tone="neutral">API {info.sdk_level}</Badge>
          )}
        </div>
      </div>

      <dl className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <InfoField label="Serial" value={info.serial} mono />
        {info.hardware_serial && (
          <InfoField label="HW Serial" value={info.hardware_serial} mono />
        )}
        {info.build_fingerprint && (
          <InfoField
            label="Build fingerprint"
            value={info.build_fingerprint}
            mono
            wrap
          />
        )}
        {info.security_patch && (
          <InfoField label="Security patch" value={info.security_patch} />
        )}
        {info.wifi_ip && (
          <InfoField label="Wi-Fi IP" value={info.wifi_ip} mono />
        )}
        {info.battery && (
          <InfoField
            label="Battery"
            value={formatBattery(info.battery)}
          />
        )}
        {info.storage && (
          <InfoField
            label="Storage (/data)"
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
  return (
    <Card className="overflow-hidden p-0" aria-label="Loading devices">
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

function Th({ children }: { children: ReactNode }) {
  return (
    <th className="px-4 py-3 text-left text-[11px] font-semibold uppercase tracking-[0.08em] text-anvil-400">
      {children}
    </th>
  );
}

function Td({ children }: { children: ReactNode }) {
  return <td className="px-4 py-4 align-middle text-anvil-200">{children}</td>;
}

function formatStateLabel(state: SerializedDeviceState): string {
  const label = summarizeState(state);
  return label.charAt(0).toUpperCase() + label.slice(1);
}

const REMOTE_BUTTONS: { label: string; keycode: number; icon: string }[] = [
  { label: "Home", keycode: 3, icon: "⌂" },
  { label: "Back", keycode: 4, icon: "←" },
  { label: "Recents", keycode: 187, icon: "⊞" },
  { label: "Up", keycode: 19, icon: "▲" },
  { label: "Down", keycode: 20, icon: "▼" },
  { label: "Left", keycode: 21, icon: "◀" },
  { label: "Right", keycode: 22, icon: "▶" },
  { label: "OK", keycode: 23, icon: "●" },
  { label: "Vol +", keycode: 24, icon: "🔊" },
  { label: "Vol -", keycode: 25, icon: "🔉" },
  { label: "Power", keycode: 26, icon: "⏻" },
  { label: "Menu", keycode: 82, icon: "☰" },
];

function DeviceControls({ serial }: { serial: string }) {
  const [lastKey, setLastKey] = useState<string | null>(null);
  const [screenshotMsg, setScreenshotMsg] = useState<string | null>(null);
  const [density, setDensity] = useState("");
  const [displayMsg, setDisplayMsg] = useState<string | null>(null);

  const sendKey = useCallback(
    async (keycode: number, label: string) => {
      try {
        await callShellRun(serial, [
          "input",
          "keyevent",
          String(keycode),
        ]);
        setLastKey(label);
      } catch {
        setLastKey(`${label} failed`);
      }
    },
    [serial],
  );

  const takeScreenshot = useCallback(async () => {
    setScreenshotMsg("Capturing...");
    try {
      const ts = Date.now();
      const localPath = `screenshot-${serial}-${ts}.png`;
      await callTakeScreenshot(serial, localPath);
      setScreenshotMsg(`Saved to ${localPath}`);
    } catch (e) {
      setScreenshotMsg(
        `Failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  }, [serial]);

  const applyDensity = useCallback(async () => {
    if (!density.trim()) return;
    try {
      await callShellRun(serial, ["wm", "density", density.trim()]);
      setDisplayMsg(`Density set to ${density.trim()}`);
    } catch (e) {
      setDisplayMsg(
        `Failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  }, [serial, density]);

  const resetDensity = useCallback(async () => {
    try {
      await callShellRun(serial, ["wm", "density", "reset"]);
      setDisplayMsg("Density reset to default");
    } catch (e) {
      setDisplayMsg(
        `Failed: ${e instanceof Error ? e.message : String(e)}`,
      );
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
        setDisplayMsg(
          `Failed: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
    },
    [serial],
  );

  return (
    <div className="space-y-4">
    <div className="grid gap-4 lg:grid-cols-[1fr_1fr]">
      <Card className="p-5">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold text-anvil-50">Virtual remote</h3>
          {lastKey && (
            <span className="text-xs text-anvil-400">Last: {lastKey}</span>
          )}
        </div>
        <div className="mt-4 grid grid-cols-4 gap-2">
          {REMOTE_BUTTONS.map((btn) => (
            <Button
              key={btn.label}
              type="button"
              size="sm"
              onClick={() => void sendKey(btn.keycode, btn.label)}
              title={`keyevent ${btn.keycode}`}
            >
              <span className="mr-1">{btn.icon}</span> {btn.label}
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
          <h3 className="text-sm font-semibold text-anvil-50">Display tuning</h3>
          <div className="mt-3 grid gap-3 sm:grid-cols-2">
            <div className="flex items-end gap-2">
              <label className="grid flex-1 gap-1.5">
                <span className="text-xs font-medium text-anvil-400">
                  Density (DPI)
                </span>
                <input
                  type="text"
                  value={density}
                  onChange={(e) => setDensity(e.target.value)}
                  placeholder="420"
                  inputMode="numeric"
                  className="h-9 rounded-md border border-white/10 bg-white/[0.06] px-3 font-mono text-sm text-anvil-50 outline-none transition placeholder:text-anvil-600 focus:border-circuit-300/50 focus:ring-2 focus:ring-circuit-300/20"
                />
              </label>
              <Button type="button" size="sm" onClick={() => void applyDensity()}>
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
    </div>
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
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter..."
            aria-label="Filter processes"
            className="h-8 w-40 rounded-md border border-white/10 bg-white/[0.06] px-2 font-mono text-xs text-anvil-50 outline-none placeholder:text-anvil-600 focus:border-circuit-300/50"
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
      {processes.length > 0 && (
        <div className="overflow-x-auto" style={{ maxHeight: "24rem" }}>
          <table className="min-w-full text-xs">
            <thead className="sticky top-0 bg-anvil-900">
              <tr>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">PID</th>
                <th className="px-3 py-2 text-left font-semibold text-anvil-400">User</th>
                <th
                  className="cursor-pointer px-3 py-2 text-right font-semibold text-anvil-400"
                  onClick={() => setSortBy("rss")}
                >
                  RSS {sortBy === "rss" ? "▼" : ""}
                </th>
                <th
                  className="cursor-pointer px-3 py-2 text-left font-semibold text-anvil-400"
                  onClick={() => setSortBy("name")}
                >
                  Name {sortBy === "name" ? "▼" : ""}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {filtered.slice(0, 100).map((p) => (
                <tr key={p.pid} className="hover:bg-white/[0.03]">
                  <td className="px-3 py-1.5 font-mono text-anvil-300">
                    {p.pid}
                  </td>
                  <td className="px-3 py-1.5 text-anvil-400">{p.user}</td>
                  <td className="px-3 py-1.5 text-right font-mono text-anvil-200">
                    {formatKb(p.rss_kb)}
                  </td>
                  <td className="px-3 py-1.5 font-mono text-anvil-100">
                    {p.name}
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

function AuthorizePrompt({
  devices,
  onRefresh,
}: {
  devices: ListDevicesResult["devices"];
  onRefresh: () => void;
}) {
  return (
    <Card className="mt-4 border-amber-300/20 bg-amber-950/20 p-5">
      <div className="flex gap-4">
        <span
          className="mt-1 h-2.5 w-2.5 shrink-0 rounded-full bg-amber-300 ring-4 ring-amber-300/10"
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-anvil-50">
            {devices.length === 1
              ? "Device waiting for authorization"
              : `${devices.length} devices waiting for authorization`}
          </h3>
          <div className="mt-3 text-sm leading-6 text-anvil-300">
            <p>
              {devices.length === 1
                ? `Device ${devices[0]!.serial} is connected but hasn't been authorized yet.`
                : "The following devices are connected but haven't been authorized yet:"}
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
                To authorize:
              </p>
              <ol className="mt-2 grid gap-2 text-sm sm:grid-cols-3">
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">1.</span> Look
                  at the device screen for the{" "}
                  <em>Allow USB debugging?</em> dialog.
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">2.</span>{" "}
                  Optionally check{" "}
                  <em>Always allow from this computer</em> to skip this next
                  time.
                </li>
                <li className="rounded-md border border-white/10 bg-white/[0.04] p-3">
                  <span className="font-semibold text-anvil-100">3.</span> Tap{" "}
                  <em>Allow</em>, then click Refresh below.
                </li>
              </ol>
            </div>
            <div className="mt-4 rounded-md border border-white/10 bg-white/[0.04] p-3">
              <p className="text-xs font-medium text-anvil-400">
                No dialog on-device?
              </p>
              <ul className="mt-2 space-y-1 text-xs text-anvil-400">
                <li>
                  Revoke existing authorizations:{" "}
                  <code className="text-anvil-200">
                    Settings → Developer options → Revoke USB debugging
                    authorizations
                  </code>
                </li>
                <li>Reconnect the USB cable and wait a few seconds.</li>
                <li>
                  Ensure USB mode is set to{" "}
                  <em>File Transfer / MTP</em> (some devices block debugging in
                  charge-only mode).
                </li>
              </ul>
            </div>
          </div>
          <div className="mt-4">
            <Button type="button" size="sm" variant="primary" onClick={onRefresh}>
              Refresh devices
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
