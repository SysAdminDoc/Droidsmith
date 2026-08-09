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
  callBackupPackage,
  callCancelOperation,
  callExportPackageApks,
  callExportRecoveryBaseline,
  callInspectRecoveryBaseline,
  callGetPackageMetadata,
  callGetPackageActionCapabilities,
  callInstallApk,
  callJournalList,
  callJournalUndo,
  callJournalUndoBatch,
  callListPackagesWithCapability,
  callListUsers,
  callObserveDeviceFingerprint,
  callAssessUninstallRecovery,
  callPlanAction,
  callPlanActionBatch,
  callPreflightPackageBackup,
  callSelectHostPath,
  callGrantDroppedPath,
  inTauri,
  deviceTarget,
  newOperationId,
  type ActionKind,
  type BaselineRoundTrip,
  type FingerprintObservation,
  type AndroidUser,
  type AppPackageMetadata,
  type BatchActionItemResult,
  type BatchActionResult,
  type JournalEntry,
  type InstallOptions,
  type PackageFilter,
  type OperationEvent,
  type PackageBackupPreflight,
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

import {
  canRunLegacyExport,
  packageExportDefaultFileName,
  packageExportDisplayState,
} from "./appsBackup";
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
import {
  BackupStatePanel,
  InstallOverrideDialog,
  InstallStatePanel,
} from "./apps/InstallPanels";
import type {
  ActionState,
  BackupNotice,
  InstallState,
  JournalState,
  PackagesState,
  RecoveryState,
} from "./apps/types";
import { presentRecovery } from "./apps/uninstallRecovery";
import { useFocusTrap } from "../lib/useFocusTrap";
import {
  Badge,
  Button,
  Card,
  DevicePicker,
  FieldInput,
  PaneHeader,
  SkeletonLine,
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
  const [backupNotice, setBackupNotice] = useState<BackupNotice | null>(null);
  const [showAdvancedBackups, setShowAdvancedBackups] = useState(false);
  const [installState, setInstallState] = useState<InstallState>({
    kind: "idle",
  });
  const [incrementalInstall, setIncrementalInstall] = useState(false);
  const [recoveryState, setRecoveryState] = useState<RecoveryState>({
    kind: "idle",
  });
  const [otaNotice, setOtaNotice] = useState(false);
  const activeBackupRef = useRef<string | null>(null);
  const backupGenerationRef = useRef(0);
  const activeInstallRef = useRef<string | null>(null);
  const installGenerationRef = useRef(0);
  const metadataRequestedRef = useRef(new Set<string>());
  // Mirrors installState for the drag-drop listener, which is intentionally
  // not re-subscribed on every state change (drops during the unlisten window
  // would be lost).
  const installStateRef = useRef(installState);
  installStateRef.current = installState;

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
  const backupOperation = useTargetOperation(
    authorizedTarget,
    `apps-backup:${selectedUser}`,
  );
  const installOperation = useTargetOperation(authorizedTarget, "apps-install");
  const recoveryOperation = useTargetOperation(
    authorizedTarget,
    `apps-recovery:${selectedUser}`,
  );
  const undoOperation = useTargetOperation(
    authorizedTarget,
    `apps-undo:${selectedUser}`,
  );

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
    backupGenerationRef.current += 1;
    installGenerationRef.current += 1;
    const operationId = activeBackupRef.current;
    const installOperationId = activeInstallRef.current;
    activeBackupRef.current = null;
    activeInstallRef.current = null;
    if (operationId) void callCancelOperation(operationId);
    if (installOperationId) void callCancelOperation(installOperationId);
    setSelectedSerial(next?.serial ?? null);
    setSelectedTransportId(next?.transport_id ?? null);
    setActionState({ kind: "idle" });
    // The manual picker path clears the selection too; keeping it here would
    // carry the previous device's picks onto the auto-rebound device (the
    // pruning effect keeps package names both devices share).
    setSelectedPackages([]);
    setInspectedPkg(null);
    setBackupNotice(null);
    setInstallState({ kind: "idle" });
    setRecoveryState({ kind: "idle" });
  }, [authorizedDevices, selectedSerial, selectedTransportId]);

  useEffect(() => {
    return () => {
      backupGenerationRef.current += 1;
      installGenerationRef.current += 1;
      const operationId = activeBackupRef.current;
      const installOperationId = activeInstallRef.current;
      activeBackupRef.current = null;
      activeInstallRef.current = null;
      if (operationId) void callCancelOperation(operationId);
      if (installOperationId) void callCancelOperation(installOperationId);
    };
  }, []);

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

  const runPackageExport = useCallback(
    async (
      pkg: string,
      mode: "apk_export" | "legacy_data",
      inspected?: PackageBackupPreflight,
    ) => {
      if (!selectedDevice || !authorizedTarget || !usersReady) return;
      // One export at a time: claiming a fresh generation while another export
      // runs would silently orphan its completion/failure handling and make it
      // uncancellable.
      if (activeBackupRef.current) return;
      const lease = backupOperation.begin();
      // Claim the generation before the first await so a device switch during
      // the preflight/save dialog invalidates this export before it starts.
      const generation = backupGenerationRef.current + 1;
      backupGenerationRef.current = generation;
      let startedOperationId: string | null = null;
      try {
        const preflight =
          inspected ??
          (await callPreflightPackageBackup(
            authorizedTarget,
            pkg,
            selectedUser,
          ));
        if (backupGenerationRef.current !== generation || !lease.isCurrent())
          return;
        if (mode === "legacy_data" && !canRunLegacyExport(preflight)) {
          setBackupNotice({
            title: t("apps.legacyBlockedTitle"),
            message: preflight.evidence.reason,
            tone: "warning",
            evidence: preflight.evidence,
            showLimitations: true,
          });
          return;
        }

        const pathGrant = await callSelectHostPath(
          mode === "apk_export" ? "package_export_save" : "backup_save",
          packageExportDefaultFileName(pkg, mode),
        );
        if (backupGenerationRef.current !== generation || !lease.isCurrent())
          return;
        if (!pathGrant) {
          setBackupNotice({
            title: t("apps.exportCancelledTitle"),
            message: t("apps.exportCancelled"),
            tone: "neutral",
          });
          return;
        }

        const operationId = newOperationId(
          mode === "apk_export" ? "package-export" : "legacy-backup",
        );
        startedOperationId = operationId;
        activeBackupRef.current = operationId;
        lease.registerCancellation(operationId);
        setBackupNotice({
          title:
            mode === "apk_export"
              ? t("apps.apkExportRunningTitle", { package: pkg })
              : t("apps.legacyRunningTitle", { package: pkg }),
          message:
            mode === "apk_export"
              ? t("apps.apkExportRunningBody", {
                  count: preflight.apk_paths.length,
                })
              : t("apps.legacyLimitations"),
          tone: "info",
          path: pathGrant.local_path,
          operationId,
          progress: t("apps.exportStarting"),
          evidence: preflight.evidence,
          showLimitations: mode === "legacy_data",
        });

        const options = {
          operationId,
          onEvent: (event: OperationEvent) => {
            if (
              activeBackupRef.current !== operationId ||
              backupGenerationRef.current !== generation ||
              !lease.isCurrent()
            )
              return;
            setBackupNotice((previous) => {
              if (!previous || previous.operationId !== operationId)
                return previous;
              if (event.kind === "progress") {
                return {
                  ...previous,
                  progress:
                    event.message ??
                    t("apps.exportProgress", {
                      seconds: Math.max(
                        1,
                        Math.round((event.elapsed_ms ?? 0) / 1000),
                      ),
                    }),
                };
              }
              if (event.kind === "output" && event.chunk) {
                return {
                  ...previous,
                  output: `${previous.output ?? ""}${event.chunk}`.slice(
                    -64 * 1024,
                  ),
                };
              }
              return previous;
            });
          },
        };
        const result =
          mode === "apk_export"
            ? await callExportPackageApks(
                authorizedTarget,
                pkg,
                selectedUser,
                pathGrant.id,
                options,
              )
            : await callBackupPackage(
                authorizedTarget,
                pkg,
                selectedUser,
                pathGrant.id,
                options,
              );
        if (backupGenerationRef.current !== generation || !lease.isCurrent())
          return;
        activeBackupRef.current = null;
        const displayState = packageExportDisplayState(result);
        const titleByState: Record<typeof displayState, string> = {
          apk_exported: t("apps.apkExportSavedTitle"),
          legacy_entries_detected: t("apps.legacyInspectedTitle"),
          legacy_no_data: t("apps.legacyNoDataTitle"),
        };
        const messageByState: Record<typeof displayState, string> = {
          apk_exported: t("apps.apkExportSaved", {
            file: result.artifact.local_path,
            count: result.manifest.artifacts.length,
          }),
          legacy_entries_detected: t("apps.legacyEntriesDetected", {
            file: result.artifact.local_path,
          }),
          legacy_no_data: t("apps.legacyNoDataBody", {
            file: result.artifact.local_path,
          }),
        };
        setBackupNotice({
          title: titleByState[displayState],
          message: messageByState[displayState],
          tone: displayState === "apk_exported" ? "success" : "warning",
          path: result.artifact.local_path,
          sizeBytes: result.artifact.size_bytes,
          showLimitations: mode === "legacy_data",
          evidence: result.manifest.eligibility,
        });
      } catch (e) {
        if (backupGenerationRef.current !== generation || !lease.isCurrent())
          return;
        if (
          startedOperationId &&
          activeBackupRef.current !== startedOperationId
        )
          return;
        activeBackupRef.current = null;
        setBackupNotice({
          title: t("apps.exportFailedTitle"),
          message: t("apps.exportFailed", {
            message: errorMessage(e),
          }),
          tone: "danger",
        });
      }
    },
    [
      authorizedTarget,
      backupOperation,
      selectedDevice,
      selectedUser,
      t,
      usersReady,
    ],
  );

  const inspectLegacyExport = useCallback(
    async (pkg: string) => {
      if (!authorizedTarget || !usersReady) return;
      const lease = backupOperation.begin();
      try {
        const preflight = await callPreflightPackageBackup(
          authorizedTarget,
          pkg,
          selectedUser,
        );
        const runnable = canRunLegacyExport(preflight);
        lease.commit(() =>
          setBackupNotice({
            title: runnable
              ? t("apps.legacyReviewTitle")
              : t("apps.legacyBlockedTitle"),
            message: preflight.evidence.reason,
            tone:
              preflight.legacy_capability === "legacy_data_eligible"
                ? "info"
                : "warning",
            evidence: preflight.evidence,
            showLimitations: true,
            pendingLegacy: runnable ? { package: pkg, preflight } : undefined,
          }),
        );
      } catch (error) {
        lease.commit(() =>
          setBackupNotice({
            title: t("apps.exportFailedTitle"),
            message: t("apps.exportFailed", {
              message: errorMessage(error),
            }),
            tone: "danger",
          }),
        );
      }
    },
    [authorizedTarget, backupOperation, selectedUser, t, usersReady],
  );

  const cancelBackup = useCallback(async () => {
    const operationId = activeBackupRef.current;
    if (!operationId) return;
    setBackupNotice((previous) =>
      previous ? { ...previous, progress: t("apps.backupCancelling") } : null,
    );
    await callCancelOperation(operationId);
  }, [t]);

  const runInstall = useCallback(
    async (
      pathGrant: string,
      localPath: string,
      installOptions: InstallOptions,
    ) => {
      if (!selectedDevice || !authorizedTarget) return;
      const lease = installOperation.begin();
      const operationId = newOperationId("install");
      const generation = installGenerationRef.current + 1;
      installGenerationRef.current = generation;
      activeInstallRef.current = operationId;
      lease.registerCancellation(operationId);
      setInstallState({
        kind: "running",
        operationId,
        progress: t("apps.installStarting"),
        output: "",
      });
      try {
        const result = await callInstallApk(
          authorizedTarget,
          pathGrant,
          installOptions,
          {
            operationId,
            onEvent: (event: OperationEvent) => {
              if (
                activeInstallRef.current !== operationId ||
                installGenerationRef.current !== generation ||
                !lease.isCurrent()
              )
                return;
              setInstallState((previous) => {
                if (
                  previous.kind !== "running" ||
                  previous.operationId !== operationId
                )
                  return previous;
                if (event.kind === "output" && event.chunk) {
                  return {
                    ...previous,
                    output: `${previous.output}${event.chunk}`.slice(
                      -64 * 1024,
                    ),
                  };
                }
                if (event.kind === "progress" && event.message) {
                  return { ...previous, progress: event.message };
                }
                return previous;
              });
            },
          },
        );
        if (installGenerationRef.current !== generation || !lease.isCurrent())
          return;
        activeInstallRef.current = null;
        setInstallState({ kind: "result", localPath, result });
        if (result.succeeded) await loadPackages();
      } catch (error) {
        if (
          activeInstallRef.current !== operationId ||
          installGenerationRef.current !== generation ||
          !lease.isCurrent()
        )
          return;
        activeInstallRef.current = null;
        setInstallState({
          kind: "error",
          message: errorMessage(error),
        });
      }
    },
    [authorizedTarget, installOperation, loadPackages, selectedDevice, t],
  );

  const startInstall = useCallback(async () => {
    if (!selectedDevice) return;
    const lease = installOperation.begin();
    setInstallState({ kind: "choosing" });
    try {
      const selected = await callSelectHostPath("install_open");
      if (!lease.isCurrent()) return;
      if (!selected) {
        setInstallState({ kind: "idle" });
        return;
      }
      await runInstall(selected.id, selected.local_path, {
        incremental: incrementalInstall,
      });
    } catch (error) {
      lease.commit(() =>
        setInstallState({
          kind: "error",
          message: errorMessage(error),
        }),
      );
    }
  }, [incrementalInstall, installOperation, runInstall, selectedDevice]);

  useEffect(() => {
    if (!inTauri() || !selectedDevice) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) => {
      if (cancelled) return;
      void getCurrentWebview()
        .onDragDropEvent(async (event) => {
          if (event.payload.type !== "drop" || cancelled) return;
          // The install button is disabled while an install runs; a drop must
          // not start a second concurrent install that silently orphans the
          // first (read via ref — the listener is not re-subscribed on state
          // changes).
          if (
            installStateRef.current.kind === "running" ||
            installStateRef.current.kind === "choosing"
          )
            return;
          const apkPaths = event.payload.paths.filter((p) => {
            const ext = p.split(".").pop()?.toLowerCase() ?? "";
            return ["apk", "apks", "xapk", "apkm"].includes(ext);
          });
          if (apkPaths.length === 0) return;
          const path = apkPaths[0];
          const lease = installOperation.begin();
          try {
            const grant = await callGrantDroppedPath(path);
            if (cancelled || !lease.isCurrent()) return;
            await runInstall(grant.id, grant.local_path, {
              incremental: incrementalInstall,
            });
          } catch (error) {
            if (cancelled || !lease.isCurrent()) return;
            setInstallState({
              kind: "error",
              message: errorMessage(error),
            });
          }
        })
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        });
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [incrementalInstall, installOperation, runInstall, selectedDevice]);

  const cancelInstall = useCallback(async () => {
    const operationId = activeInstallRef.current;
    if (!operationId) return;
    setInstallState((previous) =>
      previous.kind === "running"
        ? { ...previous, progress: t("apps.installCancelling") }
        : previous,
    );
    await callCancelOperation(operationId);
  }, [t]);

  const confirmInstallOverride = useCallback(async () => {
    if (
      installState.kind !== "confirming_override" ||
      !installState.result.failure?.suggested_override
    )
      return;
    const installOptions: InstallOptions = {
      override_confirmed: true,
      allow_downgrade:
        installState.result.failure.suggested_override === "allow_downgrade",
      bypass_low_target_sdk_block:
        installState.result.failure.suggested_override ===
        "bypass_low_target_sdk_block",
    };
    const retryGrant = installState.result.retry_path_grant;
    if (!retryGrant) {
      setInstallState({
        kind: "error",
        message: t("apps.installGrantExpired"),
      });
      return;
    }
    await runInstall(retryGrant, installState.localPath, installOptions);
  }, [installState, runInstall, t]);

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
              backupGenerationRef.current += 1;
              installGenerationRef.current += 1;
              const operationId = activeBackupRef.current;
              const installOperationId = activeInstallRef.current;
              activeBackupRef.current = null;
              activeInstallRef.current = null;
              if (operationId) void callCancelOperation(operationId);
              if (installOperationId)
                void callCancelOperation(installOperationId);
              setSelectedSerial(device.serial);
              setSelectedTransportId(device.transport_id);
              setActionState({ kind: "idle" });
              setSelectedPackages([]);
              setInspectedPkg(null);
              setBackupNotice(null);
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

