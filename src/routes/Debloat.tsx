import { useCallback, useEffect, useState } from "react";

import {
  callApplyAction,
  callListDevices,
  callListPacks,
  callPlanAction,
  inTauri,
  type ActionKind,
  type Device,
  type ListDevicesResult,
  type Pack,
  type PackEntry,
  type RemovalLevel,
} from "../lib/tauri";

import {
  Badge,
  Button,
  Card,
  PaneHeader,
  SkeletonLine,
  StatePanel,
} from "./common";

type DevicesState =
  | { kind: "loading" }
  | { kind: "no_tauri" }
  | { kind: "ok"; value: ListDevicesResult }
  | { kind: "error"; message: string };

type PacksState =
  | { kind: "loading" }
  | { kind: "ok"; packs: Pack[] }
  | { kind: "error"; message: string };

type WizardStep =
  | { step: "pick_pack" }
  | { step: "preview"; pack: Pack; selected: Set<string> }
  | {
      step: "applying";
      pack: Pack;
      queue: PackEntry[];
      applied: number;
      errors: string[];
    }
  | { step: "done"; pack: Pack; applied: number; errors: string[] };

export default function DebloatRoute() {
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [packsState, setPacksState] = useState<PacksState>({ kind: "loading" });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [wizard, setWizard] = useState<WizardStep>({ step: "pick_pack" });

  const loadDevices = useCallback(async () => {
    if (!inTauri()) {
      setDevicesState({ kind: "no_tauri" });
      return;
    }
    setDevicesState({ kind: "loading" });
    try {
      const value = await callListDevices();
      setDevicesState({ kind: "ok", value });
      const authorized = value.devices.filter(
        (d) => typeof d.state === "string" && d.state === "device",
      );
      if (authorized.length === 1 && !selectedSerial) {
        setSelectedSerial(authorized[0]!.serial);
      }
    } catch (e) {
      setDevicesState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedSerial]);

  const loadPacks = useCallback(async () => {
    if (!inTauri()) return;
    setPacksState({ kind: "loading" });
    try {
      const packs = await callListPacks();
      setPacksState({ kind: "ok", packs });
    } catch (e) {
      setPacksState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  useEffect(() => {
    void loadDevices();
    void loadPacks();
  }, [loadDevices, loadPacks]);

  const selectPack = useCallback((pack: Pack) => {
    const recommended = new Set(
      pack.packages.filter((p) => p.removal === "recommended").map((p) => p.id),
    );
    setWizard({ step: "preview", pack, selected: recommended });
  }, []);

  const toggleEntry = useCallback((id: string) => {
    setWizard((prev) => {
      if (prev.step !== "preview") return prev;
      const next = new Set(prev.selected);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return { ...prev, selected: next };
    });
  }, []);

  const applyPack = useCallback(async () => {
    if (wizard.step !== "preview" || !selectedSerial) return;
    const queue = wizard.pack.packages.filter((p) => wizard.selected.has(p.id));
    setWizard({
      step: "applying",
      pack: wizard.pack,
      queue,
      applied: 0,
      errors: [],
    });

    let applied = 0;
    const errors: string[] = [];

    for (const entry of queue) {
      try {
        const plan = await callPlanAction({
          serial: selectedSerial,
          package: entry.id,
          kind: "disable" as ActionKind,
        });
        await callApplyAction(plan);
        applied++;
      } catch (e) {
        errors.push(
          `${entry.id}: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
      setWizard((prev) =>
        prev.step === "applying"
          ? { ...prev, applied: applied, errors: [...errors] }
          : prev,
      );
    }

    setWizard({ step: "done", pack: wizard.pack, applied, errors });
  }, [wizard, selectedSerial]);

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];

  return (
    <>
      <PaneHeader
        title="Debloat"
        milestone="R-033"
        description="Choose an OEM-aware pack, understand the risk before applying it, and keep every change reversible through the journal."
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {selectedSerial && (
              <Badge tone="info">
                <code className="font-mono">{selectedSerial}</code>
              </Badge>
            )}
            {packsState.kind === "ok" && (
              <Badge tone="neutral">{packsState.packs.length} packs</Badge>
            )}
          </div>
        }
      />

      <section className="mt-6 max-w-7xl space-y-4">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title="Desktop shell required" tone="info">
            <p>Debloat operations run inside the Tauri runtime.</p>
          </StatePanel>
        )}

        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title="No authorized devices" tone="warning">
            <p>Connect a device and accept the USB debugging prompt.</p>
          </StatePanel>
        )}

        {authorizedDevices.length > 1 && (
          <DevicePicker
            devices={authorizedDevices}
            selected={selectedSerial}
            onSelect={setSelectedSerial}
          />
        )}

        {selectedSerial && wizard.step === "pick_pack" && (
          <PackPicker
            state={packsState}
            onSelect={selectPack}
            onRefresh={() => void loadPacks()}
          />
        )}

        {wizard.step === "preview" && (
          <PackPreview
            pack={wizard.pack}
            selected={wizard.selected}
            onToggle={toggleEntry}
            onApply={() => void applyPack()}
            onBack={() => setWizard({ step: "pick_pack" })}
          />
        )}

        {wizard.step === "applying" && (
          <ApplyProgress
            pack={wizard.pack}
            total={wizard.queue.length}
            applied={wizard.applied}
            errors={wizard.errors}
          />
        )}

        {wizard.step === "done" && (
          <ApplyResult
            pack={wizard.pack}
            applied={wizard.applied}
            errors={wizard.errors}
            onReset={() => setWizard({ step: "pick_pack" })}
          />
        )}
      </section>
    </>
  );
}

function DevicePicker({
  devices,
  selected,
  onSelect,
}: {
  devices: Device[];
  selected: string | null;
  onSelect: (serial: string) => void;
}) {
  return (
    <Card className="p-4">
      <h3 className="text-sm font-semibold text-anvil-50">Select device</h3>
      <div className="mt-3 flex flex-wrap gap-2">
        {devices.map((d) => (
          <Button
            key={d.serial}
            type="button"
            variant={d.serial === selected ? "primary" : "secondary"}
            size="sm"
            onClick={() => onSelect(d.serial)}
          >
            {d.model ?? d.serial}
          </Button>
        ))}
      </div>
    </Card>
  );
}

function PackPicker({
  state,
  onSelect,
  onRefresh,
}: {
  state: PacksState;
  onSelect: (pack: Pack) => void;
  onRefresh: () => void;
}) {
  if (state.kind === "loading") {
    return (
      <Card className="p-5">
        <SkeletonLine className="w-40" />
        <div className="mt-4 grid gap-3 sm:grid-cols-2">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="rounded-lg border border-white/10 p-4">
              <SkeletonLine className="w-32" />
              <SkeletonLine className="mt-3 w-full" />
            </div>
          ))}
        </div>
      </Card>
    );
  }

  if (state.kind === "error") {
    return (
      <StatePanel
        title="Could not load packs"
        tone="danger"
        actions={
          <Button type="button" size="sm" variant="danger" onClick={onRefresh}>
            Retry
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  if (state.packs.length === 0) {
    return (
      <StatePanel title="No debloat packs available" tone="info">
        <p>
          Drop <code>.yaml</code> pack files into the <code>packs/</code>{" "}
          directory. See <code>packs/_example.yaml</code> for the schema.
        </p>
      </StatePanel>
    );
  }

  return (
    <Card className="p-5">
      <h3 className="text-sm font-semibold text-anvil-50">
        Choose a debloat pack
      </h3>
      <p className="mt-1 text-xs text-anvil-400">
        Each pack targets a specific OEM or ROM. Packages are grouped by removal
        safety tier.
      </p>
      <div className="mt-4 grid gap-3 sm:grid-cols-2">
        {state.packs.map((pack) => (
          <button
            key={pack.name}
            type="button"
            onClick={() => onSelect(pack)}
            className="group rounded-lg border border-white/10 bg-white/[0.02] p-4 text-left transition hover:border-circuit-300/30 hover:bg-circuit-300/[0.04] focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300"
          >
            <div className="flex items-start justify-between gap-3">
              <h4 className="text-sm font-semibold text-anvil-50">
                {pack.name}
              </h4>
              <Badge tone="neutral">{pack.packages.length} pkgs</Badge>
            </div>
            <p className="mt-2 line-clamp-3 text-xs leading-5 text-anvil-400">
              {pack.description}
            </p>
            <div className="mt-3 flex flex-wrap gap-1.5">
              {pack.targets.manufacturer.map((m) => (
                <Badge key={m} tone="info">
                  {m}
                </Badge>
              ))}
              {pack.targets.rom.map((r) => (
                <Badge key={r} tone="neutral">
                  {r}
                </Badge>
              ))}
            </div>
          </button>
        ))}
      </div>
    </Card>
  );
}

function PackPreview({
  pack,
  selected,
  onToggle,
  onApply,
  onBack,
}: {
  pack: Pack;
  selected: Set<string>;
  onToggle: (id: string) => void;
  onApply: () => void;
  onBack: () => void;
}) {
  const tiers = groupByTier(pack.packages);

  return (
    <>
      <Card className="p-5">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h3 className="text-lg font-semibold text-anvil-50">{pack.name}</h3>
            <p className="mt-1 text-sm text-anvil-400">{pack.description}</p>
          </div>
          <div className="flex gap-2">
            <Badge tone="info">{selected.size} selected</Badge>
            <Badge tone="neutral">{pack.packages.length} total</Badge>
          </div>
        </div>
      </Card>

      {(["recommended", "advanced", "expert", "unsafe"] as RemovalLevel[]).map(
        (tier) => {
          const entries = tiers.get(tier);
          if (!entries?.length) return null;
          return (
            <Card key={tier} className="overflow-hidden p-0">
              <div className="flex items-center justify-between border-b border-white/10 p-4">
                <div className="flex items-center gap-2">
                  <Badge tone={tierTone(tier)}>
                    {tier.charAt(0).toUpperCase() + tier.slice(1)}
                  </Badge>
                  <span className="text-xs text-anvil-400">
                    {entries.length} packages
                  </span>
                </div>
                <span className="text-xs text-anvil-500">
                  {entries.filter((e) => selected.has(e.id)).length} selected
                </span>
              </div>
              <div className="divide-y divide-white/10">
                {entries.map((entry) => (
                  <label
                    key={entry.id}
                    className="flex cursor-pointer gap-3 p-4 transition hover:bg-white/[0.03]"
                  >
                    <input
                      type="checkbox"
                      checked={selected.has(entry.id)}
                      onChange={() => onToggle(entry.id)}
                      className="mt-1 h-4 w-4 shrink-0 rounded border-white/20 bg-white/[0.06] text-circuit-300 focus:ring-2 focus:ring-circuit-300/30"
                    />
                    <div className="min-w-0 flex-1">
                      <code className="font-mono text-xs text-anvil-50">
                        {entry.id}
                      </code>
                      <p className="mt-1 text-xs leading-5 text-anvil-400">
                        {entry.description}
                      </p>
                      {entry.needed_by.length > 0 && (
                        <p className="mt-1 text-[11px] text-amber-300/80">
                          Needed by: {entry.needed_by.join(", ")}
                        </p>
                      )}
                      {entry.labels.length > 0 && (
                        <div className="mt-2 flex flex-wrap gap-1">
                          {entry.labels.map((l) => (
                            <span
                              key={l}
                              className="rounded-md border border-white/10 px-1.5 py-0.5 text-[10px] text-anvil-500"
                            >
                              {l}
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                  </label>
                ))}
              </div>
            </Card>
          );
        },
      )}

      <div className="flex justify-between">
        <Button type="button" onClick={onBack}>
          Back
        </Button>
        <Button
          type="button"
          variant="primary"
          onClick={onApply}
          disabled={selected.size === 0}
        >
          Apply {selected.size} {selected.size === 1 ? "package" : "packages"}
        </Button>
      </div>
    </>
  );
}

function ApplyProgress({
  pack,
  total,
  applied,
  errors,
}: {
  pack: Pack;
  total: number;
  applied: number;
  errors: string[];
}) {
  const pct = total > 0 ? Math.round((applied / total) * 100) : 0;
  return (
    <Card className="p-5">
      <h3 className="text-sm font-semibold text-anvil-50">
        Applying {pack.name}
      </h3>
      <div className="mt-4">
        <div className="flex items-center justify-between text-xs text-anvil-400">
          <span>
            {applied} / {total} packages
          </span>
          <span>{pct}%</span>
        </div>
        <div className="mt-2 h-2 overflow-hidden rounded-sm bg-white/[0.08]">
          <div
            className="h-full rounded-sm bg-circuit-300 transition-all duration-300"
            style={{ width: `${pct}%` }}
          />
        </div>
      </div>
      {errors.length > 0 && (
        <div className="mt-4">
          <p className="text-xs font-medium text-red-300">
            {errors.length} {errors.length === 1 ? "error" : "errors"}
          </p>
          <ul className="mt-2 space-y-1">
            {errors.map((e) => (
              <li key={e} className="font-mono text-[11px] text-red-200/70">
                {e}
              </li>
            ))}
          </ul>
        </div>
      )}
    </Card>
  );
}

function ApplyResult({
  pack,
  applied,
  errors,
  onReset,
}: {
  pack: Pack;
  applied: number;
  errors: string[];
  onReset: () => void;
}) {
  return (
    <>
      <StatePanel
        title={`${pack.name} — debloat complete`}
        tone={errors.length > 0 ? "warning" : "success"}
        actions={
          <Button type="button" size="sm" onClick={onReset}>
            Start over
          </Button>
        }
      >
        <p>
          {applied} {applied === 1 ? "package" : "packages"} disabled
          successfully.
          {errors.length > 0 &&
            ` ${errors.length} ${errors.length === 1 ? "package" : "packages"} failed.`}
        </p>
        <p className="mt-2">
          All changes are recorded in the per-device journal and can be undone
          from the Apps tab.
        </p>
      </StatePanel>

      {errors.length > 0 && (
        <Card className="p-4">
          <h4 className="text-xs font-semibold text-red-200">
            Failed packages
          </h4>
          <ul className="mt-2 space-y-1">
            {errors.map((e) => (
              <li key={e} className="font-mono text-[11px] text-red-200/70">
                {e}
              </li>
            ))}
          </ul>
        </Card>
      )}
    </>
  );
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
