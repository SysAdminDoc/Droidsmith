import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";

import {
  callApplyAction,
  callListDevices,
  callListPackages,
  callPlanAction,
  inTauri,
  type ActionKind,
  type AppPackage,
  type Device,
  type ListDevicesResult,
  type PackageFilter,
  type PlannedAction,
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

type PackagesState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ok"; packages: AppPackage[] }
  | { kind: "error"; message: string };

type ActionState =
  | { kind: "idle" }
  | { kind: "confirming"; plan: PlannedAction }
  | { kind: "applying"; plan: PlannedAction }
  | { kind: "success"; message: string }
  | { kind: "error"; message: string };

const FILTERS: { value: PackageFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "user", label: "User" },
  { value: "system", label: "System" },
  { value: "enabled", label: "Enabled" },
  { value: "disabled", label: "Disabled" },
];

export default function AppsRoute() {
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [filter, setFilter] = useState<PackageFilter>("all");
  const [pkgState, setPkgState] = useState<PackagesState>({ kind: "idle" });
  const [actionState, setActionState] = useState<ActionState>({ kind: "idle" });
  const [search, setSearch] = useState("");

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

  const loadPackages = useCallback(async () => {
    if (!selectedSerial) return;
    setPkgState({ kind: "loading" });
    try {
      const packages = await callListPackages(selectedSerial, filter);
      setPkgState({ kind: "ok", packages });
    } catch (e) {
      setPkgState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedSerial, filter]);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  useEffect(() => {
    if (selectedSerial) {
      void loadPackages();
    }
  }, [selectedSerial, filter, loadPackages]);

  const startAction = useCallback(
    async (pkg: string, kind: ActionKind) => {
      if (!selectedSerial) return;
      try {
        const plan = await callPlanAction({
          serial: selectedSerial,
          package: pkg,
          kind,
        });
        setActionState({ kind: "confirming", plan });
      } catch (e) {
        setActionState({
          kind: "error",
          message: e instanceof Error ? e.message : String(e),
        });
      }
    },
    [selectedSerial],
  );

  const confirmAction = useCallback(async () => {
    if (actionState.kind !== "confirming") return;
    const { plan } = actionState;
    setActionState({ kind: "applying", plan });
    try {
      await callApplyAction(plan);
      setActionState({
        kind: "success",
        message: `${plan.description} — completed.`,
      });
      void loadPackages();
    } catch (e) {
      setActionState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [actionState, loadPackages]);

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];

  const filteredPackages =
    pkgState.kind === "ok"
      ? pkgState.packages.filter((p) =>
          search ? p.package.toLowerCase().includes(search.toLowerCase()) : true,
        )
      : [];

  return (
    <>
      <PaneHeader
        title="Apps"
        milestone="R-020"
        description="Review installed packages with precise filters, and a preview-before-apply workflow for disable, enable, uninstall, and data-clear actions."
        actions={
          selectedSerial ? (
            <Button
              type="button"
              onClick={() => void loadPackages()}
              disabled={pkgState.kind === "loading"}
              variant="primary"
            >
              {pkgState.kind === "loading" ? "Loading..." : "Refresh packages"}
            </Button>
          ) : undefined
        }
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {devicesState.kind === "ok" && (
              <Badge tone="success">
                {authorizedDevices.length} authorized{" "}
                {authorizedDevices.length === 1 ? "device" : "devices"}
              </Badge>
            )}
            {selectedSerial && (
              <Badge tone="info">
                <code className="font-mono">{selectedSerial}</code>
              </Badge>
            )}
          </div>
        }
      />

      <section className="mt-6 max-w-7xl space-y-4">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title="Desktop shell required" tone="info">
            <p>Package management runs inside the Tauri runtime.</p>
          </StatePanel>
        )}

        {devicesState.kind === "error" && (
          <StatePanel title="Device scan failed" tone="danger">
            <p>{devicesState.message}</p>
          </StatePanel>
        )}

        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title="No authorized devices" tone="warning">
            <p>
              Connect a device and accept the USB debugging prompt, then refresh.
            </p>
          </StatePanel>
        )}

        {authorizedDevices.length > 1 && (
          <DevicePicker
            devices={authorizedDevices}
            selected={selectedSerial}
            onSelect={setSelectedSerial}
          />
        )}

        {selectedSerial && (
          <>
            <div className="flex flex-wrap items-center gap-3">
              <FilterChips
                active={filter}
                onChange={(f) => {
                  setFilter(f);
                  setSearch("");
                }}
              />
              <div className="flex-1" />
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search packages..."
                aria-label="Filter packages by name"
                className="h-9 w-64 max-w-full rounded-md border border-white/10 bg-white/[0.06] px-3 font-mono text-sm text-anvil-50 outline-none transition placeholder:text-anvil-600 focus:border-circuit-300/50 focus:ring-2 focus:ring-circuit-300/20"
              />
            </div>

            {pkgState.kind === "loading" && <PackagesSkeleton />}

            {pkgState.kind === "error" && (
              <StatePanel title="Package enumeration failed" tone="danger">
                <p>{pkgState.message}</p>
              </StatePanel>
            )}

            {pkgState.kind === "ok" && (
              <PackageTable
                packages={filteredPackages}
                totalCount={pkgState.packages.length}
                onAction={startAction}
              />
            )}
          </>
        )}

        <ActionOverlay
          state={actionState}
          onConfirm={() => void confirmAction()}
          onCancel={() => setActionState({ kind: "idle" })}
          onDismiss={() => setActionState({ kind: "idle" })}
        />
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

