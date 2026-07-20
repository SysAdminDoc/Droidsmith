import type { Pack } from "../lib/tauri";

export type PackSelectionSummary = {
  total: number;
  unsafeIds: string[];
};

/**
 * IMP-63: curated debloat presets. Each preset matches pack entries whose
 * free-form `labels` intersect its tag set, so selecting one pre-checks a
 * themed group (privacy, bloat, de-Google, carrier) that the user then reviews
 * and edits before applying. Presets compose with the existing tier/pack model
 * — they are purely a selection convenience.
 */
export type DebloatPreset = {
  id: "privacy" | "bloatware" | "degoogle" | "carrier";
  tags: string[];
};

export const DEBLOAT_PRESETS: readonly DebloatPreset[] = [
  {
    id: "privacy",
    tags: ["telemetry", "ads", "analytics", "tracking", "facebook"],
  },
  { id: "bloatware", tags: ["bloat", "cosmetic", "preload", "demo", "gaming"] },
  { id: "degoogle", tags: ["google"] },
  { id: "carrier", tags: ["carrier", "regional"] },
];

/**
 * Ready package ids in `pack` whose labels intersect the preset's tags. Only
 * ready ids (present + supported) are eligible so a preset never pre-checks a
 * package the device can't act on.
 */
export function packagesForPreset(
  pack: Pack,
  preset: DebloatPreset,
  readyIds: ReadonlySet<string>,
): Set<string> {
  const tags = new Set(preset.tags);
  return new Set(
    pack.packages
      .filter(
        (entry) =>
          readyIds.has(entry.id) &&
          entry.labels.some((label) => tags.has(label)),
      )
      .map((entry) => entry.id),
  );
}

export function summarizePackSelection(
  pack: Pack,
  selected: Iterable<string>,
): PackSelectionSummary {
  const selectedIds = new Set(selected);
  const knownIds = new Set(pack.packages.map((entry) => entry.id));
  for (const id of selectedIds) {
    if (!knownIds.has(id)) {
      throw new Error(`Selected package ${id} is not in pack ${pack.id}`);
    }
  }
  return {
    total: selectedIds.size,
    unsafeIds: pack.packages
      .filter(
        (entry) => entry.removal === "unsafe" && selectedIds.has(entry.id),
      )
      .map((entry) => entry.id),
  };
}

/** Expand the recursive pack dependency closure without changing pack order. */
export function expandPackDependencies(
  pack: Pack,
  selected: Iterable<string>,
): Set<string> {
  const entries = new Map(pack.packages.map((entry) => [entry.id, entry]));
  const expanded = new Set<string>();
  const visiting = new Set<string>();

  const visit = (id: string) => {
    if (expanded.has(id)) return;
    const entry = entries.get(id);
    if (!entry) throw new Error(`Package ${id} is not in pack ${pack.id}`);
    if (visiting.has(id)) {
      throw new Error(`Dependency cycle includes package ${id}`);
    }
    visiting.add(id);
    entry.depends_on.forEach(visit);
    visiting.delete(id);
    expanded.add(id);
  };

  for (const id of selected) visit(id);
  return expanded;
}
