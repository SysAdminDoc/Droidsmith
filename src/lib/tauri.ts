import { invoke } from "@tauri-apps/api/core";

// Domain types — kept aligned with src-tauri/src/adb + src-tauri/src/commands.

export type ResolveSource =
  | "path"
  | "android_home"
  | "android_studio"
  | "homebrew"
  | "distro_package"
  | "bundled"
  | "not_found";

export type AdbResolution = {
  path: string | null;
  source: ResolveSource;
  version: string | null;
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

export type Device = {
  serial: string;
  state: SerializedDeviceState;
  model: string | null;
  product: string | null;
  device: string | null;
  build_fingerprint: string | null;
  transport_id: number | null;
  connection_generation: number;
  wireless: boolean;
};

export type DeviceTarget = Pick<
  Device,
  | "serial"
  | "transport_id"
  | "connection_generation"
  | "model"
  | "product"
  | "device"
  | "build_fingerprint"
>;

export function deviceTarget(device: Device): DeviceTarget {
  const {
    serial,
    transport_id,
    connection_generation,
    model,
    product,
    device: codename,
    build_fingerprint,
  } = device;
  return {
    serial,
    transport_id,
    connection_generation,
    model,
    product,
    device: codename,
    build_fingerprint,
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
};

export type WirelessCommandResult = {
  endpoint: string;
  stdout: string;
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
  | "clear_data"
  | "force_stop";

export type ActionRequest = {
  serial: string;
  target: DeviceTarget;
  package: string;
  kind: ActionKind;
  /** Android user id the action targets (`pm --user`). Defaults to 0. */
  user_id: number;
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
};

export type AppliedAction = {
  plan: PlannedAction;
  stdout: string;
  applied_at: string;
};

export type JournalEntry = {
  id: number;
  applied: AppliedAction;
  undone_by: number | null;
  undoes: number | null;
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
  android_min: number | null;
  android_max: number | null;
};

export type Pack = {
  name: string;
  version: string;
  description: string;
  targets: PackTargets;
  packages: PackEntry[];
  attribution: string | null;
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

export async function callListDevices(): Promise<ListDevicesResult> {
  return invoke<ListDevicesResult>("list_devices");
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
): Promise<JournalEntry> {
  return invoke<JournalEntry>("apply_action", { plan });
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

export type BackupPackageResult = {
  local_path: string;
  stdout: string;
  size_bytes: number | null;
  empty: boolean;
  /** Non-empty but header-only: `adb backup` excluded the app's data. */
  header_only: boolean;
};

export async function callBackupPackage(
  target: DeviceTarget,
  pkg: string,
  localPath: string,
): Promise<BackupPackageResult> {
  return invoke<BackupPackageResult>("backup_package", {
    target,
    package: pkg,
    local_path: localPath,
  });
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
  localPath: string,
  remotePath: string,
): Promise<string> {
  return invoke<string>("push_file", {
    target,
    local_path: localPath,
    remote_path: remotePath,
  });
}

export async function callPullFile(
  target: DeviceTarget,
  remotePath: string,
  localPath: string,
): Promise<string> {
  return invoke<string>("pull_file", {
    target,
    remote_path: remotePath,
    local_path: localPath,
  });
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
): Promise<string> {
  return invoke<string>("set_permission", {
    target,
    package: pkg,
    permission,
    grant,
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
  localPath: string,
): Promise<string> {
  return invoke<string>("take_screenshot", { target, local_path: localPath });
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
): Promise<string> {
  return invoke<string>("shell_run", { target, argv });
}

export async function callInstallApk(
  target: DeviceTarget,
  apkPath: string,
): Promise<string> {
  return invoke<string>("install_apk", { target, apk_path: apkPath });
}

export async function callExtractApk(
  target: DeviceTarget,
  remotePath: string,
  localPath: string,
): Promise<string> {
  return invoke<string>("extract_apk", {
    target,
    remote_path: remotePath,
    local_path: localPath,
  });
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
  packs: Pack[];
  errors: PackLoadError[];
};

export async function callListPacks(): Promise<PackListing> {
  return invoke<PackListing>("list_packs");
}
