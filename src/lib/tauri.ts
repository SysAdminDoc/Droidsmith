import { Channel, invoke } from "@tauri-apps/api/core";

// Domain types — kept aligned with src-tauri/src/adb + src-tauri/src/commands.

export type ResolveSource =
  | "path"
  | "android_home"
  | "android_studio"
  | "homebrew"
  | "distro_package"
  | "bundled"
  | "not_found";

export type PlatformToolsStatus = "supported" | "warn" | "blocked";

export type PlatformToolsAssessment = {
  status: PlatformToolsStatus;
  rationale: string;
  recommended_version: string;
  warning_below_version: string;
  policy_reviewed_on: string;
  source_url: string;
};

export type AdbResolution = {
  path: string | null;
  source: ResolveSource;
  version: string | null;
  compatibility: PlatformToolsAssessment;
};

export type OsInfo = { family: string; version: string; arch: string };

export type Heartbeat = {
  version: string;
  os: OsInfo;
  tauri_version: string;
  rust_version: string;
  app_data_dir: string | null;
  adb: AdbResolution;
};

export type DeviceState =
  | { kind: "device" }
  | { kind: "unauthorized" }
  | { kind: "offline" }
  | { kind: "recovery" }
  | { kind: "bootloader" }
  | { kind: "sideload" }
  | { kind: "no_permissions" }
  | { kind: "other"; raw: string };

export type DeviceTransportKind =
  | "usb"
  | "tls_wifi"
  | "legacy_tcp"
  | "unknown_tcp";

export type Device = {
  serial: string;
  state: SerializedDeviceState;
  model: string | null;
  product: string | null;
  device: string | null;
  build_fingerprint: string | null;
  transport_id: number | null;
  connection_generation: number;
  transport_kind: DeviceTransportKind;
  wireless: boolean;
};

export type DeviceTarget = Pick<
  Device,
  | "serial"
  | "transport_id"
  | "connection_generation"
  | "transport_kind"
  | "model"
  | "product"
  | "device"
  | "build_fingerprint"
> & { untrusted_transport_override: boolean };

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

// Rust's untagged enum sometimes shows up as either the bare variant
// name (for unit variants) or { Other: "raw" } for tuple variants.
// Normalising in `summarizeState` below.
export type SerializedDeviceState =
  | "device"
  | "unauthorized"
  | "offline"
  | "recovery"
  | "bootloader"
  | "sideload"
  | "no_permissions"
  | { other: string }
  | { Other: string };

export type ListDevicesResult = {
  adb_resolved: boolean;
  adb_path: string | null;
  devices: Device[];
};

export type WifiV2State = "supported" | "not_detected" | "probe_unavailable";

export type AdbHealth = {
  server_status_supported: boolean;
  client_version: string | null;
  server_version: string | null;
  server_build: string | null;
  usb_backend: string | null;
  mdns_backend: string | null;
  mdns_enabled: boolean | null;
  mdns_check: string | null;
  burst_mode: boolean | null;
  recommended_for_wifi_v2: boolean;
  wifi_v2_state: WifiV2State;
  wifi_v2_devices: string[];
  warning: string | null;
  platform_tools: PlatformToolsAssessment;
};

export type HostFinding = {
  code: string;
  severity: "info" | "warning" | "error";
  title: string;
  summary: string;
  evidence: string[];
  remediation: string[];
  official_url: string;
};

export type HostDoctorReport = {
  scanned_at: string;
  platform: "windows" | "linux" | "macos" | "other";
  adb: {
    resolved: boolean;
    source:
      | "path"
      | "android_home"
      | "android_studio"
      | "homebrew"
      | "distro_package"
      | "bundled"
      | "not_found";
    version: string | null;
    query_succeeded: boolean;
    client_version: string | null;
    server_version: string | null;
    compatibility: PlatformToolsAssessment;
  };
  device_state_counts: Record<string, number>;
  findings: HostFinding[];
  privacy: string[];
};

export type DeviceLifecycleEvent =
  | {
      kind: "snapshot";
      result: ListDevicesResult;
      health: AdbHealth | null;
      observed_at: string;
    }
  | { kind: "error"; message: string; observed_at: string };