function ActionOverlay({
  state,
  archiveWarnings,
  onConfirm,
  onExportBaseline,
  exportingBaseline,
  baselineFeedback,
  onCancel,
  onDismiss,
}: {
  state: ActionState;
  archiveWarnings: { package: string; outlook: UnarchiveOutlook }[];
  onConfirm: () => void;
  onExportBaseline: () => void;
  exportingBaseline: boolean;
  baselineFeedback: string | null;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const confirming =
    state.kind === "confirming" || state.kind === "confirming_batch";
  const applying = state.kind === "applying" || state.kind === "applying_batch";
  const trapRef = useFocusTrap<HTMLDivElement>(confirming || applying);

  useEffect(() => {
    if (!confirming) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [confirming, onCancel]);

  useEffect(() => {
    if (applying) trapRef.current?.focus();
  }, [applying, trapRef]);

  if (state.kind === "idle") return null;

  if (state.kind === "confirming" || state.kind === "confirming_batch") {
    const plans = state.kind === "confirming" ? [state.plan] : state.plan.plans;
    const description = state.plan.description;
    const portableBaselineSupported = plans.every(
      (plan) =>
        ![
          "suspend",
          "unsuspend",
          "unstop",
          "hide",
          "unhide",
          "disable_until_used",
          "default_state",
          "suspend_quarantine",
          "archive",
          "request_unarchive",
        ].includes(plan.request.kind),
    );
    const tier = actionTier(plans[0]?.request.kind);
    const recovery =
      state.kind === "confirming" ? presentRecovery(state.recovery) : null;
    return (
      <div
        ref={trapRef}
        tabIndex={-1}
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 px-4 outline-none backdrop-blur-sm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-title"
        aria-describedby="confirm-dialog-description"
      >
        <Card surface="dialog" className="w-full max-w-lg p-6">
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
            {description}
          </p>
          <div className="mt-3 rounded-md border border-circuit-300/25 bg-circuit-950/20 p-3">
            <p className="text-xs font-semibold text-circuit-100">
              {t(`apps.actionTier.${tier}.title`)}
            </p>
            <p className="mt-1 text-xs leading-5 text-anvil-200">
              {t(`apps.actionTier.${tier}.body`)}
            </p>
          </div>
          {recovery && (
            <div
              className="mt-3 rounded-md border border-white/10 bg-white/[0.04] p-3"
              data-testid="uninstall-recovery"
              data-recovery-tone={recovery.tone}
            >
              <Badge tone={recovery.tone}>{t(recovery.titleKey)}</Badge>
              <p className="mt-2 text-xs leading-5 text-anvil-200">
                {t(recovery.detailKey)}
              </p>
            </div>
          )}
          <div className="mt-3 max-h-56 space-y-2 overflow-y-auto rounded-md border border-white/10 bg-white/[0.04] p-3">
            <p className="text-xs font-medium text-anvil-400">
              {plans.length === 1
                ? t("apps.commandPreview")
                : t("apps.batchCommandPreview", { count: plans.length })}
            </p>
            {plans.map((plan) => (
              <code
                key={plan.incident_id}
                className="block break-all font-mono text-xs text-anvil-100"
              >
                adb -s {plan.request.serial} shell {plan.args.join(" ")}
              </code>
            ))}
          </div>
          {archiveWarnings.length > 0 && (
            <div
              className="mt-3 rounded-md border border-amber-300/30 bg-amber-950/20 p-3"
              role="alert"
            >
              <p className="text-xs font-semibold text-amber-200">
                {t("apps.archiveNotReversibleTitle")}
              </p>
              <p className="mt-1 text-xs leading-5 text-amber-100/90">
                {t("apps.archiveNotReversibleBody")}
              </p>
              <ul className="mt-2 space-y-1">
                {archiveWarnings.map((entry) => (
                  <li
                    key={entry.package}
                    className="break-all font-mono text-xs text-amber-100"
                  >
                    {entry.package} —{" "}
                    <span className="font-sans">
                      {t(`apps.unarchiveOutlook.${entry.outlook}`)}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {baselineFeedback && (
            <p
              className="mt-3 text-xs leading-5 text-circuit-100"
              role="status"
            >
              {baselineFeedback}
            </p>
          )}
          <div className="mt-5 flex justify-end gap-3">
            {portableBaselineSupported && (
              <Button
                type="button"
                variant="ghost"
                onClick={onExportBaseline}
                disabled={exportingBaseline}
              >
                {exportingBaseline
                  ? t("apps.recoveryExporting")
                  : t("apps.recoveryExportBeforeApply")}
              </Button>
            )}
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

  if (state.kind === "applying" || state.kind === "applying_batch") {
    return (
      <div
        ref={trapRef}
        tabIndex={-1}
        className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 px-4 outline-none backdrop-blur-sm"
        role="dialog"
        aria-modal="true"
        aria-busy="true"
        aria-labelledby="applying-dialog-title"
        aria-describedby="applying-dialog-description"
      >
        <Card surface="dialog" className="w-full max-w-lg p-6">
          <h3
            id="applying-dialog-title"
            className="text-sm font-semibold text-anvil-50"
          >
            {t("apps.applyingChange")}
          </h3>
          <p
            id="applying-dialog-description"
            className="mt-2 text-xs text-anvil-400"
          >
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
        live="polite"
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        }
      >
        <p>{state.message}</p>
        {state.details && state.details.length > 0 && (
          <ul className="mt-3 list-disc space-y-1 ps-5 text-sm text-anvil-200">
            {state.details.map((detail) => (
              <li key={detail} className="break-words font-mono text-xs">
                {detail}
              </li>
            ))}
          </ul>
        )}
      </StatePanel>
    );
  }

  return (
    <StatePanel
      title={t("apps.actionFailed")}
      tone="danger"
      live="assertive"
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

function actionTier(kind: ActionKind | undefined): string {
  if (kind === "suspend" || kind === "unsuspend") return "suspend";
  if (kind === "disable" || kind === "enable") return "disable";
  if (kind === "archive" || kind === "request_unarchive") return "archive";
  if (kind === "uninstall_for_user" || kind === "clear_data") {
    return "destructive";
  }
  return "utility";
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
