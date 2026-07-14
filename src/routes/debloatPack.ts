import type { Pack } from "../lib/tauri";

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
