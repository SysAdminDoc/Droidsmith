import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  callApplyAction,
  callListDevices,
  callListPackages,
  callListPacks,
  callListUsers,
  callPlanPack,
  deviceTarget,
  inTauri,
  type AndroidUser,
  type CompatibilityStatus,
  type Device,
  type JournalEntry,
  type ListDevicesResult,
  type Pack,
  type PackAssessment,
  type PackCandidate,
  type PackEntry,
  type PackEntryAssessment,
  type PackLoadError,
  type PlannedAction,
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
import { expandPackDependencies } from "./debloatPack";
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
  | { kind: "ok"; packs: PackCandidate[]; errors: PackLoadError[] }
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
  | {
      step: "preview";
      pack: Pack;
      assessment: PackAssessment;
      selected: Set<string>;
      overrideAccepted: boolean;
      planError: string | null;
    }
  | {
      step: "applying";
      pack: Pack;
      assessment: PackAssessment;
      queue: DebloatQueueRow[];
      currentPackage: string | null;
      cancelRequested: boolean;
      overrideAccepted: boolean;
    }
  | {
      step: "done";
      pack: Pack;
      assessment: PackAssessment;
      queue: DebloatQueueRow[];
      cancelled: boolean;
      overrideAccepted: boolean;
    };

export default function DebloatRoute() {
  const { t } = useTranslation();
  const [devicesState, setDevicesState] = useState<DevicesState>({
    kind: "loading",
  });
  const [packsState, setPacksState] = useState<PacksState>({ kind: "loading" });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [selectedTransportId, setSelectedTransportId] = useState<number | null>(
    null,
  );
  const [users, setUsers] = useState<AndroidUser[]>([]);
  const [usersReady, setUsersReady] = useState(false);
  const [userError, setUserError] = useState<string | null>(null);
  const [selectedUser, setSelectedUser] = useState<number>(0);
  const [wizard, setWizard] = useState<WizardStep>({ step: "pick_pack" });
  const cancelRequestedRef = useRef(false);

  const authorizedDevices =
    devicesState.kind === "ok"
      ? devicesState.value.devices.filter(
          (d) => typeof d.state === "string" && d.state === "device",
        )
      : [];
  const selectedDevice =
    authorizedDevices.find((device) =>
      selectedTransportId != null
        ? device.transport_id === selectedTransportId
        : device.serial === selectedSerial,
    ) ?? null;

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
        setSelectedTransportId((prev) => prev ?? authorized[0]!.transport_id);
      }
    } catch (e) {
      setDevicesState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  const loadPacks = useCallback(async () => {
    if (!inTauri() || !selectedDevice || !usersReady) return;
    setPacksState({ kind: "loading" });
    try {
      const listing = await callListPacks(
        deviceTarget(selectedDevice),
        selectedUser,
      );
      setPacksState({
        kind: "ok",
        packs: listing.packs,
        errors: listing.errors,
      });
    } catch (e) {
      setPacksState({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, [selectedDevice, selectedUser, usersReady]);

  const loadUsers = useCallback(async () => {
    if (!selectedDevice) {
      setUsers([]);
      setUsersReady(false);
      setUserError(null);
      return;
    }
    setUsersReady(false);
    setUserError(null);
    try {
      const found = await callListUsers(deviceTarget(selectedDevice));
      setUsers(found);
      const foreground = found.find((u) => u.current) ?? found[0];
      if (!foreground)
        throw new Error("Android user discovery returned no users");
      setSelectedUser(foreground.id);
      setUsersReady(true);
    } catch (e) {
      setUsers([]);
      setUserError(e instanceof Error ? e.message : String(e));
    }
  }, [selectedDevice]);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  useEffect(() => {
    void loadUsers();
  }, [loadUsers]);

  useEffect(() => {
    if (selectedDevice && usersReady) {
      setWizard({ step: "pick_pack" });
      void loadPacks();
    }
  }, [loadPacks, selectedDevice, usersReady]);

  const selectPack = useCallback((candidate: PackCandidate) => {
    const { pack, assessment } = candidate;
    const ready = new Set(
      assessment.entries
        .filter((entry) => entry.status === "ready")
        .map((entry) => entry.id),
    );
    const recommended = new Set(
      pack.packages
        .filter((p) => p.removal === "recommended" && ready.has(p.id))
        .map((p) => p.id),
    );
    setWizard({
      step: "preview",
      pack,
      assessment,
      selected: expandPackDependencies(pack, recommended),
      overrideAccepted: false,
      planError: null,
    });
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
      return {
        ...prev,
        selected: expandPackDependencies(prev.pack, next),
        planError: null,
      };
    });
  }, []);

  const readPackageSnapshot = useCallback(
    async (packageId: string): Promise<PackageSnapshot> => {
      if (!selectedDevice || !usersReady) {
        throw new Error("Android user discovery has not completed");
      }
      const packages = await callListPackages(
        deviceTarget(selectedDevice),
        "all",
        selectedUser,
      );
      return snapshotPackage(packages, packageId);
    },
    [selectedDevice, selectedUser, usersReady],
  );

  const verificationMessage = useCallback(
    (result: DisableVerification): string | null => {
      if (result === "ok") return null;
      return t(`debloat.verify.${result}`);
    },
    [t],
  );

  const runQueue = useCallback(
    async (
      pack: Pack,
      assessment: PackAssessment,
      rows: DebloatQueueRow[],
      plans: PlannedAction[],
      overrideAccepted: boolean,
    ) => {
      if (!selectedDevice || !usersReady) return;
      cancelRequestedRef.current = false;
      let queue = rows;
      const plansByPackage = new Map(
        plans.map((plan) => [plan.request.package, plan]),
      );
      const assessmentsByPackage = new Map(
        assessment.entries.map((entry) => [entry.id, entry]),
      );

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
        assessment,
        queue,
        currentPackage: null,
        cancelRequested: false,
        overrideAccepted,
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
          const plan = plansByPackage.get(row.entry.id);
          if (!plan) {
            const support = assessmentsByPackage.get(row.entry.id);
            throw new Error(
              support?.detail ?? t("debloat.entryUnavailableForPlan"),
            );
          }
          before = await readPackageSnapshot(row.entry.id);
          journal = (await callApplyAction(plan)).entry;
          after = await readPackageSnapshot(row.entry.id);
          error = verificationMessage(verifyDisabled(after));
        } catch (e) {
          error = e instanceof Error ? e.message : String(e);
          try {
            after = await readPackageSnapshot(row.entry.id);
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

      setWizard({
        step: "done",
        pack,
        assessment,
        queue,
        cancelled,
        overrideAccepted,
      });
    },
    [readPackageSnapshot, selectedDevice, t, usersReady, verificationMessage],
  );

  const applyPack = useCallback(async () => {
    if (wizard.step !== "preview" || !selectedDevice || !usersReady) return;
    try {
      const planned = await callPlanPack({
        target: deviceTarget(selectedDevice),
        user_id: selectedUser,
        pack_id: wizard.pack.id,
        revision: wizard.pack.revision,
        selected: [...wizard.selected],
        override_compatibility: wizard.overrideAccepted,
      });
      const queue = makeQueueRows(
        wizard.pack.packages.filter((entry) =>
          planned.selected_ids.includes(entry.id),
        ),
      );
      await runQueue(
        wizard.pack,
        planned.assessment,
        queue,
        planned.plans,
        wizard.overrideAccepted,
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setWizard((previous) =>
        previous.step === "preview"
          ? { ...previous, planError: message }
          : previous,
      );
    }
  }, [runQueue, selectedDevice, selectedUser, usersReady, wizard]);

  const cancelAfterCurrent = useCallback(() => {
    cancelRequestedRef.current = true;
    setWizard((prev) =>
      prev.step === "applying" ? { ...prev, cancelRequested: true } : prev,
    );
  }, []);

  const retryFailed = useCallback(async () => {
    if (wizard.step !== "done" || !selectedDevice || !usersReady) return;
    const failedIds = wizard.queue
      .filter((row) => row.status === "failed")
      .map((row) => row.entry.id);
    const planned = await callPlanPack({
      target: deviceTarget(selectedDevice),
      user_id: selectedUser,
      pack_id: wizard.pack.id,
      revision: wizard.pack.revision,
      selected: failedIds,
      override_compatibility: wizard.overrideAccepted,
    });
    const previousRows = new Map(
      wizard.queue.map((row) => [row.entry.id, row]),
    );
    const queue = planned.selected_ids.map((id) => {
      const previous = previousRows.get(id);
      if (previous && previous.status !== "failed") return previous;
      const entry = wizard.pack.packages.find((item) => item.id === id)!;
      return {
        ...makeQueueRows([entry])[0]!,
        attempts: previous?.attempts ?? 0,
      };
    });
    await runQueue(
      wizard.pack,
      planned.assessment,
      queue,
      planned.plans,
      wizard.overrideAccepted,
    );
  }, [runQueue, selectedDevice, selectedUser, usersReady, wizard]);

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
            selected={selectedTransportId}
            selectedSerial={selectedSerial}
            onSelect={(device) => {
              setSelectedSerial(device.serial);
              setSelectedTransportId(device.transport_id);
            }}
          />
        )}

        {selectedSerial && userError && (
          <StatePanel title={t("apps.userDiscoveryFailed")} tone="danger">
            <p>{userError}</p>
          </StatePanel>
        )}

        {selectedSerial && usersReady && users.length > 1 && (
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

        {selectedSerial && usersReady && wizard.step === "pick_pack" && (
          <PackPicker
            state={packsState}
            onSelect={selectPack}
            onRefresh={() => void loadPacks()}
          />
        )}

        {usersReady && wizard.step === "preview" && (
          <PackPreview
            pack={wizard.pack}
            assessment={wizard.assessment}
            selected={wizard.selected}
            overrideAccepted={wizard.overrideAccepted}
            planError={wizard.planError}
            onToggle={toggleEntry}
            onOverrideChange={(accepted) =>
              setWizard((previous) =>
                previous.step === "preview"
                  ? {
                      ...previous,
                      overrideAccepted: accepted,
                      planError: null,
                    }
                  : previous,
              )
            }
            onApply={() => void applyPack()}
            onBack={() => setWizard({ step: "pick_pack" })}
          />
        )}

        {usersReady && wizard.step === "applying" && (
          <QueueApplyProgress
            pack={wizard.pack}
            queue={wizard.queue}
            currentPackage={wizard.currentPackage}
            cancelRequested={wizard.cancelRequested}
            onCancel={cancelAfterCurrent}
          />
        )}

        {usersReady && wizard.step === "done" && (
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
  selectedSerial,
  onSelect,
}: {
  devices: Device[];
  selected: number | null;
  selectedSerial: string | null;
  onSelect: (device: Device) => void;
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
            key={`${d.transport_id ?? d.serial}:${d.connection_generation}`}
            type="button"
            variant={
              (
                d.transport_id != null
                  ? d.transport_id === selected
                  : d.serial === selectedSerial
              )
                ? "primary"
                : "secondary"
            }
            size="sm"
            onClick={() => onSelect(d)}
          >
            {d.model ?? d.serial}
          </Button>
        ))}
      </div>
    </Card>
  );
}

/// Surfaces bundled packs that failed to load so a packaging defect is
/// visible (with a copyable message) instead of silently vanishing from
/// the list. Healthy packs still render alongside.
function PackErrors({ errors }: { errors: PackLoadError[] }) {
  const { t } = useTranslation();
  if (errors.length === 0) return null;
  return (
    <StatePanel
      title={t("debloat.packErrorsTitle", { count: errors.length })}
      tone="warning"
    >
      <p className="text-xs text-anvil-300">{t("debloat.packErrorsBody")}</p>
      <ul className="mt-2 space-y-2">
        {errors.map((err) => (
          <li key={err.file}>
            <pre className="select-text whitespace-pre-wrap rounded-md border border-white/10 bg-white/[0.04] p-2 font-mono text-xs text-anvil-200">
              {`${err.file} [${err.code}]\n${err.message}`}
            </pre>
          </li>
        ))}
      </ul>
    </StatePanel>
  );
}

function PackPicker({
  state,
  onSelect,
  onRefresh,
}: {
  state: PacksState;
  onSelect: (candidate: PackCandidate) => void;
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
      <>
        <PackErrors errors={state.errors} />
        <StatePanel title={t("debloat.noPacksTitle")} tone="info">
          <p>
            {t("debloat.noPacksBodyPrefix")} <code>.yaml</code>{" "}
            {t("debloat.noPacksBodyMiddle")} <code>packs/</code>{" "}
            {t("debloat.noPacksBodySuffix")} <code>packs/_example.yaml</code>.
          </p>
        </StatePanel>
      </>
    );
  }

  return (
    <Card className="p-5">
      <PackErrors errors={state.errors} />
      <h3 className="text-sm font-semibold text-anvil-50">
        {t("debloat.choosePack")}
      </h3>
      <p className="mt-1 text-xs text-anvil-400">
        {t("debloat.choosePackBody")}
      </p>
      <div className="mt-4 grid gap-3 sm:grid-cols-2">
        {state.packs.map((candidate) => {
          const { pack, assessment } = candidate;
          return (
            <button
              key={pack.id}
              type="button"
              onClick={() => onSelect(candidate)}
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
                <Badge tone={compatibilityTone(assessment.status)}>
                  {t(`debloat.compatibility.${assessment.status}`)}
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
          );
        })}
      </div>
    </Card>
  );
}

function PackPreview({
  pack,
  assessment,
  selected,
  overrideAccepted,
  planError,
  onToggle,
  onOverrideChange,
  onApply,
  onBack,
}: {
  pack: Pack;
  assessment: PackAssessment;
  selected: Set<string>;
  overrideAccepted: boolean;
  planError: string | null;
  onToggle: (id: string) => void;
  onOverrideChange: (accepted: boolean) => void;
  onApply: () => void;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const tiers = groupByTier(pack.packages);
  const assessments = new Map(
    assessment.entries.map((entry) => [entry.id, entry]),
  );

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
            <Badge tone={compatibilityTone(assessment.status)}>
              {t(`debloat.compatibility.${assessment.status}`)}
            </Badge>
          </div>
        </div>
        <p className="mt-3 text-xs text-anvil-400">
          {t("debloat.packIdentity", {
            id: pack.id,
            revision: pack.revision,
            license: pack.provenance.license,
          })}
        </p>
      </Card>

      <CompatibilityChecks assessment={assessment} />

      {planError && (
        <StatePanel title={t("debloat.planFailed")} tone="danger">
          <p>{planError}</p>
        </StatePanel>
      )}

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
                {entries.map((entry) => {
                  const support = assessments.get(entry.id);
                  const selectable = support?.status === "ready";
                  return (
                    <label
                      key={entry.id}
                      className={`flex gap-3 p-4 transition ${
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
                            className="ml-2"
                          >
                            {t(`debloat.entryStatus.${support.status}`)}
                          </Badge>
                        )}
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
                        {entry.depends_on.length > 0 && (
                          <p className="mt-1 text-[11px] text-circuit-100/80">
                            {t("debloat.dependsOn", {
                              items: entry.depends_on.join(", "),
                            })}
                          </p>
                        )}
                        {support?.detail && (
                          <p className="mt-1 text-[11px] text-red-200/80">
                            {support.detail}
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

      <div className="flex justify-between">
        <Button type="button" onClick={onBack}>
          {t("debloat.back")}
        </Button>
        <Button
          type="button"
          variant="primary"
          onClick={onApply}
          disabled={
            selected.size === 0 ||
            (assessment.override_required && !overrideAccepted)
          }
        >
          {t("debloat.applyCount", { count: selected.size })}
        </Button>
      </div>
    </>
  );
}

function CompatibilityChecks({ assessment }: { assessment: PackAssessment }) {
  const { t } = useTranslation();
  return (
    <Card className="p-4">
      <h4 className="text-xs font-semibold text-anvil-200">
        {t("debloat.compatibilityChecks")}
      </h4>
      <ul className="mt-3 grid gap-2 sm:grid-cols-2">
        {assessment.checks.map((check) => (
          <li
            key={check.field}
            className="rounded-md border border-white/10 bg-white/[0.02] p-3"
          >
            <div className="flex items-center justify-between gap-2">
              <code className="font-mono text-xs text-anvil-200">
                {check.field}
              </code>
              <Badge tone={compatibilityTone(check.status)}>
                {t(`debloat.compatibility.${check.status}`)}
              </Badge>
            </div>
            <p className="mt-2 text-[11px] text-anvil-400">
              {t("debloat.expectedActual", {
                expected: check.expected.join(", "),
                actual: check.actual ?? t("debloat.unknownValue"),
              })}
            </p>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function compatibilityTone(
  status: CompatibilityStatus,
): "success" | "warning" | "danger" {
  switch (status) {
    case "compatible":
      return "success";
    case "unknown":
      return "warning";
    case "mismatch":
      return "danger";
  }
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
