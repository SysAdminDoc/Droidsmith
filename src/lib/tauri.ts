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
  transport_id: number | null;
  wireless: boolean;
};

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
  package: string;
  kind: ActionKind;
};

export type PlannedAction = {
  request: ActionRequest;
  args: string[];
  description: string;
};

export type JournalEntry = {
  id: number;
  action: string;
  package: string;
  kind: ActionKind;
  applied_at: string;
  stdout: string;
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
  serial: string,
): Promise<DeviceInfo> {
  return invoke<DeviceInfo>("get_device_info", { serial });
}

export async function callListPackages(
  serial: string,
  filter: PackageFilter,
): Promise<AppPackage[]> {
  return invoke<AppPackage[]>("list_packages", { serial, filter });
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

export async function callJournalList(
  serial: string,
): Promise<JournalEntry[]> {
  return invoke<JournalEntry[]>("journal_list", { serial });
}

export async function callJournalUndo(
  serial: string,
  entryId: number,
): Promise<JournalEntry> {
  return invoke<JournalEntry>("journal_undo", { serial, entry_id: entryId });
}

export type RemoteFileEntry = {
  name: string;
  is_dir: boolean;
  size: number | null;
  permissions: string;
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
};

export async function callListNetworkConnections(
  serial: string,
): Promise<NetworkConnection[]> {
  return invoke<NetworkConnection[]>("list_network_connections", { serial });
}

export async function callBackupPackage(
  serial: string,
  pkg: string,
  localPath: string,
): Promise<string> {
  return invoke<string>("backup_package", {
    serial,
    package: pkg,
    local_path: localPath,
  });
}

export async function callListRemoteFiles(
  serial: string,
  remotePath: string,
): Promise<RemoteListing> {
  return invoke<RemoteListing>("list_remote_files", {
    serial,
    remote_path: remotePath,
  });
}

export async function callPushFile(
  serial: string,
  localPath: string,
  remotePath: string,
): Promise<string> {
  return invoke<string>("push_file", {
    serial,
    local_path: localPath,
    remote_path: remotePath,
  });
}

export async function callPullFile(
  serial: string,
  remotePath: string,
  localPath: string,
): Promise<string> {
  return invoke<string>("pull_file", {
    serial,
    remote_path: remotePath,
    local_path: localPath,
  });
}

export type PermissionInfo = {
  permission: string;
  granted: boolean;
};

export async function callListPermissions(
  serial: string,
  pkg: string,
): Promise<PermissionInfo[]> {
  return invoke<PermissionInfo[]>("list_permissions", { serial, package: pkg });
}

export async function callSetPermission(
  serial: string,
  pkg: string,
  permission: string,
  grant: boolean,
): Promise<string> {
  return invoke<string>("set_permission", { serial, package: pkg, permission, grant });
}

export type ProcessInfo = {
  pid: number;
  user: string;
  vsz_kb: number;
  rss_kb: number;
  name: string;
};

export async function callListProcesses(
  serial: string,
): Promise<ProcessInfo[]> {
  return invoke<ProcessInfo[]>("list_processes", { serial });
}

export async function callTakeScreenshot(
  serial: string,
  localPath: string,
): Promise<string> {
  return invoke<string>("take_screenshot", { serial, local_path: localPath });
}

export async function callLocateScrcpy(): Promise<string | null> {
  return invoke<string | null>("locate_scrcpy");
}

export type LaunchScrcpyOptions = {
  serial: string;
  max_size?: number | null;
  bit_rate?: string | null;
  no_audio: boolean;
  record_path?: string | null;
};

export async function callLaunchScrcpy(
  opts: LaunchScrcpyOptions,
): Promise<number> {
  return invoke<number>("launch_scrcpy", opts);
}

export async function callShellRun(
  serial: string,
  argv: string[],
): Promise<string> {
  return invoke<string>("shell_run", { serial, argv });
}

export async function callInstallApk(
  serial: string,
  apkPath: string,
): Promise<string> {
  return invoke<string>("install_apk", { serial, apk_path: apkPath });
}

export async function callExtractApk(
  serial: string,
  remotePath: string,
  localPath: string,
): Promise<string> {
  return invoke<string>("extract_apk", {
    serial,
    remote_path: remotePath,
    local_path: localPath,
  });
}

export type FastbootDevice = {
  serial: string;
  mode: string;
  product: string | null;
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

export async function callListPacks(): Promise<Pack[]> {
  return invoke<Pack[]>("list_packs");
}