export type AdbRecoveryOutcome =
  | "pending"
  | "succeeded"
  | "failed"
  | "cancelled";

export type AdbRecoveryRecord = {
  schema_version: number;
  operation_id: string;
  operation: "adb_server_recovery";
  confirmation_source: "devices_health_review";
  outcome: AdbRecoveryOutcome;
  started_at: string;
  completed_at: string | null;
  commands: string[][];
  health_before: AdbHealth | null;
  health_after: AdbHealth | null;
  failure: string | null;
};

export type AdbRecoveryResult = {
  record: AdbRecoveryRecord;
  record_path: string;
};

export type SupportPreview = {
  generated_at: string;
  content: string;
  byte_size: number;
  device_count: number;
  failed_operation_count: number;
  crash_line_count: number;
  local_only: boolean;
};

export type SavedDiagnostics = {
  path: string;
  byte_size: number;
  generated_at: string;
};

export type WipeDiagnosticsResult = {
  files_removed: number;
  bytes_removed: number;
  device_journals_preserved: boolean;
};

export type OperationEventKind =
  | "started"
  | "output"
  | "progress"
  | "reconnecting"
  | "finished"
  | "cancelled";

export type OperationEvent = {
  operation_id: string;
  kind: OperationEventKind;
  stream?: "stdout" | "stderr";
  chunk?: string;
  message?: string;
  elapsed_ms?: number;
  attempt?: number;
};

export type OperationOptions<TEvent = OperationEvent> = {
  operationId?: string;
  onEvent?: (event: TEvent) => void;
};

let operationCounter = 0;

export function newOperationId(prefix: string): string {
  operationCounter += 1;
  const safePrefix = prefix.toLowerCase().replace(/[^a-z0-9_.-]/g, "-");
  return `${safePrefix}-${Date.now().toString(36)}-${operationCounter.toString(36)}`;
}

function invokeOperation<T, TEvent = OperationEvent>(
  command: string,
  args: Record<string, unknown>,
  prefix: string,
  options?: OperationOptions<TEvent>,
): Promise<T> {
  const operationId = options?.operationId ?? newOperationId(prefix);
  const channel = new Channel<TEvent>();
  channel.onmessage = (event) => options?.onEvent?.(event);
  return invoke<T>(command, {
    ...args,
    operation_id: operationId,
    on_event: channel,
  });
}

export type WirelessServiceKind = "pairing" | "connect" | "other";

export type WirelessAdbService = {
  name: string;
  service_type: string;
  kind: WirelessServiceKind;
  host: string;
  port: number;
  endpoint: string;
};

export type ListWirelessServicesResult = {
  adb_resolved: boolean;
  adb_path: string | null;
  services: WirelessAdbService[];
};

export type WirelessPairRequest = {
  host: string;
  port: number;
  pairing_code: string;
};

export type WirelessConnectRequest = {
  host: string;
  port: number;
  legacy_tcp: boolean;
};

export type WirelessCommandResult = {
  endpoint: string;
  stdout: string;
  transport_kind: DeviceTransportKind | null;
};

export type BatteryInfo = {
  level: number | null;
  status: string | null;
  temperature: number | null;
};

export type StorageInfo = {
  total_kb: number | null;
  used_kb: number | null;
  available_kb: number | null;
};

export type DeviceInfo = {
  serial: string;
  model: string | null;
  manufacturer: string | null;
  android_version: string | null;
  sdk_level: string | null;
  build_fingerprint: string | null;
  security_patch: string | null;
  hardware_serial: string | null;
  battery: BatteryInfo | null;
  storage: StorageInfo | null;
  wifi_ip: string | null;
};

export type PackageFilter = "all" | "user" | "system" | "enabled" | "disabled";

export type AppPackage = {
  package: string;
  enabled: boolean;
  system: boolean;
  apk_path: string | null;
  uid: number | null;
  installer: string | null;
};

export type ActionKind =
  | "disable"
  | "enable"
  | "uninstall_for_user"
  | "restore_existing_for_user"
  | "clear_data"
  | "force_stop"
  | "grant_permission"
  | "revoke_permission"
  | "shell";

