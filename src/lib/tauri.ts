import { Channel } from "@tauri-apps/api/core";

import {
  commands,
  type ActionContext as GeneratedActionContext,
  type ActionRequest as GeneratedActionRequest,
  type AdbRecoveryResult,
  type AndroidUser,
  type AppPackage,
  type AppPackageMetadata,
  type AppliedAction as GeneratedAppliedAction,
  type ApplyActionResult as GeneratedApplyActionResult,
  type BaselineActionInput,
  type BaselinePack,
  type BatchActionItemResult as GeneratedBatchActionItemResult,
  type BatchActionPlan as GeneratedBatchActionPlan,
  type BatchActionResult as GeneratedBatchActionResult,
  type BugreportCaptureResult,
  type Device,
  type DeviceInfo,
  type DeviceLifecycleEvent,
  type DeviceState,
  type DeviceTarget as GeneratedDeviceTarget,
  type DisconnectResult,
  type FastbootDevice,
  type Heartbeat,
  type HostArtifact,
  type HostDoctorReport,
  type HostPathGrant,
  type HostPathPurpose,
  type InstallOptions,
  type InstallPackageResult,
  type JournalEntry as GeneratedJournalEntry,
  type LaunchScrcpyRequest,
  type LayoutSnapshot,
  type LegacySettingsImport,
  type LogcatQuery,
  type LogcatQueryLibrary,
  type LogcatQueryScope,
  type ListDevicesResult,
  type ListWirelessServicesResult,
  type NetworkConnection,
  type OperationEvent,
  type PackageBackupPreflight,
  type PackageExportResult,
  type PackageFilter,
  type PackageListing,
  type Pack as GeneratedPack,
  type PackCandidate as GeneratedPackCandidate,
  type PackEntry as GeneratedPackEntry,
  type PackListing as GeneratedPackListing,
  type PackTargets as GeneratedPackTargets,
  type PermissionInfo,
  type PlanPackRequest as GeneratedPlanPackRequest,
  type PlannedAction as GeneratedPlannedAction,
  type PlannedPack as GeneratedPlannedPack,
  type ProcessInfo,
  type RunningService,
  type Profile,
  type ProfilePreview,
  type RecoveryBaselineDiff as GeneratedRecoveryBaselineDiff,
  type RemoteFileMutationPlan,
  type RemoteFileMutationRequest,
  type RemoteListing,
  type SavedResult,
  type ScrcpyCapabilities as GeneratedScrcpyCapabilities,
  type ScrcpySession,
  type ScrcpyVideoEncoder as GeneratedScrcpyVideoEncoder,
  type ShellActionPlan as GeneratedShellActionPlan,
  type SettingsExportResult,
  type SettingsLanguage,
  type SettingsLoadResult,
  type SettingsScope,
  type SettingsSnapshot,
  type MirrorPreset as GeneratedSettingsMirrorPreset,
  type SupportPreview,
  type WipeResult,
  type WirelessCommandResult,
  type WirelessConnectRequest,
  type WirelessFailureDiagnostics,
  type WirelessFailureHintCode,
  type WirelessPairRequest,
} from "./bindings";

export * from "./bindings";

type RequiredFields<T, K extends keyof T> = Omit<T, K> & Required<Pick<T, K>>;

/**
 * Generated inputs retain Rust's serde-default optionality. The renderer
 * receives these records after backend normalization, so refine only those
 * guaranteed output fields used by the UI.
 */
export type DeviceTarget = RequiredFields<
  GeneratedDeviceTarget,
  "transport_kind" | "untrusted_transport_override"
>;
export type ActionContext = RequiredFields<
  GeneratedActionContext,
  | "confirmation_source"
  | "permission"
  | "shell_argv"
  | "transport_override"
  | "restore_enabled_state"
  | "batch_id"
>;
export type ActionRequest = Omit<
  GeneratedActionRequest,
  "target" | "user_id" | "context"
