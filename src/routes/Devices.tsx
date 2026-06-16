import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";

import {
  callGetDeviceInfo,
  callListDevices,
  inTauri,
  summarizeState,
  type DeviceInfo,
  type ListDevicesResult,
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
      </section>

      {detail.kind !== "idle" && (
        <section className="mt-4 max-w-6xl" aria-live="polite">
          <DeviceDetail
            state={detail}
            onRetry={(serial) => void selectDevice(serial)}
          />
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