export type ConfirmationSource =
  | "unspecified"
  | "internal"
  | "apps_preview"
  | "debloat_preview"
  | "permission_toggle"
  | "console_review"
  | "device_control"
  | "cli_apply"
  | "journal_undo"
  | "recovery_baseline";

export type ActionContext = {
  confirmation_source: ConfirmationSource;
  permission: string | null;
  shell_argv: string[];
  transport_override: DeviceTransportKind | null;
  restore_enabled_state: boolean | null;
};

export type ActionRequest = {
  serial: string;
  target: DeviceTarget;
  package: string;
  kind: ActionKind;
  /** Android user id the action targets (`pm --user`). Defaults to 0. */
  user_id: number;
  pack_context?: PackActionContext | null;
  context?: ActionContext;
};

export type PackActionContext = {
  pack_id: string;
  revision: number;
  provenance_source: string;
  provenance_license: string;
  compatibility_status: string;
  override_accepted: boolean;
};

export type AndroidUser = {
  id: number;
  name: string;
  running: boolean;
  current: boolean;
};

export type PlannedAction = {
  request: ActionRequest;
  args: string[];
  description: string;
  incident_id: string;
  before_state: string;
};

export type AppliedAction = {
  plan: PlannedAction;
  stdout: string;
  before_state: string;
  after_state: string;
  applied_at: string;
};

export type ApplyActionResult = {
  entry: JournalEntry;
  stdout: string;
};

export type BaselineActionInput = {
  package: string;
  kind: ActionKind;
};

export type BaselinePack = {
  id: string;
  revision: number;
};

export type BaselineUndoPlan = {
  kind: ActionKind;
  user_id: number;
};

export type BaselinePackage = {
  package: string;
  present: boolean;
  enabled: boolean | null;
  system: boolean | null;
  requested_action: ActionKind;
  undo_plan: BaselineUndoPlan | null;
};

export type RecoveryBaseline = {
  format: "droidsmith_recovery_baseline";
  schema_version: 1;
  exported_at: string;
  device: {
    identity_sha256: string;
    build_fingerprint: string;
  };
  android_user: number;
  pack: BaselinePack | null;
  packages: BaselinePackage[];
};

export type BaselineDiffRow = {
  package: string;
  baseline_present: boolean;
  baseline_enabled: boolean | null;
  live_present: boolean;
  live_enabled: boolean | null;
  requested_action: ActionKind;
  status: "ready" | "already_matches" | "skipped";
  reason_code: string | null;
  reason: string;
};

export type RecoveryBaselineDiff = {
  baseline: RecoveryBaseline;
  compatibility: {
    device_identity_matches: boolean;
    build_fingerprint_matches: boolean;
    android_user_available: boolean;
    current_device_identity_sha256: string;
    current_build_fingerprint: string;
  };
  rows: BaselineDiffRow[];
  plans: PlannedAction[];
};

export type JournalEntry = {
  id: number;
  applied: AppliedAction;
  undone_by: number | null;
  undoes: number | null;
  outcome: "pending" | "succeeded" | "failed" | "interrupted";
  failure: string | null;
};

export type RemovalLevel = "recommended" | "advanced" | "expert" | "unsafe";

export type PackEntry = {
  id: string;
  removal: RemovalLevel;
  description: string;
  depends_on: string[];
  needed_by: string[];
  labels: string[];
};

export type PackTargets = {
  manufacturer: string[];
  rom: string[];
  model: string[];
  build_fingerprint: string[];
  android_min: number | null;
  android_max: number | null;
  user_scope: "owner" | "current" | "any";
};

export type Pack = {
  id: string;
  revision: number;
  name: string;
  version: string;
  description: string;
  targets: PackTargets;
  packages: PackEntry[];
  attribution: string | null;
  provenance: {
    source: string;
    license: string;
  };
};

export type CompatibilityStatus = "compatible" | "unknown" | "mismatch";

export type CompatibilityCheck = {
  field: string;
  status: CompatibilityStatus;
  expected: string[];
  actual: string | null;
};

export type PackEntryStatus = "ready" | "missing" | "unsupported";

export type PackEntryAssessment = {
  id: string;
  status: PackEntryStatus;
  detail: string | null;
};

