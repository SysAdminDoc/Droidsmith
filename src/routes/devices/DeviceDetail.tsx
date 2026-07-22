// Selected-device detail panel + device-health cards (IMP-72: extracted
// verbatim from the former Devices.tsx god-file).

import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "../../lib/cn";
import {
  errorMessage,
  callDisconnectDevice,
  type DeviceInfo,
  type DeviceTarget,
} from "../../lib/tauri";
import { Badge, Button, Card, SkeletonLine, StatePanel } from "../common";
import type { DetailState } from "./common";
import { formatKb } from "./common";

export function DeviceDetail({
  state,
  onRetry,
}: {
  state: DetailState;
  onRetry: (target: DeviceTarget) => void;
}) {
  const { t } = useTranslation();
  const [disconnectMessage, setDisconnectMessage] = useState<string | null>(
    null,
  );
  const doDisconnect = useCallback(
    async (target: DeviceTarget) => {
      try {
        const result = await callDisconnectDevice(target);
        if (result.disconnected) {
          setDisconnectMessage(t("devices.disconnectSuccess"));
        } else {
          setDisconnectMessage(result.message);
        }
      } catch (error) {
        setDisconnectMessage(
          t("devices.disconnectFailed", {
            message: errorMessage(error),
          }),
        );
      }
    },
    [t],
  );

  if (state.kind === "idle") return null;

  if (state.kind === "loading") {
    return (
      <section className="border-b border-white/10 py-5">
        <h3 className="text-sm font-semibold text-anvil-50">
          {t("devices.loadingDeviceInfo")}
        </h3>
        <p className="mt-1 text-xs text-anvil-400">
          {t("devices.queryingSerial", { serial: state.target.serial })}
        </p>
        <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <div key={i}>
              <SkeletonLine className="w-20" />
              <SkeletonLine className="mt-2 w-36" />
            </div>
          ))}
        </div>
      </section>
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
            onClick={() => onRetry(state.target)}
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
    <section
      className="border-b border-white/10 py-5"
      aria-labelledby="device-details-title"
    >
      <div className="flex items-center justify-between">
        <h3
          id="device-details-title"
          className="text-sm font-semibold text-anvil-50"
        >
          {t("devices.detailTitle")}
        </h3>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => void doDisconnect(state.target)}
        >
          {t("devices.disconnect")}
        </Button>
      </div>
      {disconnectMessage && (
        <p role="status" className="mt-2 text-xs text-anvil-300">
          {disconnectMessage}
        </p>
      )}
      <dl className="mt-4 grid gap-x-10 gap-y-4 sm:grid-cols-2 lg:grid-cols-3">
        <InfoField
          label={t("devices.model")}
          value={info.model ?? t("devices.unknownModel")}
        />
        {info.manufacturer && (
          <InfoField
            label={t("devices.manufacturer")}
            value={info.manufacturer}
          />
        )}
        {info.android_version && (
          <InfoField
            label={t("devices.androidVersion")}
            value={`Android ${info.android_version}`}
          />
        )}
        {info.sdk_level && (
          <InfoField
            label={t("devices.apiLevelLabel")}
            value={t("devices.apiLevel", { level: info.sdk_level })}
          />
        )}
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
            value={formatBattery(info.battery, t("common.unknown"))}
          />
        )}
        {info.storage && (
          <InfoField
            label={t("devices.storageData")}
            value={formatStorage(info.storage, t("common.unknown"))}
          />
        )}
      </dl>
      <DeviceHealthCards info={info} />
    </section>
  );
}

