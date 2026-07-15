import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
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
  callListPermissions,
  callListUsers,
  callPlanAction,
  callPlanActionBatch,
  callPreflightPackageBackup,
  callSelectHostPath,
  callSetPermission,
  deviceTarget,
  newOperationId,
  type ActionKind,
  type AndroidUser,
  type AppPackage,
  type AppPackageMetadata,
  type BatchActionItemResult,
  type BatchActionPlan,
  type BatchActionResult,
  type Device,
  type DeviceTarget,
  type JournalEntry,
  type InstallOptions,
  type InstallPackageResult,
  type PackageFilter,
  type PackageArchiveCapability,
  type OperationEvent,
  type PackageBackupPreflight,
  type PermissionInfo,
  type PlannedAction,
  type RecoveryBaselineDiff,
} from "../lib/tauri";
import {
  useAuthorizedDevices,
  useTransportAuthorization,
} from "../lib/useAuthorizedDevices";
import { formatDateTime } from "../lib/i18n";

import {
  canRunLegacyExport,
  formatBackupSize,
  packageExportDefaultFileName,
  packageExportDisplayState,
} from "./appsBackup";
import { journalEntryStatus, type JournalEntryStatus } from "./appsJournal";
import { useFocusTrap } from "../lib/useFocusTrap";
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
  TransportBadge,
  TransportTrustNotice,
} from "./common";

type PackagesState =
  | { kind: "idle" }
  | { kind: "loading" }
  | {
      kind: "ok";
      packages: AppPackage[];
      archive: PackageArchiveCapability;
    }
  | { kind: "error"; message: string };

type ActionState =
  | { kind: "idle" }
  | { kind: "confirming"; plan: PlannedAction }
  | { kind: "applying"; plan: PlannedAction }
  | { kind: "confirming_batch"; plan: BatchActionPlan }
  | { kind: "applying_batch"; plan: BatchActionPlan }
  | { kind: "success"; message: string; details?: string[] }
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
  operationId?: string;
  progress?: string;
  evidence?: PackageBackupPreflight["evidence"];
  pendingLegacy?: {
    package: string;
    preflight: PackageBackupPreflight;
  };
};

type InstallState =
  | { kind: "idle" }
  | { kind: "choosing" }
  | {
      kind: "running";
      operationId: string;
      progress: string;
      output: string;
    }
  | { kind: "result"; localPath: string; result: InstallPackageResult }
  | { kind: "error"; message: string }
  | {
      kind: "confirming_override";
      localPath: string;
      result: InstallPackageResult;
    };

type RecoveryState =
  | { kind: "idle" }
  | { kind: "busy"; message: string }
  | { kind: "saved"; path: string; sha256: string }
  | { kind: "review"; diff: RecoveryBaselineDiff }
  | {
      kind: "result";
      diff: RecoveryBaselineDiff;
      applied: number;
      failures: string[];
    }
  | { kind: "error"; message: string };