export type PackAssessment = {
  status: CompatibilityStatus;
  override_required: boolean;
  checks: CompatibilityCheck[];
  entries: PackEntryAssessment[];
};

export type PackCandidate = {
  pack: Pack;
  assessment: PackAssessment;
};

export function summarizeState(s: SerializedDeviceState): string {
  if (typeof s === "string") {
    return s.replace(/_/g, " ");
  }
  const raw = "other" in s ? s.other : s.Other;
  return `other (${raw})`;
}

/** Detect whether we're running inside Tauri vs a plain Vite dev page.
 *  When false, every command-invoke would throw — components should render
 *  a "Tauri runtime not available" hint instead. */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function callHeartbeat(): Promise<Heartbeat> {
  return invoke<Heartbeat>("heartbeat");
}

export async function callRunHostDoctor(): Promise<HostDoctorReport> {
  return invoke<HostDoctorReport>("run_host_doctor");
}

export async function callListDevices(): Promise<ListDevicesResult> {
  return invoke<ListDevicesResult>("list_devices");
}

export async function callWatchDevices(
  options?: OperationOptions<DeviceLifecycleEvent>,
): Promise<void> {
  return invokeOperation<void, DeviceLifecycleEvent>(
    "watch_devices",
    {},
    "devices",
    options,
  );
}

export async function callRecoverAdb(
  confirmed: boolean,
  options?: OperationOptions,
): Promise<AdbRecoveryResult> {
  return invokeOperation<AdbRecoveryResult>(
    "recover_adb",
    { confirmed },
    "adb-recovery",
    options,
  );
}

export async function callPreviewDiagnostics(): Promise<SupportPreview> {
  return invoke<SupportPreview>("preview_diagnostics");
}

export type HostPathPurpose =
  | "diagnostics_save"
  | "bugreport_save"
  | "logcat_save"
  | "package_export_save"
  | "backup_save"
  | "screenshot_save"
  | "pull_save"
  | "extract_apk_save"
  | "recovery_baseline_save"
  | "push_open"
  | "install_open"
  | "recovery_baseline_open";

export type HostPathGrant = {
  id: string;
  purpose: HostPathPurpose;
  local_path: string;
};

export async function callSelectHostPath(
  purpose: HostPathPurpose,
  suggestedName?: string,
): Promise<HostPathGrant | null> {
  return invoke<HostPathGrant | null>("select_host_path", {
    purpose,
    suggested_name: suggestedName ?? null,
  });
}

export async function callSaveDiagnostics(
  pathGrant: string,
): Promise<SavedDiagnostics> {
  return invoke<SavedDiagnostics>("save_diagnostics", {
    path_grant: pathGrant,
  });
}

export async function callWipeDiagnostics(
  confirmed: boolean,
): Promise<WipeDiagnosticsResult> {
  return invoke<WipeDiagnosticsResult>("wipe_diagnostics", { confirmed });
}

export async function callListWirelessServices(): Promise<ListWirelessServicesResult> {
  return invoke<ListWirelessServicesResult>("list_wireless_services");
}

export async function callPairWireless(
  request: WirelessPairRequest,
): Promise<WirelessCommandResult> {
  return invoke<WirelessCommandResult>("pair_wireless", { request });
}

export async function callConnectWireless(
  request: WirelessConnectRequest,
): Promise<WirelessCommandResult> {
  return invoke<WirelessCommandResult>("connect_wireless", { request });
}

export async function callGetDeviceInfo(
  target: DeviceTarget,
): Promise<DeviceInfo> {
  return invoke<DeviceInfo>("get_device_info", { target });
}

export async function callListPackages(
  target: DeviceTarget,
  filter: PackageFilter,
  userId = 0,
): Promise<AppPackage[]> {
  return invoke<AppPackage[]>("list_packages", { target, filter, userId });
}

export async function callListUsers(
  target: DeviceTarget,
): Promise<AndroidUser[]> {
  return invoke<AndroidUser[]>("list_users", { target });
}

export async function callPlanAction(
  request: ActionRequest,
): Promise<PlannedAction> {
  return invoke<PlannedAction>("plan_action", { request });
}

