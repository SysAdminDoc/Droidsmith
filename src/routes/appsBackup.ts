import type { BackupPackageResult } from "../lib/tauri";

export type BackupDisplayState = "saved" | "empty" | "header_only";

export function backupDisplayState(
  result: Pick<BackupPackageResult, "empty" | "size_bytes" | "header_only">,
): BackupDisplayState {
  if (result.empty || result.size_bytes === 0) return "empty";
  // A non-empty but header-only .ab means adb backup excluded the app's
  // private data (targetSDK 31+/Android 12 deprecation).
  if (result.header_only) return "header_only";
  return "saved";
}

export function backupDefaultFileName(packageId: string): string {
  const safeName = packageId.replace(/[^A-Za-z0-9._-]+/g, "_");
  return `${safeName || "android-package"}.ab`;
}

export function formatBackupSize(sizeBytes: number | null): string | null {
  if (sizeBytes === null) return null;
  if (sizeBytes < 1024) return `${sizeBytes} B`;
  if (sizeBytes < 1024 * 1024) return `${(sizeBytes / 1024).toFixed(1)} KiB`;
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MiB`;
}
