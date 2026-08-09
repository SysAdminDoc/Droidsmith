import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callExplainPackageHazards,
  callListRunningServices,
  errorMessage,
  type DeviceTarget,
  type Pack,
  type PackAssessment,
  type Quirk,
} from "../../lib/tauri";
import { useTargetOperation } from "../../lib/targetOperation";
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

type HazardState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; quirks: Quirk[] }
  | { kind: "error"; message: string };

export function DebloatApplyReview({
  pack,
  assessment,
  selected,
  target,
  deviceContext,
  onCancel,
  onConfirm,
}: {
  pack: Pack;
  assessment: PackAssessment;
  selected: Set<string>;
  target: DeviceTarget | null;
  deviceContext: { manufacturer: string | null; rom: string | null };
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const trapRef = useFocusTrap<HTMLDivElement>();
  const serviceOperation = useTargetOperation(target, "running-services");
  const hazardOperation = useTargetOperation(target, "package-hazards");
  const assessmentById = new Map(
    assessment.entries.map((entry) => [entry.id, entry]),
  );
  const runtimeUnsafe = assessment.entries
    .filter((entry) => entry.effective_removal === "unsafe")
    .map((entry) => entry.id);
  const summary = summarizePackSelection(pack, selected, runtimeUnsafe);
  const [unsafeAcknowledged, setUnsafeAcknowledged] = useState<Set<string>>(
    () => new Set(),
  );
  const hasUnsafe = summary.unsafeIds.length > 0;
  const confirmBlocked = summary.unsafeIds.some(
    (id) => !unsafeAcknowledged.has(id),
  );

  const [hazards, setHazards] = useState<HazardState>({ kind: "idle" });
  useEffect(() => {
    if (!target || selected.size === 0) {
      setHazards({ kind: "idle" });
      return;
    }
    const lease = hazardOperation.begin();
    setHazards({ kind: "loading" });
    void callExplainPackageHazards({
      manufacturer: deviceContext.manufacturer,
      rom: deviceContext.rom,
      package_ids: [...selected].sort(),
    })
      .then((quirks) =>
        lease.commit(() => setHazards({ kind: "ready", quirks })),
      )
      .catch((error: unknown) =>
        lease.commit(() =>
          setHazards({ kind: "error", message: errorMessage(error) }),
        ),
      );
    return () => void hazardOperation.invalidate();
  }, [deviceContext, hazardOperation, selected, target]);

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
    const lease = serviceOperation.begin();
    setServices({ kind: "loading" });
    void (async () => {
      try {
        const results = await Promise.all(
          probeList.map(async (pkg) => ({
            package: pkg,
            count: (await callListRunningServices(target, pkg)).length,
          })),
        );
        lease.commit(() =>
          setServices({
            kind: "ready",
            running: results.filter((entry) => entry.count > 0),
            probed: probeList.length,
            total: packages.length,
          }),
        );
      } catch (error) {
        lease.commit(() =>
          setServices({ kind: "error", message: errorMessage(error) }),
        );
      }
    })();
    return () => void serviceOperation.invalidate();
  }, [serviceOperation, target, selected]);

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
          {t("debloat.confirmApplyBodyActions", { name: pack.name })}
        </p>
        <dl className="mt-4 grid grid-cols-2 gap-3">
          <div className="rounded-md border border-white/10 bg-white/[0.04] p-3">
            <dt className="text-xs text-anvil-400">
              {t("debloat.confirmTotalActions")}
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
            <ul className="mt-3 space-y-2">
              {summary.unsafeIds.map((id) => {
                const runtimeEvidence = assessmentById.get(id);
                return (
                  <li key={id}>
                    <label className="flex cursor-pointer items-start gap-2 border border-red-300/20 bg-red-950/20 p-2.5 text-xs leading-5 text-red-100">
                      <input
                        type="checkbox"
                        checked={unsafeAcknowledged.has(id)}
                        onChange={(event) => {
                          setUnsafeAcknowledged((current) => {
                            const next = new Set(current);
                            if (event.target.checked) next.add(id);
                            else next.delete(id);
                            return next;
                          });
                        }}
                        className="mt-0.5 h-4 w-4 shrink-0 rounded border-red-300/40 bg-red-300/10 text-red-400 focus:ring-2 focus:ring-red-300/40"
                      />
                      <span className="min-w-0">
                        <code className="block break-all font-mono text-xs text-red-50">
                          {id}
                        </code>
                        {runtimeEvidence?.shared_system_uid && (
                          <span className="mt-1 block text-red-100/80">
                            {t("debloat.sharedSystemUidReason")}
                          </span>
                        )}
                        <span className="mt-1 block">
                          {t("debloat.unsafeAcknowledgePackage")}
                        </span>
                      </span>
                    </label>
                  </li>
                );
              })}
            </ul>
          </div>
        ) : (
          <p className="mt-4 rounded-md border border-circuit-300/20 bg-circuit-300/[0.06] p-3 text-sm text-circuit-100">
            {t("debloat.noUnsafeSelected")}
          </p>
        )}
        {hazards.kind === "loading" && (
          <p className="mt-4 text-xs text-anvil-400" role="status">
            {t("debloat.hazardsChecking")}
          </p>
        )}
        {hazards.kind === "error" && (
          <p className="mt-4 text-xs text-anvil-400" role="status">
            {t("debloat.hazardsError", { message: hazards.message })}
          </p>
        )}
        {hazards.kind === "ready" && hazards.quirks.length > 0 && (
          <section
            className="mt-4 rounded-md border border-red-300/25 bg-red-950/20 p-3"
            aria-labelledby="debloat-package-hazards-title"
          >
            <h4
              id="debloat-package-hazards-title"
              className="text-sm font-semibold text-red-100"
            >
              {t("debloat.hazardsTitle")}
            </h4>
            <ul className="mt-3 space-y-3">
              {hazards.quirks.map((quirk) => {
                const affected = (quirk.matches?.package_id ?? []).filter(
                  (id) => selected.has(id),
                );
                const documentation =
                  quirk.mitigation?.kind === "documentation"
                    ? quirk.mitigation
                    : null;
                return (
                  <li
                    key={quirk.id}
                    className="border-s-2 border-red-300/35 ps-3"
                  >
                    <p className="text-xs font-semibold text-red-50">
                      {quirk.title}
                    </p>
                    {affected.length > 0 && (
                      <p className="mt-1 break-all font-mono text-[11px] text-red-100/75">
                        {t("debloat.hazardsAffected", {
                          packages: affected.join(", "),
                        })}
                      </p>
                    )}
                    <p className="mt-1 whitespace-pre-wrap text-xs leading-5 text-red-100/85">
                      {quirk.explanation}
                    </p>
                    {documentation && (
                      <a
                        className="mt-2 inline-flex text-xs font-medium text-red-100 underline underline-offset-2"
                        href={documentation.url}
                        target="_blank"
                        rel="noreferrer"
                      >
                        {t("debloat.hazardsSource")}
                      </a>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
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
            {t("debloat.confirmApply", { count: summary.total })}
          </Button>
        </div>
      </div>
    </div>
  );
}