export async function callApplyAction(
  plan: PlannedAction,
): Promise<ApplyActionResult> {
  return invoke<ApplyActionResult>("apply_action", { plan });
}

export async function callExportRecoveryBaseline(
  target: DeviceTarget,
  userId: number,
  actions: BaselineActionInput[],
  pack: BaselinePack | null,
  pathGrant: string,
): Promise<HostArtifact> {
  return invoke<HostArtifact>("export_recovery_baseline", {
    target,
    userId,
    actions,
    pack,
    path_grant: pathGrant,
  });
}

export async function callInspectRecoveryBaseline(
  target: DeviceTarget,
  pathGrant: string,
): Promise<RecoveryBaselineDiff> {
  return invoke<RecoveryBaselineDiff>("inspect_recovery_baseline", {
    target,
    path_grant: pathGrant,
  });
}

export async function callJournalList(serial: string): Promise<JournalEntry[]> {
  return invoke<JournalEntry[]>("journal_list", { serial });
}

export async function callJournalUndo(
  target: DeviceTarget,
  entryId: number,
): Promise<JournalEntry> {
  return invoke<JournalEntry>("journal_undo", { target, entry_id: entryId });
}

export type RemoteFileEntry = {
  name: string;
  is_dir: boolean;
  size: number | null;
  permissions: string;
  parse_error?: string;
};

export type RemoteListing = {
  path: string;
  entries: RemoteFileEntry[];
  free_space_kb: number | null;
};

export type NetworkConnection = {
  state: string;
  protocol: string;
  local_addr: string;
  remote_addr: string;
  process: string | null;
  parse_error?: string;
};

export async function callListNetworkConnections(
  target: DeviceTarget,
): Promise<NetworkConnection[]> {
  return invoke<NetworkConnection[]>("list_network_connections", { target });
}

export type BackupCapability =
  | "apk_export"
  | "legacy_data_eligible"
  | "legacy_data_blocked"
  | "legacy_data_unknown";

export type LegacyEligibilityEvidence = {
  device_sdk: number | null;
  target_sdk: number | null;
  debuggable: boolean | null;
  allow_backup: boolean | null;
  reason: string;
};

export type PackageBackupPreflight = {
  package: string;
  android_user: number;
  default_capability: "apk_export";
  legacy_capability: BackupCapability;
  apk_paths: string[];
  evidence: LegacyEligibilityEvidence;
};

export type PackageExportManifest = {
  format: "droidsmith_package_export";
  schema_version: 1;
  created_at: string;
  mode: "apk_export" | "legacy_data";
  package: string;
  android_user: number;
  device: {
    device_identity_sha256: string;
    build_identity_sha256: string | null;
  };
  eligibility: LegacyEligibilityEvidence;
  legacy_content: "app_data_entries_detected" | "no_app_data_entries" | null;
  artifacts: Array<{
    name: string;
    role: string;
    size_bytes: number;
    sha256: string;
  }>;
};

export type PackageExportResult = {
  artifact: HostArtifact;
  manifest: PackageExportManifest;
};

export type HostArtifact = {
  /** Canonical host path installed only after validation succeeds. */
  local_path: string;
  size_bytes: number;
  sha256: string;
};

export type BugreportCaptureResult = {
  report: HostArtifact;
  sidecar: HostArtifact;
  captured_at: string;
};

export async function callPreflightPackageBackup(
  target: DeviceTarget,
  pkg: string,
  userId: number,
): Promise<PackageBackupPreflight> {
  return invoke<PackageBackupPreflight>("preflight_package_backup", {
    target,
    package: pkg,
    userId,
  });
}

export async function callExportPackageApks(
  target: DeviceTarget,
  pkg: string,
  userId: number,
  pathGrant: string,
  options?: OperationOptions,
): Promise<PackageExportResult> {
  return invokeOperation<PackageExportResult>(
    "export_package_apks",
    { target, package: pkg, userId, path_grant: pathGrant },
    "package-export",
    options,
  );
}

export async function callBackupPackage(
  target: DeviceTarget,
  pkg: string,
  userId: number,
  pathGrant: string,
  options?: OperationOptions,
): Promise<PackageExportResult> {
  return invokeOperation<PackageExportResult>(
    "backup_package",
    { target, package: pkg, userId, path_grant: pathGrant },
    "legacy-backup",
    options,
  );
}

