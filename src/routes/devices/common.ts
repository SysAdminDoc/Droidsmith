// Shared local types for the Devices route and its extracted sub-panels
// (IMP-67, IMP-72). Kept separate so the god-file split does not duplicate them.

import {
  summarizeState,
  type AdbRecoveryResult,
  type DeviceInfo,
  type DeviceTarget,
  type SerializedDeviceState,
} from "../../lib/tauri";

export type StatusTone = "neutral" | "success" | "danger";
export type StatusMessage = { text: string; tone: StatusTone } | null;

/** Selected-device detail lifecycle (device info query). */
export type DetailState =
  | { kind: "idle" }
  | { kind: "loading"; target: DeviceTarget }
  | { kind: "ok"; info: DeviceInfo; target: DeviceTarget }
  | { kind: "error"; target: DeviceTarget; message: string };

/** ADB server recovery lifecycle for the recovery dialog. */
export type RecoveryState =
  | { kind: "idle" }
  | { kind: "running"; status: string }
  | { kind: "done"; result: AdbRecoveryResult }
  | { kind: "error"; message: string };

export function statusToneClass(tone: StatusTone): string {
  if (tone === "success") return "text-emerald-200";
  if (tone === "danger") return "text-red-200";
  return "text-anvil-300";
}

export function formatKb(kb: number): string {
  if (kb >= 1048576) return `${(kb / 1048576).toFixed(1)} GB`;
  if (kb >= 1024) return `${(kb / 1024).toFixed(1)} MB`;
  return `${kb} KB`;
}

/** Title-cased device state label for the connected-device table. */
export function formatStateLabel(state: SerializedDeviceState): string {
  const label = summarizeState(state);
  return label.charAt(0).toUpperCase() + label.slice(1);
}

/** Badge tone for a device state (used by the connected-device table). */
export function deviceStateTone(
  state: SerializedDeviceState,
): "neutral" | "info" | "success" | "warning" | "danger" {
  if (typeof state !== "string") {
    return "neutral";
  }

  if (state === "device") {
    return "success";
  }

  if (state === "bootloader" || state === "recovery" || state === "sideload") {
    return "info";
  }

  if (state === "unauthorized" || state === "offline") {
    return "warning";
  }

  if (state === "no_permissions") {
    return "danger";
  }

  return "neutral";
}

export function formatBytes(bytes: number | null, unknown: string): string {
  if (bytes == null) return unknown;
  if (bytes >= 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`;
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

/**
 * Resolve an app package to force-stop from a process name, or null when the
 * row is a native/kernel process that `am force-stop` cannot target (R-090).
 * App processes report their package (optionally with a `:private` suffix);
 * native binaries, kernel threads, and daemons (paths, no dot, or a slash) are
 * skipped.
 */
export function appProcessPackage(name: string): string | null {
  const base = name.split(":")[0]?.trim() ?? "";
  const isPackage = /^[a-zA-Z][a-zA-Z0-9_]*(\.[a-zA-Z][a-zA-Z0-9_]*)+$/.test(
    base,
  );
  return isPackage ? base : null;
}