const FILTERS: { value: PackageFilter; labelKey: string }[] = [
  { value: "all", labelKey: "apps.filterAll" },
  { value: "user", labelKey: "apps.filterUser" },
  { value: "system", labelKey: "apps.filterSystem" },
  { value: "enabled", labelKey: "apps.filterEnabled" },
  { value: "disabled", labelKey: "apps.filterDisabled" },
  { value: "archived", labelKey: "apps.filterArchived" },
];

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
  const [recoveryState, setRecoveryState] = useState<RecoveryState>({
    kind: "idle",
  });
  const activeBackupRef = useRef<string | null>(null);
  const backupGenerationRef = useRef(0);
  const activeInstallRef = useRef<string | null>(null);
  const installGenerationRef = useRef(0);
  const metadataGenerationRef = useRef(0);
  const metadataRequestedRef = useRef(new Set<string>());

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
      setUserError(e instanceof Error ? e.message : String(e));
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
        message: e instanceof Error ? e.message : String(e),
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
          message: e instanceof Error ? e.message : String(e),
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
          message: error instanceof Error ? error.message : String(error),
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
            message: e instanceof Error ? e.message : String(e),
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
            message: error instanceof Error ? error.message : String(error),
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
          message: installErrorMessage(error),
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
      await runInstall(selected.id, selected.local_path, {});
    } catch (error) {
      setInstallState({
        kind: "error",
        message: installErrorMessage(error),
      });
    }
  }, [runInstall, selectedDevice]);

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
        message: e instanceof Error ? e.message : String(e),
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
        message: error instanceof Error ? error.message : String(error),
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
        message: error instanceof Error ? error.message : String(error),
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
        failures.push(
          `${plan.request.package}: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    }
    setRecoveryState({ kind: "result", diff, applied, failures });
    await Promise.all([loadPackages(), loadJournal()]);
  }, [loadJournal, loadPackages, recoveryState, t]);

  const undoJournalEntry = useCallback(
    async (entry: JournalEntry) => {
      if (!selectedDevice || !authorizedTarget || !usersReady) return;
      setUndoingEntryId(entry.id);
      try {
        await callJournalUndo(authorizedTarget, entry.id);
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
      setUndoingBatchId(batchId);
      try {
        const result = await callJournalUndoBatch(authorizedTarget, batchId);
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
        setActionState({
          kind: "error",
          message: error instanceof Error ? error.message : String(error),
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

  const filteredPackages =
    pkgState.kind === "ok"
      ? pkgState.packages.filter((p) =>
          search
            ? [p.package, packageMetadata[p.package]?.label ?? ""]
                .join(" ")
                .toLowerCase()
                .includes(search.toLowerCase())
            : true,
        )
      : [];
  const selectedPackageSet = new Set(selectedPackages);
  const selectedRows =
    pkgState.kind === "ok"
      ? pkgState.packages.filter((pkg) => selectedPackageSet.has(pkg.package))
      : [];
  const batchReady = selectedRows.length >= 2;
  const canBatchDisable =
    batchReady && selectedRows.every((pkg) => pkg.enabled && !pkg.archived);
  const canBatchEnable =
    batchReady && selectedRows.every((pkg) => !pkg.enabled && !pkg.archived);
  const canBatchArchive =
    batchReady &&
    pkgState.kind === "ok" &&
    pkgState.archive.supported &&
    selectedRows.every((pkg) => !pkg.system && !pkg.archived);
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
                <label className="flex items-center gap-2 text-sm text-slate-300">
                  <span>{t("apps.userLabel")}</span>
                  <select
                    value={selectedUser}
                    onChange={(e) => {
                      setSelectedUser(Number(e.target.value));
                      setSelectedPackages([]);
                      setRecoveryState({ kind: "idle" });
                    }}
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

function InstallStatePanel({
  state,
  onCancel,
  onDismiss,
  onReviewOverride,
}: {
  state: InstallState;
  onCancel: () => void;
  onDismiss: () => void;
  onReviewOverride: () => void;
}) {
  const { t } = useTranslation();
  if (
    state.kind === "idle" ||
    state.kind === "choosing" ||
    state.kind === "confirming_override"
  )
    return null;
  if (state.kind === "running") {
    return (
      <StatePanel
        title={t("apps.installRunning")}
        tone="info"
        actions={
          <Button type="button" size="sm" variant="danger" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
        }
      >
        <p>{state.progress}</p>
        {state.output.trim() && (
          <pre className="mt-3 max-h-40 overflow-auto rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
            {state.output.trim()}
          </pre>
        )}
      </StatePanel>
    );
  }
  if (state.kind === "error") {
    return (
      <StatePanel
        title={t("apps.installFailed")}
        tone="danger"
        actions={
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  const { result } = state;
  const failure = result.failure;
  return (
    <StatePanel
      title={
        result.succeeded ? t("apps.installSucceeded") : t("apps.installFailed")
      }
      tone={result.succeeded ? "success" : "danger"}
      actions={
        <div className="flex flex-wrap gap-2">
          {failure?.suggested_override && (
            <Button
              type="button"
              size="sm"
              variant="danger"
              onClick={onReviewOverride}
            >
              {t("apps.installReviewOverride")}
            </Button>
          )}
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        </div>
      }
    >
      <p>
        {result.succeeded
          ? t("apps.installSucceededBody", {
              count: result.file_count,
              size: formatBackupSize(result.total_bytes),
            })
          : failure?.cause}
      </p>
      {!result.succeeded && failure && (
        <>
          <dl className="mt-3 grid gap-2 text-xs sm:grid-cols-[7rem_minmax(0,1fr)]">
            <dt className="font-medium text-anvil-400">
              {t("apps.installCode")}
            </dt>
            <dd className="break-words font-mono text-anvil-100">
              {failure.code}
            </dd>
            <dt className="font-medium text-anvil-400">
              {t("apps.installRemedy")}
            </dt>
            <dd className="text-anvil-100">{failure.remedy}</dd>
          </dl>
          <p className="mt-3 text-xs text-anvil-400">
            {t("apps.installNoAutomaticOverride")}
          </p>
          <pre className="mt-3 max-h-48 overflow-auto whitespace-pre-wrap rounded-md border border-white/10 bg-black/30 p-3 font-mono text-xs leading-5 text-anvil-200">
            {failure.raw_output}
          </pre>
        </>
      )}
      <p className="mt-3 text-xs text-anvil-500">
        {t("apps.installAudit", { id: result.audit_id })}
      </p>
    </StatePanel>
  );
}

function InstallOverrideDialog({
  result,
  onCancel,
  onConfirm,
}: {
  result: InstallPackageResult;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const trapRef = useFocusTrap<HTMLDivElement>();
  const override = result.failure?.suggested_override;
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);
  if (!override || !result.failure) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 p-4 backdrop-blur-sm">
      <div
        ref={trapRef}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="install-override-title"
        aria-describedby="install-override-description"
        tabIndex={-1}
        className="w-full max-w-xl rounded-lg border border-red-300/20 bg-anvil-950 p-5 shadow-2xl outline-none"
      >
        <h3
          id="install-override-title"
          className="text-lg font-semibold text-anvil-50"
        >
          {t("apps.installOverrideTitle")}
        </h3>
        <p
          id="install-override-description"
          className="mt-2 text-sm leading-6 text-anvil-300"
        >
          {override === "allow_downgrade"
            ? t("apps.installDowngradeWarning")
            : t("apps.installLowTargetWarning")}
        </p>
        <div className="mt-4 rounded-md border border-red-300/20 bg-red-300/10 p-3 text-sm text-red-100">
          <p>{result.failure.cause}</p>
          <p className="mt-2">{result.failure.remedy}</p>
        </div>
        <p className="mt-3 text-xs leading-5 text-anvil-400">
          {t("apps.installOverrideAudit")}
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button type="button" variant="danger" onClick={onConfirm}>
            {t("apps.installConfirmOverride")}
          </Button>
        </div>
      </div>
    </div>
  );
}

function BackupStatePanel({
  notice,
  onDismiss,
  onCancel,
  onContinueLegacy,
}: {
  notice: BackupNotice;
  onDismiss: () => void;
  onCancel: () => void;
  onContinueLegacy: (
    pending: NonNullable<BackupNotice["pendingLegacy"]>,
  ) => void;
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
        notice.operationId ? (
          <Button type="button" size="sm" variant="danger" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
        ) : notice.pendingLegacy ? (
          <div className="flex flex-wrap gap-2">
            <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
              {t("common.cancel")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="primary"
              onClick={() => onContinueLegacy(notice.pendingLegacy!)}
            >
              {t("apps.continueLegacyExport")}
            </Button>
          </div>
        ) : (
          <Button type="button" size="sm" variant="ghost" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        )
      }
    >
      <p>{notice.message}</p>
      {notice.progress && (
        <p className="mt-2 text-xs font-medium text-circuit-200">
          {notice.progress}
        </p>
      )}
      {notice.showLimitations && (
        <p className="mt-2 text-xs leading-5 text-anvil-400">
          {t("apps.legacyLimitations")}
        </p>
      )}
      {notice.evidence && (
        <dl className="mt-3 grid gap-2 text-xs sm:grid-cols-[8rem_minmax(0,1fr)]">
          <dt className="font-medium text-anvil-400">{t("apps.deviceApi")}</dt>
          <dd className="font-mono text-anvil-100">
            {notice.evidence.device_sdk ?? t("common.notReported")}
          </dd>
          <dt className="font-medium text-anvil-400">{t("apps.targetApi")}</dt>
          <dd className="font-mono text-anvil-100">
            {notice.evidence.target_sdk ?? t("common.notReported")}
          </dd>
          <dt className="font-medium text-anvil-400">
            {t("apps.allowBackup")}
          </dt>
          <dd className="font-mono text-anvil-100">
            {notice.evidence.allow_backup === null
              ? t("common.notReported")
              : String(notice.evidence.allow_backup)}
          </dd>
          <dt className="font-medium text-anvil-400">{t("apps.debuggable")}</dt>
          <dd className="font-mono text-anvil-100">
            {notice.evidence.debuggable === null
              ? t("common.notReported")
              : String(notice.evidence.debuggable)}
          </dd>
        </dl>
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

function installErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  )
    return error.message;
  return String(error);
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

function BatchActionBar({
  selectedCount,
  canDisable,
  canEnable,
  canArchive,
  canUnarchive,
  onClear,
  onAction,
}: {
  selectedCount: number;
  canDisable: boolean;
  canEnable: boolean;
  canArchive: boolean;
  canUnarchive: boolean;
  onClear: () => void;
  onAction: (kind: ActionKind) => void;
}) {
  const { t } = useTranslation();
  if (selectedCount === 0) return null;
  return (
    <Card className="flex flex-col gap-3 border-circuit-300/20 bg-circuit-950/10 p-4 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <p className="text-sm font-semibold text-anvil-50">
          {t("apps.batchSelected", { count: selectedCount })}
        </p>
        <p className="mt-1 text-xs text-anvil-400">
          {selectedCount < 2
            ? t("apps.batchSelectMore")
            : t("apps.batchReviewBody")}
        </p>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          size="sm"
          onClick={() => onAction("disable")}
          disabled={!canDisable}
        >
          {t("apps.batchDisable")}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => onAction("enable")}
          disabled={!canEnable}
        >
          {t("apps.batchEnable")}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => onAction("archive")}
          disabled={!canArchive}
        >
          {t("apps.batchArchive")}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => onAction("request_unarchive")}
          disabled={!canUnarchive}
        >
          {t("apps.batchUnarchive")}
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={onClear}>
          {t("apps.batchClear")}
        </Button>
      </div>
    </Card>
  );
}

function PackageTable({
  packages,
  metadata,
  totalCount,
  archiveSupported,
  selectedPackages,
  onToggleSelected,
  onToggleAll,
  onMetadataRequest,
  onAction,
  onInspect,
  onExport,
  onLegacyExport,
  showLegacyExport,
}: {
  packages: AppPackage[];
  metadata: Record<string, AppPackageMetadata | null>;
  totalCount: number;
  archiveSupported: boolean;
  selectedPackages: Set<string>;
  onToggleSelected: (pkg: string) => void;
  onToggleAll: () => void;
  onMetadataRequest: (pkg: string) => void;
  onAction: (pkg: string, kind: ActionKind) => void;
  onInspect: (pkg: string) => void;
  onExport: (pkg: string) => void;
  onLegacyExport: (pkg: string) => void;
  showLegacyExport: boolean;
}) {
  const { t } = useTranslation();
  const allVisibleSelected =
    packages.length > 0 &&
    packages.every((pkg) => selectedPackages.has(pkg.package));

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
                <TableHeaderCell>
                  <input
                    type="checkbox"
                    checked={allVisibleSelected}
                    onChange={onToggleAll}
                    disabled={packages.length === 0}
                    aria-label={t("apps.selectAllPackages")}
                    className="h-4 w-4 accent-circuit-400"
                  />
                </TableHeaderCell>
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
                    <input
                      type="checkbox"
                      checked={selectedPackages.has(pkg.package)}
                      onChange={() => onToggleSelected(pkg.package)}
                      aria-label={t("apps.selectPackage", {
                        package: pkg.package,
                      })}
                      className="h-4 w-4 accent-circuit-400"
                    />
                  </TableCell>
                  <TableCell>
                    <PackageIdentity
                      pkg={pkg}
                      metadata={metadata[pkg.package]}
                      onRequest={onMetadataRequest}
                    />
                  </TableCell>
                  <TableCell>
                    <Badge tone={pkg.system ? "warning" : "neutral"}>
                      {pkg.system
                        ? t("apps.filterSystem")
                        : t("apps.filterUser")}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <Badge
                      tone={
                        pkg.archived
                          ? "warning"
                          : pkg.enabled
                            ? "success"
                            : "danger"
                      }
                    >
                      {pkg.archived
                        ? t("apps.filterArchived")
                        : pkg.enabled
                          ? t("apps.filterEnabled")
                          : t("apps.filterDisabled")}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <div className="flex min-w-[10rem] flex-wrap gap-1.5">
                      {pkg.archived ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() =>
                            onAction(pkg.package, "request_unarchive")
                          }
                        >
                          {t("apps.unarchive")}
                        </Button>
                      ) : pkg.enabled ? (
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
                      {!pkg.archived && archiveSupported && !pkg.system && (
                        <Button
                          type="button"
                          size="sm"
                          onClick={() => onAction(pkg.package, "archive")}
                        >
                          {t("apps.archive")}
                        </Button>
                      )}
                      {!pkg.archived && (
                        <>
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
                            onClick={() => onExport(pkg.package)}
                          >
                            {t("apps.exportApks")}
                          </Button>
                        </>
                      )}
                      {!pkg.archived && showLegacyExport && (
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          onClick={() => onLegacyExport(pkg.package)}
                        >
                          {t("apps.legacyData")}
                        </Button>
                      )}
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

function PackageIdentity({
  pkg,
  metadata,
  onRequest,
}: {
  pkg: AppPackage;
  metadata: AppPackageMetadata | null | undefined;
  onRequest: (pkg: string) => void;
}) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const label = metadata?.label ?? packageFallbackLabel(pkg.package);

  useEffect(() => {
    const element = containerRef.current;
    if (!element || metadata !== undefined || pkg.archived) return;
    if (typeof IntersectionObserver === "undefined") {
      onRequest(pkg.package);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          onRequest(pkg.package);
          observer.disconnect();
        }
      },
      { rootMargin: "320px 0px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [metadata, onRequest, pkg.archived, pkg.package]);

  return (
    <div ref={containerRef} className="flex min-w-[18rem] items-center gap-3">
      <PackageIcon label={label} iconDataUri={metadata?.icon_data_uri} />
      <div className="min-w-0">
        <p className="truncate text-sm font-medium text-anvil-50">{label}</p>
        <code className="mt-1 block truncate font-mono text-xs text-anvil-300">
          {pkg.package}
        </code>
        {pkg.installer && (
          <p className="mt-1 text-[11px] text-anvil-500">
            {t("apps.viaInstaller", { installer: pkg.installer })}
          </p>
        )}
      </div>
    </div>
  );
}

function PackageIcon({
  label,
  iconDataUri,
}: {
  label: string;
  iconDataUri: string | null | undefined;
}) {
  if (iconDataUri) {
    return (
      <img
        src={iconDataUri}
        alt=""
        className="h-9 w-9 shrink-0 rounded-lg border border-white/10 bg-anvil-900 object-contain p-1"
        loading="lazy"
      />
    );
  }
  return (
    <div
      className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-white/10 bg-circuit-300/10 text-xs font-semibold text-circuit-100"
      aria-hidden="true"
    >
      {initials(label)}
    </div>
  );
}

function packageFallbackLabel(pkg: string): string {
  return pkg.split(".").filter(Boolean).pop() ?? pkg;
}

function initials(label: string): string {
  const parts = label.trim().split(/\s+/u).filter(Boolean);
  if (parts.length >= 2) {
    return `${parts[0]?.[0] ?? ""}${parts[1]?.[0] ?? ""}`.toUpperCase();
  }
  return (parts[0] ?? "AP").slice(0, 2).toUpperCase();
}

function JournalPanel({
  state,
  undoingEntryId,
  undoingBatchId,
  onRefresh,
  onUndo,
  onUndoBatch,
}: {
  state: JournalState;
  undoingEntryId: number | null;
  undoingBatchId: string | null;
  onRefresh: () => void;
  onUndo: (entry: JournalEntry) => void;
  onUndoBatch: (batchId: string) => void;
}) {
  const { t, i18n } = useTranslation();

  if (state.kind === "idle") return null;

  const entries =
    state.kind === "ok"
      ? [...state.entries].sort((a, b) => b.id - a.id).slice(0, 16)
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
                const status = journalEntryStatus(
                  entry,
                  state.entries.find(
                    (candidate) => candidate.id === entry.undone_by,
                  )?.outcome,
                );
                const request = entry.applied.plan.request;
                const batchId = request.context?.batch_id ?? null;
                const batchUndoableEntries = batchId
                  ? state.entries.filter(
                      (candidate) =>
                        candidate.undoes === null &&
                        candidate.applied.plan.request.context?.batch_id ===
                          batchId &&
                        journalEntryStatus(
                          candidate,
                          state.entries.find(
                            (undo) => undo.id === candidate.undone_by,
                          )?.outcome,
                        ) === "undoable",
                    )
                  : [];
                const batchUndoAnchor = Math.max(
                  ...batchUndoableEntries.map((candidate) => candidate.id),
                );
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
                          {formatDateTime(
                            entry.applied.applied_at,
                            i18n.resolvedLanguage ?? i18n.language,
                          )}
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
                        <p className="mt-1 font-mono text-[10px] text-anvil-500">
                          {t("apps.journalAuditMeta", {
                            incident:
                              entry.applied.plan.incident_id || "legacy",
                            source:
                              request.context?.confirmation_source ?? "legacy",
                          })}
                        </p>
                        {batchId && (
                          <p className="mt-1 font-mono text-[10px] text-circuit-200">
                            {t("apps.journalBatchMeta", {
                              id: batchId,
                              count: state.entries.filter(
                                (candidate) =>
                                  candidate.undoes === null &&
                                  candidate.applied.plan.request.context
                                    ?.batch_id === batchId,
                              ).length,
                            })}
                          </p>
                        )}
                        {(entry.applied.before_state ||
                          entry.applied.after_state) && (
                          <p className="mt-1 text-[10px] text-anvil-500">
                            {t("apps.journalStateChange", {
                              before:
                                entry.applied.before_state ||
                                t("debloat.stateUnknown"),
                              after:
                                entry.applied.after_state ||
                                t("debloat.stateUnknown"),
                            })}
                          </p>
                        )}
                      </div>
                    </TableCell>
                    <TableCell>
                      <code className="block min-w-[16rem] font-mono text-xs text-anvil-100">
                        {request.package || entry.applied.plan.description}
                      </code>
                    </TableCell>
                    <TableCell>
                      <Badge tone={journalStatusTone(status)}>
                        {journalStatusLabel(entry, status, t)}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <pre className="max-w-[22rem] whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-anvil-400">
                        {entry.failure ||
                          summarizeJournalOutput(entry.applied.stdout) ||
                          t("apps.journalNoOutput")}
                      </pre>
                    </TableCell>
                    <TableCell>
                      {batchId && batchUndoAnchor === entry.id ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="primary"
                          onClick={() => onUndoBatch(batchId)}
                          disabled={undoingBatchId === batchId}
                        >
                          {undoingBatchId === batchId
                            ? t("apps.journalUndoingBatch")
                            : t("apps.journalUndoBatch", {
                                count: batchUndoableEntries.length,
                              })}
                        </Button>
                      ) : batchId ? (
                        <span className="text-xs text-anvil-500">
                          {t("apps.journalBatchMember")}
                        </span>
                      ) : status === "undoable" ? (
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
    case "pending":
      return "info";
    case "failed":
      return "danger";
    case "interrupted":
      return "warning";
    case "undo_interrupted":
      return "warning";
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
    case "pending":
      return t("apps.journalPending");
    case "failed":
      return t("apps.journalFailed");
    case "interrupted":
      return t("apps.journalInterrupted");
    case "undo_interrupted":
      return t("apps.journalUndoInterrupted", { id: entry.undone_by });
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

function summarizeJournalOutput(output: string): string {
  const trimmed = output.trim();
  if (trimmed.length <= 180) return trimmed;
  return `${trimmed.slice(0, 180)}...`;
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
        <Card className="w-full max-w-lg p-6">
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
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            {t("common.dismiss")}
          </Button>
        }
      >
        <p>{state.message}</p>
        {state.details && state.details.length > 0 && (
          <ul className="mt-3 list-disc space-y-1 pl-5 text-sm text-anvil-200">
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

function RecoveryBaselinePanel({
  state,
  onApply,
  onDismiss,
}: {
  state: RecoveryState;
  onApply: () => void;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  if (state.kind === "idle") return null;
  if (state.kind === "busy") {
    return (
      <StatePanel title={t("apps.recoveryWorking")} tone="info">
        <p>{state.message}</p>
      </StatePanel>
    );
  }
  if (state.kind === "saved") {
    return (
      <StatePanel
        title={t("apps.recoverySavedTitle")}
        tone="success"
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            {t("apps.recoveryDismiss")}
          </Button>
        }
      >
        <p>{t("apps.recoverySaved", { path: state.path })}</p>
        <code className="mt-2 block break-all font-mono text-xs">
          sha256 {state.sha256}
        </code>
      </StatePanel>
    );
  }
  if (state.kind === "error") {
    return (
      <StatePanel
        title={t("apps.recoveryFailed")}
        tone="danger"
        actions={
          <Button type="button" size="sm" onClick={onDismiss}>
            {t("apps.recoveryDismiss")}
          </Button>
        }
      >
        <p>{state.message}</p>
      </StatePanel>
    );
  }

  const { diff } = state;
  const ready = diff.rows.filter((row) => row.status === "ready").length;
  const skipped = diff.rows.filter((row) => row.status === "skipped").length;
  const matching = diff.rows.filter(
    (row) => row.status === "already_matches",
  ).length;
  return (
    <Card className="overflow-hidden p-0">
      <div className="flex flex-wrap items-start justify-between gap-3 border-b border-white/10 p-5">
        <div>
          <h3 className="font-semibold text-anvil-50">
            {t("apps.recoveryReviewTitle")}
          </h3>
          <p className="mt-1 text-xs text-anvil-400">
            {t("apps.recoveryReviewBody", {
              user: diff.baseline.android_user,
              date: diff.baseline.exported_at,
            })}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Badge
            tone={
              diff.compatibility.device_identity_matches ? "success" : "danger"
            }
          >
            {diff.compatibility.device_identity_matches
              ? t("apps.recoveryDeviceMatch")
              : t("apps.recoveryDeviceMismatch")}
          </Badge>
          <Badge
            tone={
              diff.compatibility.build_fingerprint_matches
                ? "success"
                : "warning"
            }
          >
            {diff.compatibility.build_fingerprint_matches
              ? t("apps.recoveryBuildMatch")
              : t("apps.recoveryBuildChanged")}
          </Badge>
          <Badge tone="info">{t("apps.recoveryReady", { count: ready })}</Badge>
          <Badge tone="neutral">
            {t("apps.recoveryMatching", { count: matching })}
          </Badge>
          <Badge tone={skipped ? "warning" : "neutral"}>
            {t("apps.recoverySkipped", { count: skipped })}
          </Badge>
        </div>
      </div>
      <div className="max-h-72 overflow-auto">
        <table className="w-full text-left text-xs">
          <thead className="sticky top-0 bg-slate-950">
            <tr>
              <TableHeaderCell>{t("apps.package")}</TableHeaderCell>
              <TableHeaderCell>
                {t("apps.recoveryBaselineState")}
              </TableHeaderCell>
              <TableHeaderCell>{t("apps.recoveryLiveState")}</TableHeaderCell>
              <TableHeaderCell>{t("apps.recoveryDecision")}</TableHeaderCell>
            </tr>
          </thead>
          <tbody className="divide-y divide-white/10">
            {diff.rows.map((row) => (
              <tr key={row.package}>
                <TableCell>
                  <code className="font-mono">{row.package}</code>
                </TableCell>
                <TableCell>
                  {recoveryPackageState(row.baseline_enabled, t)}
                </TableCell>
                <TableCell>
                  {recoveryPackageState(row.live_enabled, t)}
                </TableCell>
                <TableCell>
                  <Badge
                    tone={
                      row.status === "ready"
                        ? "info"
                        : row.status === "skipped"
                          ? "warning"
                          : "success"
                    }
                  >
                    {t(`apps.recoveryStatus.${row.status}`)}
                  </Badge>
                  <p className="mt-1 text-anvil-400">{row.reason}</p>
                </TableCell>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="flex flex-wrap justify-end gap-2 border-t border-white/10 p-4">
        {state.kind === "result" && (
          <p className="mr-auto text-xs text-anvil-300">
            {t("apps.recoveryResult", {
              applied: state.applied,
              failed: state.failures.length,
            })}
            {state.failures.length > 0 && ` ${state.failures.join("; ")}`}
          </p>
        )}
        <Button type="button" onClick={onDismiss}>
          {t("apps.recoveryDismiss")}
        </Button>
        {state.kind === "review" && (
          <Button
            type="button"
            variant="primary"
            onClick={onApply}
            disabled={diff.plans.length === 0}
          >
            {t("apps.recoveryApply", { count: diff.plans.length })}
          </Button>
        )}
      </div>
    </Card>
  );
}

function recoveryPackageState(
  enabled: boolean | null,
  t: (key: string) => string,
): string {
  if (enabled === null) return t("apps.recoveryAbsent");
  return enabled ? t("apps.recoveryEnabled") : t("apps.recoveryDisabled");
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

function PermissionsPanel({
  target,
  pkg,
  userId,
  onClose,
}: {
  target: DeviceTarget;
  pkg: string;
  userId: number;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [perms, setPerms] = useState<PermissionInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [toggling, setToggling] = useState<string | null>(null);
  const [permError, setPermError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setPermError(null);
    try {
      const result = await callListPermissions(target, pkg);
      setPerms(result);
    } catch (e) {
      setPerms([]);
      setPermError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [target, pkg]);

  useEffect(() => {
    void load();
  }, [load]);

  const togglePerm = useCallback(
    async (permission: string, grant: boolean) => {
      setToggling(permission);
      setPermError(null);
      try {
        await callSetPermission(target, pkg, permission, grant, userId);
        setPerms((prev) =>
          prev.map((p) =>
            p.permission === permission ? { ...p, granted: grant } : p,
          ),
        );
      } catch (e) {
        // Many permissions (signature/system, or fixed by policy) can't be
        // changed via `pm grant`. Surface why instead of silently snapping
        // the toggle back, then reload to show the real state.
        setPermError(
          t("apps.permissionToggleFailed", {
            message: e instanceof Error ? e.message : String(e),
          }),
        );
        void load();
      } finally {
        setToggling(null);
      }
    },
    [target, pkg, userId, load, t],
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
      {permError && (
        <div
          role="alert"
          className="border-b border-rose-500/20 bg-rose-500/10 px-4 py-3 text-xs text-rose-200"
        >
          {permError}
        </div>
      )}
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
        <div className="max-h-80 divide-y divide-white/10 overflow-y-auto">
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
                aria-pressed={p.granted}
                aria-label={t(
                  p.granted ? "apps.revokePermission" : "apps.grantPermission",
                  { permission: p.permission },
                )}
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