function DeviceHealthCards({ info }: { info: DeviceInfo }) {
  const { t } = useTranslation();
  const battery = info.battery;
  const partitions = info.storage_partitions;
  const zones = info.thermal_zones;

  const hasBatteryHealth =
    battery != null &&
    (battery.health != null ||
      battery.cycle_count != null ||
      battery.voltage_mv != null ||
      battery.charge_counter_uah != null ||
      battery.technology != null);

  if (!hasBatteryHealth && partitions.length === 0 && zones.length === 0) {
    return null;
  }

  return (
    <div className="mt-6">
      <h4 className="text-xs font-semibold uppercase tracking-wide text-anvil-400">
        {t("devices.deviceHealth.sectionTitle")}
      </h4>
      <div className="mt-3 grid gap-4 lg:grid-cols-3">
        {hasBatteryHealth && battery && (
          <Card>
            <h5 className="text-sm font-semibold text-anvil-50">
              {t("devices.deviceHealth.batteryTitle")}
            </h5>
            {battery.level != null && (
              <HealthBar
                label={t("devices.deviceHealth.level")}
                value={`${battery.level}%`}
                fraction={battery.level / 100}
              />
            )}
            <dl className="mt-3 space-y-1.5 text-xs">
              {battery.health && (
                <HealthRow
                  label={t("devices.deviceHealth.healthLabel")}
                  value={battery.health}
                />
              )}
              {battery.cycle_count != null && (
                <HealthRow
                  label={t("devices.deviceHealth.cycleCount")}
                  value={String(battery.cycle_count)}
                />
              )}
              {battery.charge_counter_uah != null && (
                <HealthRow
                  label={t("devices.deviceHealth.capacity")}
                  value={`${Math.round(battery.charge_counter_uah / 1000)} mAh`}
                />
              )}
              {battery.voltage_mv != null && (
                <HealthRow
                  label={t("devices.deviceHealth.voltage")}
                  value={`${(battery.voltage_mv / 1000).toFixed(3)} V`}
                />
              )}
              {battery.technology && (
                <HealthRow
                  label={t("devices.deviceHealth.technology")}
                  value={battery.technology}
                />
              )}
            </dl>
          </Card>
        )}
        {partitions.length > 0 && (
          <Card>
            <h5 className="text-sm font-semibold text-anvil-50">
              {t("devices.deviceHealth.storageTitle")}
            </h5>
            <div className="mt-2 space-y-3">
              {partitions.map((partition) => {
                const fraction = partitionUsedFraction(partition);
                const detail = formatPartitionDetail(partition, t);
                return (
                  <HealthBar
                    key={partition.mount}
                    label={partition.mount}
                    value={detail}
                    fraction={fraction}
                  />
                );
              })}
            </div>
          </Card>
        )}
        {zones.length > 0 && (
          <Card>
            <h5 className="text-sm font-semibold text-anvil-50">
              {t("devices.deviceHealth.thermalTitle")}
            </h5>
            <dl className="mt-2 space-y-1.5 text-xs">
              {zones.map((zone) => (
                <div
                  key={zone.name}
                  className="flex items-center justify-between gap-3"
                >
                  <dt className="min-w-0 truncate text-anvil-300">
                    {zone.name}
                  </dt>
                  <dd className="flex items-center gap-2 text-anvil-100">
                    <span className="font-mono">
                      {zone.temperature_c.toFixed(1)}°C
                    </span>
                    {zone.status && zone.status !== "None" && (
                      <Badge tone={thermalTone(zone.status)}>
                        {zone.status}
                      </Badge>
                    )}
                  </dd>
                </div>
              ))}
            </dl>
          </Card>
        )}
      </div>
    </div>
  );
}

function HealthRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3">
      <dt className="text-anvil-400">{label}</dt>
      <dd className="text-anvil-100">{value}</dd>
    </div>
  );
}

function HealthBar({
  label,
  value,
  fraction,
}: {
  label: string;
  value: string;
  fraction: number | null;
}) {
  const pct =
    fraction == null ? null : Math.min(100, Math.max(0, fraction * 100));
  return (
    <div className="mt-2">
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="min-w-0 truncate font-medium text-anvil-200">
          {label}
        </span>
        <span className="text-anvil-400">{value}</span>
      </div>
      <div
        className="mt-1 h-1.5 overflow-hidden rounded-full bg-white/[0.08]"
        role="progressbar"
        aria-label={label}
        aria-valuenow={pct == null ? undefined : Math.round(pct)}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div
          className={cn(
            "h-full rounded-full transition-[width]",
            pct != null && pct >= 90 ? "bg-red-400/70" : "bg-circuit-300/70",
          )}
          style={{ width: `${pct ?? 0}%` }}
        />
      </div>
    </div>
  );
}

function partitionUsedFraction(
  p: DeviceInfo["storage_partitions"][number],
): number | null {
  if (p.total_kb == null || p.total_kb === 0) return null;
  if (p.used_kb != null) return p.used_kb / p.total_kb;
  if (p.available_kb != null) return 1 - p.available_kb / p.total_kb;
  return null;
}

function formatPartitionDetail(
  p: DeviceInfo["storage_partitions"][number],
  t: (key: string, opts?: Record<string, unknown>) => string,
): string {
  const total = p.total_kb != null ? formatKb(p.total_kb) : null;
  if (p.used_kb != null && total) {
    return t("devices.deviceHealth.usedOfTotal", {
      used: formatKb(p.used_kb),
      total,
    });
  }
  if (p.available_kb != null) {
    return t("devices.deviceHealth.freeSpace", {
      free: formatKb(p.available_kb),
    });
  }
  return total ?? "";
}

function thermalTone(status: string): "warning" | "danger" {
  return status === "Light" || status === "Moderate" ? "warning" : "danger";
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

function formatBattery(
  b: NonNullable<DeviceInfo["battery"]>,
  unknown: string,
): string {
  const parts: string[] = [];
  if (b.level != null) parts.push(`${b.level}%`);
  if (b.status) parts.push(b.status);
  if (b.temperature != null) parts.push(`${b.temperature}°C`);
  return parts.join(" · ") || unknown;
}

function formatStorage(
  s: NonNullable<DeviceInfo["storage"]>,
  unknown: string,
): string {
  if (s.total_kb == null || s.available_kb == null) return unknown;
  const totalGb = (s.total_kb / 1048576).toFixed(1);
  const availGb = (s.available_kb / 1048576).toFixed(1);
  return `${availGb} GB free / ${totalGb} GB`;
}
