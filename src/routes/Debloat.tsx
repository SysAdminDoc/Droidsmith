import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callApplyAction,
  callExportRecoveryBaseline,
  callGetDeviceInfo,
  callListPackages,
  callListPacks,
  callListUsers,
  callPlanPack,
  callRemoveImportedPack,
  callSelectHostPath,
  deviceTarget,
  inTauri,
  type AndroidUser,
  type JournalEntry,
  type Pack,
  type PackAssessment,
  type PackCandidate,
  type PlannedAction,
} from "../lib/tauri";
import {
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";

import {
  snapshotJournalPackageState,
  snapshotPackage,
  verifyDisabled,
  type DisableVerification,
  type PackageSnapshot,
} from "./debloatQueue";
import {
  expandPackDependencies,
  packagesForPreset,
  type DebloatPreset,
} from "./debloatPack";
import {
  Badge,
  Button,
  DevicePicker,
  FieldSelect,
  PaneHeader,
  StatePanel,
  TransportBadge,
  TransportTrustNotice,
} from "./common";
import { PackPicker, type PacksState } from "./debloat/PackPicker";
import { DebloatApplyReview } from "./debloat/ApplyReview";
import { PackPreview } from "./debloat/PackPreview";
import { QueueApplyProgress } from "./debloat/QueueApplyProgress";
import { QueueApplyResult } from "./debloat/QueueApplyResult";
import {
  makeQueueRows,
  patchQueueRow,
  type DebloatQueueRow,
} from "./debloat/queue";

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

type BaselineNotice =
  | { kind: "busy"; message: string }
  | { kind: "success"; message: string }
  | { kind: "error"; message: string };

export default function DebloatRoute() {
  const { t } = useTranslation();
  const { devicesState, authorizedDevices } = useAuthorizedDevices();
  const [packsState, setPacksState] = useState<PacksState>({ kind: "loading" });
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [selectedTransportId, setSelectedTransportId] = useState<number | null>(
    null,
  );
  const [users, setUsers] = useState<AndroidUser[]>([]);
  const [deviceContext, setDeviceContext] = useState<{
    manufacturer: string | null;
    rom: string | null;
  }>({ manufacturer: null, rom: null });
  const [usersReady, setUsersReady] = useState(false);
  const [userError, setUserError] = useState<string | null>(null);
  const [selectedUser, setSelectedUser] = useState<number>(0);
  const [wizard, setWizard] = useState<WizardStep>({ step: "pick_pack" });
  const [baselineNotice, setBaselineNotice] = useState<BaselineNotice | null>(
    null,
  );
  const [applyReviewOpen, setApplyReviewOpen] = useState(false);
  const cancelRequestedRef = useRef(false);

  const selectedDevice =
    authorizedDevices.find((device) =>
      selectedTransportId != null
        ? device.transport_id === selectedTransportId
        : device.serial === selectedSerial,
    ) ?? null;
  const selectedTarget = selectedDevice ? deviceTarget(selectedDevice) : null;
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(selectedTarget);

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
        message: errorMessage(e),
      });
    }
  }, [selectedDevice, selectedUser, usersReady]);

  const loadUsers = useCallback(async () => {
    if (!selectedDevice) {
      setUsers([]);
      setUsersReady(false);
      setUserError(null);
      setDeviceContext({ manufacturer: null, rom: null });
      return;
    }
    setUsersReady(false);
    setUserError(null);
    // Best-effort device context for quirk matching; a failure here must not
    // block the debloat flow.
    void (async () => {
      try {
        const info = await callGetDeviceInfo(deviceTarget(selectedDevice));
        setDeviceContext({
          manufacturer: info.manufacturer,
          rom: info.build_fingerprint,
        });
      } catch {
        setDeviceContext({ manufacturer: null, rom: null });
      }
    })();
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
      setUserError(errorMessage(e));
    }
  }, [selectedDevice]);

  useEffect(() => {
    const current = authorizedDevices.find((device) =>
      selectedTransportId != null
        ? device.transport_id === selectedTransportId
        : device.serial === selectedSerial,
    );
    if (current) return;

    const sameSerial = authorizedDevices.filter(
      (device) => device.serial === selectedSerial,
    );
    const next =
      sameSerial.length === 1
        ? sameSerial[0]!
        : authorizedDevices.length === 1
          ? authorizedDevices[0]!
          : null;
    setSelectedSerial(next?.serial ?? null);
    setSelectedTransportId(next?.transport_id ?? null);
  }, [authorizedDevices, selectedSerial, selectedTransportId]);

  useEffect(() => {
    void loadUsers();
  }, [loadUsers]);

  useEffect(() => {
    if (selectedDevice && usersReady) {
      setBaselineNotice(null);
      setWizard({ step: "pick_pack" });
      void loadPacks();
    }
  }, [loadPacks, selectedDevice, usersReady]);

  const selectPack = useCallback((candidate: PackCandidate) => {
    setBaselineNotice(null);
    setApplyReviewOpen(false);
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
    setBaselineNotice(null);
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

  // IMP-63: replace the current selection with the packages a named preset
  // matches, then let the user review/edit before applying.
  const applyPreset = useCallback((preset: DebloatPreset) => {
    setBaselineNotice(null);
    setWizard((prev) => {
      if (prev.step !== "preview") return prev;
      const ready = new Set(
        prev.assessment.entries
          .filter((entry) => entry.status === "ready")
          .map((entry) => entry.id),
      );
      const matched = packagesForPreset(prev.pack, preset, ready);
      return {
        ...prev,
        selected: expandPackDependencies(prev.pack, matched),
        planError: null,
      };
    });
  }, []);

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
      if (!selectedDevice || !authorizedTarget || !usersReady) return;
      cancelRequestedRef.current = false;
      // One baseline listing supplies presence and system metadata for the
      // whole batch. Successful mutations use the backend's targeted journal
      // probes below, avoiding two complete package listings per queue row.
      const initialPackages = await callListPackages(
        deviceTarget(selectedDevice),
        "all",
        selectedUser,
      );
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

        const baseline = snapshotPackage(initialPackages, row.entry.id);
        let before: PackageSnapshot | null = baseline;
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
          journal = (await callApplyAction(plan)).entry;
          before =
            snapshotJournalPackageState(
              journal.applied.before_state,
              baseline.system,
            ) ?? baseline;
          after = snapshotJournalPackageState(
            journal.applied.after_state,
            baseline.system,
          );
          error = verificationMessage(verifyDisabled(after));
        } catch (e) {
          error = errorMessage(e);
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

      // If an apply or its targeted verification probe failed, recover all
      // missing after-states with one batch listing. A successfully journaled
      // action can become verified here; operation failures retain their
      // original error even if the package state is now observable.
      if (queue.some((row) => row.status === "failed" && row.after === null)) {
        try {
          const recoveredPackages = await callListPackages(
            deviceTarget(selectedDevice),
            "all",
            selectedUser,
          );
          queue = queue.map((row) => {
            if (row.status !== "failed" || row.after !== null) return row;
            const recovered = snapshotPackage(recoveredPackages, row.entry.id);
            if (row.journalId === null) return { ...row, after: recovered };
            const verificationError = verificationMessage(
              verifyDisabled(recovered),
            );
            return {
              ...row,
              status: verificationError ? "failed" : "verified",
              after: recovered,
              error: verificationError,
            };
          });
          commitQueue({ currentPackage: null });
        } catch {
          // The original per-row error and null after-state remain visible.
        }
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
    [
      authorizedTarget,
      selectedDevice,
      selectedUser,
      t,
      usersReady,
      verificationMessage,
    ],
  );

  const applyPack = useCallback(async () => {
    if (wizard.step !== "preview" || !authorizedTarget || !usersReady) return;
    setApplyReviewOpen(false);
    try {
      const planned = await callPlanPack({
        target: authorizedTarget,
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
      const message = errorMessage(error);
      setWizard((previous) =>
        previous.step === "preview"
          ? { ...previous, planError: message }
          : previous,
      );
    }
  }, [authorizedTarget, runQueue, selectedUser, usersReady, wizard]);

  const exportPackBaseline = useCallback(async () => {
    if (wizard.step !== "preview" || !authorizedTarget || !usersReady) return;
    setBaselineNotice({
      kind: "busy",
      message: t("debloat.recoveryExporting"),
    });
    try {
      const planned = await callPlanPack({
        target: authorizedTarget,
        user_id: selectedUser,
        pack_id: wizard.pack.id,
        revision: wizard.pack.revision,
        selected: [...wizard.selected],
        override_compatibility: wizard.overrideAccepted,
      });
      const selected = await callSelectHostPath(
        "recovery_baseline_save",
        debloatRecoveryFileName(wizard.pack.id),
      );
      if (!selected) {
        setBaselineNotice(null);
        return;
      }
      const artifact = await callExportRecoveryBaseline(
        authorizedTarget,
        selectedUser,
        planned.selected_ids.map((packageName) => ({
          package: packageName,
          kind: "disable",
        })),
        { id: wizard.pack.id, revision: wizard.pack.revision },
        selected.id,
      );
      setBaselineNotice({
        kind: "success",
        message: t("debloat.recoverySaved", {
          path: artifact.local_path,
          sha256: artifact.sha256,
        }),
      });
    } catch (error) {
      setBaselineNotice({
        kind: "error",
        message: errorMessage(error),
      });
    }
  }, [authorizedTarget, selectedUser, t, usersReady, wizard]);

  const cancelAfterCurrent = useCallback(() => {
    cancelRequestedRef.current = true;
    setWizard((prev) =>
      prev.step === "applying" ? { ...prev, cancelRequested: true } : prev,
    );
  }, []);

  const retryFailed = useCallback(async () => {
    if (wizard.step !== "done" || !authorizedTarget || !usersReady) return;
    const failedIds = wizard.queue
      .filter((row) => row.status === "failed")
      .map((row) => row.entry.id);
    const planned = await callPlanPack({
      target: authorizedTarget,
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
  }, [authorizedTarget, runQueue, selectedUser, usersReady, wizard]);

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
            {selectedTarget && (
              <TransportBadge kind={selectedTarget.transport_kind} />
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

        <TransportTrustNotice
          target={selectedTarget}
          accepted={transportOverrideAccepted}
          onAcceptedChange={setTransportOverrideAccepted}
        />

        {selectedSerial && userError && (
          <StatePanel title={t("apps.userDiscoveryFailed")} tone="danger">
            <p>{userError}</p>
          </StatePanel>
        )}

        {selectedSerial && usersReady && users.length > 1 && (
          <label className="flex items-center gap-2 text-sm text-anvil-300">
            <span>{t("apps.userLabel")}</span>
            <FieldSelect
              value={selectedUser}
              onChange={(e) => setSelectedUser(Number(e.target.value))}
              aria-label={t("apps.userLabel")}
              className="h-auto px-2 py-1 font-mono"
            >
              {users.map((u) => (
                <option key={u.id} value={u.id}>
                  {u.id} · {u.name}
                  {u.current ? ` (${t("apps.userCurrent")})` : ""}
                </option>
              ))}
            </FieldSelect>
          </label>
        )}

        {selectedSerial && usersReady && wizard.step === "pick_pack" && (
          <PackPicker
            state={packsState}
            target={selectedDevice ? deviceTarget(selectedDevice) : null}
            userId={selectedUser}
            onSelect={selectPack}
            onRefresh={() => void loadPacks()}
            onRemove={async (packId) => {
              await callRemoveImportedPack(packId);
              await loadPacks();
            }}
          />
        )}

        {usersReady && wizard.step === "preview" && (
          <>
            {baselineNotice && (
              <StatePanel
                title={t("debloat.recoveryBaseline")}
                tone={
                  baselineNotice.kind === "error"
                    ? "danger"
                    : baselineNotice.kind === "success"
                      ? "success"
                      : "info"
                }
                actions={
                  baselineNotice.kind !== "busy" ? (
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => setBaselineNotice(null)}
                    >
                      {t("common.dismiss")}
                    </Button>
                  ) : undefined
                }
              >
                <p className="break-all">{baselineNotice.message}</p>
              </StatePanel>
            )}
            <PackPreview
              pack={wizard.pack}
              assessment={wizard.assessment}
              selected={wizard.selected}
              overrideAccepted={wizard.overrideAccepted}
              planError={wizard.planError}
              onToggle={toggleEntry}
              onApplyPreset={applyPreset}
              onOverrideChange={(accepted) => {
                setBaselineNotice(null);
                setWizard((previous) =>
                  previous.step === "preview"
                    ? {
                        ...previous,
                        overrideAccepted: accepted,
                        planError: null,
                      }
                    : previous,
                );
              }}
              onExportBaseline={() => void exportPackBaseline()}
              exportingBaseline={baselineNotice?.kind === "busy"}
              onApply={() => setApplyReviewOpen(true)}
              onBack={() => {
                setBaselineNotice(null);
                setApplyReviewOpen(false);
                setWizard({ step: "pick_pack" });
              }}
            />
            {applyReviewOpen && (
              <DebloatApplyReview
                pack={wizard.pack}
                selected={wizard.selected}
                onCancel={() => setApplyReviewOpen(false)}
                onConfirm={() => void applyPack()}
              />
            )}
          </>
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
            deviceContext={deviceContext}
            onRetryFailed={() => void retryFailed()}
            onReset={() => setWizard({ step: "pick_pack" })}
          />
        )}
      </section>
    </>
  );
}

function debloatRecoveryFileName(packId: string): string {
  const date = new Date().toISOString().slice(0, 10);
  const safePack = packId.replace(/[^A-Za-z0-9_.-]/g, "_");
  return `droidsmith-recovery-${date}-${safePack}.json`;
}