function FilterChips({
  active,
  onChange,
}: {
  active: PackageFilter;
  onChange: (f: PackageFilter) => void;
}) {
  return (
    <div className="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Package filter">
      {FILTERS.map((f) => (
        <button
          key={f.value}
          type="button"
          role="radio"
          aria-checked={active === f.value}
          onClick={() => onChange(f.value)}
          className={[
            "rounded-full border px-3 py-1 text-xs font-medium transition",
            "focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300",
            active === f.value
              ? "border-circuit-300/30 bg-circuit-300/10 text-circuit-100"
              : "border-white/10 bg-white/[0.04] text-anvil-300 hover:border-white/20 hover:text-anvil-100",
          ].join(" ")}
        >
          {f.label}
        </button>
      ))}
    </div>
  );
}

function PackageTable({
  packages,
  totalCount,
  onAction,
}: {
  packages: AppPackage[];
  totalCount: number;
  onAction: (pkg: string, kind: ActionKind) => void;
}) {
  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            Installed packages
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            Actions preview before applying. Disable and enable are reversible
            via the journal.
          </p>
        </div>
        <div className="flex gap-2">
          {packages.length !== totalCount && (
            <Badge tone="info">
              {packages.length} / {totalCount} shown
            </Badge>
          )}
          <Badge tone="neutral">{totalCount} total</Badge>
        </div>
      </div>
      {packages.length === 0 ? (
        <div className="p-6 text-center text-sm text-anvil-400">
          No packages match the current filter and search.
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="min-w-full text-sm">
            <thead className="bg-white/[0.04]">
              <tr>
                <Th>Package</Th>
                <Th>Type</Th>
                <Th>State</Th>
                <Th>Actions</Th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {packages.map((pkg) => (
                <tr
                  key={pkg.package}
                  className="bg-anvil-950/20 transition hover:bg-white/[0.035]"
                >
                  <Td>
                    <div className="min-w-[16rem]">
                      <code className="font-mono text-xs text-anvil-50">
                        {pkg.package}
                      </code>
                      {pkg.installer && (
                        <p className="mt-1 text-[11px] text-anvil-500">
                          via {pkg.installer}
                        </p>
                      )}
                    </div>
                  </Td>
                  <Td>
                    <Badge tone={pkg.system ? "warning" : "neutral"}>
                      {pkg.system ? "System" : "User"}
                    </Badge>
                  </Td>
                  <Td>
                    <Badge tone={pkg.enabled ? "success" : "danger"}>
                      {pkg.enabled ? "Enabled" : "Disabled"}
                    </Badge>
                  </Td>
                  <Td>
                    <div className="flex min-w-[10rem] flex-wrap gap-1.5">
                      {pkg.enabled ? (
                        <>
                          <Button
                            type="button"
                            size="sm"
                            onClick={() => onAction(pkg.package, "disable")}
                          >
                            Disable
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            onClick={() =>
                              onAction(pkg.package, "uninstall_for_user")
                            }
                            variant="danger"
                          >
                            Uninstall
                          </Button>
                        </>
                      ) : (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() => onAction(pkg.package, "enable")}
                        >
                          Enable
                        </Button>
                      )}
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={() => onAction(pkg.package, "force_stop")}
                      >
                        Stop
                      </Button>
                    </div>
                  </Td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

function ActionOverlay({
  state,
  onConfirm,
  onCancel,
  onDismiss,
}: {
  state: ActionState;
  onConfirm: () => void;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  if (state.kind === "idle") return null;

  if (state.kind === "confirming") {
    return (
      <div
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        onKeyDown={(e) => { if (e.key === "Escape") onCancel(); }}
      >
        <Card className="mx-4 max-w-lg p-6">
          <h3 id="confirm-dialog-title" className="text-lg font-semibold text-anvil-50">
            Confirm action
          </h3>
          <p className="mt-3 text-sm leading-6 text-anvil-200">
            {state.plan.description}
          </p>
          <div className="mt-3 rounded-md border border-white/10 bg-white/[0.04] p-3">
            <p className="text-xs font-medium text-anvil-400">
              ADB command preview
            </p>
            <code className="mt-1 block font-mono text-xs text-anvil-100">
              adb -s {state.plan.request.serial} shell{" "}
              {state.plan.args.join(" ")}
            </code>
          </div>
          <div className="mt-5 flex justify-end gap-3">
            <Button type="button" onClick={onCancel}>
              Cancel
            </Button>
            <Button type="button" variant="primary" onClick={onConfirm}>
              Apply
            </Button>
          </div>
        </Card>
      </div>
    );
  }

  if (state.kind === "applying") {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
        <Card className="mx-4 max-w-lg p-6">
          <h3 className="text-sm font-semibold text-anvil-50">Applying...</h3>
          <p className="mt-2 text-xs text-anvil-400">
            {state.plan.description}
          </p>
        </Card>
      </div>
    );
  }

  if (state.kind === "success") {
    return (
      <StatePanel
        title="Action completed"
        tone="success"
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            Dismiss
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  return (
    <StatePanel
      title="Action failed"
      tone="danger"
      actions={
        <Button type="button" size="sm" variant="danger" onClick={onDismiss}>
          Dismiss
        </Button>
      }
    >
      <p>{state.message}</p>
    </StatePanel>
  );
}

function PackagesSkeleton() {
  return (
    <Card className="overflow-hidden p-0" aria-label="Loading packages">
      <div className="border-b border-white/10 p-4">
        <SkeletonLine className="w-40" />
        <SkeletonLine className="mt-3 w-64 max-w-full" />
      </div>
      <div className="divide-y divide-white/10">
        {Array.from({ length: 8 }).map((_, i) => (
          <div
            key={i}
            className="grid gap-4 p-4 sm:grid-cols-[2fr_0.5fr_0.5fr_1fr]"
          >
            <SkeletonLine className="w-52" />
            <SkeletonLine className="w-16" />
            <SkeletonLine className="w-16" />
            <SkeletonLine className="w-28" />
          </div>
        ))}
      </div>
    </Card>
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
  return <td className="px-4 py-4 align-middle text-anvil-200">{children}</td>;
}
