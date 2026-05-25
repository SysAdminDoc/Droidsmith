import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";

import {
  callListDevices,
  inTauri,
  summarizeState,
  type ListDevicesResult,
} from "../lib/tauri";

import { Card, PaneHeader } from "./common";

type State =
  | { kind: "loading" }
  | { kind: "no_tauri" }
  | { kind: "ok"; value: ListDevicesResult }
  | { kind: "error"; message: string };

export default function DevicesRoute() {
  const [state, setState] = useState<State>({ kind: "loading" });

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

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <>
      <PaneHeader
        title="Devices"
        milestone="R-012"
        description="Connected devices via USB and TCP/IP. Wireless pairing wizard ships with R-015."
      />

      <div className="mt-4 flex items-center gap-2">
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={state.kind === "loading"}
          className="rounded border border-anvil-700 bg-anvil-800 px-3 py-1.5 text-sm text-anvil-50 hover:bg-anvil-700 focus:outline-none focus-visible:ring-2 focus-visible:ring-anvil-300 disabled:opacity-50"
        >
          {state.kind === "loading" ? "Refreshing…" : "Refresh"}
        </button>
        {state.kind === "ok" && state.value.adb_path && (
          <p className="text-xs text-anvil-300">
            via <code className="font-mono">{state.value.adb_path}</code>
          </p>
        )}
      </div>

      <section className="mt-6 max-w-3xl" aria-live="polite">
        {state.kind === "no_tauri" && (
          <Card>
            <p className="text-sm text-anvil-200">
              Tauri runtime not detected. The Devices pane needs the app's IPC
              bridge. Run <code className="font-mono">npm run tauri:dev</code>{" "}
              instead of plain <code className="font-mono">npm run dev</code>.
            </p>
          </Card>
        )}

        {state.kind === "loading" && (
          <Card>
            <p role="status" className="text-sm text-anvil-200">
              Loading device list…
            </p>
          </Card>
        )}

        {state.kind === "error" && (
          <Card>
            <p role="alert" className="text-sm text-red-300">
              Failed to list devices: {state.message}
            </p>
            <button
              type="button"
              onClick={() => void refresh()}
              className="mt-3 rounded border border-anvil-700 bg-anvil-800 px-2 py-1 text-xs text-anvil-50 hover:bg-anvil-700 focus:outline-none focus-visible:ring-2 focus-visible:ring-anvil-300"
            >
              Retry
            </button>
          </Card>
        )}

        {state.kind === "ok" && !state.value.adb_resolved && (
          <Card>
            <p className="text-sm text-anvil-200">
              <strong>adb binary not found.</strong> Droidsmith looks on{" "}
              <code className="font-mono">$PATH</code>, then{" "}
              <code className="font-mono">$ANDROID_HOME</code>, then the Android
              Studio default install path, then Homebrew (macOS) / the distro
              package manager (Linux). The bundled sidecar fallback lands in
              R-010 — see{" "}
              <code className="font-mono">scripts/fetch-platform-tools.*</code>.
            </p>
          </Card>
        )}

        {state.kind === "ok" &&
          state.value.adb_resolved &&
          state.value.devices.length === 0 && (
            <Card>
              <p className="text-sm text-anvil-200">
                No devices connected. Plug in a USB cable and tap{" "}
                <em>Allow USB debugging</em> on the device, or pair over Wi-Fi
                (R-015 ships the pairing wizard).
              </p>
            </Card>
          )}

        {state.kind === "ok" && state.value.devices.length > 0 && (
          <DeviceTable devices={state.value.devices} />
        )}
      </section>
    </>
  );
}

function DeviceTable({ devices }: { devices: ListDevicesResult["devices"] }) {
  return (
    <div className="overflow-x-auto rounded-lg border border-anvil-800">
      <table className="min-w-full divide-y divide-anvil-800 text-sm">
        <thead className="bg-anvil-900">
          <tr>
            <Th>Serial</Th>
            <Th>State</Th>
            <Th>Model</Th>
            <Th>Transport</Th>
          </tr>
        </thead>
        <tbody className="divide-y divide-anvil-800">
          {devices.map((d) => (
            <tr key={d.serial} className="bg-anvil-950/50">
              <Td>
                <code className="font-mono text-xs text-anvil-50">
                  {d.serial}
                </code>
                {d.wireless && (
                  <span className="ml-2 rounded bg-blue-900/40 px-1.5 py-0.5 text-[10px] uppercase text-blue-200">
                    wireless
                  </span>
                )}
              </Td>
              <Td>{summarizeState(d.state)}</Td>
              <Td>{d.model ?? "—"}</Td>
              <Td>{d.transport_id != null ? `id ${d.transport_id}` : "—"}</Td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Th({ children }: { children: ReactNode }) {
  return (
    <th className="px-3 py-2 text-left text-[11px] font-semibold uppercase tracking-wide text-anvil-300">
      {children}
    </th>
  );
}

function Td({ children }: { children: ReactNode }) {
  return <td className="px-3 py-2 text-anvil-100">{children}</td>;
}
