import { useState } from "react";
import { useTranslation } from "react-i18next";

import type {
  Pack,
  PackAssessment,
  PackEntry,
  PackEntryAssessment,
  RemovalLevel,
} from "../../lib/tauri";
import {
  DEBLOAT_PRESETS,
  packagesForPreset,
  type DebloatPreset,
} from "../debloatPack";
import { Badge, Button, Card, FieldInput, StatePanel } from "../common";
import { CompatibilityChecks } from "./CompatibilityChecks";
import { compatibilityTone } from "./tones";

export function PackPreview({
  pack,
  assessment,
  selected,
  overrideAccepted,
  planError,
  onToggle,
  onApplyPreset,
  onOverrideChange,
  onExportBaseline,
  exportingBaseline,
  onApply,
  onBack,
}: {
  pack: Pack;
  assessment: PackAssessment;
  selected: Set<string>;
  overrideAccepted: boolean;
  planError: string | null;
  onToggle: (id: string) => void;
  onApplyPreset: (preset: DebloatPreset) => void;
  onOverrideChange: (accepted: boolean) => void;
  onExportBaseline: () => void;
  exportingBaseline: boolean;
  onApply: () => void;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const [entrySearch, setEntrySearch] = useState("");
  const readyIds = new Set(
    assessment.entries
      .filter((entry) => entry.status === "ready")
      .map((entry) => entry.id),
  );
  const presetMatches = DEBLOAT_PRESETS.map((preset) => ({
    preset,
    count: packagesForPreset(pack, preset, readyIds).size,
  })).filter((item) => item.count > 0);
  const query = entrySearch.trim().toLowerCase();
  const visiblePackages = query
    ? pack.packages.filter((entry) =>
        [entry.id, entry.description, ...entry.labels]
          .join(" ")
          .toLowerCase()
          .includes(query),
      )
    : pack.packages;
  const tiers = groupByTier(visiblePackages);
  const assessments = new Map(
    assessment.entries.map((entry) => [entry.id, entry]),
  );

  return (
    <>
      <Card className="px-0 py-3">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h3 className="text-lg font-semibold text-anvil-50">{pack.name}</h3>
            <p className="sr-only">{pack.description}</p>
          </div>
          <div className="flex gap-2">
            <Badge tone="info">
              {t("debloat.selected", { count: selected.size })}
            </Badge>
            <Badge tone="neutral">
              {t("common.totalCount", { count: pack.packages.length })}
            </Badge>
            <Badge tone={compatibilityTone(assessment.status)}>
              {t(`debloat.compatibility.${assessment.status}`)}
            </Badge>
          </div>
        </div>
        <p className="mt-1 text-xs text-anvil-400">
          {t("debloat.packIdentity", {
            id: pack.id,
            revision: pack.revision,
            license: pack.provenance.license,
          })}
        </p>
      </Card>

      <CompatibilityChecks assessment={assessment} />

      {presetMatches.length > 0 && (
        <Card className="px-0 py-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h4 className="text-sm font-semibold text-anvil-50">
              {t("debloat.presets.title")}
            </h4>
            <span className="sr-only">{t("debloat.presets.hint")}</span>
          </div>
          <div className="mt-3 flex flex-wrap gap-2">
            {presetMatches.map(({ preset, count }) => (
              <Button
                key={preset.id}
                type="button"
                size="sm"
                variant="secondary"
                onClick={() => onApplyPreset(preset)}
              >
                {t(`debloat.presets.${preset.id}`)}
                <Badge tone="neutral">{count}</Badge>
              </Button>
            ))}
          </div>
        </Card>
      )}

      {planError && (
        <StatePanel title={t("debloat.planFailed")} tone="danger">
          <p>{planError}</p>
        </StatePanel>
      )}

      <div className="flex flex-wrap items-center gap-3">
        <FieldInput
          type="search"
          value={entrySearch}
          onChange={(event) => setEntrySearch(event.target.value)}
          placeholder={t("debloat.searchPlaceholder")}
          aria-label={t("debloat.searchLabel")}
          className="w-72 max-w-full font-mono"
        />
        {query && (
          <span className="text-xs text-anvil-400">
            {t("debloat.searchMatches", { count: visiblePackages.length })}
          </span>
        )}
      </div>

      {query && visiblePackages.length === 0 && (
        <StatePanel title={t("debloat.noMatches")} tone="info">
          <p>{t("debloat.noMatchesBody")}</p>
        </StatePanel>
      )}

      {(["recommended", "advanced", "expert", "unsafe"] as RemovalLevel[]).map(
        (tier) => {
          const entries = tiers.get(tier);
          if (!entries?.length) return null;
          return (
            <Card key={tier} className="overflow-hidden p-0">
              <div className="flex items-center justify-between border-b border-white/10 px-3 py-2.5">
                <div className="flex items-center gap-2">
                  <Badge tone={tierTone(tier)}>
                    {t(`debloat.tiers.${tier}`)}
                  </Badge>
                  <span className="text-xs text-anvil-400">
                    {t("common.packageCount", { count: entries.length })}
                  </span>
                </div>
                <span className="text-xs text-anvil-500">
                  {t("debloat.selected", {
                    count: entries.filter((e) => selected.has(e.id)).length,
                  })}
                </span>
              </div>
              <div className="divide-y divide-white/10">
                {entries.map((entry) => {
                  const support = assessments.get(entry.id);
                  const selectable = support?.status === "ready";
                  return (
                    <label
                      key={entry.id}
                      className={`flex gap-3 px-3 py-3 transition ${
                        selectable
                          ? "cursor-pointer hover:bg-white/[0.03]"
                          : "cursor-not-allowed opacity-70"
                      }`}
                    >
                      <input
                        type="checkbox"
                        checked={selected.has(entry.id)}
                        onChange={() => onToggle(entry.id)}
                        disabled={!selectable}
                        className="mt-1 h-4 w-4 shrink-0 rounded border-white/20 bg-white/[0.06] text-circuit-300 focus:ring-2 focus:ring-circuit-300/30"
                      />
                      <div className="min-w-0 flex-1">
                        <code className="font-mono text-xs text-anvil-50">
                          {entry.id}
                        </code>
                        {support && (
                          <Badge
                            tone={entryStatusTone(support.status)}
                            className="ms-2"
                          >
                            {t(`debloat.entryStatus.${support.status}`)}
                          </Badge>
                        )}
                        <p className="mt-1 text-xs leading-5 text-anvil-400">
                          {entry.description}
                        </p>
                        {entry.needed_by.length > 0 && (
                          <p className="mt-1 text-xs text-amber-300/80">
                            {t("debloat.neededBy", {
                              items: entry.needed_by.join(", "),
                            })}
                          </p>
                        )}
                        {entry.depends_on.length > 0 && (
                          <p className="mt-1 text-xs text-circuit-100/80">
                            {t("debloat.dependsOn", {
                              items: entry.depends_on.join(", "),
                            })}
                          </p>
                        )}
                        {support?.detail && (
                          <p className="mt-1 text-xs text-red-200/80">
                            {support.detail}
                          </p>
                        )}
                        {entry.labels.length > 0 && (
                          <div className="mt-2 flex flex-wrap gap-1">
                            {entry.labels.map((l) => (
                              <span
                                key={l}
                                className="text-xs text-anvil-500 after:ms-1.5 after:content-['·'] last:after:hidden"
                              >
                                {l}
                              </span>
                            ))}
                          </div>
                        )}
                      </div>
                    </label>
                  );
                })}
              </div>
            </Card>
          );
        },
      )}

      {assessment.override_required && (
        <label className="flex items-start gap-3 rounded-lg border border-amber-300/30 bg-amber-300/[0.06] p-4 text-sm text-amber-100">
          <input
            type="checkbox"
            checked={overrideAccepted}
            onChange={(event) => onOverrideChange(event.target.checked)}
            className="mt-0.5 h-4 w-4 shrink-0 rounded border-amber-200/40 bg-anvil-950 text-amber-300 focus:ring-2 focus:ring-amber-300/30"
          />
          <span>{t("debloat.compatibilityOverride")}</span>
        </label>
      )}

      <div className="sticky bottom-0 z-10 flex flex-wrap justify-between gap-3 border-t border-white/[0.1] bg-anvil-950/95 px-1 py-3 backdrop-blur">
        <Button type="button" onClick={onBack}>
          {t("debloat.back")}
        </Button>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant="ghost"
            onClick={onExportBaseline}
            disabled={
              selected.size === 0 ||
              exportingBaseline ||
              (assessment.override_required && !overrideAccepted)
            }
          >
            {exportingBaseline
              ? t("debloat.recoveryExporting")
              : t("debloat.recoveryExport")}
          </Button>
          <Button
            type="button"
            variant="primary"
            onClick={onApply}
            disabled={
              selected.size === 0 ||
              exportingBaseline ||
              (assessment.override_required && !overrideAccepted)
            }
          >
            {t("debloat.applyCount", { count: selected.size })}
          </Button>
        </div>
      </div>
    </>
  );
}

function entryStatusTone(
  status: PackEntryAssessment["status"],
): "success" | "warning" | "danger" {
  switch (status) {
    case "ready":
      return "success";
    case "missing":
      return "warning";
    case "unsupported":
      return "danger";
  }
}

function groupByTier(entries: PackEntry[]): Map<RemovalLevel, PackEntry[]> {
  const map = new Map<RemovalLevel, PackEntry[]>();
  for (const entry of entries) {
    const list = map.get(entry.removal) ?? [];
    list.push(entry);
    map.set(entry.removal, list);
  }
  return map;
}

function tierTone(
  tier: RemovalLevel,
): "success" | "info" | "warning" | "danger" {
  switch (tier) {
    case "recommended":
      return "success";
    case "advanced":
      return "info";
    case "expert":
      return "warning";
    case "unsafe":
      return "danger";
  }
}
