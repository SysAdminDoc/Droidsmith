import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import qrcode from "qrcode-generator";

import {
  callConnectWireless,
  callListWirelessServices,
  callPairWireless,
  inTauri,
  type ListWirelessServicesResult,
  type WirelessAdbService,
  type WirelessCommandResult,
} from "../lib/tauri";

import {
  Badge,
  Button,
  Card,
  FieldInput,
  PaneHeader,
  SkeletonLine,
  StatePanel,
} from "./common";

type ServicesState =
  | { kind: "loading" }
  | { kind: "no_tauri" }
  | { kind: "ok"; value: ListWirelessServicesResult }
  | { kind: "error"; message: string };

type ActionState =
  | { kind: "idle" }
  | { kind: "busy"; label: string }
  | { kind: "success"; label: string; result: WirelessCommandResult }
  | { kind: "error"; label: string; message: string };

export default function WirelessRoute() {
  const [servicesState, setServicesState] = useState<ServicesState>({
    kind: "loading",
  });
  const [actionState, setActionState] = useState<ActionState>({ kind: "idle" });
  const [qrName, setQrName] = useState(() => `Droidsmith-${randomToken(5)}`);
  const [qrCode, setQrCode] = useState(() => randomPairingCode());
  const [manualHost, setManualHost] = useState("");
  const [manualPort, setManualPort] = useState("");
  const [manualCode, setManualCode] = useState("");
  const [connectHost, setConnectHost] = useState("");
  const [connectPort, setConnectPort] = useState("");

  const qrPayload = useMemo(
    () =>
      `WIFI:T:ADB;S:${escapeWifiQrValue(qrName)};P:${escapeWifiQrValue(
        qrCode,
      )};;`,
    [qrCode, qrName],
  );

  const qrDataUrl = useMemo(() => {
    const qr = qrcode(0, "M");
    qr.addData(qrPayload);
    qr.make();
    const svg = qr.createSvgTag({ cellSize: 5, margin: 1, scalable: true });
    return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
  }, [qrPayload]);

  const loadServices = useCallback(async () => {
    if (!inTauri()) {
      setServicesState({ kind: "no_tauri" });
      return;
    }

    setServicesState({ kind: "loading" });
    try {
      const value = await callListWirelessServices();
      setServicesState({ kind: "ok", value });
    } catch (e) {
      setServicesState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  useEffect(() => {
    void loadServices();
  }, [loadServices]);

  const pairManual = useCallback(async () => {
    await runWirelessAction(
      setActionState,
      "Pairing",
      () =>
        callPairWireless({
          host: manualHost.trim(),
          port: Number(manualPort),
          pairing_code: manualCode.trim(),
        }),
      loadServices,
    );
  }, [loadServices, manualCode, manualHost, manualPort]);

  const connectManual = useCallback(async () => {
    await runWirelessAction(
      setActionState,
      "Connecting",
      () =>
        callConnectWireless({
          host: connectHost.trim(),
          port: Number(connectPort),
        }),
      loadServices,
    );
  }, [connectHost, connectPort, loadServices]);

  const pairService = useCallback(
    async (service: WirelessAdbService) => {
      await runWirelessAction(
        setActionState,
        "Pairing",
        () =>
          callPairWireless({
            host: service.host,
            port: service.port,
            pairing_code: qrCode,
          }),
        loadServices,
      );
    },
    [loadServices, qrCode],
  );

  const connectService = useCallback(
    async (service: WirelessAdbService) => {
      await runWirelessAction(
        setActionState,
        "Connecting",
        () =>
          callConnectWireless({
            host: service.host,
            port: service.port,
          }),
        loadServices,
      );
    },
    [loadServices],
  );

  const regenerateQr = useCallback(() => {
    setQrName(`Droidsmith-${randomToken(5)}`);
    setQrCode(randomPairingCode());
    setActionState({ kind: "idle" });
  }, []);

  const manualPairValid =
    manualHost.trim().length > 0 &&
    validPortText(manualPort) &&
    validPairingCode(manualCode);
  const manualConnectValid =
    connectHost.trim().length > 0 && validPortText(connectPort);

  return (
    <>
      <PaneHeader
        title="Wireless"
        milestone="R-015"
        description="Pair Android 11+ devices over Wi-Fi, discover mDNS pairing/connect endpoints, and attach paired devices without leaving Droidsmith."
        actions={
          <Button
            type="button"
            onClick={() => void loadServices()}
            disabled={servicesState.kind === "loading"}
            variant="primary"
          >
            {servicesState.kind === "loading" ? "Scanning..." : "Scan mDNS"}
          </Button>
        }
        meta={<WirelessHeaderMeta state={servicesState} />}
      />

      <section className="mt-6 grid max-w-7xl gap-4 xl:grid-cols-[minmax(320px,0.85fr)_minmax(0,1.15fr)]">
        <Card className="p-5">
          <div className="flex flex-col gap-5 lg:flex-row xl:flex-col 2xl:flex-row">
            <div className="shrink-0">
              <div className="grid aspect-square w-full max-w-52 place-items-center rounded-lg bg-slate-50 p-3 shadow-[0_20px_55px_rgba(0,0,0,0.28)]">
                {qrDataUrl ? (
                  <img
                    src={qrDataUrl}
                    alt="Wireless ADB pairing QR code"
                    className="h-full w-full"
                  />
                ) : (
                  <SkeletonLine className="h-44 w-44 bg-slate-300" />
                )}
              </div>
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <h3 className="text-sm font-semibold text-anvil-50">
                  QR pairing
                </h3>
                <Badge tone="info">ADB QR</Badge>
              </div>
              <dl className="mt-4 grid gap-3 text-sm">
                <QrMetric label="Name" value={qrName} />
                <QrMetric label="Code" value={qrCode} strong />
                <QrMetric label="Payload" value={qrPayload} mono />
              </dl>
              <div className="mt-5 flex flex-wrap gap-2">
                <Button type="button" onClick={regenerateQr}>
                  Regenerate
                </Button>
                <Button
                  type="button"
                  onClick={() => void loadServices()}
                  variant="ghost"
                >
                  Refresh services
                </Button>
              </div>
            </div>
          </div>
        </Card>

        <Card className="p-5">
          <div className="grid gap-6 lg:grid-cols-2">
            <div>
              <div className="flex flex-wrap items-center gap-2">
                <h3 className="text-sm font-semibold text-anvil-50">
                  Pairing code
                </h3>
                <Badge tone="neutral">adb pair</Badge>
              </div>
              <div className="mt-4 grid gap-3">
                <TextField
                  label="Host"
                  value={manualHost}
                  onChange={setManualHost}
                  placeholder="192.168.1.42"
                />
                <TextField
                  label="Port"
                  value={manualPort}
                  onChange={setManualPort}
                  placeholder="37099"
                  inputMode="numeric"
                />
                <TextField
                  label="Code"
                  value={manualCode}
                  onChange={setManualCode}
                  placeholder="123456"
                  inputMode="numeric"
                  maxLength={6}
                />
              </div>
              <Button
                type="button"
                onClick={() => void pairManual()}
                disabled={!manualPairValid || actionState.kind === "busy"}
                variant="primary"
                className="mt-4"
              >
                Pair
              </Button>
            </div>

            <div>
              <div className="flex flex-wrap items-center gap-2">
                <h3 className="text-sm font-semibold text-anvil-50">
                  Paired endpoint
                </h3>
                <Badge tone="neutral">adb connect</Badge>
              </div>
              <div className="mt-4 grid gap-3">
                <TextField
                  label="Host"
                  value={connectHost}
                  onChange={setConnectHost}
                  placeholder="192.168.1.42"
                />
                <TextField
                  label="Port"
                  value={connectPort}
                  onChange={setConnectPort}
                  placeholder="38899"
                  inputMode="numeric"
                />
              </div>
              <Button
                type="button"
                onClick={() => void connectManual()}
                disabled={!manualConnectValid || actionState.kind === "busy"}
                variant="primary"
                className="mt-4"
              >
                Connect
              </Button>
            </div>
          </div>
        </Card>
      </section>

      <section className="mt-4 max-w-7xl" aria-live="polite">
        <ActionStatus state={actionState} />
      </section>

      <section className="mt-4 max-w-7xl" aria-live="polite">
        <ServicesPanel
          state={servicesState}
          onRefresh={() => void loadServices()}
          onPair={(service) => void pairService(service)}
          onConnect={(service) => void connectService(service)}
          busy={actionState.kind === "busy"}
        />
      </section>
    </>
  );
}

async function runWirelessAction(
  setActionState: (state: ActionState) => void,
  label: string,
  run: () => Promise<WirelessCommandResult>,
  refresh: () => Promise<void>,
) {
  setActionState({ kind: "busy", label });
  try {
    const result = await run();
    setActionState({ kind: "success", label, result });
    await refresh();
  } catch (e) {
    setActionState({
      kind: "error",
      label,
      message: e instanceof Error ? e.message : String(e),
    });
  }
}

function WirelessHeaderMeta({ state }: { state: ServicesState }) {
  if (state.kind === "loading") {
    return (
      <div className="flex flex-wrap gap-2">
        <Badge tone="info">Scanning mDNS</Badge>
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
    return <Badge tone="danger">mDNS scan failed</Badge>;
  }

  if (!state.value.adb_resolved) {
    return <Badge tone="warning">ADB missing</Badge>;
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Badge tone="success">ADB resolved</Badge>
      <Badge tone="info">{state.value.services.length} services</Badge>
    </div>
  );
}

function ServicesPanel({
  state,
  onRefresh,
  onPair,
  onConnect,
  busy,
}: {
  state: ServicesState;
  onRefresh: () => void;
  onPair: (service: WirelessAdbService) => void;
  onConnect: (service: WirelessAdbService) => void;
  busy: boolean;
}) {
  if (state.kind === "no_tauri") {
    return (
      <StatePanel title="Desktop shell required" tone="info">
        <p>Wireless ADB commands run inside the Tauri runtime.</p>
      </StatePanel>
    );
  }

  if (state.kind === "loading") {
    return <ServicesSkeleton />;
  }

  if (state.kind === "error") {
    return (
      <StatePanel
        title="Wireless service scan failed"
        tone="danger"
        actions={
          <Button type="button" onClick={onRefresh} variant="danger" size="sm">
            Retry scan
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  if (!state.value.adb_resolved) {
    return (
      <StatePanel title="ADB is not available yet" tone="warning">
        <p>Install Android platform tools or stage the bundled sidecar.</p>
      </StatePanel>
    );
  }

  if (state.value.services.length === 0) {
    return (
      <StatePanel
        title="No wireless ADB services discovered"
        tone="info"
        actions={
          <Button
            type="button"
            onClick={onRefresh}
            variant="secondary"
            size="sm"
          >
            Scan again
          </Button>
        }
      >
        <p>mDNS did not return pairing or connect targets on this network.</p>
      </StatePanel>
    );
  }

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            Discovered wireless services
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            Pairing services use the QR code above; connect services attach
            already-paired devices.
          </p>
        </div>
        <Badge tone="info">{state.value.services.length} found</Badge>
      </div>
      <div className="overflow-x-auto">
        <table className="min-w-full text-sm">
          <thead className="bg-white/[0.04]">
            <tr>
              <Th>Name</Th>
              <Th>Kind</Th>
              <Th>Endpoint</Th>
              <Th>Action</Th>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/10">
            {state.value.services.map((service) => (
              <tr
                key={`${service.service_type}:${service.endpoint}:${service.name}`}
                className="bg-anvil-950/20 transition hover:bg-white/[0.035]"
              >
                <Td>
                  <code className="font-mono text-xs text-anvil-50">
                    {service.name}
                  </code>
                  <p className="mt-1 font-mono text-[11px] text-anvil-500">
                    {service.service_type}
                  </p>
                </Td>
                <Td>
                  <Badge tone={serviceKindTone(service.kind)}>
                    {service.kind}
                  </Badge>
                </Td>
                <Td>
                  <code className="font-mono text-xs text-anvil-100">
                    {service.endpoint}
                  </code>
                </Td>
                <Td>
                  {service.kind === "pairing" && (
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => onPair(service)}
                      disabled={busy}
                      variant="primary"
                    >
                      Pair
                    </Button>
                  )}
                  {service.kind === "connect" && (
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => onConnect(service)}
                      disabled={busy}
                      variant="primary"
                    >
                      Connect
                    </Button>
                  )}
                  {service.kind === "other" && (
                    <span className="text-xs text-anvil-500">No action</span>
                  )}
                </Td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Card>
  );
}

function ActionStatus({ state }: { state: ActionState }) {
  if (state.kind === "idle") {
    return null;
  }

  if (state.kind === "busy") {
    return (
      <StatePanel title={`${state.label} in progress`} tone="info">
        <p>Waiting for platform-tools to finish.</p>
      </StatePanel>
    );
  }

  if (state.kind === "error") {
    return (
      <StatePanel title={`${state.label} failed`} tone="danger">
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  return (
    <StatePanel title={`${state.label} complete`} tone="success">
      <dl className="grid gap-2 text-sm sm:grid-cols-[auto_minmax(0,1fr)]">
        <dt className="text-anvil-500">Endpoint</dt>
        <dd>
          <code className="font-mono text-anvil-100">
            {state.result.endpoint}
          </code>
        </dd>
        <dt className="text-anvil-500">ADB output</dt>
        <dd className="whitespace-pre-wrap font-mono text-xs text-anvil-200">
          {state.result.stdout.trim() || "Command completed."}
        </dd>
      </dl>
    </StatePanel>
  );
}

function ServicesSkeleton() {
  return (
    <Card
      className="overflow-hidden p-0"
      aria-label="Loading wireless services"
    >
      <div className="border-b border-white/10 p-4">
        <SkeletonLine className="w-48" />
        <SkeletonLine className="mt-3 w-80 max-w-full" />
      </div>
      <div className="divide-y divide-white/10">
        {Array.from({ length: 3 }).map((_, index) => (
          <div
            key={index}
            className="grid gap-4 p-4 sm:grid-cols-[1.2fr_0.6fr_1fr_0.5fr]"
          >
            <div>
              <SkeletonLine className="w-44" />
              <SkeletonLine className="mt-2 w-36" />
            </div>
            <SkeletonLine className="w-20" />
            <SkeletonLine className="w-40" />
            <SkeletonLine className="w-24" />
          </div>
        ))}
      </div>
    </Card>
  );
}

function TextField({
  label,
  value,
  onChange,
  placeholder,
  inputMode,
  maxLength,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  inputMode?: "text" | "numeric";
  maxLength?: number;
}) {
  return (
    <label className="grid gap-1.5">
      <span className="text-xs font-medium text-anvil-400">{label}</span>
      <FieldInput
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        inputMode={inputMode}
        maxLength={maxLength}
        className="font-mono"
      />
    </label>
  );
}

function QrMetric({
  label,
  value,
  strong = false,
  mono = false,
}: {
  label: string;
  value: string;
  strong?: boolean;
  mono?: boolean;
}) {
  return (
    <div className="grid gap-1">
      <dt className="text-xs font-medium text-anvil-500">{label}</dt>
      <dd
        className={[
          "min-w-0 break-words text-sm text-anvil-200",
          strong ? "text-lg font-semibold text-circuit-100" : "",
          mono ? "font-mono text-xs leading-5" : "",
        ].join(" ")}
      >
        {value}
      </dd>
    </div>
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

function serviceKindTone(
  kind: WirelessAdbService["kind"],
): "neutral" | "info" | "success" | "warning" | "danger" {
  if (kind === "pairing") {
    return "warning";
  }
  if (kind === "connect") {
    return "success";
  }
  return "neutral";
}

function validPairingCode(value: string): boolean {
  return /^\d{6}$/.test(value.trim());
}

function validPortText(value: string): boolean {
  if (!/^\d+$/.test(value.trim())) {
    return false;
  }
  const port = Number(value);
  return Number.isInteger(port) && port > 0 && port <= 65535;
}

function randomPairingCode(): string {
  return String(randomInt(1_000_000)).padStart(6, "0");
}

function randomToken(length: number): string {
  const alphabet = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  return Array.from(
    { length },
    () => alphabet[randomInt(alphabet.length)],
  ).join("");
}

function randomInt(max: number): number {
  const cryptoApi = globalThis.crypto;
  if (cryptoApi?.getRandomValues) {
    const value = new Uint32Array(1);
    cryptoApi.getRandomValues(value);
    return value[0]! % max;
  }
  return Math.floor(Math.random() * max);
}

function escapeWifiQrValue(value: string): string {
  return value.replace(/([\\;,:"])/g, "\\$1");
}
