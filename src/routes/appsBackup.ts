import type { PackageBackupPreflight, PackageExportResult } from "../lib/tauri";

export type PackageExportDisplayState =
  | "apk_exported"
  | "legacy_entries_detected"
  | "legacy_no_data";

export function packageExportDisplayState(
  result: PackageExportResult,
): PackageExportDisplayState {
  if (result.manifest.mode === "apk_export") return "apk_exported";
  return result.manifest.legacy_content === "app_data_entries_detected"
    ? "legacy_entries_detected"
    : "legacy_no_data";
}

export function packageExportDefaultFileName(
  packageId: string,
  mode: "apk_export" | "legacy_data",
): string {
  const safeName = packageId.replace(/[^A-Za-z0-9._-]+/g, "_");
  const base = safeName || "android-package";
  return `${base}.${mode === "apk_export" ? "apks" : "legacy-data"}.zip`;
}

export function canRunLegacyExport(preflight: PackageBackupPreflight): boolean {
  return preflight.legacy_capability !== "legacy_data_blocked";
}

export function formatBackupSize(sizeBytes: number | null): string | null {
  if (sizeBytes === null) return null;
  if (sizeBytes < 1024) return `${sizeBytes} B`;
  if (sizeBytes < 1024 * 1024) return `${(sizeBytes / 1024).toFixed(1)} KiB`;
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MiB`;
}