> & {
  target: DeviceTarget;
  user_id: number;
  context?: ActionContext;
};
export type PlannedAction = Omit<GeneratedPlannedAction, "request"> & {
  request: ActionRequest;
};
export type AppliedAction = Omit<
  GeneratedAppliedAction,
  "plan" | "before_state" | "after_state"
> & {
  plan: PlannedAction;
  before_state: string;
  after_state: string;
};
export type JournalEntry = Omit<
  GeneratedJournalEntry,
  "applied" | "outcome" | "failure"
> & {
  applied: AppliedAction;
  outcome: NonNullable<GeneratedJournalEntry["outcome"]>;
  failure: string | null;
};
export type ApplyActionResult = Omit<GeneratedApplyActionResult, "entry"> & {
  entry: JournalEntry;
};
export type BatchActionPlan = Omit<GeneratedBatchActionPlan, "plans"> & {
  plans: PlannedAction[];
};
export type BatchActionItemResult = Omit<
  GeneratedBatchActionItemResult,
  "entry"
> & {
  entry: JournalEntry | null;
};
export type BatchActionResult = Omit<GeneratedBatchActionResult, "items"> & {
  items: BatchActionItemResult[];
};
export type PackEntry = RequiredFields<
  GeneratedPackEntry,
  "depends_on" | "needed_by" | "labels"
>;
export type PackTargets = RequiredFields<
  GeneratedPackTargets,
  | "manufacturer"
  | "rom"
  | "model"
  | "build_fingerprint"
  | "android_min"
  | "android_max"
  | "user_scope"
>;
export type Pack = Omit<
  GeneratedPack,
  "id" | "revision" | "targets" | "packages" | "provenance"
> & {
  id: string;
  revision: number;
  targets: PackTargets;
  packages: PackEntry[];
  provenance: NonNullable<GeneratedPack["provenance"]>;
};
export type PackCandidate = Omit<GeneratedPackCandidate, "pack"> & {
  pack: Pack;
};
export type PackListing = Omit<GeneratedPackListing, "packs"> & {
  packs: PackCandidate[];
};
export type PlanPackRequest = RequiredFields<
  GeneratedPlanPackRequest,
  "override_compatibility"
>;
export type PlannedPack = Omit<GeneratedPlannedPack, "plans"> & {
  plans: PlannedAction[];
};
export type RecoveryBaselineDiff = Omit<
  GeneratedRecoveryBaselineDiff,
  "plans"
> & { plans: PlannedAction[] };
export type ShellActionPlan = Omit<GeneratedShellActionPlan, "plan"> & {
  plan: PlannedAction | null;
};
export type ScrcpyVideoEncoder = Omit<GeneratedScrcpyVideoEncoder, "codec"> & {
  codec: ScrcpyVideoCodec;
};
export type ScrcpyCapabilities = Omit<
  GeneratedScrcpyCapabilities,
  "available_video_codecs" | "video_encoders"
> & {
  available_video_codecs: ScrcpyVideoCodec[];
  video_encoders: ScrcpyVideoEncoder[];
};

/** Compatibility aliases for established renderer-facing names. */
export type SavedDiagnostics = SavedResult;
export type WipeDiagnosticsResult = WipeResult;
export type SerializedDeviceState = DeviceState | { Other: string };

export type OperationOptions<TEvent = OperationEvent> = {
  operationId?: string;
  onEvent?: (event: TEvent) => void;
};

export type ScrcpyVideoCodec = "h264" | "h265" | "av1" | "vp8" | "vp9";

export type LaunchScrcpyOptions = Omit<
  LaunchScrcpyRequest,
  "max_size" | "bit_rate" | "keyboard_mode" | "video_codec" | "video_encoder"
> & {
  max_size?: number | null;
  bit_rate?: string | null;
  keyboard_mode?: string | null;
  video_codec?: ScrcpyVideoCodec | null;
  video_encoder?: string | null;
};

let operationCounter = 0;

export function deviceTarget(device: Device): DeviceTarget {
  const {
    serial,
    transport_id,
    connection_generation,
    transport_kind,
    model,
    product,
    device: codename,
    build_fingerprint,
  } = device;
  return {
    serial,
    transport_id,
    connection_generation,
    transport_kind,
    untrusted_transport_override: false,
    model,
    product,
    device: codename,
    build_fingerprint,
  };
}

