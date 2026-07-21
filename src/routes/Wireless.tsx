import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import qrcode from "qrcode-generator";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callConnectWireless,
  callForgetWirelessEndpoint,
  callListWirelessHistory,
  callListWirelessServices,
  callPairWireless,
  callSetWirelessAutoReconnect,
  inTauri,
  type ListWirelessServicesResult,
  type WirelessAdbService,
  WirelessCommandFailure,
  type WirelessCommandResult,
  type WirelessEndpoint,
  type WirelessHistorySnapshot,
} from "../lib/tauri";

import {
  Badge,
  Button,
  Card,
  FieldInput,
  FieldTextArea,
  PaneHeader,
  SkeletonLine,
  StatePanel,
  TableCell,
  TableHeaderCell,
  TransportBadge,
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
  | { kind: "error"; label: string; failure: WirelessCommandFailure };

export default function WirelessRoute() {
  const { t } = useTranslation();
  const [servicesState, setServicesState] = useState<ServicesState>({
    kind: "loading",
  });
  const [actionState, setActionState] = useState<ActionState>({ kind: "idle" });
  const [history, setHistory] = useState<WirelessHistorySnapshot | null>(null);
  const autoReconnectDone = useRef(false);
  const [qrName, setQrName] = useState(() => `Droidsmith-${randomToken(5)}`);
  const [qrCode, setQrCode] = useState(() => randomPairingCode());
  const [manualHost, setManualHost] = useState("");
  const [manualPort, setManualPort] = useState("");
  const [manualCode, setManualCode] = useState("");
  const [connectHost, setConnectHost] = useState("");
  const [connectPort, setConnectPort] = useState("");
  const [connectLegacyTcp, setConnectLegacyTcp] = useState(false);

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
        message: errorMessage(e),
      });
    }
  }, []);

  const loadHistory = useCallback(async () => {
    if (!inTauri()) {
      return;
    }
    try {
      setHistory(await callListWirelessHistory());
    } catch {
      // History is a convenience surface; a load failure must not block pairing.
    }
  }, []);

  const refreshAll = useCallback(async () => {
    await loadServices();
    await loadHistory();
  }, [loadHistory, loadServices]);

  useEffect(() => {
    void loadServices();
  }, [loadServices]);

  useEffect(() => {
    void loadHistory();
  }, [loadHistory]);

  // Opt-in reconnect-on-launch: only the first loaded snapshot drives this, so
  // toggling the preference later never re-triggers a reconnect sweep. When the
  // persisted flag is set, attempt each saved endpoint once; failures are silent
  // (the device may simply be offline).
  useEffect(() => {
    if (autoReconnectDone.current || history === null) {
      return;
    }
    autoReconnectDone.current = true;
    if (!history.autoReconnect || history.endpoints.length === 0) {
      return;
    }
    void (async () => {
      for (const endpoint of history.endpoints) {
        try {
          await callConnectWireless({
            host: endpoint.host,
            port: endpoint.port,
            legacy_tcp: false,
          });
        } catch {
          // Best-effort; skip endpoints that are no longer reachable.
        }
      }
      await refreshAll();
    })();
  }, [history, refreshAll]);

  const pairManual = useCallback(async () => {
    await runWirelessAction(
      setActionState,
      t("wireless.pairing"),
      () =>
        callPairWireless({
          host: manualHost.trim(),
          port: Number(manualPort),
          pairing_code: manualCode.trim(),
        }),
      refreshAll,
    );
  }, [manualCode, manualHost, manualPort, refreshAll, t]);

  const connectManual = useCallback(async () => {
    await runWirelessAction(
      setActionState,
      t("wireless.connecting"),
      () =>
        callConnectWireless({
          host: connectHost.trim(),
          port: Number(connectPort),
          legacy_tcp: connectLegacyTcp,
        }),
      refreshAll,
    );
  }, [connectHost, connectLegacyTcp, connectPort, refreshAll, t]);

  const reconnectEndpoint = useCallback(
    async (endpoint: WirelessEndpoint) => {
      await runWirelessAction(
        setActionState,
        t("wireless.connecting"),
        () =>
          callConnectWireless({
            host: endpoint.host,
            port: endpoint.port,
            legacy_tcp: false,
          }),
        refreshAll,
      );
    },
    [refreshAll, t],
  );

  const forgetEndpoint = useCallback(async (endpoint: WirelessEndpoint) => {
    try {
      setHistory(
        await callForgetWirelessEndpoint(endpoint.host, endpoint.port),
      );
    } catch {
      // Ignore; the row stays and the user can retry.
    }
  }, []);

  const toggleAutoReconnect = useCallback(async (enabled: boolean) => {
    try {
      setHistory(await callSetWirelessAutoReconnect(enabled));
    } catch {
      // Ignore; the toggle reflects the last confirmed state.
    }
  }, []);

  const pairService = useCallback(
    async (service: WirelessAdbService) => {
      await runWirelessAction(
        setActionState,
        t("wireless.pairing"),
        () =>
          callPairWireless({
            host: service.host,
            port: service.port,
            pairing_code: qrCode,
          }),
        refreshAll,
      );
    },
    [qrCode, refreshAll, t],
  );

  const connectService = useCallback(
    async (service: WirelessAdbService) => {
      await runWirelessAction(
        setActionState,
        t("wireless.connecting"),
        () =>
          callConnectWireless({
            host: service.host,
            port: service.port,
            legacy_tcp: false,
          }),
        refreshAll,
      );
    },
    [refreshAll, t],
  );

  const regenerateQr = useCallback(() => {
    setQrName(`Droidsmith-${randomToken(5)}`);
    setQrCode(randomPairingCode());
    setActionState({ kind: "idle" });
  }, []);

  const manualPairValid =
    validHost(manualHost) &&
    validPortText(manualPort) &&
    validPairingCode(manualCode);
  const manualConnectValid =
    validHost(connectHost) && validPortText(connectPort);

  return (
    <>
      <PaneHeader
        title={t("wireless.title")}
        milestone="R-015"
        description={t("wireless.description")}
        actions={
          <Button
            type="button"
            onClick={() => void loadServices()}
            disabled={servicesState.kind === "loading"}
            variant="primary"
          >
            {servicesState.kind === "loading"
              ? t("wireless.scanning")
              : t("wireless.scanMdns")}
          </Button>
        }
        meta={<WirelessHeaderMeta state={servicesState} />}
      />

      <section className="mt-6 grid max-w-7xl gap-4 xl:grid-cols-[minmax(320px,0.85fr)_minmax(0,1.15fr)]">
        <Card className="p-5">
          <div className="flex flex-col gap-5 lg:flex-row xl:flex-col 2xl:flex-row">
            <div className="shrink-0">
              <div className="grid aspect-square w-full max-w-52 place-items-center rounded-lg bg-slate-50 p-3 shadow-glow">
                {qrDataUrl ? (
                  <img
                    src={qrDataUrl}
                    alt={t("wireless.qrAlt")}
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
                  {t("wireless.qrPairing")}
                </h3>
                <Badge tone="info">ADB QR</Badge>
              </div>
              <dl className="mt-4 grid gap-3 text-sm">
                <QrMetric label={t("wireless.name")} value={qrName} />
                <QrMetric label={t("wireless.code")} value={qrCode} strong />
                <QrMetric
                  label={t("wireless.payload")}
                  value={qrPayload}
                  mono
                />
              </dl>
              <div className="mt-5 flex flex-wrap gap-2">
                <Button type="button" onClick={regenerateQr}>
                  {t("wireless.regenerate")}
                </Button>
                <Button
                  type="button"
                  onClick={() => void loadServices()}
                  variant="ghost"
                >
                  {t("wireless.refreshServices")}
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
                  {t("wireless.pairingCode")}
                </h3>
                <Badge tone="neutral">adb pair</Badge>
              </div>
              <div className="mt-4 grid gap-3">
                <TextField
                  label={t("wireless.host")}
                  value={manualHost}
                  onChange={setManualHost}
                  placeholder="192.168.1.42"
                />
                <TextField
                  label={t("wireless.port")}
                  value={manualPort}
                  onChange={setManualPort}
                  placeholder="37099"
                  inputMode="numeric"
                />
                <TextField
                  label={t("wireless.code")}
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
                {t("wireless.pair")}
              </Button>
            </div>

            <div>
              <div className="flex flex-wrap items-center gap-2">
                <h3 className="text-sm font-semibold text-anvil-50">
                  {t("wireless.pairedEndpoint")}
                </h3>
                <Badge tone="neutral">adb connect</Badge>
              </div>
              <div className="mt-4 grid gap-3">
                <TextField
                  label={t("wireless.host")}
                  value={connectHost}
                  onChange={setConnectHost}
                  placeholder="192.168.1.42"
                />
                <TextField
                  label={t("wireless.port")}
                  value={connectPort}
                  onChange={setConnectPort}
                  placeholder="38899"
                  inputMode="numeric"
                />
                <label className="flex items-start gap-3 rounded-md border border-amber-300/20 bg-amber-300/[0.05] p-3 text-sm leading-5 text-anvil-200">
                  <input
                    type="checkbox"
                    className="mt-1 h-4 w-4 accent-amber-300"
                    checked={connectLegacyTcp}
                    onChange={(event) =>
                      setConnectLegacyTcp(event.currentTarget.checked)
                    }
                  />
                  <span>{t("wireless.legacyTcpEndpoint")}</span>
                </label>
              </div>
              <Button
                type="button"
                onClick={() => void connectManual()}
                disabled={!manualConnectValid || actionState.kind === "busy"}
                variant="primary"
                className="mt-4"
              >
                {t("wireless.connect")}
              </Button>
            </div>
          </div>
        </Card>
      </section>

      <section className="mt-4 max-w-7xl" aria-live="polite">
        <ActionStatus state={actionState} />
      </section>

      {history && (
        <section className="mt-4 max-w-7xl">
          <HistoryPanel
            history={history}
            busy={actionState.kind === "busy"}
            onReconnect={(endpoint) => void reconnectEndpoint(endpoint)}
            onForget={(endpoint) => void forgetEndpoint(endpoint)}
            onToggleAuto={(enabled) => void toggleAutoReconnect(enabled)}
          />
        </section>
      )}

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
      failure:
        e instanceof WirelessCommandFailure
          ? e
          : new WirelessCommandFailure(errorMessage(e)),
    });
  }
}

