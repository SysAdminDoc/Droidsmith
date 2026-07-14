import { useCallback, useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";

import {
  callApplyAction,
  callBackupPackage,
  callJournalList,
  callJournalUndo,
  callListDevices,
  callListPackages,
  callListPermissions,
  callListUsers,
  callPlanAction,
  callSetPermission,
  inTauri,
  type ActionKind,
  type AndroidUser,
  type AppPackage,
  type Device,
  type JournalEntry,
  type ListDevicesResult,
  type PackageFilter,
  type PermissionInfo,
  type PlannedAction,
} from "../lib/tauri";

import {
  backupDefaultFileName,
  backupDisplayState,
  formatBackupSize,
} from "./appsBackup";
import { journalEntryStatus, type JournalEntryStatus } from "./appsJournal";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  FieldInput,
  PaneHeader,
  SkeletonLine,
  StatePanel,
  TableCell,
  TableHeaderCell,
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

type JournalState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ok"; entries: JournalEntry[] }
  | { kind: "error"; message: string };

type BackupNotice = {
  title: string;
  message: string;
  tone: "neutral" | "info" | "success" | "warning" | "danger";
  path?: string;
  output?: string;
  sizeBytes?: number | null;
  showLimitations?: boolean;
};

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
  const [users, setUsers] = useState<AndroidUser[]>([]);
  const [selectedUser, setSelectedUser] = useState<number>(0);
  const [filter, setFilter] = useState<PackageFilter>("all");
  const [pkgState, setPkgState] = useState<PackagesState>({ kind: "idle" });
  const [actionState, setActionState] = useState<ActionState>({ kind: "idle" });
  const [journalState, setJournalState] = useState<JournalState>({
    kind: "idle",
  });
  const [undoingEntryId, setUndoingEntryId] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  const [inspectedPkg, setInspectedPkg] = useState<string | null>(null);
  const [backupNotice, setBackupNotice] = useState<BackupNotice | null>(null);

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
      if (authorized.length === 1) {
        setSelectedSerial((prev) => prev ?? authorized[0]!.serial);
      }
    } catch (e) {
      setDevicesState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  const loadUsers = useCallback(async () => {
    if (!selectedSerial) {
      setUsers([]);
      return;
    }
    try {
      const found = await callListUsers(selectedSerial);
      setUsers(found);
      // Default the target to the foreground user, falling back to the
      // owner (0). Never silently keep a stale selection from device A.
      const foreground = found.find((u) => u.current) ?? found[0];
      setSelectedUser(foreground ? foreground.id : 0);
    } catch {
      // Users are an enhancement; on failure fall back to owner-only.
      setUsers([]);
      setSelectedUser(0);
    }
  }, [selectedSerial]);

  const loadPackages = useCallback(async () => {
    if (!selectedSerial) return;
    setPkgState({ kind: "loading" });
    try {
      const packages = await callListPackages(
        selectedSerial,
        filter,
        selectedUser,
      );
      setPkgState({ kind: "ok", packages });
    } catch (e) {
      setPkgState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedSerial, filter, selectedUser]);

  const loadJournal = useCallback(async () => {
    if (!selectedSerial) {
      setJournalState({ kind: "idle" });
      return;
    }
    setJournalState({ kind: "loading" });
    try {
      const entries = await callJournalList(selectedSerial);
      setJournalState({ kind: "ok", entries });
    } catch (e) {
      setJournalState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedSerial]);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  useEffect(() => {
    void loadUsers();
  }, [loadUsers]);

  useEffect(() => {
    if (selectedSerial) {
      void loadPackages();
      void loadJournal();
    } else {
      setJournalState({ kind: "idle" });
    }
  }, [selectedSerial, filter, selectedUser, loadPackages, loadJournal]);

  const startAction = useCallback(
    async (pkg: string, kind: ActionKind) => {
      if (!selectedSerial) return;
      try {
        const plan = await callPlanAction({
          serial: selectedSerial,
          package: pkg,
          kind,
          user_id: selectedUser,
        });
        setActionState({ kind: "confirming", plan });
      } catch (e) {
        setActionState({
          kind: "error",
          message: e instanceof Error ? e.message : String(e),
        });
      }
    },
    [selectedSerial, selectedUser],
  );

  const startBackup = useCallback(
    async (pkg: string) => {
      if (!selectedSerial) return;
      setBackupNotice({
        title: t("apps.backupChooseDestination"),
        message: t("apps.backupLimitations"),
        tone: "info",
      });
      try {
        const localPath = await save({
          title: t("apps.backupChooseDestination"),
          defaultPath: backupDefaultFileName(pkg),
          filters: [{ name: t("apps.backupFileFilter"), extensions: ["ab"] }],
        });
        if (!localPath) {
          setBackupNotice({
            title: t("apps.backupCancelledTitle"),
            message: t("apps.backupCancelled"),
            tone: "neutral",
          });
          return;
        }

        setBackupNotice({
          title: t("apps.backupRunningTitle", { package: pkg }),
          message: t("apps.backupLimitations"),
          tone: "info",
          path: localPath,
        });

        const result = await callBackupPackage(selectedSerial, pkg, localPath);
        const displayState = backupDisplayState(result);
        const titleByState: Record<typeof displayState, string> = {
          empty: t("apps.backupEmptyTitle"),
          header_only: t("apps.backupHeaderOnlyTitle"),
          saved: t("apps.backupSavedTitle"),
        };
        const messageByState: Record<typeof displayState, string> = {
          empty: t("apps.backupEmptyBody"),
          header_only: t("apps.backupHeaderOnlyBody"),
          saved: t("apps.backupSaved", { file: result.local_path }),
        };
        setBackupNotice({
          title: titleByState[displayState],
          message: messageByState[displayState],
          tone: displayState === "saved" ? "success" : "warning",
          path: result.local_path,
          output: result.stdout,
          sizeBytes: result.size_bytes,
          showLimitations: true,
        });
      } catch (e) {
        setBackupNotice({
          title: t("apps.backupFailedTitle"),
          message: t("apps.backupFailed", {
            message: e instanceof Error ? e.message : String(e),
          }),
          tone: "danger",
        });
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
      void loadJournal();
    } catch (e) {
      setActionState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [actionState, loadJournal, loadPackages, t]);

  const undoJournalEntry = useCallback(
    async (entry: JournalEntry) => {
      if (!selectedSerial) return;
      setUndoingEntryId(entry.id);
      try {
        await callJournalUndo(selectedSerial, entry.id);
        setActionState({
          kind: "success",
          message: t("apps.journalUndoCompleted", {
            package: entry.applied.plan.request.package,
          }),
        });
        await Promise.all([loadPackages(), loadJournal()]);
      } catch (e) {
        setActionState({
          kind: "error",
          message: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setUndoingEntryId(null);
      }
    },
    [loadJournal, loadPackages, selectedSerial, t],
  );

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
              {users.length > 1 && (
                <label className="flex items-center gap-2 text-sm text-slate-300">
                  <span>{t("apps.userLabel")}</span>
                  <select
                    value={selectedUser}
                    onChange={(e) => setSelectedUser(Number(e.target.value))}
                    aria-label={t("apps.userLabel")}
                    className="rounded-md border border-slate-700 bg-slate-900 px-2 py-1 font-mono text-slate-100"
                  >
                    {users.map((u) => (
                      <option key={u.id} value={u.id}>
                        {u.id} · {u.name}
                        {u.current ? ` (${t("apps.userCurrent")})` : ""}
                      </option>
                    ))}
                  </select>
                </label>
              )}
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

            <JournalPanel
              state={journalState}
              undoingEntryId={undoingEntryId}
              onRefresh={() => void loadJournal()}
              onUndo={(entry) => void undoJournalEntry(entry)}
            />
          </>
        )}

        {inspectedPkg && selectedSerial && (
          <PermissionsPanel
            serial={selectedSerial}
            pkg={inspectedPkg}
            onClose={() => setInspectedPkg(null)}
          />
        )}

        {backupNotice && (
          <BackupStatePanel
            notice={backupNotice}
            onDismiss={() => setBackupNotice(null)}
          />
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

function BackupStatePanel({
  notice,
  onDismiss,
}: {
  notice: BackupNotice;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const formattedSize =
    notice.sizeBytes === undefined
      ? undefined
      : formatBackupSize(notice.sizeBytes);

  return (
    <StatePanel
      title={notice.title}
      tone={notice.tone}
      actions={
        <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
          {t("common.dismiss")}
        </Button>
      }
    >
      <p>{notice.message}</p>
      {notice.showLimitations && (
        <p className="mt-2 text-xs leading-5 text-anvil-400">
          {t("apps.backupLimitations")}
        </p>
      )}
      {(notice.path || formattedSize !== undefined) && (
        <dl className="mt-3 grid gap-2 text-xs sm:grid-cols-[8rem_minmax(0,1fr)]">
          {notice.path && (
            <>
              <dt className="font-medium text-anvil-400">
                {t("apps.backupPath")}
              </dt>
              <dd className="min-w-0 break-words font-mono text-anvil-100">
                {notice.path}
              </dd>
            </>
          )}
          {formattedSize !== undefined && (
            <>
              <dt className="font-medium text-anvil-400">
                {t("apps.backupSize")}
              </dt>
              <dd className="font-mono text-anvil-100">
                {formattedSize ?? t("common.notReported")}
              </dd>
            </>
          )}
        </dl>
      )}
      {notice.output !== undefined && (
        <div className="mt-4">
          <p className="text-xs font-medium text-anvil-400">
            {t("apps.backupOutput")}
          </p>
          <pre className="mt-2 max-h-48 overflow-auto rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
            {notice.output.trim() || t("apps.backupNoOutput")}
          </pre>
        </div>
      )}
    </StatePanel>
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
                <TableHeaderCell>{t("apps.package")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.type")}</TableHeaderCell>
                <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.actions")}</TableHeaderCell>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {packages.map((pkg) => (
                <tr
                  key={pkg.package}
                  className="bg-anvil-950/20 transition hover:bg-white/[0.035]"
                >
                  <TableCell>
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
                  </TableCell>
                  <TableCell>
                    <Badge tone={pkg.system ? "warning" : "neutral"}>
                      {pkg.system
                        ? t("apps.filterSystem")
                        : t("apps.filterUser")}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <Badge tone={pkg.enabled ? "success" : "danger"}>
                      {pkg.enabled
                        ? t("apps.filterEnabled")
                        : t("apps.filterDisabled")}
                    </Badge>
                  </TableCell>
                  <TableCell>
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
                  </TableCell>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

function JournalPanel({
  state,
  undoingEntryId,
  onRefresh,
  onUndo,
}: {
  state: JournalState;
  undoingEntryId: number | null;
  onRefresh: () => void;
  onUndo: (entry: JournalEntry) => void;
}) {
  const { t } = useTranslation();

  if (state.kind === "idle") return null;

  const entries =
    state.kind === "ok"
      ? [...state.entries].sort((a, b) => b.id - a.id).slice(0, 8)
      : [];

  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-col gap-3 border-b border-white/10 p-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("apps.journalTitle")}
          </h3>
          <p className="mt-1 text-xs leading-5 text-anvil-400">
            {t("apps.journalBody")}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {state.kind === "ok" && (
            <Badge tone="neutral">
              {t("apps.journalEntryCount", { count: state.entries.length })}
            </Badge>
          )}
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={onRefresh}
            disabled={state.kind === "loading"}
          >
            {state.kind === "loading"
              ? t("apps.journalLoading")
              : t("apps.journalRefresh")}
          </Button>
        </div>
      </div>

      {state.kind === "loading" && (
        <div className="divide-y divide-white/10">
          {[0, 1, 2].map((i) => (
            <div
              key={i}
              className="grid gap-4 p-4 md:grid-cols-[0.5fr_1.2fr_1.6fr_1fr_1fr]"
            >
              <SkeletonLine className="w-14" />
              <SkeletonLine className="w-32" />
              <SkeletonLine className="w-56" />
              <SkeletonLine className="w-24" />
              <SkeletonLine className="w-20" />
            </div>
          ))}
        </div>
      )}

      {state.kind === "error" && (
        <div className="border-t border-red-300/20 bg-red-950/15 p-4">
          <Badge tone="danger">{t("apps.journalLoadFailed")}</Badge>
          <p className="mt-3 text-sm leading-6 text-red-100">{state.message}</p>
        </div>
      )}

      {state.kind === "ok" && entries.length === 0 && (
        <EmptyState title={t("apps.journalEmpty")}>
          <p>{t("apps.journalEmptyBody")}</p>
        </EmptyState>
      )}

      {state.kind === "ok" && entries.length > 0 && (
        <div className="overflow-x-auto">
          <table className="min-w-full text-sm">
            <thead className="bg-white/[0.04]">
              <tr>
                <TableHeaderCell>{t("apps.journalId")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.journalAction")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.package")}</TableHeaderCell>
                <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.journalOutput")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.actions")}</TableHeaderCell>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/10">
              {entries.map((entry) => {
                const status = journalEntryStatus(entry);
                const request = entry.applied.plan.request;
                return (
                  <tr
                    key={entry.id}
                    className="bg-anvil-950/20 transition hover:bg-white/[0.035]"
                  >
                    <TableCell>
                      <div className="min-w-[6rem]">
                        <code className="font-mono text-xs text-anvil-50">
                          #{entry.id}
                        </code>
                        <p className="mt-1 text-[11px] text-anvil-500">
                          {formatJournalTime(entry.applied.applied_at)}
                        </p>
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="min-w-[8rem]">
                        <Badge tone="info">
                          {t(journalActionKey(request.kind))}
                        </Badge>
                        <p className="mt-2 max-w-xs text-xs leading-5 text-anvil-400">
                          {entry.applied.plan.description}
                        </p>
                      </div>
                    </TableCell>
                    <TableCell>
                      <code className="block min-w-[16rem] font-mono text-xs text-anvil-100">
                        {request.package}
                      </code>
                    </TableCell>
                    <TableCell>
                      <Badge tone={journalStatusTone(status)}>
                        {journalStatusLabel(entry, status, t)}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <pre className="max-w-[22rem] whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-anvil-400">
                        {summarizeJournalOutput(entry.applied.stdout) ||
                          t("apps.journalNoOutput")}
                      </pre>
                    </TableCell>
                    <TableCell>
                      {status === "undoable" ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() => onUndo(entry)}
                          disabled={undoingEntryId === entry.id}
                        >
                          {undoingEntryId === entry.id
                            ? t("apps.journalUndoing")
                            : t("apps.journalUndo")}
                        </Button>
                      ) : (
                        <span className="text-xs text-anvil-500">
                          {t("apps.journalNoUndo")}
                        </span>
                      )}
                    </TableCell>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

function journalActionKey(kind: ActionKind): string {
  return `apps.actionKind.${kind}`;
}

function journalStatusTone(
  status: JournalEntryStatus,
): "neutral" | "info" | "success" | "warning" | "danger" {
  switch (status) {
    case "undoable":
      return "warning";
    case "undone":
      return "success";
    case "undo_record":
      return "info";
    case "irreversible":
      return "neutral";
  }
}

function journalStatusLabel(
  entry: JournalEntry,
  status: JournalEntryStatus,
  t: ReturnType<typeof useTranslation>["t"],
): string {
  switch (status) {
    case "undoable":
      return t("apps.journalUndoable");
    case "undone":
      return t("apps.journalUndoneBy", { id: entry.undone_by });
    case "undo_record":
      return t("apps.journalUndoRecord", { id: entry.undoes });
    case "irreversible":
      return t("apps.journalIrreversible");
  }
}

function formatJournalTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(date);
}

function summarizeJournalOutput(output: string): string {
  const trimmed = output.trim();
  if (trimmed.length <= 180) return trimmed;
  return `${trimmed.slice(0, 180)}...`;
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