export function requiresTransportOverride(
  target: DeviceTarget | null | undefined,
): boolean {
  return (
    target?.transport_kind === "legacy_tcp" ||
    target?.transport_kind === "unknown_tcp"
  );
}

export function withTransportOverride(
  target: DeviceTarget,
  accepted: boolean,
): DeviceTarget {
  return {
    ...target,
    untrusted_transport_override: accepted && requiresTransportOverride(target),
  };
}

export function newOperationId(prefix: string): string {
  operationCounter += 1;
  const safePrefix = prefix.toLowerCase().replace(/[^a-z0-9_.-]/g, "-");
  return `${safePrefix}-${Date.now().toString(36)}-${operationCounter.toString(36)}`;
}

function operationChannel<TEvent>(
  prefix: string,
  options?: OperationOptions<TEvent>,
): { operationId: string; channel: Channel<TEvent> } {
  const operationId = options?.operationId ?? newOperationId(prefix);
  const channel = new Channel<TEvent>();
  channel.onmessage = (event) => options?.onEvent?.(event);
  return { operationId, channel };
}

function rendererRecord<T>(value: unknown): T {
  // Rust fills serde-defaulted fields before any command returns. Specta
  // correctly keeps them optional for inputs; this facade exposes the stricter
  // post-deserialization shape already guaranteed by the backend.
  return value as T;
}

export class WirelessCommandFailure extends Error {
  readonly code: string;
  readonly hintCode: WirelessFailureHintCode | null;
  readonly diagnostics: WirelessFailureDiagnostics | null;

  constructor(
    message: string,
    code = "wireless_adb_failed",
    hintCode: WirelessFailureHintCode | null = null,
    diagnostics: WirelessFailureDiagnostics | null = null,
  ) {
    super(message);
    this.name = "WirelessCommandFailure";
    this.code = code;
    this.hintCode = hintCode;
    this.diagnostics = diagnostics;
  }
}

export function summarizeState(s: SerializedDeviceState): string {
  if (typeof s === "string") return s.replace(/_/g, " ");
  const raw = "other" in s ? s.other : s.Other;
  return `other (${raw})`;
}

/** True only inside the Tauri runtime, not the plain Vite development page. */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function callHeartbeat(): Promise<Heartbeat> {
  return commands.heartbeat();
}

export type SettingsMirrorPreset = GeneratedSettingsMirrorPreset;

export async function callInitializeSettings(
  legacy: LegacySettingsImport,
): Promise<SettingsLoadResult> {
  return commands.initializeSettings(legacy);
}

export async function callSetSettingsLanguage(
  language: SettingsLanguage,
): Promise<SettingsSnapshot> {
  return commands.setSettingsLanguage(language);
}

export async function callGetSettingsMirrorPreset(
  deviceIdentity: string,
): Promise<SettingsMirrorPreset | null> {
  return commands.getSettingsMirrorPreset(deviceIdentity);
}

export async function callSetSettingsMirrorPreset(
  deviceIdentity: string,
  preset: SettingsMirrorPreset,
): Promise<SettingsSnapshot> {
  return commands.setSettingsMirrorPreset(deviceIdentity, preset);
}

export async function callResetSettingsMirrorPreset(
  deviceIdentity: string,
): Promise<SettingsSnapshot> {
  return commands.resetSettingsMirrorPreset(deviceIdentity);
}

export async function callResetSettings(
  scope: SettingsScope,
): Promise<SettingsSnapshot> {
  return commands.resetSettings(scope);
}

export async function callExportSettings(
  scope: SettingsScope,
  pathGrant: string,
): Promise<SettingsExportResult> {
  return commands.exportSettings(scope, pathGrant);
}

export async function callListLogcatQueries(
  deviceIdentity: string | null,
): Promise<LogcatQueryLibrary> {
  return commands.listLogcatQueries(deviceIdentity);
}

