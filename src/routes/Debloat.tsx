import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callApplyAction,
  callListDevices,
  callListPackages,
  callListPacks,
  callListUsers,
  callPlanAction,
  inTauri,
  type ActionKind,
  type AndroidUser,
  type Device,
  type JournalEntry,
  type ListDevicesResult,
  type Pack,
  type PackEntry,
  type RemovalLevel,
} from "../lib/tauri";

import {
  queueStats,
  snapshotPackage,
  verifyDisabled,
  type DisableVerification,
  type PackageSnapshot,
  type QueueStatus,
} from "./debloatQueue";
import {
  Badge,
  Button,
  Card,
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

type PacksState =
  | { kind: "loading" }
  | { kind: "ok"; packs: Pack[] }
  | { kind: "error"; message: string };

type DebloatQueueRow = {
  entry: PackEntry;
  status: QueueStatus;
  attempts: number;
  before: PackageSnapshot | null;
  after: PackageSnapshot | null;
  journalId: number | null;
  error: string | null;
};

type WizardStep =
  | { step: "pick_pack" }
  | { step: "preview"; pack: Pack; selected: Set<string> }
  | {
      step: "applying";
      pack: Pack;
      queue: DebloatQueueRow[];
      currentPackage: string | null;
      cancelRequested: boolean;
    }
  | {
      step: "done";
      pack: Pack;
      queue: DebloatQueueRow[];
      cancelled: boolean;
    };

const DEBLOAT_ACTION_KIND: ActionKind = "disable";

export default function DebloatRoute() {
  const { t } = useTranslation();
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [packsState, setPacksState] = useState<PacksState>({ kind: "loading" });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [users, setUsers] = useState<AndroidUser[]>([]);
  const [selectedUser, setSelectedUser] = useState<number>(0);
  const [wizard, setWizard] = useState<WizardStep>({ step: "pick_pack" });
  const cancelRequestedRef = useRef(false);

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

  const loadUsers = useCallback(async () => {
    if (!selectedSerial) {
      setUsers([]);
      return;
    }
    try {
      const found = await callListUsers(selectedSerial);
      setUsers(found);
      const foreground = found.find((u) => u.current) ?? found[0];
      setSelectedUser(foreground ? foreground.id : 0);
    } catch {
      setUsers([]);
      setSelectedUser(0);
    }
  }, [selectedSerial]);

  useEffect(() => {
    void loadDevices();
    void loadPacks();
  }, [loadDevices, loadPacks]);

  useEffect(() => {
    void loadUsers();
  }, [loadUsers]);

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

  const readPackageSnapshot = useCallback(
    async (serial: string, packageId: string): Promise<PackageSnapshot> => {
      const packages = await callListPackages(serial, "all", selectedUser);
      return snapshotPackage(packages, packageId);
    },
    [selectedUser],
  );

  const verificationMessage = useCallback(
    (result: DisableVerification): string | null => {
      if (result === "ok") return null;
      return t(`debloat.verify.${result}`);
    },
    [t],
  );

  const runQueue = useCallback(
    async (pack: Pack, rows: DebloatQueueRow[]) => {
      if (!selectedSerial) return;
      cancelRequestedRef.current = false;
      let queue = rows;

      const commitQueue = (
        patch: Partial<Extract<WizardStep, { step: "applying" }>> = {},
      ) => {
        setWizard((prev) =>
          prev.step === "applying" ? { ...prev, queue, ...patch } : prev,
        );
      };

      setWizard({
        step: "applying",
        pack,
        queue,
        currentPackage: null,
        cancelRequested: false,
      });

      for (const row of queue.filter((item) => item.status === "pending")) {
        if (cancelRequestedRef.current) break;

        queue = patchQueueRow(queue, row.entry.id, (current) => ({
          ...current,
          status: "running",
          attempts: current.attempts + 1,
          before: null,
          after: null,
          journalId: null,
          error: null,
        }));
        commitQueue({ currentPackage: row.entry.id });

        let before: PackageSnapshot | null = null;
        let after: PackageSnapshot | null = null;
        let journal: JournalEntry | null = null;
        let error: string | null = null;

        try {
          before = await readPackageSnapshot(selectedSerial, row.entry.id);
          const plan = await callPlanAction({
            serial: selectedSerial,
            package: row.entry.id,
            kind: DEBLOAT_ACTION_KIND,
            user_id: selectedUser,
          });
          journal = await callApplyAction(plan);
          after = await readPackageSnapshot(selectedSerial, row.entry.id);
          error = verificationMessage(verifyDisabled(after));
        } catch (e) {
          error = e instanceof Error ? e.message : String(e);
          try {
            after = await readPackageSnapshot(selectedSerial, row.entry.id);
          } catch {
            // Preserve the original apply error; missing after-state is visible as null.
          }
        }

        queue = patchQueueRow(queue, row.entry.id, (current) => ({
          ...current,
          status: error ? "failed" : "verified",
          before,
          after,
          journalId: journal?.id ?? null,
          error,
        }));
        commitQueue({ currentPackage: row.entry.id });
      }

      const cancelled = cancelRequestedRef.current;
      if (cancelled) {
        queue = queue.map((row) =>
          row.status === "pending"
            ? {
                ...row,
                status: "cancelled",
                error: t("debloat.cancelledBeforeApply"),
              }
            : row,
        );
      }

      setWizard({ step: "done", pack, queue, cancelled });
    },
    [readPackageSnapshot, selectedSerial, selectedUser, t, verificationMessage],
  );

  const applyPack = useCallback(async () => {
    if (wizard.step !== "preview" || !selectedSerial) return;
    const queue = makeQueueRows(
      wizard.pack.packages.filter((p) => wizard.selected.has(p.id)),
    );
    await runQueue(wizard.pack, queue);
  }, [runQueue, selectedSerial, wizard]);

  const cancelAfterCurrent = useCallback(() => {
    cancelRequestedRef.current = true;
    setWizard((prev) =>
      prev.step === "applying" ? { ...prev, cancelRequested: true } : prev,
    );
  }, []);

  const retryFailed = useCallback(async () => {
    if (wizard.step !== "done" || !selectedSerial) return;
    const queue = wizard.queue.map((row) =>
      row.status === "failed"
        ? {
            ...row,
            status: "pending" as QueueStatus,
            before: null,
            after: null,
            journalId: null,
            error: null,
          }
        : row,
    );
    await runQueue(wizard.pack, queue);
  }, [runQueue, selectedSerial, wizard]);

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];

  return (
    <>
      <PaneHeader
        title={t("debloat.title")}
        milestone="R-033"
        description={t("debloat.description")}
        meta={
          <div className="flex flex-wrap items-center gap-2">
            {selectedSerial && (
              <Badge tone="info">
                <code className="font-mono">{selectedSerial}</code>
              </Badge>
            )}
            {packsState.kind === "ok" && (
              <Badge tone="neutral">
                {t("debloat.packCount", { count: packsState.packs.length })}
              </Badge>
            )}
          </div>
        }
      />

      <section className="mt-6 max-w-7xl space-y-4">
        {devicesState.kind === "no_tauri" && (
          <StatePanel title={t("common.desktopRequired")} tone="info">
            <p>{t("debloat.desktopRequiredBody")}</p>
          </StatePanel>
        )}

        {devicesState.kind === "ok" && authorizedDevices.length === 0 && (
          <StatePanel title={t("common.noAuthorized")} tone="warning">
            <p>{t("debloat.noAuthorizedBody")}</p>
          </StatePanel>
        )}

        {authorizedDevices.length > 1 && (
          <DevicePicker
            devices={authorizedDevices}
            selected={selectedSerial}
            onSelect={setSelectedSerial}
          />
        )}

        {selectedSerial && users.length > 1 && (
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
          <QueueApplyProgress
            pack={wizard.pack}
            queue={wizard.queue}
            currentPackage={wizard.currentPackage}
            cancelRequested={wizard.cancelRequested}
            onCancel={cancelAfterCurrent}
          />
        )}

        {wizard.step === "done" && (
          <QueueApplyResult
            pack={wizard.pack}
            queue={wizard.queue}
            cancelled={wizard.cancelled}
            onRetryFailed={() => void retryFailed()}
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

function PackPicker({
  state,
  onSelect,
  onRefresh,
}: {
  state: PacksState;
  onSelect: (pack: Pack) => void;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();

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
        title={t("debloat.loadPacksFailed")}
        tone="danger"
        actions={
          <Button type="button" size="sm" variant="danger" onClick={onRefresh}>
            {t("runtime.retry")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  if (state.packs.length === 0) {
    return (
      <StatePanel title={t("debloat.noPacksTitle")} tone="info">
        <p>
          {t("debloat.noPacksBodyPrefix")} <code>.yaml</code>{" "}
          {t("debloat.noPacksBodyMiddle")} <code>packs/</code>{" "}
          {t("debloat.noPacksBodySuffix")} <code>packs/_example.yaml</code>.
        </p>
      </StatePanel>
    );
  }

  return (
    <Card className="p-5">
      <h3 className="text-sm font-semibold text-anvil-50">
        {t("debloat.choosePack")}
      </h3>
      <p className="mt-1 text-xs text-anvil-400">
        {t("debloat.choosePackBody")}
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
              <Badge tone="neutral">
                {t("debloat.packageShortCount", {
                  count: pack.packages.length,
                })}
              </Badge>
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
  const { t } = useTranslation();
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
            <Badge tone="info">
              {t("debloat.selected", { count: selected.size })}
            </Badge>
            <Badge tone="neutral">
              {t("common.totalCount", { count: pack.packages.length })}
            </Badge>
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
                          {t("debloat.neededBy", {
                            items: entry.needed_by.join(", "),
                          })}
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
          {t("debloat.back")}
        </Button>
        <Button
          type="button"
          variant="primary"
          onClick={onApply}
          disabled={selected.size === 0}
        >
          {t("debloat.applyCount", { count: selected.size })}
        </Button>
      </div>
    </>
  );
}

function makeQueueRows(entries: PackEntry[]): DebloatQueueRow[] {
  return entries.map((entry) => ({
    entry,
    status: "pending",
    attempts: 0,
    before: null,
    after: null,
    journalId: null,
    error: null,
  }));
}

function patchQueueRow(
  rows: DebloatQueueRow[],
  entryId: string,
  patch: (row: DebloatQueueRow) => DebloatQueueRow,
): DebloatQueueRow[] {
  return rows.map((row) => (row.entry.id === entryId ? patch(row) : row));
}

function QueueApplyProgress({
  pack,
  queue,
  currentPackage,
  cancelRequested,
  onCancel,
}: {
  pack: Pack;
  queue: DebloatQueueRow[];
  currentPackage: string | null;
  cancelRequested: boolean;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const stats = queueStats(queue);
  const pct =
    stats.total > 0 ? Math.round((stats.completed / stats.total) * 100) : 0;

  return (
    <Card className="p-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h3 className="text-sm font-semibold text-anvil-50">
            {t("debloat.applyingPack", { name: pack.name })}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {currentPackage
              ? t("debloat.currentPackage", { package: currentPackage })
              : t("debloat.preparingQueue")}
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="danger"
          onClick={onCancel}
          disabled={cancelRequested}
        >
          {cancelRequested
            ? t("debloat.cancelRequested")
            : t("debloat.cancelAfterCurrent")}
        </Button>
      </div>

      <div className="mt-4">
        <div className="flex items-center justify-between text-xs text-anvil-400">
          <span>
            {t("debloat.progressCount", {
              applied: stats.completed,
              total: stats.total,
            })}
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

      <QueueRows rows={queue} />
    </Card>
  );
}

function QueueApplyResult({
  pack,
  queue,
  cancelled,
  onRetryFailed,
  onReset,
}: {
  pack: Pack;
  queue: DebloatQueueRow[];
  cancelled: boolean;
  onRetryFailed: () => void;
  onReset: () => void;
}) {
  const { t } = useTranslation();
  const stats = queueStats(queue);
  const failedRows = queue.filter((row) => row.status === "failed");
  const tone =
    stats.failed > 0 || cancelled
      ? "warning"
      : stats.verified === stats.total
        ? "success"
        : "info";

  return (
    <>
      <StatePanel
        title={t("debloat.completeTitle", { name: pack.name })}
        tone={tone}
        actions={
          <>
            {failedRows.length > 0 && (
              <Button
                type="button"
                size="sm"
                variant="primary"
                onClick={onRetryFailed}
              >
                {t("debloat.retryFailed", { count: failedRows.length })}
              </Button>
            )}
            <Button type="button" size="sm" onClick={onReset}>
              {t("debloat.startOver")}
            </Button>
          </>
        }
      >
        <p>
          {t("debloat.disabledCount", { count: stats.verified })}
          {stats.failed > 0 &&
            ` ${t("debloat.failedCount", { count: stats.failed })}`}
          {stats.cancelled > 0 &&
            ` ${t("debloat.cancelledCount", { count: stats.cancelled })}`}
        </p>
        {cancelled && <p className="mt-2">{t("debloat.cancelledSummary")}</p>}
        <p className="mt-2">{t("debloat.journalUndo")}</p>
      </StatePanel>

      <Card className="p-4">
        <h4 className="text-xs font-semibold text-anvil-200">
          {t("debloat.queueResults")}
        </h4>
        <QueueRows rows={queue} />
      </Card>
    </>
  );
}

function QueueRows({ rows }: { rows: DebloatQueueRow[] }) {
  const { t } = useTranslation();

  return (
    <div className="mt-4 overflow-x-auto rounded-lg border border-white/10">
      <table className="min-w-full text-sm">
        <thead className="bg-white/[0.04]">
          <tr>
            <TableHeaderCell>{t("apps.package")}</TableHeaderCell>
            <TableHeaderCell>{t("devices.state")}</TableHeaderCell>
            <TableHeaderCell>{t("debloat.beforeAfter")}</TableHeaderCell>
            <TableHeaderCell>{t("debloat.journalId")}</TableHeaderCell>
          </tr>
        </thead>
        <tbody className="divide-y divide-white/10">
          {rows.map((row) => (
            <tr key={row.entry.id} className="bg-anvil-950/20">
              <TableCell>
                <code className="font-mono text-xs text-anvil-100">
                  {row.entry.id}
                </code>
                {row.error && (
                  <p className="mt-1 max-w-xl text-xs leading-5 text-red-200/80">
                    {row.error}
                  </p>
                )}
              </TableCell>
              <TableCell>
                <Badge tone={queueStatusTone(row.status)}>
                  {t(`debloat.queueStatus.${row.status}`)}
                </Badge>
                {row.attempts > 0 && (
                  <p className="mt-1 text-[11px] text-anvil-500">
                    {t("debloat.attemptCount", { count: row.attempts })}
                  </p>
                )}
              </TableCell>
              <TableCell>
                <div className="min-w-[12rem] text-xs text-anvil-400">
                  <p>
                    {t("debloat.beforeState", snapshotLabel(row.before, t))}
                  </p>
                  <p className="mt-1">
                    {t("debloat.afterState", snapshotLabel(row.after, t))}
                  </p>
                </div>
              </TableCell>
              <TableCell>
                {row.journalId ? (
                  <code className="font-mono text-xs text-circuit-100">
                    #{row.journalId}
                  </code>
                ) : (
                  <span className="text-xs text-anvil-500">
                    {t("debloat.noJournalEntry")}
                  </span>
                )}
              </TableCell>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function queueStatusTone(
  status: QueueStatus,
): "neutral" | "info" | "success" | "warning" | "danger" {
  switch (status) {
    case "pending":
      return "neutral";
    case "running":
      return "info";
    case "verified":
      return "success";
    case "failed":
      return "danger";
    case "cancelled":
      return "warning";
  }
}

function snapshotLabel(
  snapshot: PackageSnapshot | null,
  t: ReturnType<typeof useTranslation>["t"],
): { state: string } {
  if (!snapshot) return { state: t("debloat.stateUnknown") };
  if (!snapshot.present) return { state: t("debloat.stateMissing") };
  return {
    state: snapshot.enabled
      ? t("debloat.stateEnabled")
      : t("debloat.stateDisabled"),
  };
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
