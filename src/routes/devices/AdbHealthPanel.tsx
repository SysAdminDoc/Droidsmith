// ADB server health summary panel (IMP-72: extracted verbatim from the former
// Devices.tsx god-file).

import { useTranslation } from "react-i18next";

import { cn } from "../../lib/cn";
import { formatDateTime } from "../../lib/i18n";
import type { AdbHealth } from "../../lib/tauri";
import { SkeletonLine } from "../common";
import { HealthCheckIcon } from "./icons";

export function AdbHealthPanel({
  health,
  observedAt,
  watching,
}: {
  health: AdbHealth | null;
  observedAt: string | null;
  watching: boolean;
}) {
  const { t, i18n } = useTranslation();
  return (
    <section
      className="border-b border-white/[0.08] py-5"
      aria-labelledby="adb-health-title"
    >
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-2.5">
          <HealthCheckIcon healthy={watching} />
          <div>
            <h3
              id="adb-health-title"
              className="text-sm font-semibold text-anvil-50"
            >
              {t("devices.health.title")}
            </h3>
            {!watching && (
              <p className="mt-0.5 text-xs text-amber-200">
                {t("devices.health.stopped")}
              </p>
            )}
          </div>
        </div>
        <p className="text-xs text-anvil-500">
          {observedAt
            ? t("devices.health.observed", {
                time: formatDateTime(
                  observedAt,
                  i18n.resolvedLanguage ?? i18n.language,
                ),
              })
            : t("devices.health.probing")}
        </p>
      </div>

      {health ? (
        <dl className="mt-4 grid gap-y-4 text-xs sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-8">
          <HealthMetric
            label={t("devices.health.client")}
            value={health.client_version ?? t("common.notReported")}
          />
          <HealthMetric
            label={t("devices.health.server")}
            value={health.server_version ?? t("common.notReported")}
          />
          <HealthMetric
            label={t("devices.health.usbBackend")}
            value={health.usb_backend ?? t("common.notReported")}
          />
          <HealthMetric
            label={t("devices.health.mdnsBackend")}
            value={health.mdns_backend ?? t("common.notReported")}
          />
          <HealthMetric
            label={t("devices.health.mdns")}
            value={
              health.mdns_enabled == null
                ? t("common.notReported")
                : health.mdns_enabled
                  ? t("devices.health.enabled")
                  : t("devices.health.disabled")
            }
          />
          <HealthMetric
            label={t("devices.health.wifiTwo")}
            value={t(`devices.health.wifiTwoState.${health.wifi_v2_state}`)}
          />
          <HealthMetric
            label={t("devices.health.wifiTwoDevices")}
            value={
              health.wifi_v2_devices.join(", ") ||
              t("devices.health.noneDetected")
            }
          />
          <HealthMetric
            label={t("devices.health.mdnsCheck")}
            value={health.mdns_check ?? t("common.notReported")}
          />
        </dl>
      ) : (
        <div className="mt-4 grid gap-2 sm:grid-cols-3" aria-hidden="true">
          <SkeletonLine />
          <SkeletonLine />
          <SkeletonLine />
        </div>
      )}
      {health?.warning && (
        <p
          role="status"
          className="mt-4 border-s-2 border-amber-300/70 ps-3 text-xs text-amber-100"
        >
          {health.warning}
        </p>
      )}
      {health && (
        <p
          className={cn(
            "mt-3 border-s-2 ps-3 text-xs",
            health.platform_tools.status === "blocked"
              ? "border-red-300/70 text-red-100"
              : health.platform_tools.status === "warn"
                ? "border-amber-300/70 text-amber-100"
                : "border-emerald-300/70 text-anvil-300",
          )}
        >
          {health.platform_tools.rationale}{" "}
          <a
            className="font-medium underline underline-offset-2"
            href={health.platform_tools.source_url}
            target="_blank"
            rel="noreferrer"
          >
            {t("devices.health.policyLink", {
              date: health.platform_tools.policy_reviewed_on,
            })}
          </a>
        </p>
      )}
    </section>
  );
}

function HealthMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 border-s border-white/[0.08] px-3 first:border-s-0 first:ps-0">
      <dt className="text-[11px] leading-4 text-anvil-500">{label}</dt>
      <dd className="mt-1 break-words text-[13px] font-medium leading-5 text-anvil-100">
        {value}
      </dd>
    </div>
  );
}