export async function callSaveLogcatQueries(
  scope: LogcatQueryScope,
  deviceIdentity: string | null,
  queries: LogcatQuery[],
): Promise<LogcatQueryLibrary> {
  return commands.saveLogcatQueries(scope, deviceIdentity, queries);
}

export async function callRunHostDoctor(): Promise<HostDoctorReport> {
  return commands.runHostDoctor();
}

export async function callListDevices(): Promise<ListDevicesResult> {
  return commands.listDevices();
}

export async function callWatchDevices(
  options?: OperationOptions<DeviceLifecycleEvent>,
): Promise<void> {
  const { operationId, channel } = operationChannel("devices", options);
  await commands.watchDevices(operationId, channel);
}

export async function callRecoverAdb(
  confirmed: boolean,
  options?: OperationOptions,
): Promise<AdbRecoveryResult> {
  const { operationId, channel } = operationChannel("adb-recovery", options);
  return commands.recoverAdb(confirmed, operationId, channel);
}

export async function callPreviewDiagnostics(): Promise<SupportPreview> {
  return commands.previewDiagnostics();
}

export async function callSelectHostPath(
  purpose: HostPathPurpose,
  suggestedName?: string,
): Promise<HostPathGrant | null> {
  return commands.selectHostPath(purpose, suggestedName ?? null);
}

export async function callGrantDroppedPath(
  path: string,
): Promise<HostPathGrant> {
  return commands.grantDroppedPath(path);
}

export async function callDisconnectDevice(
  target: DeviceTarget,
): Promise<DisconnectResult> {
  return commands.disconnectDevice(target);
}

export async function callRevealInFolder(path: string): Promise<void> {
  await commands.revealInFolder(path);
}

export async function callSaveDiagnostics(
  pathGrant: string,
): Promise<SavedDiagnostics> {
  return commands.saveDiagnostics(pathGrant);
}

export async function callWipeDiagnostics(
  confirmed: boolean,
): Promise<WipeDiagnosticsResult> {
  return commands.wipeDiagnostics(confirmed);
}

export async function callListWirelessServices(): Promise<ListWirelessServicesResult> {
  return commands.listWirelessServices();
}

export async function callPairWireless(
  request: WirelessPairRequest,
): Promise<WirelessCommandResult> {
  try {
    return await commands.pairWireless(request);
  } catch (error) {
    throw normalizeWirelessFailure(error);
  }
}

export async function callConnectWireless(
  request: WirelessConnectRequest,
): Promise<WirelessCommandResult> {
  try {
    return await commands.connectWireless(request);
  } catch (error) {
    throw normalizeWirelessFailure(error);
  }
}

/**
 * Extract a human-readable message from an unknown thrown value. Tauri command
 * rejections arrive as plain `{ code, message }` objects (the serialized
 * `CommandError`), not `Error` instances, so a naive `String(error)` renders
 * the useless "[object Object]". Prefer this everywhere errors reach the UI.
 */
export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof (error as { message: unknown }).message === "string"
  ) {
    return (error as { message: string }).message;
  }
  return String(error);
}

export function normalizeWirelessFailure(
  error: unknown,
): WirelessCommandFailure {
  if (error instanceof WirelessCommandFailure) return error;

  let payload: unknown = error;
  if (error instanceof Error) payload = errorPayload(error.message) ?? error;
  else if (typeof error === "string") payload = errorPayload(error) ?? error;

  if (isRecord(payload)) {
    const message =
      typeof payload.message === "string" ? payload.message : String(error);
    const code =
      typeof payload.code === "string" ? payload.code : "wireless_adb_failed";
    const hintCode =
      payload.hint_code === "vpn_interference_likely" ||
      payload.hint_code === "mdns_interference_likely"
        ? payload.hint_code
        : null;
    const diagnostics = isWirelessFailureDiagnostics(payload.diagnostics)
      ? payload.diagnostics
      : null;
    return new WirelessCommandFailure(message, code, hintCode, diagnostics);
  }

  return new WirelessCommandFailure(errorMessage(error));
}

function errorPayload(message: string): unknown | null {
  try {
    return JSON.parse(message) as unknown;
  } catch {
    return null;
  }
}

