import type { BackupPackageResult } from "../lib/tauri";

export type BackupDisplayState = "saved" | "empty";

export function backupDisplayState(
  result: Pick<BackupPackageResult, "empty" | "size_bytes">,
): BackupDisplayState {
  return result.empty || result.size_bytes === 0 ? "empty" : "saved";
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