function WirelessHeaderMeta({ state }: { state: ServicesState }) {
  const { t } = useTranslation();

  if (state.kind === "loading") {
    return (
      <div className="flex flex-wrap gap-2">
        <Badge tone="info">{t("wireless.scanningMdns")}</Badge>
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
    return <Badge tone="danger">{t("wireless.mdnsScanFailed")}</Badge>;
  }

  if (!state.value.adb_resolved) {
    return <Badge tone="warning">{t("devices.adbMissing")}</Badge>;
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Badge tone="success">{t("devices.adbResolved")}</Badge>
      <Badge tone="info">
        {t("wireless.serviceCount", { count: state.value.services.length })}
      </Badge>
    </div>
  );
}

function HistoryPanel({
  history,
  busy,
  onReconnect,
  onForget,
  onToggleAuto,
}: {
  history: WirelessHistorySnapshot;
  busy: boolean;
  onReconnect: (endpoint: WirelessEndpoint) => void;
  onForget: (endpoint: WirelessEndpoint) => void;
  onToggleAuto: (enabled: boolean) => void;
}) {
  const { t } = useTranslation();

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("wireless.historyTitle")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("wireless.historyBody")}
          </p>
        </div>
        <label className="flex items-start gap-3 rounded-md border border-white/10 bg-white/[0.03] p-3 text-sm leading-5 text-anvil-200 sm:max-w-xs">
          <input
            type="checkbox"
            className="mt-1 h-4 w-4 accent-circuit-300"
            checked={history.autoReconnect}
            onChange={(event) => onToggleAuto(event.currentTarget.checked)}
          />
          <span>
            <span className="font-medium text-anvil-100">
              {t("wireless.autoReconnect")}
            </span>
            <span className="mt-0.5 block text-xs text-anvil-400">
              {t("wireless.autoReconnectHint")}
            </span>
          </span>
        </label>
      </div>
      {history.endpoints.length === 0 ? (
        <p className="p-4 text-sm text-anvil-400">
          {t("wireless.historyEmpty")}
        </p>
      ) : (
        <div className="overflow-x-auto">
          <table className="min-w-full text-sm">
            <thead className="bg-white/[0.04]">
              <tr>
                <TableHeaderCell>{t("wireless.endpoint")}</TableHeaderCell>
                <TableHeaderCell>{t("wireless.lastConnected")}</TableHeaderCell>
                <TableHeaderCell>{t("wireless.action")}</TableHeaderCell>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {history.endpoints.map((endpoint) => (
                <tr
                  key={`${endpoint.host}:${endpoint.port}`}
                  className="bg-anvil-950/20 transition hover:bg-white/[0.035]"
                >
                  <TableCell>
                    <code className="font-mono text-xs text-anvil-50">
                      {endpoint.host}:{endpoint.port}
                    </code>
                    {endpoint.label && (
                      <p className="mt-1 text-[11px] text-anvil-400">
                        {endpoint.label}
                      </p>
                    )}
                  </TableCell>
                  <TableCell>
                    <span className="text-xs text-anvil-300">
                      {formatTimestamp(endpoint.lastConnectedMs)}
                    </span>
                  </TableCell>
                  <TableCell>
                    <div className="flex flex-wrap gap-2">
                      <Button
                        type="button"
                        size="sm"
                        variant="primary"
                        disabled={busy}
                        onClick={() => onReconnect(endpoint)}
                      >
                        {t("wireless.reconnect")}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        disabled={busy}
                        onClick={() => onForget(endpoint)}
                      >
                        {t("wireless.forget")}
                      </Button>
                    </div>
                  </TableCell>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
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
  const { t } = useTranslation();

  if (state.kind === "no_tauri") {
    return (
      <StatePanel title={t("common.desktopRequired")} tone="info">
        <p>{t("wireless.desktopRequiredBody")}</p>
      </StatePanel>
    );
  }

  if (state.kind === "loading") {
    return <ServicesSkeleton />;
  }

  if (state.kind === "error") {
    return (
      <StatePanel
        title={t("wireless.serviceScanFailed")}
        tone="danger"
        actions={
          <Button type="button" onClick={onRefresh} variant="danger" size="sm">
            {t("common.retryScan")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  if (!state.value.adb_resolved) {
    return (
      <StatePanel title={t("devices.noAdb")} tone="warning">
        <p>{t("wireless.noAdbBody")}</p>
      </StatePanel>
    );
  }

  if (state.value.services.length === 0) {
    return (
      <StatePanel
        title={t("wireless.noServicesTitle")}
        tone="info"
        actions={
          <Button
            type="button"
            onClick={onRefresh}
            variant="secondary"
            size="sm"
          >
            {t("common.scanAgain")}
          </Button>
        }
      >
        <p>{t("wireless.noServicesBody")}</p>
      </StatePanel>
    );
  }

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("wireless.discoveredServices")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("wireless.discoveredServicesBody")}
          </p>
        </div>
        <Badge tone="info">
          {t("wireless.foundCount", { count: state.value.services.length })}
        </Badge>
      </div>
      <div className="overflow-x-auto">
        <table className="min-w-full text-sm">
          <thead className="bg-white/[0.04]">
            <tr>
              <TableHeaderCell>{t("wireless.name")}</TableHeaderCell>
              <TableHeaderCell>{t("wireless.kind")}</TableHeaderCell>
              <TableHeaderCell>{t("wireless.endpoint")}</TableHeaderCell>
              <TableHeaderCell>{t("wireless.action")}</TableHeaderCell>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/10">
            {state.value.services.map((service) => (
              <tr
                key={`${service.service_type}:${service.endpoint}:${service.name}`}
                className="bg-anvil-950/20 transition hover:bg-white/[0.035]"
              >
                <TableCell>
                  <code className="font-mono text-xs text-anvil-50">
                    {service.name}
                  </code>
                  <p className="mt-1 font-mono text-[11px] text-anvil-500">
                    {service.service_type}
                  </p>
                </TableCell>
                <TableCell>
                  <Badge tone={serviceKindTone(service.kind)}>
                    {service.kind}
                  </Badge>
                </TableCell>
                <TableCell>
                  <code className="font-mono text-xs text-anvil-100">
                    {service.endpoint}
                  </code>
                </TableCell>
                <TableCell>
                  {service.kind === "pairing" && (
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => onPair(service)}
                      disabled={busy}
                      variant="primary"
                    >
                      {t("wireless.pair")}
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
                      {t("wireless.connect")}
                    </Button>
                  )}
                  {service.kind === "other" && (
                    <span className="text-xs text-anvil-500">
                      {t("common.noAction")}
                    </span>
                  )}
                </TableCell>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Card>
  );
}

function ActionStatus({ state }: { state: ActionState }) {
  const { t } = useTranslation();

  if (state.kind === "idle") {
    return null;
  }

  if (state.kind === "busy") {
    return (
      <StatePanel
        title={t("wireless.actionInProgress", { action: state.label })}
        tone="info"
      >
        <p>{t("wireless.waitingForPlatformTools")}</p>
      </StatePanel>
    );
  }

  if (state.kind === "error") {
    const diagnostics = state.failure.diagnostics
      ? `${JSON.stringify(
          {
            code: state.failure.code,
            hint_code: state.failure.hintCode,
            ...state.failure.diagnostics,
          },
          null,
          2,
        )}\n`
      : null;
    return (
      <StatePanel
        title={t("wireless.actionFailed", { action: state.label })}
        tone="danger"
      >
        <div className="grid gap-4">
          <p>{state.failure.message}</p>
          <WirelessFailureHint hintCode={state.failure.hintCode} />
          {diagnostics && (
            <div>
              <label
                htmlFor="wireless-failure-diagnostics"
                className="text-sm font-semibold text-anvil-50"
              >
                {t("wireless.failureDiagnostics")}
              </label>
              <p className="mt-1 text-xs text-anvil-400">
                {t("wireless.failureDiagnosticsPrivacy")}
              </p>
              <FieldTextArea
                id="wireless-failure-diagnostics"
                rows={8}
                readOnly
                spellCheck={false}
                value={diagnostics}
                className="mt-2 resize-y bg-anvil-950/70 p-3 font-mono text-xs text-anvil-100"
              />
            </div>
          )}
        </div>
      </StatePanel>
    );
  }

  return (
    <StatePanel
      title={t("wireless.actionComplete", { action: state.label })}
      tone="success"
    >
      <dl className="grid gap-2 text-sm sm:grid-cols-[auto_minmax(0,1fr)]">
        <dt className="text-anvil-500">{t("wireless.endpoint")}</dt>
        <dd>
          <span className="flex flex-wrap items-center gap-2">
            <code className="font-mono text-anvil-100">
              {state.result.endpoint}
            </code>
            {state.result.transport_kind && (
              <TransportBadge kind={state.result.transport_kind} />
            )}
          </span>
        </dd>
        <dt className="text-anvil-500">{t("wireless.adbOutput")}</dt>
        <dd className="whitespace-pre-wrap font-mono text-xs text-anvil-200">
          {state.result.stdout.trim() || t("wireless.commandCompleted")}
        </dd>
      </dl>
    </StatePanel>
  );
}

function WirelessFailureHint({
  hintCode,
}: {
  hintCode: WirelessCommandFailure["hintCode"];
}) {
  const { t } = useTranslation();

  if (!hintCode) {
    return null;
  }

  const prefix =
    hintCode === "vpn_interference_likely" ? "vpnHint" : "mdnsHint";
  return (
    <div className="rounded-md border border-amber-300/25 bg-amber-300/[0.07] p-4">
      <h4 className="text-sm font-semibold text-amber-100">
        {t(`wireless.${prefix}Title`)}
      </h4>
      <p className="mt-1 text-sm text-anvil-200">
        {t(`wireless.${prefix}Body`)}
      </p>
      <ol className="mt-3 list-decimal space-y-1 ps-5 text-sm text-anvil-200">
        <li>{t(`wireless.${prefix}Step1`)}</li>
        <li>{t(`wireless.${prefix}Step2`)}</li>
      </ol>
    </div>
  );
}

function ServicesSkeleton() {
  const { t } = useTranslation();

  return (
    <Card
      className="overflow-hidden p-0"
      aria-label={t("wireless.loadingServices")}
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

function formatTimestamp(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return "—";
  }
  try {
    return new Date(ms).toLocaleString();
  } catch {
    return "—";
  }
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

// Permissive host gate: the backend does the authoritative IPv4/IPv6/mDNS
// parsing (and rejects bare IPv6), so only block obviously-invalid input —
// empty, whitespace, a URL scheme, or a path — before the round-trip.
function validHost(value: string): boolean {
  const host = value.trim();
  if (host.length === 0 || host.length > 255) return false;
  return !/[\s/\\]/.test(host) && !host.includes("://");
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