export async function callCaptureBugreport(
  target: DeviceTarget,
  pathGrant: string,
  privacyConfirmed: boolean,
  options?: OperationOptions,
): Promise<BugreportCaptureResult> {
  return invokeOperation<BugreportCaptureResult>(
    "capture_bugreport",
    {
      target,
      path_grant: pathGrant,
      privacy_confirmed: privacyConfirmed,
    },
    "bugreport",
    options,
  );
}

export async function callListRemoteFiles(
  target: DeviceTarget,
  remotePath: string,
): Promise<RemoteListing> {
  return invoke<RemoteListing>("list_remote_files", {
    target,
    remote_path: remotePath,
  });
}

export async function callPushFile(
  target: DeviceTarget,
  pathGrant: string,
  remotePath: string,
  options?: OperationOptions,
): Promise<string> {
  return invokeOperation<string>(
    "push_file",
    { target, path_grant: pathGrant, remote_path: remotePath },
    "push",
    options,
  );
}

export async function callPullFile(
  target: DeviceTarget,
  remotePath: string,
  pathGrant: string,
  options?: OperationOptions,
): Promise<HostArtifact> {
  return invokeOperation<HostArtifact>(
    "pull_file",
    { target, remote_path: remotePath, path_grant: pathGrant },
    "pull",
    options,
  );
}

export type PermissionInfo = {
  permission: string;
  granted: boolean;
};

export async function callListPermissions(
  target: DeviceTarget,
  pkg: string,
): Promise<PermissionInfo[]> {
  return invoke<PermissionInfo[]>("list_permissions", { target, package: pkg });
}

export async function callSetPermission(
  target: DeviceTarget,
  pkg: string,
  permission: string,
  grant: boolean,
  userId: number,
): Promise<ApplyActionResult> {
  return invoke<ApplyActionResult>("set_permission", {
    target,
    package: pkg,
    permission,
    grant,
    userId,
  });
}

export type ProcessInfo = {
  pid: number;
  user: string;
  vsz_kb: number;
  rss_kb: number;
  name: string;
  parse_error?: string;
};

export async function callListProcesses(
  target: DeviceTarget,
): Promise<ProcessInfo[]> {
  return invoke<ProcessInfo[]>("list_processes", { target });
}

export async function callTakeScreenshot(
  target: DeviceTarget,
  pathGrant: string,
): Promise<HostArtifact> {
  return invoke<HostArtifact>("take_screenshot", {
    target,
    path_grant: pathGrant,
  });
}

export async function callLocateScrcpy(): Promise<string | null> {
  return invoke<string | null>("locate_scrcpy");
}

export type LaunchScrcpyOptions = {
  serial: string;
  target: DeviceTarget;
  max_size?: number | null;
  bit_rate?: string | null;
  no_audio: boolean;
  record_path?: string | null;
  keyboard_mode?: string | null;
  turn_screen_off: boolean;
  stay_awake: boolean;
  show_touches: boolean;
};

export type ScrcpySessionState = "running" | "exited" | "stopped";

export type ScrcpySession = {
  id: number;
  serial: string;
  pid: number;
  args: string[];
  started_at: string;
  state: ScrcpySessionState;
  exit_code: number | null;
};

export async function callLaunchScrcpy(
  opts: LaunchScrcpyOptions,
): Promise<ScrcpySession> {
  return invoke<ScrcpySession>("launch_scrcpy", { request: opts });
}

export async function callScrcpySessionStatus(
  sessionId: number,
): Promise<ScrcpySession> {
  return invoke<ScrcpySession>("scrcpy_session_status", {
    session_id: sessionId,
  });
}

export async function callStopScrcpy(
  sessionId: number,
): Promise<ScrcpySession> {
  return invoke<ScrcpySession>("stop_scrcpy", { session_id: sessionId });
}

export async function callShellRun(
  target: DeviceTarget,
  argv: string[],
  options?: OperationOptions,
): Promise<string> {
  return invokeOperation<string>(
    "shell_run",
    { target, argv },
    "shell",
    options,
  );
}

