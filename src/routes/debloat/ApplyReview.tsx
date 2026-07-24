import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callListRunningServices,
  errorMessage,
  type DeviceTarget,
  type Pack,
} from "../../lib/tauri";
import { useFocusTrap } from "../../lib/useFocusTrap";
import { summarizePackSelection } from "../debloatPack";
import { Badge, Button } from "../common";

// Bound the per-package `dumpsys` probes so a large pack selection cannot fan
// out into hundreds of shell calls when the review opens.
const MAX_SERVICE_PROBE = 16;

type ServiceProbeState =
  | { kind: "idle" }
  | { kind: "loading" }
  | {
      kind: "ready";
      running: { package: string; count: number }[];
      probed: number;
      total: number;
    }
  | { kind: "error"; message: string };

export function DebloatApplyReview({
  pack,
  selected,
  target,
  onCancel,
  onConfirm,
}: {
  pack: Pack;
  selected: Set<string>;
  target: DeviceTarget | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const trapRef = useFocusTrap<HTMLDivElement>();
  const summary = summarizePackSelection(pack, selected);
  const [unsafeAcknowledged, setUnsafeAcknowledged] = useState(false);
  const hasUnsafe = summary.unsafeIds.length > 0;
  const confirmBlocked = hasUnsafe && !unsafeAcknowledged;

  // R-112: surface which selected apps are running services right now, so the
  // reviewer knows a disable won't stop live services until reboot/force-stop.
  const [services, setServices] = useState<ServiceProbeState>({ kind: "idle" });
  useEffect(() => {
    if (!target) {
      setServices({ kind: "idle" });
      return;
    }
    const packages = [...selected].sort();
    if (packages.length === 0) {
      setServices({ kind: "idle" });
      return;
    }
    const probeList = packages.slice(0, MAX_SERVICE_PROBE);
    let cancelled = false;
    setServices({ kind: "loading" });
    void (async () => {
      try {
        const results = await Promise.all(
          probeList.map(async (pkg) => ({
            package: pkg,
            count: (await callListRunningServices(target, pkg)).length,
          })),
        );
        if (cancelled) return;
        setServices({
          kind: "ready",
          running: results.filter((entry) => entry.count > 0),
          probed: probeList.length,
          total: packages.length,
        });
      } catch (error) {
        if (cancelled) return;
        setServices({ kind: "error", message: errorMessage(error) });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [target, selected]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 p-4 backdrop-blur-sm">
      <div
        ref={trapRef}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="debloat-apply-review-title"
        aria-describedby="debloat-apply-review-description"
        tabIndex={-1}
        className="max-h-[calc(100vh-2rem)] w-full max-w-xl overflow-y-auto overscroll-contain rounded-lg border border-amber-300/25 bg-anvil-950 p-5 shadow-2xl outline-none"
      >
        <Badge tone={summary.unsafeIds.length > 0 ? "danger" : "warning"}>
          {t("debloat.reviewBeforeApply")}
        </Badge>
        <h3
          id="debloat-apply-review-title"
          className="mt-4 text-lg font-semibold text-anvil-50"
        >
          {t("debloat.confirmApplyTitle")}
        </h3>
        <p
          id="debloat-apply-review-description"
          className="mt-2 text-sm leading-6 text-anvil-300"
        >
          {t("debloat.confirmApplyBody", { name: pack.name })}
        </p>
        <dl className="mt-4 grid grid-cols-2 gap-3">
          <div className="rounded-md border border-white/10 bg-white/[0.04] p-3">
            <dt className="text-xs text-anvil-400">
              {t("debloat.confirmTotal")}
            </dt>
            <dd className="mt-1 text-xl font-semibold text-anvil-50">
              {summary.total}
            </dd>
          </div>
          <div className="rounded-md border border-red-300/20 bg-red-300/[0.06] p-3">
            <dt className="text-xs text-red-200">
              {t("debloat.confirmUnsafe")}
            </dt>
            <dd className="mt-1 text-xl font-semibold text-red-100">
              {summary.unsafeIds.length}
            </dd>
          </div>
        </dl>
        {summary.unsafeIds.length > 0 ? (
          <div className="mt-4 rounded-md border border-red-300/25 bg-red-300/[0.08] p-3">
            <p className="text-sm font-semibold text-red-100">
              {t("debloat.unsafeSelectedTitle")}
            </p>
            <p className="mt-1 text-xs leading-5 text-red-100/80">
              {t("debloat.unsafeSelectedBody")}
            </p>
            <ul className="mt-3 space-y-1">
              {summary.unsafeIds.map((id) => (
                <li key={id}>
                  <code className="break-all font-mono text-xs text-red-50">
                    {id}
                  </code>
                </li>
              ))}
            </ul>
            <label className="mt-3 flex items-start gap-2 text-xs leading-5 text-red-100">
              <input
                type="checkbox"
                checked={unsafeAcknowledged}
                onChange={(event) =>
                  setUnsafeAcknowledged(event.target.checked)
                }
                className="mt-0.5 h-4 w-4 shrink-0 rounded border-red-300/40 bg-red-300/10 text-red-400 focus:ring-2 focus:ring-red-300/40"
              />
              <span>{t("debloat.unsafeAcknowledge")}</span>
            </label>
          </div>
        ) : (
          <p className="mt-4 rounded-md border border-circuit-300/20 bg-circuit-300/[0.06] p-3 text-sm text-circuit-100">
            {t("debloat.noUnsafeSelected")}
          </p>
        )}
        {services.kind === "loading" && (
          <p className="mt-4 text-xs text-anvil-400" role="status">
            {t("debloat.servicesChecking")}
          </p>
        )}
        {services.kind === "error" && (
          <p className="mt-4 text-xs text-anvil-400" role="status">
            {t("debloat.servicesError", { message: services.message })}
          </p>
        )}
        {services.kind === "ready" && services.running.length > 0 && (
          <div
            className="mt-4 rounded-md border border-amber-300/25 bg-amber-950/20 p-3"
            role="status"
          >
            <p className="text-sm font-semibold text-amber-200">
              {t("debloat.servicesTitle")}
            </p>
            <p className="mt-1 text-xs leading-5 text-amber-100/90">
              {t("debloat.servicesBody")}
            </p>
            <ul className="mt-3 space-y-1">
              {services.running.map((entry) => (
                <li
                  key={entry.package}
                  className="flex items-center justify-between gap-2"
                >
                  <code className="break-all font-mono text-xs text-amber-100">
                    {entry.package}
                  </code>
                  <span
                    className="shrink-0 rounded bg-amber-300/15 px-1.5 py-0.5 text-xs font-medium text-amber-100"
                    aria-label={t("debloat.servicesRunningAria", {
                      count: entry.count,
                    })}
                  >
                    {entry.count}
                  </span>
                </li>
              ))}
            </ul>
            {services.probed < services.total && (
              <p className="mt-2 text-xs text-amber-100/70">
                {t("debloat.servicesTruncated", {
                  probed: services.probed,
                  total: services.total,
                })}
              </p>
            )}
          </div>
        )}
        {services.kind === "ready" && services.running.length === 0 && (
          <p className="mt-4 text-xs text-anvil-400" role="status">
            {t("debloat.servicesNone")}
            {services.probed < services.total
              ? ` ${t("debloat.servicesTruncated", {
                  probed: services.probed,
                  total: services.total,
                })}`
              : ""}
          </p>
        )}
        <p className="mt-4 text-xs leading-5 text-anvil-400">
          {t("debloat.confirmJournal")}
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant={hasUnsafe ? "danger" : "primary"}
            onClick={onConfirm}
            disabled={confirmBlocked}
          >
            {t("debloat.confirmDisable", { count: summary.total })}
          </Button>
        </div>
      </div>
    </div>
  );
}
