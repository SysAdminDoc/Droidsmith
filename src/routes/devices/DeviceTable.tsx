// Device selector toolbar, connected-device table, loading skeleton, and
// device-state helpers (IMP-72: extracted verbatim from the former Devices.tsx
// god-file).

import { useTranslation } from "react-i18next";

import { type Device, type ListDevicesResult } from "../../lib/tauri";
import {
  Badge,
  Card,
  FieldSelect,
  SkeletonLine,
  TableCell,
  TableHeaderCell,
  TransportBadge,
} from "../common";
import { deviceStateTone, formatStateLabel } from "./common";
import { MoreIcon, SelectionIcon } from "./icons";

export function DeviceToolbar({
  devices,
  selectedDeviceKey,
  onSelect,
}: {
  devices: ListDevicesResult["devices"];
  selectedDeviceKey: string | null;
  onSelect: (device: Device) => void;
}) {
  const { t } = useTranslation();
  const selectable = devices.filter(
    (device) => typeof device.state === "string" && device.state === "device",
  );

  return (
    <div className="mt-4 flex flex-col gap-3 border-b border-white/[0.08] pb-4 sm:flex-row sm:items-center">
      <FieldSelect
        aria-label={t("common.selectDevice")}
        value={selectedDeviceKey ?? ""}
        onChange={(event) => {
          const device = selectable.find(
            (candidate) =>
              String(candidate.transport_id ?? candidate.serial) ===
              event.currentTarget.value,
          );
          if (device) onSelect(device);
        }}
        className="min-w-64 font-medium"
      >
        <option value="" disabled>
          {t("common.selectDevice")}
        </option>
        {selectable.map((device) => (
          <option
            key={
              String(device.transport_id ?? device.serial) +
              ":" +
              device.connection_generation
            }
            value={String(device.transport_id ?? device.serial)}
          >
            {device.model ?? device.serial}
          </option>
        ))}
      </FieldSelect>
      <span className="hidden h-7 w-px bg-white/[0.08] sm:block" />
      <Badge tone="success">{t("devices.adbReady")}</Badge>
    </div>
  );
}

export function DeviceTable({
  devices,
  selectedSerial,
  onSelect,
}: {
  devices: ListDevicesResult["devices"];
  selectedSerial?: number | null;
  onSelect: (device: Device) => void;
}) {
  const { t } = useTranslation();

  return (
    <section className="border-t border-white/[0.08] pt-5">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold text-anvil-50">
          {t("devices.connected")}
        </h3>
        <span className="text-xs text-anvil-500">
          {t("common.deviceCount", { count: devices.length })}
        </span>
      </div>
      <div className="overflow-x-auto rounded-md border border-white/[0.08]">
        <table className="min-w-full text-sm">
          <thead className="bg-white/[0.025]">
            <tr>
              <TableHeaderCell className="w-14">
                <span className="sr-only">{t("common.selectDevice")}</span>
              </TableHeaderCell>
              <TableHeaderCell>{t("devices.serial")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.identity")}</TableHeaderCell>
              <TableHeaderCell>{t("devices.transport")}</TableHeaderCell>
              <TableHeaderCell className="w-14">
                <span className="sr-only">{t("devices.moreActions")}</span>
              </TableHeaderCell>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/[0.07]">
            {devices.map((device) => {
              const isDevice =
                typeof device.state === "string" && device.state === "device";
              const isSelected = device.transport_id === selectedSerial;
              return (
                <tr
                  key={`${device.transport_id ?? device.serial}:${device.connection_generation}`}
                  title={!isDevice ? t("devices.mustAuthorize") : undefined}
                  className={[
                    "transition",
                    isDevice
                      ? "hover:bg-white/[0.055]"
                      : "bg-anvil-950/20 opacity-75",
                  ].join(" ")}
                >
                  <TableCell className="pe-0">
                    <button
                      type="button"
                      disabled={!isDevice}
                      aria-pressed={isDevice ? isSelected : undefined}
                      aria-label={
                        isDevice
                          ? t("devices.selectDeviceLabel", {
                              device: device.model ?? device.serial,
                            })
                          : undefined
                      }
                      onClick={() => onSelect(device)}
                      className="flex h-7 w-7 items-center justify-center rounded-full text-anvil-600 transition hover:text-anvil-300 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300 focus-visible:ring-offset-2 focus-visible:ring-offset-anvil-900"
                    >
                      <SelectionIcon selected={isSelected} />
                    </button>
                  </TableCell>
                  <TableCell>
                    <div className="flex min-w-[10rem] items-center gap-2">
                      <code className="font-mono text-xs text-anvil-50">
                        {device.serial}
                      </code>
                      <TransportBadge kind={device.transport_kind} />
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
                  <TableCell className="ps-0 text-end">
                    <button
                      type="button"
                      disabled={!isDevice}
                      aria-label={t("devices.moreDeviceActions", {
                        device: device.model ?? device.serial,
                      })}
                      onClick={() => onSelect(device)}
                      className="inline-flex h-8 w-8 items-center justify-center rounded-md text-anvil-500 transition hover:bg-white/[0.07] hover:text-anvil-100 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300"
                    >
                      <MoreIcon />
                    </button>
                  </TableCell>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

export function DeviceTableSkeleton() {
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
