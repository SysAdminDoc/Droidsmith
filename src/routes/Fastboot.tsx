import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  callFastbootGetvar,
  callListFastbootDevices,
  callLocateFastboot,
  inTauri,
  type FastbootDevice,
} from "../lib/tauri";

import { Badge, Button, Card, PaneHeader, StatePanel } from "./common";

type FastbootState =
  | { kind: "checking" }
  | { kind: "found"; path: string }
  | { kind: "not_found" };

type DeviceListState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ok"; devices: FastbootDevice[] }
  | { kind: "error"; message: string };

type VarMap = Record<string, string>;

const GETVAR_KEYS = [
  "product",
  "variant",
  "serialno",
  "secure",
  "unlocked",
  "current-slot",
  "slot-count",
  "max-download-size",
  "battery-voltage",
];

export default function FastbootRoute() {
  const { t } = useTranslation();
  const [fbState, setFbState] = useState<FastbootState>({ kind: "checking" });
  const [devicesState, setDevicesState] = useState<DeviceListState>({
    kind: "idle",
  });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [vars, setVars] = useState<VarMap>({});
  const [loadingVars, setLoadingVars] = useState(false);

  const checkFastboot = useCallback(async () => {
    if (!inTauri()) {
      setFbState({ kind: "not_found" });
      return;
    }
    setFbState({ kind: "checking" });
    try {
      const path = await callLocateFastboot();
      if (path) {
        setFbState({ kind: "found", path });
      } else {
        setFbState({ kind: "not_found" });
      }
    } catch {
      setFbState({ kind: "not_found" });
    }
  }, []);

  const scanDevices = useCallback(async () => {
    setDevicesState({ kind: "loading" });
    try {
      const devices = await callListFastbootDevices();
      setDevicesState({ kind: "ok", devices });
      if (devices.length === 1 && !selectedSerial) {
        setSelectedSerial(devices[0]!.serial);
      }
    } catch (e) {
      setDevicesState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedSerial]);

  const loadVars = useCallback(async () => {
    if (!selectedSerial) return;
    setLoadingVars(true);
    const result: VarMap = {};
    for (const key of GETVAR_KEYS) {
      try {
        const val = await callFastbootGetvar(selectedSerial, key);
        const cleaned = val
          .split("\n")
          .find((l) => l.includes(key))
          ?.replace(/.*:\s*/, "")
          .trim();
        if (cleaned) result[key] = cleaned;
      } catch {
        // Skip unavailable vars
      }
    }
    setVars(result);
    setLoadingVars(false);
  }, [selectedSerial]);

  useEffect(() => {
    void checkFastboot();
  }, [checkFastboot]);

  return (
    <>
      <PaneHeader
        title={t("fastboot.title")}
        milestone="R-052"
        description={t("fastboot.description")}
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {fbState.kind === "found" && (
              <Badge tone="success">{t("fastboot.fastbootFound")}</Badge>
            )}
            {fbState.kind === "not_found" && (
              <Badge tone="warning">{t("fastboot.fastbootMissingShort")}</Badge>
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
        {fbState.kind === "not_found" && (
          <StatePanel title={t("fastboot.fastbootMissing")} tone="warning">
            <p>
              {t("fastboot.installPrefix")} <code>fastboot</code>{" "}
              {t("fastboot.installSuffix")}
            </p>
            <div className="mt-3">
              <Button
                type="button"
                size="sm"
                onClick={() => void checkFastboot()}
              >
                {t("common.checkAgain")}
              </Button>
            </div>
          </StatePanel>
        )}

        {fbState.kind === "found" && (
          <>
            <Card className="p-4">
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-sm font-semibold text-anvil-50">
                    {t("fastboot.bootloaderDevices")}
                  </h3>
                  <p className="mt-1 text-xs text-anvil-400">
                    {t("fastboot.fastbootAt")}{" "}
                    <code className="text-anvil-200">{fbState.path}</code>
                  </p>
                </div>
                <Button
                  type="button"
                  size="sm"
                  variant="primary"
                  onClick={() => void scanDevices()}
                  disabled={devicesState.kind === "loading"}
                >
                  {devicesState.kind === "loading"
                    ? t("fastboot.scanning")
                    : t("fastboot.scan")}
                </Button>
              </div>

              {devicesState.kind === "ok" &&
                devicesState.devices.length === 0 && (
                  <p className="mt-4 text-sm text-anvil-400">
                    {t("fastboot.noBootloaderDevices")}{" "}
                    <code>adb reboot bootloader</code>{" "}
                    {t("fastboot.noBootloaderDevicesSuffix")}
                  </p>
                )}

              {devicesState.kind === "ok" &&
                devicesState.devices.length > 0 && (
                  <div className="mt-4 overflow-x-auto">
                    <table className="min-w-full text-sm">
                      <thead className="bg-white/[0.04]">
                        <tr>
                          <Th>{t("devices.serial")}</Th>
                          <Th>{t("fastboot.mode")}</Th>
                          <Th>{t("fastboot.product")}</Th>
                          <Th>{t("wireless.action")}</Th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-white/10">
                        {devicesState.devices.map((d) => (
                          <tr key={d.serial} className="hover:bg-white/[0.03]">
                            <Td>
                              <code className="font-mono text-xs text-anvil-50">
                                {d.serial}
                              </code>
                            </Td>
                            <Td>
                              <Badge tone="info">{d.mode}</Badge>
                              {d.parse_error && (
                                <Badge tone="warning" className="ml-2">
                                  Parse issue
                                </Badge>
                              )}
                            </Td>
                            <Td>{d.product ?? t("common.notReported")}</Td>
                            <Td>
                              <Button
                                type="button"
                                size="sm"
                                variant={
                                  d.serial === selectedSerial
                                    ? "primary"
                                    : "secondary"
                                }
                                onClick={() => {
                                  setSelectedSerial(d.serial);
                                  setVars({});
                                }}
                              >
                                {d.serial === selectedSerial
                                  ? t("common.selected")
                                  : t("common.select")}
                              </Button>
                            </Td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}

              {devicesState.kind === "error" && (
                <p className="mt-4 text-sm text-red-300">
                  {devicesState.message}
                </p>
              )}
            </Card>

            {selectedSerial && (
              <Card className="p-5">
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-semibold text-anvil-50">
                    {t("fastboot.deviceVariables")}
                  </h3>
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => void loadVars()}
                    disabled={loadingVars}
                  >
                    {loadingVars
                      ? t("fastboot.querying")
                      : t("fastboot.queryVars")}
                  </Button>
                </div>

                {Object.keys(vars).length > 0 && (
                  <dl className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                    {Object.entries(vars).map(([key, value]) => (
                      <div key={key}>
                        <dt className="text-xs font-medium text-anvil-500">
                          {key}
                        </dt>
                        <dd className="mt-1 font-mono text-sm text-anvil-100">
                          {value}
                        </dd>
                      </div>
                    ))}
                  </dl>
                )}

                {Object.keys(vars).length === 0 && !loadingVars && (
                  <p className="mt-4 text-xs text-anvil-400">
                    {t("fastboot.queryHint")}
                  </p>
                )}

                <div className="mt-5 rounded-md border border-amber-300/20 bg-amber-950/20 p-3">
                  <p className="text-xs leading-5 text-amber-200">
                    {t("fastboot.safetyWarning")} <code>fastboot</code>{" "}
                    {t("fastboot.safetyWarningSuffix")}
                  </p>
                </div>
              </Card>
            )}
          </>
        )}
      </section>
    </>
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
  return <td className="px-4 py-3 align-middle text-anvil-200">{children}</td>;
}
