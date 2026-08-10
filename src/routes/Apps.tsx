import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import {
  errorMessage,
  callApplyAction,
  callApplyActionBatch,
  callExportRecoveryBaseline,
  callInspectRecoveryBaseline,
  callGetPackageMetadata,
  callGetPackageActionCapabilities,
  callJournalList,
  callJournalUndo,
  callJournalUndoBatch,
  callListPackagesWithCapability,
  callListUsers,
  callObserveDeviceFingerprint,
  callAssessUninstallRecovery,
  callPlanAction,
  callPlanActionBatch,
  callSelectHostPath,
  deviceTarget,
  type ActionKind,
  type BaselineRoundTrip,
  type FingerprintObservation,
  type AndroidUser,
  type AppPackageMetadata,
  type BatchActionItemResult,
  type BatchActionResult,
  type JournalEntry,
  type PackageFilter,
} from "../lib/tauri";
import {
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";
import {
  targetFingerprint,
  useTargetOperation,
  useTargetOperationGroup,
} from "../lib/targetOperation";

import { PackageTable } from "./apps/PackageTable";
import { JournalPanel } from "./apps/JournalPanel";
import { PermissionsPanel } from "./apps/PermissionsPanel";
import { RecoveryBaselinePanel } from "./apps/RecoveryBaselinePanel";
import { BatchActionBar, FilterChips } from "./apps/FilterControls";
import {
  archiveIsRisky,
  classifyUnarchive,
  type UnarchiveOutlook,
} from "./apps/unarchive";
import { ActionOverlay } from "./apps/ActionOverlay";
import {
  BackupStatePanel,
  InstallOverrideDialog,
  InstallStatePanel,
} from "./apps/InstallPanels";
import { PackagesSkeleton } from "./apps/PackagesSkeleton";
import { usePackageBackups } from "./apps/usePackageBackups";
import { usePackageInstall } from "./apps/usePackageInstall";
import type {
  ActionState,
  JournalState,
  PackagesState,
  RecoveryState,
} from "./apps/types";
import {
  Badge,
  Button,
  DevicePicker,
  FieldInput,
  PaneHeader,
  StatePanel,
  FieldSelect,
  TransportBadge,
  TransportTrustNotice,
} from "./common";

// Session caches for the R-087 OTA-drift notice. The promise cache guarantees a
// single backend observation per device+fingerprint — surviving React
// StrictMode remounts and Apps-route revisits (observe records the fingerprint,
// so a second call would report no change) — and the dismissed set makes a
// dismissal sticky for that device+fingerprint.
const fingerprintObservations = new Map<
  string,
  Promise<FingerprintObservation>
>();
const dismissedFingerprintNotices = new Set<string>();

export default function AppsRoute() {
  const { t } = useTranslation();
  const { devicesState, authorizedDevices } = useAuthorizedDevices();
  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [selectedTransportId, setSelectedTransportId] = useState<number | null>(
    null,
  );
  const [users, setUsers] = useState<AndroidUser[]>([]);
  const [usersReady, setUsersReady] = useState(false);
  const [userError, setUserError] = useState<string | null>(null);
  const [selectedUser, setSelectedUser] = useState<number>(0);
  const [filter, setFilter] = useState<PackageFilter>("all");
  const [pkgState, setPkgState] = useState<PackagesState>({ kind: "idle" });
  const [packageMetadata, setPackageMetadata] = useState<
    Record<string, AppPackageMetadata | null>
  >({});
  const [actionState, setActionState] = useState<ActionState>({ kind: "idle" });
  const [journalState, setJournalState] = useState<JournalState>({
    kind: "idle",
  });
  const [undoingEntryId, setUndoingEntryId] = useState<number | null>(null);
  const [undoingBatchId, setUndoingBatchId] = useState<string | null>(null);
  const [selectedPackages, setSelectedPackages] = useState<string[]>([]);
  const [search, setSearch] = useState("");
  const [inspectedPkg, setInspectedPkg] = useState<string | null>(null);
  const [showAdvancedBackups, setShowAdvancedBackups] = useState(false);
  const [incrementalInstall, setIncrementalInstall] = useState(false);
  const [recoveryState, setRecoveryState] = useState<RecoveryState>({
    kind: "idle",
  });
  const [otaNotice, setOtaNotice] = useState(false);
  const metadataRequestedRef = useRef(new Set<string>());

  const selectedDevice =
    authorizedDevices.find((device) =>
      selectedTransportId != null
        ? device.transport_id === selectedTransportId
        : device.serial === selectedSerial,
    ) ?? null;
  // The device store rebuilds device objects on every snapshot, so a fresh
  // target per render would give useTransportAuthorization's memo a new
  // authorizedTarget identity each render (re-subscribing the drag-drop
  // listener and refiring PermissionsPanel loads on every keystroke). Memoize
  // on the scalar target identity instead of object identity.
  const selectedTargetIdentity = targetFingerprint(
    selectedDevice ? deviceTarget(selectedDevice) : null,
  );
  const selectedTarget = useMemo(
    () => (selectedDevice ? deviceTarget(selectedDevice) : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [selectedTargetIdentity],
  );
  const {
    accepted: transportOverrideAccepted,
    setAccepted: setTransportOverrideAccepted,
    authorizedTarget,
  } = useTransportAuthorization(selectedTarget);
  const usersOperation = useTargetOperation(selectedTarget, "apps-users");
  const packagesOperation = useTargetOperation(
    selectedTarget,
    `apps-packages:${selectedUser}:${filter}`,
  );
  const journalOperation = useTargetOperation(selectedTarget, "apps-journal");
  const fingerprintOperation = useTargetOperation(
    selectedTarget,
    "apps-fingerprint",
  );
  const metadataOperations = useTargetOperationGroup(
    selectedTarget,
    `apps-metadata:${selectedUser}`,
  );
  const actionOperation = useTargetOperation(
    authorizedTarget,
    `apps-action:${selectedUser}`,
  );
  const recoveryOperation = useTargetOperation(
    authorizedTarget,
    `apps-recovery:${selectedUser}`,
  );
  const undoOperation = useTargetOperation(
    authorizedTarget,
    `apps-undo:${selectedUser}`,
  );
  const {
    backupNotice,
    setBackupNotice,
    runPackageExport,
    inspectLegacyExport,
    cancelBackup,
  } = usePackageBackups({
    target: authorizedTarget,
    device: selectedDevice,
    userId: selectedUser,
    usersReady,
  });

  // R-087: when a device's build fingerprint has changed since Droidsmith last
  // saw it (an OTA update), flag it so the user knows disabled/removed packages
  // may have returned and can review their debloat recovery baseline.
  const selectedFingerprint = selectedDevice?.build_fingerprint ?? null;
  const currentFingerprintKeyRef = useRef<string | null>(null);
  useEffect(() => {
    const key = selectedTarget
      ? `${selectedTarget.serial}:${selectedFingerprint ?? ""}`
      : null;
    currentFingerprintKeyRef.current = key;
    if (!selectedTarget || !key || dismissedFingerprintNotices.has(key)) {
      setOtaNotice(false);
      return;
    }
    const lease = fingerprintOperation.begin();
    setOtaNotice(false);
    // One shared observation per device+fingerprint for the whole session.
    let pending = fingerprintObservations.get(key);
    if (!pending) {
      pending = callObserveDeviceFingerprint(selectedTarget);
      fingerprintObservations.set(key, pending);
    }
    void pending
      .then((observation) => {
        // Ignore a result for a device the user has since switched away from,
        // or one that was dismissed while the observation was in flight.
        lease.commit(() => {
          if (
            observation.changed &&
            currentFingerprintKeyRef.current === key &&
            !dismissedFingerprintNotices.has(key)
          ) {
            setOtaNotice(true);
          }
        });
      })
      .catch(() => {
        // A convenience signal; a failure must not disrupt the Apps route.
      });
  }, [
    fingerprintOperation,
    selectedDevice?.serial,
    selectedFingerprint,
    selectedTarget,
  ]);

  const dismissOtaNotice = useCallback(() => {
    const key = currentFingerprintKeyRef.current;
    if (key) {
      dismissedFingerprintNotices.add(key);
    }
    setOtaNotice(false);
  }, []);

  // Keyed on the memoized scalar target identity (not device object identity):
  // plugging or unplugging an unrelated device rebuilds every device object,
  // and refiring user discovery here would reset pkgState and wipe the
  // metadata cache for the still-selected device.
  const loadUsers = useCallback(async () => {
    if (!selectedTarget) {
      setUsers([]);
      setUsersReady(false);
      setUserError(null);
      return;
    }
    const lease = usersOperation.begin();
    setUsersReady(false);
    setUserError(null);
    try {
      const found = await callListUsers(selectedTarget);
      lease.commit(() => {
        setUsers(found);
        // The backend rejects empty or ambiguous discovery instead of
        // fabricating user 0, so a resolved list always has a foreground user.
        const foreground = found.find((u) => u.current) ?? found[0];
        setSelectedUser(foreground.id);
        setUsersReady(true);
      });
    } catch (e) {
      lease.commit(() => {
        setUsers([]);
        setUserError(errorMessage(e));
      });
    }
  }, [selectedTarget, usersOperation]);

  const loadPackages = useCallback(async () => {
    if (!selectedTarget || !usersReady) return;
    const lease = packagesOperation.begin();
    metadataOperations.invalidate();
    metadataRequestedRef.current.clear();
    setPackageMetadata({});
    setPkgState({ kind: "loading" });
    try {
      const [listing, actions] = await Promise.all([
        callListPackagesWithCapability(selectedTarget, filter, selectedUser),
        callGetPackageActionCapabilities(selectedTarget),
      ]);
      lease.commit(() =>
        setPkgState({
          kind: "ok",
          packages: listing.packages,
          archive: listing.archive,
          actions,
        }),
      );
    } catch (e) {
      lease.commit(() =>
        setPkgState({
          kind: "error",
          message: errorMessage(e),
        }),
      );
    }
  }, [
    filter,
    metadataOperations,
    packagesOperation,
    selectedTarget,
    selectedUser,
    usersReady,
  ]);

  const {
    installState,
    setInstallState,
    startInstall,
    cancelInstall,
    confirmInstallOverride,
  } = usePackageInstall({
    target: authorizedTarget,
    device: selectedDevice,
    incremental: incrementalInstall,
    loadPackages,
  });

  const requestPackageMetadata = useCallback(
    (packageName: string) => {
      if (!selectedTarget || !usersReady) return;
      if (metadataRequestedRef.current.has(packageName)) return;
      metadataRequestedRef.current.add(packageName);
      const lease = metadataOperations.begin(packageName);
      void callGetPackageMetadata(selectedTarget, packageName, selectedUser)
        .then((metadata) => {
          lease.commit(() =>
            setPackageMetadata((current) => ({
              ...current,
              [packageName]: metadata,
            })),
          );
        })
        .catch(() => {
          // Unsupported vendor resource shapes degrade to the package-name
          // fallback and are not retried until the package list refreshes.
          lease.commit(() =>
            setPackageMetadata((current) => ({
              ...current,
              [packageName]: null,
            })),
          );
        });
    },
    [metadataOperations, selectedTarget, selectedUser, usersReady],
  );

  const loadJournal = useCallback(async () => {
    if (!selectedTarget) {
      setJournalState({ kind: "idle" });
      return;
    }
    const lease = journalOperation.begin();
    setJournalState({ kind: "loading" });
    try {
      const entries = await callJournalList(selectedTarget);
      lease.commit(() => setJournalState({ kind: "ok", entries }));
    } catch (e) {
      lease.commit(() =>
        setJournalState({
          kind: "error",
          message: errorMessage(e),
        }),
      );
    }
  }, [journalOperation, selectedTarget]);

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
    setActionState({ kind: "idle" });
    // The manual picker path clears the selection too; keeping it here would
    // carry the previous device's picks onto the auto-rebound device (the
    // pruning effect keeps package names both devices share).
    setSelectedPackages([]);
    setInspectedPkg(null);
    setRecoveryState({ kind: "idle" });
  }, [authorizedDevices, selectedSerial, selectedTransportId]);

  useEffect(() => {
    void loadUsers();
  }, [loadUsers]);

  useEffect(() => {
    if (selectedSerial && usersReady) {
      void loadPackages();
    } else {
      setPkgState({ kind: "idle" });
    }
  }, [selectedSerial, usersReady, filter, selectedUser, loadPackages]);

  useEffect(() => {
    if (selectedSerial) void loadJournal();
    else setJournalState({ kind: "idle" });
  }, [selectedSerial, loadJournal]);

  useEffect(() => {
    if (pkgState.kind !== "ok") return;
    const available = new Set(pkgState.packages.map((pkg) => pkg.package));
    setSelectedPackages((previous) => {
      const next = previous.filter((pkg) => available.has(pkg));
      return next.length === previous.length ? previous : next;
    });
  }, [pkgState]);

  const startAction = useCallback(
    async (pkg: string, kind: ActionKind) => {
      if (!selectedDevice || !authorizedTarget || !usersReady) return;
      const lease = actionOperation.begin();
      try {
        const plan = await callPlanAction({
          serial: selectedDevice.serial,
          target: authorizedTarget,
          package: pkg,
          kind,
          user_id: selectedUser,
        });
        // R-122: uninstall-for-user is the one reviewed action whose inverse
        // may not exist. Prove it now, while the package is still installed —
        // after the uninstall there is nothing left to read.
        const recovery =
          kind === "uninstall_for_user"
            ? ((
                await callAssessUninstallRecovery(
                  authorizedTarget,
                  selectedUser,
                  [pkg],
                )
              ).find((entry) => entry.package === pkg)?.evidence ?? null)
            : null;
        lease.commit(() =>
          setActionState({ kind: "confirming", plan, recovery }),
        );
      } catch (e) {
        lease.commit(() =>
          setActionState({
            kind: "error",
            message: errorMessage(e),
          }),
        );
      }
    },
    [
      actionOperation,
      authorizedTarget,
      selectedDevice,
      selectedUser,
      usersReady,
    ],
  );

  const startBatchAction = useCallback(
    async (kind: ActionKind) => {
      if (
        !selectedDevice ||
        !authorizedTarget ||
        !usersReady ||
        selectedPackages.length < 2
      )
        return;
      const lease = actionOperation.begin();
      try {
        const plan = await callPlanActionBatch(
          [...selectedPackages].sort().map((pkg) => ({
            serial: selectedDevice.serial,
            target: authorizedTarget,
            package: pkg,
            kind,
            user_id: selectedUser,
          })),
        );
        lease.commit(() => setActionState({ kind: "confirming_batch", plan }));
      } catch (error) {
        lease.commit(() =>
          setActionState({
            kind: "error",
            message: errorMessage(error),
          }),
        );
      }
    },
    [
      actionOperation,
      authorizedTarget,
      selectedDevice,
      selectedPackages,
      selectedUser,
      usersReady,
    ],
  );

  const confirmAction = useCallback(async () => {
    // Same mid-flight device-switch guard as the journal undo paths: a switch
    // while the action applies must not re-set panels/selection for the old
    // device or race the new device's package/journal loads.
    const lease = actionOperation.begin();
    try {
      if (actionState.kind === "confirming_batch") {
        const plan = actionState.plan;
        setActionState({ kind: "applying_batch", plan });
        const result = await callApplyActionBatch(plan);
        if (!lease.isCurrent()) return;
        const failures = batchFailures(result);
        lease.commit(() => {
          setSelectedPackages(failures.map((item) => item.package));
          setActionState({
            kind: "success",
            message: t("apps.batchCompleted", {
              succeeded: result.items.length - failures.length,
              failed: failures.length,
            }),
            details: failures.map(
              (item) => `${item.package}: ${item.error ?? t("common.unknown")}`,
            ),
          });
        });
      } else if (actionState.kind === "confirming") {
        const plan = actionState.plan;
        setActionState({ kind: "applying", plan });
        await callApplyAction(plan);
        lease.commit(() =>
          setActionState({
            kind: "success",
            message: t("apps.planCompleted", { description: plan.description }),
          }),
        );
      } else {
        return;
      }
      if (!lease.isCurrent()) return;
      void loadPackages();
      void loadJournal();
    } catch (e) {
      lease.commit(() =>
        setActionState({
          kind: "error",
          message: errorMessage(e),
        }),
      );
    }
  }, [actionOperation, actionState, loadJournal, loadPackages, t]);

  const exportActionBaseline = useCallback(async () => {
    if (
      (actionState.kind !== "confirming" &&
        actionState.kind !== "confirming_batch") ||
      !authorizedTarget
    )
      return;
    const plans =
      actionState.kind === "confirming"
        ? [actionState.plan]
        : actionState.plan.plans;
    const first = plans[0];
    if (!first) return;
    const lease = recoveryOperation.begin();
    setRecoveryState({
      kind: "busy",
      message: t("apps.recoveryExporting"),
    });
    try {
      const selected = await callSelectHostPath(
        "recovery_baseline_save",
        plans.length === 1
          ? recoveryFileName(first.request.package)
          : recoveryBatchFileName(plans.length),
      );
      if (!lease.isCurrent()) return;
      if (!selected) {
        setRecoveryState({ kind: "idle" });
        return;
      }
      const artifact = await callExportRecoveryBaseline(
        authorizedTarget,
        first.request.user_id,
        plans.map((plan) => ({
          package: plan.request.package,
          kind: plan.request.kind,
        })),
        first.request.pack_context
          ? {
              id: first.request.pack_context.pack_id,
              revision: first.request.pack_context.revision,
            }
          : null,
        selected.id,
      );
      lease.commit(() =>
        setRecoveryState({
          kind: "saved",
          path: artifact.local_path,
          sha256: artifact.sha256,
        }),
      );
    } catch (error) {
      lease.commit(() =>
        setRecoveryState({
          kind: "error",
          message: errorMessage(error),
        }),
      );
    }
  }, [actionState, authorizedTarget, recoveryOperation, t]);

  /** Open a saved baseline and plan one half of the OTA round trip.
   *
   *  The direction is chosen up front rather than switched inside the review,
   *  because the two halves happen at different times — restore, update,
   *  re-apply — and because the native read grant is one-shot, so a switch
   *  would mean re-picking the file anyway. */
  const inspectRecoveryBaseline = useCallback(
    async (roundTrip: BaselineRoundTrip) => {
      if (!authorizedTarget) return;
      const lease = recoveryOperation.begin();
      setRecoveryState({
        kind: "busy",
        message: t("apps.recoveryInspecting"),
      });
      try {
        const selected = await callSelectHostPath("recovery_baseline_open");
        if (!lease.isCurrent()) return;
        if (!selected) {
          setRecoveryState({ kind: "idle" });
          return;
        }
        const diff = await callInspectRecoveryBaseline(
          authorizedTarget,
          selected.id,
          roundTrip,
        );
        lease.commit(() => setRecoveryState({ kind: "review", diff }));
      } catch (error) {
        lease.commit(() =>
          setRecoveryState({
            kind: "error",
            message: errorMessage(error),
          }),
        );
      }
    },
    [authorizedTarget, recoveryOperation, t],
  );

  const applyRecoveryBaseline = useCallback(async () => {
    if (recoveryState.kind !== "review") return;
    const { diff } = recoveryState;
    const lease = recoveryOperation.begin();
    setRecoveryState({
      kind: "busy",
      message: t("apps.recoveryApplying", { count: diff.plans.length }),
    });
    let applied = 0;
    const failures: string[] = [];
    for (const plan of diff.plans) {
      if (!lease.isCurrent()) return;
      try {
        await callApplyAction(plan);
        if (!lease.isCurrent()) return;
        applied += 1;
      } catch (error) {
        if (!lease.isCurrent()) return;
        failures.push(`${plan.request.package}: ${errorMessage(error)}`);
      }
    }
    if (
      !lease.commit(() =>
        setRecoveryState({ kind: "result", diff, applied, failures }),
      )
    )
      return;
    await Promise.all([loadPackages(), loadJournal()]);
  }, [loadJournal, loadPackages, recoveryOperation, recoveryState, t]);

  const undoJournalEntry = useCallback(
    async (entry: JournalEntry) => {
      if (!selectedDevice || !authorizedTarget || !usersReady) return;
      const lease = undoOperation.begin();
      setUndoingEntryId(entry.id);
      try {
        await callJournalUndo(authorizedTarget, entry.id);
        if (
          !lease.commit(() =>
            setActionState({
              kind: "success",
              message: t("apps.journalUndoCompleted", {
                package: entry.applied.plan.request.package,
              }),
            }),
          )
        )
          return;
        await Promise.all([loadPackages(), loadJournal()]);
      } catch (e) {
        lease.commit(() =>
          setActionState({
            kind: "error",
            message: errorMessage(e),
          }),
        );
      } finally {
        lease.commit(() => setUndoingEntryId(null));
      }
    },
    [
      authorizedTarget,
      loadJournal,
      loadPackages,
      selectedDevice,
      t,
      undoOperation,
      usersReady,
    ],
  );

  const undoJournalBatch = useCallback(
    async (batchId: string) => {
      if (!selectedDevice || !authorizedTarget || !usersReady) return;
      const lease = undoOperation.begin();
      setUndoingBatchId(batchId);
      try {
        const result = await callJournalUndoBatch(authorizedTarget, batchId);
        if (!lease.isCurrent()) return;
        const failures = batchFailures(result);
        lease.commit(() => {
          setActionState({
            kind: "success",
            message: t("apps.batchUndoCompleted", {
              succeeded: result.items.length - failures.length,
              failed: failures.length,
            }),
            details: failures.map(
              (item) => `${item.package}: ${item.error ?? t("common.unknown")}`,
            ),
          });
        });
        await Promise.all([loadPackages(), loadJournal()]);
      } catch (error) {
        lease.commit(() =>
          setActionState({
            kind: "error",
            message: errorMessage(error),
          }),
        );
      } finally {
        lease.commit(() => setUndoingBatchId(null));
      }
    },
    [
      authorizedTarget,
      loadJournal,
      loadPackages,
      selectedDevice,
      t,
      undoOperation,
      usersReady,
    ],
  );

  // Filtering the (potentially several-hundred-entry) package list on every
  // keystroke re-renders every interactive row, each of which mounts an
  // IntersectionObserver. Deferring the search term keeps the input responsive
  // and lets React render the heavy filtered table at a lower priority, and
  // memoizing avoids recomputing the filter on unrelated re-renders.
  const deferredSearch = useDeferredValue(search);
  const filteredPackages = useMemo(
    () =>
      pkgState.kind === "ok"
        ? pkgState.packages.filter((p) =>
            deferredSearch
              ? [p.package, packageMetadata[p.package]?.label ?? ""]
                  .join(" ")
                  .toLowerCase()
                  .includes(deferredSearch.toLowerCase())
              : true,
          )
        : [],
    [pkgState, packageMetadata, deferredSearch],
  );
  const selectedPackageSet = useMemo(
    () => new Set(selectedPackages),
    [selectedPackages],
  );
  const selectedRows =
    pkgState.kind === "ok"
      ? pkgState.packages.filter((pkg) => selectedPackageSet.has(pkg.package))
      : [];
  const batchReady = selectedRows.length >= 2;
  const canBatchDisable =
    batchReady &&
    selectedRows.every((pkg) => pkg.enabled && !pkg.archived && !pkg.retained);
  const canBatchEnable =
    batchReady &&
    selectedRows.every((pkg) => !pkg.enabled && !pkg.archived && !pkg.retained);
  const canBatchArchive =
    batchReady &&
    pkgState.kind === "ok" &&
    pkgState.archive.supported &&
    selectedRows.every((pkg) => !pkg.system && !pkg.archived && !pkg.retained);
  const canBatchUnarchive =
    batchReady && selectedRows.every((pkg) => pkg.archived);
  const canBatchSuspend =
    batchReady &&
    pkgState.kind === "ok" &&
    pkgState.actions.suspend.supported &&
    selectedRows.every((pkg) => pkg.enabled && !pkg.archived && !pkg.retained);
  const canBatchUnsuspend =
    batchReady &&
    pkgState.kind === "ok" &&
    pkgState.actions.unsuspend.supported &&
    selectedRows.every((pkg) => pkg.enabled && !pkg.archived && !pkg.retained);

  // R-109: archive is only reversible via `request-unarchive` when the
  // installer-of-record can handle the unarchive intent. Flag packages in the
  // pending archive review whose installer is a sideload source, is missing, or
  // is an unverified third-party store, so the review can downgrade the
  // "reversible" promise instead of stranding the package.
  const archiveWarnings = useMemo<
    { package: string; outlook: UnarchiveOutlook }[]
  >(() => {
    if (pkgState.kind !== "ok") return [];
    const plans =
      actionState.kind === "confirming"
        ? [actionState.plan]
        : actionState.kind === "confirming_batch"
          ? actionState.plan.plans
          : [];
    const archivePlans = plans.filter(
      (plan) => plan.request.kind === "archive",
    );
    if (archivePlans.length === 0) return [];
    const installedIds = new Set(
      pkgState.packages
        .filter((pkg) => !pkg.archived && !pkg.retained)
        .map((pkg) => pkg.package),
    );
    const installerByPackage = new Map(
      pkgState.packages.map((pkg) => [pkg.package, pkg.installer ?? null]),
    );
    return archivePlans
      .map((plan) => ({
        package: plan.request.package,
        outlook: classifyUnarchive(
          installerByPackage.get(plan.request.package),
          installedIds,
        ),
      }))
      .filter((entry) => archiveIsRisky(entry.outlook));
  }, [actionState, pkgState]);

  return (
    <>
      <PaneHeader
        title={t("apps.title")}
        description={t("apps.description")}
        actions={
          selectedDevice ? (
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                onClick={() => void inspectRecoveryBaseline("restore")}
                disabled={
                  !authorizedTarget ||
                  recoveryState.kind === "busy" ||
                  !usersReady
                }
                variant="ghost"
              >
                {t("apps.recoveryRestoreOpen")}
              </Button>
              <Button
                type="button"
                onClick={() => void inspectRecoveryBaseline("reapply")}
                disabled={
                  !authorizedTarget ||
                  recoveryState.kind === "busy" ||
                  !usersReady
                }
                variant="ghost"
              >
                {t("apps.recoveryReapplyOpen")}
              </Button>
              <Button
                type="button"
                onClick={() => void startInstall()}
                disabled={
                  installState.kind === "choosing" ||
                  installState.kind === "running"
                }
                variant="primary"
              >
                {t("apps.installPackage")}
              </Button>
              <label
                className="inline-flex items-center gap-2 text-xs text-anvil-300"
                title={t("apps.installIncrementalHint")}
              >
                <input
                  type="checkbox"
                  checked={incrementalInstall}
                  onChange={(e) => setIncrementalInstall(e.target.checked)}
                  disabled={
                    installState.kind === "choosing" ||
                    installState.kind === "running"
                  }
                />
                {t("apps.installIncremental")}
              </label>
              <Button
                type="button"
                onClick={() => void loadPackages()}
                disabled={pkgState.kind === "loading"}
                variant="ghost"
              >
                {pkgState.kind === "loading"
                  ? t("apps.loading")
                  : t("apps.refreshPackages")}
              </Button>
            </div>
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
            {selectedTarget && (
              <TransportBadge kind={selectedTarget.transport_kind} />
            )}
          </div>
        }
      />

      <section className="mt-4 max-w-none space-y-3">
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

        {otaNotice && (
          <StatePanel
            title={t("apps.otaDriftTitle")}
            tone="warning"
            actions={
              <Button type="button" size="sm" onClick={dismissOtaNotice}>
                {t("common.dismiss")}
              </Button>
            }
          >
            <p>{t("apps.otaDriftBody")}</p>
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
              setActionState({ kind: "idle" });
              setSelectedPackages([]);
              setInspectedPkg(null);
              setInstallState({ kind: "idle" });
              setRecoveryState({ kind: "idle" });
            }}
          />
        )}

        <TransportTrustNotice
          target={selectedTarget}
          accepted={transportOverrideAccepted}
          onAcceptedChange={setTransportOverrideAccepted}
        />

        <RecoveryBaselinePanel
          state={recoveryState}
          onApply={() => void applyRecoveryBaseline()}
          onDismiss={() => setRecoveryState({ kind: "idle" })}
        />

        {selectedSerial && (
          <>
            {userError && (
              <StatePanel title={t("apps.userDiscoveryFailed")} tone="danger">
                <p>{userError}</p>
              </StatePanel>
            )}
            <div className="flex flex-wrap items-center gap-3">
              <FilterChips
                active={filter}
                onChange={(f) => {
                  setFilter(f);
                  setSearch("");
                  setSelectedPackages([]);
                }}
              />
              {users.length > 1 && (
                <label className="flex items-center gap-2 text-sm text-anvil-300">
                  <span>{t("apps.userLabel")}</span>
                  <FieldSelect
                    value={selectedUser}
                    onChange={(e) => {
                      setSelectedUser(Number(e.target.value));
                      setSelectedPackages([]);
                      setRecoveryState({ kind: "idle" });
                    }}
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
              <Button
                type="button"
                size="sm"
                variant="ghost"
                aria-pressed={showAdvancedBackups}
                onClick={() => setShowAdvancedBackups((visible) => !visible)}
              >
                {showAdvancedBackups
                  ? t("apps.hideAdvancedBackup")
                  : t("apps.showAdvancedBackup")}
              </Button>
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
              <>
                {!pkgState.archive.supported && (
                  <StatePanel
                    title={t("apps.archiveUnavailable")}
                    tone="warning"
                  >
                    <p>{pkgState.archive.reason}</p>
                  </StatePanel>
                )}
                {filter === "retained" && (
                  <StatePanel title={t("apps.retainedLabel")} tone="info">
                    <p>{t("apps.retainedBody")}</p>
                  </StatePanel>
                )}
                <BatchActionBar
                  selectedCount={selectedRows.length}
                  canSuspend={canBatchSuspend}
                  canUnsuspend={canBatchUnsuspend}
                  canDisable={canBatchDisable}
                  canEnable={canBatchEnable}
                  canArchive={canBatchArchive}
                  canUnarchive={canBatchUnarchive}
                  onClear={() => setSelectedPackages([])}
                  onAction={(kind) => void startBatchAction(kind)}
                />
                <PackageTable
                  packages={filteredPackages}
                  metadata={packageMetadata}
                  totalCount={pkgState.packages.length}
                  archiveSupported={pkgState.archive.supported}
                  suspendSupported={pkgState.actions.suspend.supported}
                  unsuspendSupported={pkgState.actions.unsuspend.supported}
                  hideSupported={pkgState.actions.hide.supported}
                  unhideSupported={pkgState.actions.unhide.supported}
                  unstopSupported={pkgState.actions.unstop.supported}
                  disableUntilUsedSupported={
                    pkgState.actions.disable_until_used.supported
                  }
                  defaultStateSupported={
                    pkgState.actions.default_state.supported
                  }
                  suspendQuarantineSupported={
                    pkgState.actions.suspend_quarantine.supported
                  }
                  selectedPackages={selectedPackageSet}
                  onToggleSelected={(pkg) =>
                    setSelectedPackages((previous) =>
                      previous.includes(pkg)
                        ? previous.filter((candidate) => candidate !== pkg)
                        : [...previous, pkg],
                    )
                  }
                  onToggleAll={() => {
                    const visible = filteredPackages.map((pkg) => pkg.package);
                    const allVisibleSelected = visible.every((pkg) =>
                      selectedPackageSet.has(pkg),
                    );
                    setSelectedPackages((previous) =>
                      allVisibleSelected
                        ? previous.filter((pkg) => !visible.includes(pkg))
                        : [...new Set([...previous, ...visible])],
                    );
                  }}
                  onMetadataRequest={requestPackageMetadata}
                  onAction={startAction}
                  onInspect={setInspectedPkg}
                  onExport={(pkg) => void runPackageExport(pkg, "apk_export")}
                  onLegacyExport={(pkg) => void inspectLegacyExport(pkg)}
                  showLegacyExport={showAdvancedBackups}
                />
              </>
            )}

            <JournalPanel
              state={journalState}
              undoingEntryId={undoingEntryId}
              undoingBatchId={undoingBatchId}
              onRefresh={() => void loadJournal()}
              onUndo={(entry) => void undoJournalEntry(entry)}
              onUndoBatch={(batchId) => void undoJournalBatch(batchId)}
            />
          </>
        )}

        {inspectedPkg && authorizedTarget && usersReady && (
          <PermissionsPanel
            target={authorizedTarget}
            pkg={inspectedPkg}
            userId={selectedUser}
            onClose={() => setInspectedPkg(null)}
          />
        )}

        {backupNotice && (
          <BackupStatePanel
            notice={backupNotice}
            onDismiss={() => setBackupNotice(null)}
            onCancel={() => void cancelBackup()}
            onContinueLegacy={(pending) =>
              void runPackageExport(
                pending.package,
                "legacy_data",
                pending.preflight,
              )
            }
          />
        )}

        <InstallStatePanel
          state={installState}
          onCancel={() => void cancelInstall()}
          onDismiss={() => setInstallState({ kind: "idle" })}
          onReviewOverride={() =>
            setInstallState((previous) =>
              previous.kind === "result"
                ? {
                    kind: "confirming_override",
                    localPath: previous.localPath,
                    result: previous.result,
                  }
                : previous,
            )
          }
        />

        {installState.kind === "confirming_override" && (
          <InstallOverrideDialog
            result={installState.result}
            onCancel={() =>
              setInstallState({
                kind: "result",
                localPath: installState.localPath,
                result: installState.result,
              })
            }
            onConfirm={() => void confirmInstallOverride()}
          />
        )}

        <ActionOverlay
          state={actionState}
          archiveWarnings={archiveWarnings}
          onConfirm={() => void confirmAction()}
          onExportBaseline={() => void exportActionBaseline()}
          exportingBaseline={recoveryState.kind === "busy"}
          baselineFeedback={
            recoveryState.kind === "saved"
              ? t("apps.recoveryExportedReady")
              : recoveryState.kind === "error"
                ? t("apps.recoveryExportError", {
                    message: recoveryState.message,
                  })
                : null
          }
          onCancel={() => setActionState({ kind: "idle" })}
          onDismiss={() => setActionState({ kind: "idle" })}
        />
      </section>
    </>
  );
}

function recoveryFileName(packageName: string): string {
  const date = new Date().toISOString().slice(0, 10);
  const safePackage = packageName.replace(/[^A-Za-z0-9_.-]/g, "_");
  return `droidsmith-recovery-${date}-${safePackage}.json`;
}

function recoveryBatchFileName(count: number): string {
  const date = new Date().toISOString().slice(0, 10);
  return `droidsmith-recovery-${date}-batch-${count}-packages.json`;
}

// A batch item with a non-null `error` failed at the device; successful items
// carry a journal entry and no error. Callers use this to report per-package
// failures without aborting the surviving inverses.
function batchFailures(result: BatchActionResult): BatchActionItemResult[] {
  return result.items.filter((item) => item.error != null);
}