export async function callStreamLogcat(
  target: DeviceTarget,
  options?: OperationOptions,
): Promise<void> {
  return invokeOperation<void>("stream_logcat", { target }, "logcat", options);
}

export async function callCancelOperation(
  operationId: string,
): Promise<boolean> {
  // A stop click can race the backend's validation/registration window. Retry
  // briefly so an immediate cancel or route unmount cannot orphan the child.
  for (let attempt = 0; attempt < 5; attempt += 1) {
    try {
      if (
        await invoke<boolean>("cancel_operation", {
          operation_id: operationId,
        })
      )
        return true;
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
  return invoke<string>("save_logcat_export", {
    path_grant: pathGrant,
    contents,
  });
}

export type ShellActionPlan = {
  mutating: boolean;
  dangerous: boolean;
  plan: PlannedAction | null;
};

export async function callPlanShellAction(
  target: DeviceTarget,
  argv: string[],
): Promise<ShellActionPlan> {
  return invoke<ShellActionPlan>("plan_shell_action", {
    request: { target, argv },
  });
}

export async function callApplyDeviceControl(
  target: DeviceTarget,
  argv: string[],
): Promise<ApplyActionResult> {
  return invoke<ApplyActionResult>("apply_device_control", { target, argv });
}

export async function callInstallApk(
  target: DeviceTarget,
  pathGrant: string,
  installOptions: InstallOptions = {},
  options?: OperationOptions,
): Promise<InstallPackageResult> {
  return invokeOperation<InstallPackageResult>(
    "install_apk",
    { target, path_grant: pathGrant, options: installOptions },
    "install",
    options,
  );
}

export type InstallSourceKind = "apk" | "apks" | "xapk" | "apkm";

export type InstallOptions = {
  allow_downgrade?: boolean;
  bypass_low_target_sdk_block?: boolean;
  override_confirmed?: boolean;
};

export type SuggestedInstallOverride =
  | "allow_downgrade"
  | "bypass_low_target_sdk_block";

export type InstallFailure = {
  code: string;
  cause: string;
  remedy: string;
  suggested_override: SuggestedInstallOverride | null;
  raw_output: string;
};

export type InstallPackageResult = {
  succeeded: boolean;
  source_kind: InstallSourceKind;
  file_count: number;
  total_bytes: number;
  output: string;
  failure: InstallFailure | null;
  audit_id: string;
  retry_path_grant: string | null;
};

export async function callExtractApk(
  target: DeviceTarget,
  remotePath: string,
  pathGrant: string,
  options?: OperationOptions,
): Promise<HostArtifact> {
  return invokeOperation<HostArtifact>(
    "extract_apk",
    { target, remote_path: remotePath, path_grant: pathGrant },
    "extract",
    options,
  );
}

export type FastbootDevice = {
  serial: string;
  mode: string;
  product: string | null;
  parse_error?: string;
};

export async function callLocateFastboot(): Promise<string | null> {
  return invoke<string | null>("locate_fastboot");
}

export async function callListFastbootDevices(): Promise<FastbootDevice[]> {
  return invoke<FastbootDevice[]>("list_fastboot_devices");
}

export async function callFastbootGetvar(
  serial: string,
  key: string,
): Promise<string> {
  return invoke<string>("fastboot_getvar", { serial, key });
}

export type PackLoadError = {
  file: string;
  code: string;
  message: string;
};

export type PackListing = {
  packs: PackCandidate[];
  errors: PackLoadError[];
};

export async function callListPacks(
  target: DeviceTarget,
  userId: number,
): Promise<PackListing> {
  return invoke<PackListing>("list_packs", { target, userId });
}

export type PlanPackRequest = {
  target: DeviceTarget;
  user_id: number;
  pack_id: string;
  revision: number;
  selected: string[];
  override_compatibility: boolean;
};

export type PlannedPack = {
  pack_id: string;
  revision: number;
  assessment: PackAssessment;
  selected_ids: string[];
  plans: PlannedAction[];
  skipped: PackEntryAssessment[];
};

export async function callPlanPack(
  request: PlanPackRequest,
): Promise<PlannedPack> {
  return invoke<PlannedPack>("plan_pack", { request });
}
