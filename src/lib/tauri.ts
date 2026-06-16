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
