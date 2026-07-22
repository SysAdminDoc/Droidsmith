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
  callInstallApk,
  callJournalList,
  callJournalUndo,
  callJournalUndoBatch,
  callListPackagesWithCapability,
  callListUsers,
  callObserveDeviceFingerprint,
  callPlanAction,
  callPlanActionBatch,
  callPreflightPackageBackup,
  callSelectHostPath,
  callGrantDroppedPath,
  inTauri,
  deviceTarget,
  newOperationId,
  type ActionKind,
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
  const metadataGenerationRef = useRef(0);
  const metadataRequestedRef = useRef(new Set<string>());
  // Tracks the live selection so an in-flight journal undo can detect a
  // mid-operation device switch before refreshing shared package/journal state.
  const selectedSerialRef = useRef<string | null>(null);

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
        if (
          observation.changed &&
          currentFingerprintKeyRef.current === key &&
          !dismissedFingerprintNotices.has(key)
        ) {
          setOtaNotice(true);
        }
      })
      .catch(() => {
        // A convenience signal; a failure must not disrupt the Apps route.
      });
    // Re-check only when the selected device or its fingerprint changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedDevice?.serial, selectedFingerprint]);

  const dismissOtaNotice = useCallback(() => {
    const key = currentFingerprintKeyRef.current;
    if (key) {
      dismissedFingerprintNotices.add(key);
    }
    setOtaNotice(false);
  }, []);

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
      // Never silently keep a stale selection from device A. The backend
      // rejects empty or ambiguous discovery instead of fabricating user 0.
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

  const loadPackages = useCallback(async () => {
    if (!selectedDevice || !usersReady) return;
    metadataGenerationRef.current += 1;
    metadataRequestedRef.current.clear();
    setPackageMetadata({});
    setPkgState({ kind: "loading" });
    try {
      const listing = await callListPackagesWithCapability(
        deviceTarget(selectedDevice),
        filter,
        selectedUser,
      );
      setPkgState({
        kind: "ok",
        packages: listing.packages,
        archive: listing.archive,
      });
    } catch (e) {
      setPkgState({
        kind: "error",
        message: errorMessage(e),
      });
    }
  }, [selectedDevice, filter, selectedUser, usersReady]);

  const requestPackageMetadata = useCallback(
    (packageName: string) => {
      if (!selectedDevice || !usersReady) return;
      if (metadataRequestedRef.current.has(packageName)) return;
      metadataRequestedRef.current.add(packageName);
      const generation = metadataGenerationRef.current;
      void callGetPackageMetadata(
        deviceTarget(selectedDevice),
        packageName,
        selectedUser,
      )
        .then((metadata) => {
          if (metadataGenerationRef.current !== generation) return;
          setPackageMetadata((current) => ({
            ...current,
            [packageName]: metadata,
          }));
        })
        .catch(() => {
          if (metadataGenerationRef.current !== generation) return;
          // Unsupported vendor resource shapes degrade to the package-name
          // fallback and are not retried until the package list refreshes.
          setPackageMetadata((current) => ({
            ...current,
            [packageName]: null,
          }));
        });
    },
    [selectedDevice, selectedUser, usersReady],
  );

  useEffect(() => {
    selectedSerialRef.current = selectedSerial;
  }, [selectedSerial]);

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
        message: errorMessage(e),
      });
    }
  }, [selectedSerial]);

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
      try {
        const plan = await callPlanAction({
          serial: selectedDevice.serial,
          target: authorizedTarget,
          package: pkg,
          kind,
          user_id: selectedUser,
        });
        setActionState({ kind: "confirming", plan });
      } catch (e) {
        setActionState({
          kind: "error",
          message: errorMessage(e),
        });
      }
    },
    [authorizedTarget, selectedDevice, selectedUser, usersReady],
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
        setActionState({ kind: "confirming_batch", plan });
      } catch (error) {
        setActionState({
          kind: "error",
          message: errorMessage(error),
        });
      }
    },
    [
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
      let startedOperationId: string | null = null;
      let startedGeneration: number | null = null;
      try {
        const preflight =
          inspected ??
          (await callPreflightPackageBackup(
            authorizedTarget,
            pkg,
            selectedUser,
          ));
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
        const generation = backupGenerationRef.current + 1;
        startedOperationId = operationId;
        startedGeneration = generation;
        backupGenerationRef.current = generation;
        activeBackupRef.current = operationId;
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
              backupGenerationRef.current !== generation
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
        if (backupGenerationRef.current !== generation) return;
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
        if (
          startedOperationId &&
          (backupGenerationRef.current !== startedGeneration ||
            activeBackupRef.current !== startedOperationId)
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
    [authorizedTarget, selectedDevice, selectedUser, t, usersReady],
  );

  const inspectLegacyExport = useCallback(
    async (pkg: string) => {
      if (!authorizedTarget || !usersReady) return;
      try {
        const preflight = await callPreflightPackageBackup(
          authorizedTarget,
          pkg,
          selectedUser,
        );
        const runnable = canRunLegacyExport(preflight);
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
        });
      } catch (error) {
        setBackupNotice({
          title: t("apps.exportFailedTitle"),
          message: t("apps.exportFailed", {
            message: errorMessage(error),
          }),
          tone: "danger",
        });
      }
    },
    [authorizedTarget, selectedUser, t, usersReady],
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
      const operationId = newOperationId("install");
      const generation = installGenerationRef.current + 1;
      installGenerationRef.current = generation;
      activeInstallRef.current = operationId;
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
                installGenerationRef.current !== generation
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
        if (installGenerationRef.current !== generation) return;
        activeInstallRef.current = null;
        setInstallState({ kind: "result", localPath, result });
        if (result.succeeded) await loadPackages();
      } catch (error) {
        if (
          activeInstallRef.current !== operationId ||
          installGenerationRef.current !== generation
        )
          return;
        activeInstallRef.current = null;
        setInstallState({
          kind: "error",
          message: errorMessage(error),
        });
      }
    },
    [authorizedTarget, loadPackages, selectedDevice, t],
  );

  const startInstall = useCallback(async () => {
    if (!selectedDevice) return;
    setInstallState({ kind: "choosing" });
    try {
      const selected = await callSelectHostPath("install_open");
      if (!selected) {
        setInstallState({ kind: "idle" });
        return;
      }
      await runInstall(selected.id, selected.local_path, {
        incremental: incrementalInstall,
      });
    } catch (error) {
      setInstallState({
        kind: "error",
        message: errorMessage(error),
      });
    }
  }, [incrementalInstall, runInstall, selectedDevice]);

  useEffect(() => {
    if (!inTauri() || !selectedDevice) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview").then(({ getCurrentWebview }) => {
      if (cancelled) return;
      void getCurrentWebview()
        .onDragDropEvent(async (event) => {
          if (event.payload.type !== "drop" || cancelled) return;
          const apkPaths = event.payload.paths.filter((p) => {
            const ext = p.split(".").pop()?.toLowerCase() ?? "";
            return ["apk", "apks", "xapk", "apkm"].includes(ext);
          });
          if (apkPaths.length === 0) return;
          const path = apkPaths[0];
          try {
            const grant = await callGrantDroppedPath(path);
            await runInstall(grant.id, grant.local_path, {
              incremental: incrementalInstall,
            });
          } catch (error) {
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
  }, [incrementalInstall, runInstall, selectedDevice]);

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
    try {
      if (actionState.kind === "confirming_batch") {
        const plan = actionState.plan;
        setActionState({ kind: "applying_batch", plan });
        const result = await callApplyActionBatch(plan);
        const failures = batchFailures(result);
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
      } else if (actionState.kind === "confirming") {
        const plan = actionState.plan;
        setActionState({ kind: "applying", plan });
        await callApplyAction(plan);
        setActionState({
          kind: "success",
          message: t("apps.planCompleted", { description: plan.description }),
        });
      } else {
        return;
      }
      void loadPackages();
      void loadJournal();
    } catch (e) {
      setActionState({
        kind: "error",
        message: errorMessage(e),
      });
    }
  }, [actionState, loadJournal, loadPackages, t]);

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
      setRecoveryState({
        kind: "saved",
        path: artifact.local_path,
        sha256: artifact.sha256,
      });
    } catch (error) {
      setRecoveryState({
        kind: "error",
        message: errorMessage(error),
      });
    }
  }, [actionState, authorizedTarget, t]);

  const inspectRecoveryBaseline = useCallback(async () => {
    if (!authorizedTarget) return;
    setRecoveryState({
      kind: "busy",
      message: t("apps.recoveryInspecting"),
    });
    try {
      const selected = await callSelectHostPath("recovery_baseline_open");
      if (!selected) {
        setRecoveryState({ kind: "idle" });
        return;
      }
      const diff = await callInspectRecoveryBaseline(
        authorizedTarget,
        selected.id,
      );
      setRecoveryState({ kind: "review", diff });
    } catch (error) {
      setRecoveryState({
        kind: "error",
        message: errorMessage(error),
      });
    }
  }, [authorizedTarget, t]);

  const applyRecoveryBaseline = useCallback(async () => {
    if (recoveryState.kind !== "review") return;
    const { diff } = recoveryState;
    setRecoveryState({
      kind: "busy",
      message: t("apps.recoveryApplying", { count: diff.plans.length }),
    });
    let applied = 0;
    const failures: string[] = [];
    for (const plan of diff.plans) {
      try {
        await callApplyAction(plan);
        applied += 1;
      } catch (error) {
        failures.push(`${plan.request.package}: ${errorMessage(error)}`);
      }
    }
    setRecoveryState({ kind: "result", diff, applied, failures });
    await Promise.all([loadPackages(), loadJournal()]);
  }, [loadJournal, loadPackages, recoveryState, t]);

  const undoJournalEntry = useCallback(
    async (entry: JournalEntry) => {
      if (!selectedDevice || !authorizedTarget || !usersReady) return;
      const serial = selectedDevice.serial;
      setUndoingEntryId(entry.id);
      try {
        await callJournalUndo(authorizedTarget, entry.id);
        // Bail if the device switched mid-undo: refreshing here would clobber
        // the newly selected device's package/journal state.
        if (selectedSerialRef.current !== serial) return;
        setActionState({
          kind: "success",
          message: t("apps.journalUndoCompleted", {
            package: entry.applied.plan.request.package,
          }),
        });
        await Promise.all([loadPackages(), loadJournal()]);
      } catch (e) {
        if (selectedSerialRef.current !== serial) return;
        setActionState({
          kind: "error",
          message: errorMessage(e),
        });
      } finally {
        setUndoingEntryId(null);
      }
    },
    [
      authorizedTarget,
      loadJournal,
      loadPackages,
      selectedDevice,
      t,
      usersReady,
    ],
  );

  const undoJournalBatch = useCallback(
    async (batchId: string) => {
      if (!selectedDevice || !authorizedTarget || !usersReady) return;
      const serial = selectedDevice.serial;
      setUndoingBatchId(batchId);
      try {
        const result = await callJournalUndoBatch(authorizedTarget, batchId);
        if (selectedSerialRef.current !== serial) return;
        const failures = batchFailures(result);
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
        await Promise.all([loadPackages(), loadJournal()]);
      } catch (error) {
        if (selectedSerialRef.current !== serial) return;
        setActionState({
          kind: "error",
          message: errorMessage(error),
        });
      } finally {
        setUndoingBatchId(null);
      }
    },
    [
      authorizedTarget,
      loadJournal,
      loadPackages,
      selectedDevice,
      t,
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

  return (
    <>
      <PaneHeader
        title={t("apps.title")}
        milestone="R-020"
        description={t("apps.description")}
        actions={
          selectedDevice ? (
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                onClick={() => void inspectRecoveryBaseline()}
                disabled={
                  !authorizedTarget ||
                  recoveryState.kind === "busy" ||
                  !usersReady
                }
                variant="ghost"
              >
                {t("apps.recoveryInspect")}
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
  onConfirm,
  onExportBaseline,
  exportingBaseline,
  baselineFeedback,
  onCancel,
  onDismiss,
}: {
  state: ActionState;
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
      (plan) => !["archive", "request_unarchive"].includes(plan.request.kind),
    );
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
