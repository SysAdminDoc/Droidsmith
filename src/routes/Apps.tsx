import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  callApplyAction,
  callBackupPackage,
  callListDevices,
  callListPackages,
  callListPermissions,
  callPlanAction,
  callSetPermission,
  inTauri,
  type ActionKind,
  type AppPackage,
  type Device,
  type ListDevicesResult,
  type PackageFilter,
  type PermissionInfo,
  type PlannedAction,
} from "../lib/tauri";

import {
  Badge,
  Button,
  Card,
  EmptyState,
  FieldInput,
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

const FILTERS: { value: PackageFilter; labelKey: string }[] = [
  { value: "all", labelKey: "apps.filterAll" },
  { value: "user", labelKey: "apps.filterUser" },
  { value: "system", labelKey: "apps.filterSystem" },
  { value: "enabled", labelKey: "apps.filterEnabled" },
  { value: "disabled", labelKey: "apps.filterDisabled" },
];

export default function AppsRoute() {
  const { t } = useTranslation();
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [filter, setFilter] = useState<PackageFilter>("all");
  const [pkgState, setPkgState] = useState<PackagesState>({ kind: "idle" });
  const [actionState, setActionState] = useState<ActionState>({ kind: "idle" });
  const [search, setSearch] = useState("");
  const [inspectedPkg, setInspectedPkg] = useState<string | null>(null);
  const [backupMsg, setBackupMsg] = useState<string | null>(null);

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

  const startBackup = useCallback(
    async (pkg: string) => {
      if (!selectedSerial) return;
      setBackupMsg(t("apps.backingUp", { package: pkg }));
      try {
        await callBackupPackage(selectedSerial, pkg, `${pkg}.ab`);
        setBackupMsg(t("apps.backupSaved", { file: `${pkg}.ab` }));
      } catch (e) {
        setBackupMsg(
          t("apps.backupFailed", {
            message: e instanceof Error ? e.message : String(e),
          }),
        );
      }
    },
    [selectedSerial, t],
  );

  const confirmAction = useCallback(async () => {
    if (actionState.kind !== "confirming") return;
    const { plan } = actionState;
    setActionState({ kind: "applying", plan });
    try {
      await callApplyAction(plan);
      setActionState({
        kind: "success",
        message: t("apps.planCompleted", { description: plan.description }),
      });
      void loadPackages();
    } catch (e) {
      setActionState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [actionState, loadPackages, t]);

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];

  const filteredPackages =
    pkgState.kind === "ok"
      ? pkgState.packages.filter((p) =>
          search
            ? p.package.toLowerCase().includes(search.toLowerCase())
            : true,
        )
      : [];

  return (
    <>
      <PaneHeader
        title={t("apps.title")}
        milestone="R-020"
        description={t("apps.description")}
        actions={
          selectedSerial ? (
            <Button
              type="button"
              onClick={() => void loadPackages()}
              disabled={pkgState.kind === "loading"}
              variant="primary"
            >
              {pkgState.kind === "loading"
                ? t("apps.loading")
                : t("apps.refreshPackages")}
            </Button>
          ) : undefined
        }
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {devicesState.kind === "ok" && (
              <Badge tone="success">
                {t("apps.authorizedDeviceCount", {
                  count: authorizedDevices.length,
                })}
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
          <StatePanel title={t("common.desktopRequired")} tone="info">
            <p>{t("apps.desktopRequiredBody")}</p>
          </StatePanel>
        )}

        {devicesState.kind === "error" && (
          <StatePanel title={t("devices.scanFailed")} tone="danger">
            <p>{devicesState.message}</p>
          </StatePanel>
        )}

        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title={t("common.noAuthorized")} tone="warning">
            <p>{t("apps.noAuthorizedBody")}</p>
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
              <FieldInput
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("apps.searchPlaceholder")}
                aria-label={t("apps.searchLabel")}
                className="w-64 max-w-full font-mono"
              />
            </div>

            {pkgState.kind === "loading" && <PackagesSkeleton />}

            {pkgState.kind === "error" && (
              <StatePanel
                title={t("apps.packageEnumerationFailed")}
                tone="danger"
              >
                <p>{pkgState.message}</p>
              </StatePanel>
            )}

            {pkgState.kind === "ok" && (
              <PackageTable
                packages={filteredPackages}
                totalCount={pkgState.packages.length}
                onAction={startAction}
                onInspect={setInspectedPkg}
                onBackup={(pkg) => void startBackup(pkg)}
              />
            )}
          </>
        )}

        {inspectedPkg && selectedSerial && (
          <PermissionsPanel
            serial={selectedSerial}
            pkg={inspectedPkg}
            onClose={() => setInspectedPkg(null)}
          />
        )}

        {backupMsg && (
          <StatePanel
            title={t("apps.backup")}
            tone="info"
            actions={
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => setBackupMsg(null)}
              >
                {t("common.dismiss")}
              </Button>
            }
          >
            <p>{backupMsg}</p>
          </StatePanel>
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
  const { t } = useTranslation();

  return (
    <Card className="p-4">
      <h3 className="text-sm font-semibold text-anvil-50">
        {t("common.selectDevice")}
      </h3>
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
  const { t } = useTranslation();

  return (
    <div
      className="flex flex-wrap gap-1.5 rounded-lg border border-white/10 bg-white/[0.025] p-1"
      role="radiogroup"
      aria-label={t("apps.packageFilterLabel")}
    >
      {FILTERS.map((f) => (
        <button
          key={f.value}
          type="button"
          role="radio"
          aria-checked={active === f.value}
          onClick={() => onChange(f.value)}
          className={[
            "rounded-md border px-3 py-1.5 text-xs font-medium transition",
            "focus:outline-none focus-visible:ring-2 focus-visible:ring-circuit-300",
            active === f.value
              ? "border-circuit-300/30 bg-circuit-300/12 text-circuit-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.08)]"
              : "border-transparent text-anvil-300 hover:bg-white/[0.05] hover:text-anvil-100",
          ].join(" ")}
        >
          {t(f.labelKey)}
        </button>
      ))}
    </div>
  );
}

function PackageTable({
  packages,
  totalCount,
  onAction,
  onInspect,
  onBackup,
}: {
  packages: AppPackage[];
  totalCount: number;
  onAction: (pkg: string, kind: ActionKind) => void;
  onInspect: (pkg: string) => void;
  onBackup: (pkg: string) => void;
}) {
  const { t } = useTranslation();

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("apps.installedPackages")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("apps.installedPackagesBody")}
          </p>
        </div>
        <div className="flex gap-2">
          {packages.length !== totalCount && (
            <Badge tone="info">
              {t("apps.shownCount", {
                shown: packages.length,
                total: totalCount,
              })}
            </Badge>
          )}
          <Badge tone="neutral">
            {t("common.totalCount", { count: totalCount })}
          </Badge>
        </div>
      </div>
      {packages.length === 0 ? (
        <EmptyState title={t("apps.noMatchingPackages")}>
          <p>{t("apps.noMatchingPackagesBody")}</p>
        </EmptyState>
      ) : (
        <div className="overflow-x-auto">
          <table className="min-w-full text-sm">
            <thead className="bg-white/[0.04]">
              <tr>
                <Th>{t("apps.package")}</Th>
                <Th>{t("apps.type")}</Th>
                <Th>{t("devices.state")}</Th>
                <Th>{t("apps.actions")}</Th>
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
                          {t("apps.viaInstaller", {
                            installer: pkg.installer,
                          })}
                        </p>
                      )}
                    </div>
                  </Td>
                  <Td>
                    <Badge tone={pkg.system ? "warning" : "neutral"}>
                      {pkg.system
                        ? t("apps.filterSystem")
                        : t("apps.filterUser")}
                    </Badge>
                  </Td>
                  <Td>
                    <Badge tone={pkg.enabled ? "success" : "danger"}>
                      {pkg.enabled
                        ? t("apps.filterEnabled")
                        : t("apps.filterDisabled")}
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
                            {t("apps.disable")}
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            onClick={() =>
                              onAction(pkg.package, "uninstall_for_user")
                            }
                            variant="danger"
                          >
                            {t("apps.uninstall")}
                          </Button>
                        </>
                      ) : (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() => onAction(pkg.package, "enable")}
                        >
                          {t("apps.enable")}
                        </Button>
                      )}
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={() => onAction(pkg.package, "force_stop")}
                      >
                        {t("apps.stop")}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={() => onInspect(pkg.package)}
                      >
                        {t("apps.perms")}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        onClick={() => onBackup(pkg.package)}
                      >
                        {t("apps.backup")}
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
  const { t } = useTranslation();

  if (state.kind === "idle") return null;

  if (state.kind === "confirming") {
    return (
      <div
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 px-4 backdrop-blur-sm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        aria-describedby="confirm-dialog-description"
      >
        <Card className="w-full max-w-lg p-6">
          <Badge tone="warning">{t("apps.reviewBeforeApplying")}</Badge>
          <h3
            id="confirm-dialog-title"
            className="mt-4 text-lg font-semibold text-anvil-50"
          >
            {t("apps.applyPackageAction")}
          </h3>
          <p
            id="confirm-dialog-description"
            className="mt-3 text-sm leading-6 text-anvil-200"
          >
            {state.plan.description}
          </p>
          <div className="mt-3 rounded-md border border-white/10 bg-white/[0.04] p-3">
            <p className="text-xs font-medium text-anvil-400">
              {t("apps.commandPreview")}
            </p>
            <code className="mt-1 block font-mono text-xs text-anvil-100">
              adb -s {state.plan.request.serial} shell{" "}
              {state.plan.args.join(" ")}
            </code>
          </div>
          <div className="mt-5 flex justify-end gap-3">
            <Button type="button" onClick={onCancel}>
              {t("apps.cancel")}
            </Button>
            <Button type="button" variant="primary" onClick={onConfirm}>
              {t("apps.applyChange")}
            </Button>
          </div>
        </Card>
      </div>
    );
  }

  if (state.kind === "applying") {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 px-4 backdrop-blur-sm">
        <Card className="w-full max-w-lg p-6" role="status">
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("apps.applyingChange")}
          </h3>
          <p className="mt-2 text-xs text-anvil-400">
            {state.plan.description}
          </p>
          <div className="mt-4 h-2 overflow-hidden rounded-sm bg-white/[0.08]">
            <div className="h-full w-2/3 animate-pulse rounded-sm bg-circuit-300" />
          </div>
        </Card>
      </div>
    );
  }

  if (state.kind === "success") {
    return (
      <StatePanel
        title={t("apps.actionCompleted")}
        tone="success"
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  return (
    <StatePanel
      title={t("apps.actionFailed")}
      tone="danger"
      actions={
        <Button type="button" size="sm" variant="danger" onClick={onDismiss}>
          {t("common.dismiss")}
        </Button>
      }
    >
      <p>{state.message}</p>
    </StatePanel>
  );
}

function PermissionsPanel({
  serial,
  pkg,
  onClose,
}: {
  serial: string;
  pkg: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [perms, setPerms] = useState<PermissionInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [toggling, setToggling] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const result = await callListPermissions(serial, pkg);
      setPerms(result);
    } catch {
      setPerms([]);
    } finally {
      setLoading(false);
    }
  }, [serial, pkg]);

  useEffect(() => {
    void load();
  }, [load]);

  const togglePerm = useCallback(
    async (permission: string, grant: boolean) => {
      setToggling(permission);
      try {
        await callSetPermission(serial, pkg, permission, grant);
        setPerms((prev) =>
          prev.map((p) =>
            p.permission === permission ? { ...p, granted: grant } : p,
          ),
        );
      } catch {
        // Reload on failure to get accurate state
        void load();
      } finally {
        setToggling(null);
      }
    },
    [serial, pkg, load],
  );

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex items-center justify-between border-b border-white/10 p-4">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("apps.permissions")}
          </h3>
          <code className="mt-1 block font-mono text-xs text-anvil-400">
            {pkg}
          </code>
        </div>
        <Button type="button" size="sm" variant="ghost" onClick={onClose}>
          {t("common.close")}
        </Button>
      </div>
      {loading ? (
        <div className="p-4">
          <SkeletonLine className="w-48" />
          <SkeletonLine className="mt-3 w-64" />
          <SkeletonLine className="mt-3 w-56" />
        </div>
      ) : perms.length === 0 ? (
        <EmptyState title={t("apps.noPermissionsFound")}>
          <p>{t("apps.noPermissionsFoundBody")}</p>
        </EmptyState>
      ) : (
        <div
          className="divide-y divide-white/10"
          style={{ maxHeight: "20rem", overflowY: "auto" }}
        >
          {perms.map((p) => (
            <div
              key={p.permission}
              className="flex items-center justify-between gap-3 px-4 py-2.5"
            >
              <code className="min-w-0 truncate font-mono text-xs text-anvil-200">
                {p.permission}
              </code>
              <button
                type="button"
                onClick={() => void togglePerm(p.permission, !p.granted)}
                disabled={toggling === p.permission}
                className={[
                  "shrink-0 rounded-md border px-3 py-1 text-xs font-medium transition",
                  p.granted
                    ? "border-emerald-300/20 bg-emerald-300/10 text-emerald-100 hover:bg-emerald-300/20"
                    : "border-red-300/20 bg-red-300/10 text-red-100 hover:bg-red-300/20",
                  toggling === p.permission ? "opacity-50" : "",
                ].join(" ")}
              >
                {p.granted ? t("apps.granted") : t("apps.denied")}
              </button>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}

function PackagesSkeleton() {
  const { t } = useTranslation();

  return (
    <Card
      className="overflow-hidden p-0"
      aria-label={t("apps.loadingPackages")}
    >
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