function isWirelessFailureDiagnostics(
  value: unknown,
): value is WirelessFailureDiagnostics {
  return (
    isRecord(value) &&
    (typeof value.platform_tools_version === "string" ||
      value.platform_tools_version === null) &&
    (typeof value.mdns_enabled === "boolean" || value.mdns_enabled === null) &&
    (typeof value.mdns_backend === "string" || value.mdns_backend === null) &&
    typeof value.mdns_check_succeeded === "boolean" &&
    typeof value.active_vpn_interfaces === "number" &&
    (value.endpoint_kind === "ip_address" ||
      value.endpoint_kind === "local_name") &&
    typeof value.adb_error_kind === "string"
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export async function callGetDeviceInfo(
  target: DeviceTarget,
): Promise<DeviceInfo> {
  return commands.getDeviceInfo(target);
}

export async function callListPackages(
  target: DeviceTarget,
  filter: PackageFilter,
  userId = 0,
): Promise<AppPackage[]> {
  return (await commands.listPackages(target, filter, userId)).packages;
}

export async function callListPackagesWithCapability(
  target: DeviceTarget,
  filter: PackageFilter,
  userId = 0,
): Promise<PackageListing> {
  return commands.listPackages(target, filter, userId);
}

export async function callGetPackageMetadata(
  target: DeviceTarget,
  packageName: string,
  userId = 0,
): Promise<AppPackageMetadata> {
  return commands.getPackageMetadata(target, packageName, userId);
}

export async function callListUsers(
  target: DeviceTarget,
): Promise<AndroidUser[]> {
  return commands.listUsers(target);
}

export async function callInspectProfile(
  target: DeviceTarget,
  pathGrant: string,
): Promise<ProfilePreview> {
  return rendererRecord(await commands.inspectProfile(target, pathGrant));
}

export async function callSaveProfile(
  pathGrant: string,
  profile: Profile,
): Promise<HostArtifact> {
  return commands.saveProfile(pathGrant, profile);
}

export async function callPlanAction(
  request: ActionRequest,
): Promise<PlannedAction> {
  return rendererRecord(await commands.planAction(request));
}

export async function callPlanActionBatch(
  requests: ActionRequest[],
): Promise<BatchActionPlan> {
  return rendererRecord(
    await commands.planActionBatch(requests),
  ) as BatchActionPlan;
}

export async function callApplyAction(
  plan: PlannedAction,
): Promise<ApplyActionResult> {
  return rendererRecord(await commands.applyAction(plan));
}

export async function callApplyActionBatch(
  batch: BatchActionPlan,
): Promise<BatchActionResult> {
  return rendererRecord(
    await commands.applyActionBatch(batch),
  ) as BatchActionResult;
}

export async function callExportRecoveryBaseline(
  target: DeviceTarget,
  userId: number,
  actions: BaselineActionInput[],
  pack: BaselinePack | null,
  pathGrant: string,
): Promise<HostArtifact> {
  return commands.exportRecoveryBaseline(
    target,
    userId,
    actions,
    pack,
    pathGrant,
  );
}

export async function callInspectRecoveryBaseline(
  target: DeviceTarget,
  pathGrant: string,
): Promise<RecoveryBaselineDiff> {
  return rendererRecord(
    await commands.inspectRecoveryBaseline(target, pathGrant),
  );
}

export async function callJournalList(serial: string): Promise<JournalEntry[]> {
  return rendererRecord(await commands.journalList(serial));
}

export async function callJournalUndo(
  target: DeviceTarget,
  entryId: number,
): Promise<JournalEntry> {
  return rendererRecord(await commands.journalUndo(target, entryId));
}

export async function callJournalUndoBatch(
  target: DeviceTarget,
  batchId: string,
): Promise<BatchActionResult> {
  return rendererRecord(
    await commands.journalUndoBatch(target, batchId),
  ) as BatchActionResult;
}

export async function callListNetworkConnections(
  target: DeviceTarget,
): Promise<NetworkConnection[]> {
  return commands.listNetworkConnections(target);
}

export async function callPreflightPackageBackup(
  target: DeviceTarget,
  pkg: string,
  userId: number,
): Promise<PackageBackupPreflight> {
  return commands.preflightPackageBackup(target, pkg, userId);
}

export async function callExportPackageApks(
  target: DeviceTarget,
  pkg: string,
  userId: number,
  pathGrant: string,
  options?: OperationOptions,
): Promise<PackageExportResult> {
  const { operationId, channel } = operationChannel("package-export", options);
  return commands.exportPackageApks(
    target,
    pkg,
    userId,
    pathGrant,
    operationId,
    channel,
  );
}

export async function callBackupPackage(
  target: DeviceTarget,
  pkg: string,
  userId: number,
  pathGrant: string,
  options?: OperationOptions,
): Promise<PackageExportResult> {
  const { operationId, channel } = operationChannel("legacy-backup", options);
  return commands.backupPackage(
    target,
    pkg,
    userId,
    pathGrant,
    operationId,
    channel,
  );
}

export async function callCaptureBugreport(
  target: DeviceTarget,
  pathGrant: string,
  privacyConfirmed: boolean,
  options?: OperationOptions,
): Promise<BugreportCaptureResult> {
  const { operationId, channel } = operationChannel("bugreport", options);
  return commands.captureBugreport(
    target,
    pathGrant,
    privacyConfirmed,
    operationId,
    channel,
  );
}

export async function callListRemoteFiles(
  target: DeviceTarget,
  remotePath: string,
): Promise<RemoteListing> {
  return commands.listRemoteFiles(target, remotePath);
}

export async function callPlanRemoteFileMutation(
  request: RemoteFileMutationRequest,
): Promise<RemoteFileMutationPlan> {
  return commands.planRemoteFileMutation(request);
}

export async function callApplyRemoteFileMutation(
  target: DeviceTarget,
  request: RemoteFileMutationRequest,
  confirmed: boolean,
): Promise<ApplyActionResult> {
  return rendererRecord(
    await commands.applyRemoteFileMutation(target, request, confirmed),
  );
}

export async function callPushFile(
  target: DeviceTarget,
  pathGrant: string,
  remotePath: string,
  confirmed: boolean,
  options?: OperationOptions,
): Promise<ApplyActionResult> {
  const { operationId, channel } = operationChannel("push", options);
  return rendererRecord(
    await commands.pushFile(
      target,
      pathGrant,
      remotePath,
      confirmed,
      operationId,
      channel,
    ),
  );
}

export async function callPullFile(
  target: DeviceTarget,
  remotePath: string,
  pathGrant: string,
  options?: OperationOptions,
): Promise<HostArtifact> {
  const { operationId, channel } = operationChannel("pull", options);
  return commands.pullFile(target, remotePath, pathGrant, operationId, channel);
}

export async function callListPermissions(
  target: DeviceTarget,
  pkg: string,
): Promise<PermissionInfo[]> {
  return commands.listPermissions(target, pkg);
}

export async function callSetPermission(
  target: DeviceTarget,
  pkg: string,
  permission: string,
  grant: boolean,
  userId: number,
): Promise<ApplyActionResult> {
  return rendererRecord(
    await commands.setPermission(target, pkg, permission, grant, userId),
  );
}

export async function callListProcesses(
  target: DeviceTarget,
): Promise<ProcessInfo[]> {
  return commands.listProcesses(target);
}

export async function callListRunningServices(
  target: DeviceTarget,
  pkg: string,
): Promise<RunningService[]> {
  return commands.listRunningServices(target, pkg);
}

export async function callTakeScreenshot(
  target: DeviceTarget,
  pathGrant: string,
): Promise<HostArtifact> {
  return commands.takeScreenshot(target, pathGrant);
}

export async function callLocateScrcpy(): Promise<string | null> {
  return commands.locateScrcpy();
}

export async function callScrcpyCapabilities(
  target: DeviceTarget,
): Promise<ScrcpyCapabilities> {
  return rendererRecord(await commands.scrcpyCapabilities(target));
}

export async function callLaunchScrcpy(
  opts: LaunchScrcpyOptions,
  pathGrant?: string,
): Promise<ScrcpySession> {
  const request: LaunchScrcpyRequest = {
    ...opts,
    max_size: opts.max_size ?? null,
    bit_rate: opts.bit_rate ?? null,
    keyboard_mode: opts.keyboard_mode ?? null,
    video_codec: opts.video_codec ?? null,
    video_encoder: opts.video_encoder ?? null,
  };
  return commands.launchScrcpy(request, pathGrant ?? null);
}

export async function callScrcpySessionStatus(
  sessionId: number,
): Promise<ScrcpySession> {
  return commands.scrcpySessionStatus(sessionId);
}

export async function callStopScrcpy(
  sessionId: number,
): Promise<ScrcpySession> {
  return commands.stopScrcpy(sessionId);
}

export async function callShellRun(
  target: DeviceTarget,
  argv: string[],
  options?: OperationOptions,
): Promise<string> {
  const { operationId, channel } = operationChannel("shell", options);
  return commands.shellRun(target, argv, operationId, channel);
}

export async function callStreamLogcat(
  target: DeviceTarget,
  options?: OperationOptions,
): Promise<void> {
  const { operationId, channel } = operationChannel("logcat", options);
  await commands.streamLogcat(target, operationId, channel);
}

export async function callCancelOperation(
  operationId: string,
): Promise<boolean> {
  // A stop click can race the backend's validation/registration window. Retry
  // briefly so an immediate cancel or route unmount cannot orphan the child.
  for (let attempt = 0; attempt < 5; attempt += 1) {
    try {
      if (await commands.cancelOperation(operationId)) return true;
    } catch {
      return false;
    }
    await new Promise((resolve) => globalThis.setTimeout(resolve, 40));
  }
  return false;
}

export async function callSaveLogcatExport(
  pathGrant: string,
  contents: string,
): Promise<string> {
  return commands.saveLogcatExport(pathGrant, contents);
}

export async function callCaptureLayout(
  target: DeviceTarget,
): Promise<LayoutSnapshot> {
  return commands.captureLayout(target);
}

export async function callSaveLayoutExport(
  pathGrant: string,
  contents: string,
): Promise<string> {
  return commands.saveLayoutExport(pathGrant, contents);
}

export async function callPlanShellAction(
  target: DeviceTarget,
  argv: string[],
): Promise<ShellActionPlan> {
  return rendererRecord(await commands.planShellAction({ target, argv }));
}

export async function callApplyDeviceControl(
  target: DeviceTarget,
  argv: string[],
): Promise<ApplyActionResult> {
  return rendererRecord(await commands.applyDeviceControl(target, argv));
}

export async function callInstallApk(
  target: DeviceTarget,
  pathGrant: string,
  installOptions: InstallOptions = {},
  options?: OperationOptions,
): Promise<InstallPackageResult> {
  const { operationId, channel } = operationChannel("install", options);
  return commands.installApk(
    target,
    pathGrant,
    installOptions,
    operationId,
    channel,
  );
}

export async function callExtractApk(
  target: DeviceTarget,
  remotePath: string,
  pathGrant: string,
  options?: OperationOptions,
): Promise<HostArtifact> {
  const { operationId, channel } = operationChannel("extract", options);
  return commands.extractApk(
    target,
    remotePath,
    pathGrant,
    operationId,
    channel,
  );
}

export async function callLocateFastboot(): Promise<string | null> {
  return commands.locateFastboot();
}

export async function callListFastbootDevices(): Promise<FastbootDevice[]> {
  return commands.listFastbootDevices();
}

export async function callFastbootGetvar(
  serial: string,
  key: string,
): Promise<string> {
  return commands.fastbootGetvar(serial, key);
}

export async function callListPacks(
  target: DeviceTarget,
  userId: number,
): Promise<PackListing> {
  return rendererRecord(await commands.listPacks(target, userId));
}

export async function callPlanPack(
  request: PlanPackRequest,
): Promise<PlannedPack> {
  return rendererRecord(await commands.planPack(request));
}
