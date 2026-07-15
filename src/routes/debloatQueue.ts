import type { AppPackage } from "../lib/tauri";

export type PackageSnapshot = {
  present: boolean;
  enabled: boolean | null;
  system: boolean | null;
};

export type DisableVerification =
  | "ok"
  | "missing"
  | "still_enabled"
  | "unknown";

export type QueueStatus =
  | "pending"
  | "running"
  | "verified"
  | "failed"
  | "cancelled";

export type QueueStats = {
  total: number;
  completed: number;
  verified: number;
  failed: number;
  cancelled: number;
};

export function snapshotPackage(
  packages: Pick<AppPackage, "package" | "enabled" | "system">[],
  packageId: string,
): PackageSnapshot {
  const match = packages.find((pkg) => pkg.package === packageId);
  if (!match) {
    return { present: false, enabled: null, system: null };
  }
  return {
    present: true,
    enabled: match.enabled,
    system: match.system,
  };
}

/** Convert the backend's targeted package-state journal value into the same
 * shape used by the queue UI. `null` keeps a failed state probe visible as
 * unknown instead of guessing from an older full package listing. */
export function snapshotJournalPackageState(
  state: string,
  system: boolean | null,
): PackageSnapshot | null {
  switch (state) {
    case "installed_enabled":
      return { present: true, enabled: true, system };
    case "installed_disabled":
      return { present: true, enabled: false, system };
    case "not_installed":
      return { present: false, enabled: null, system: null };
    default:
      return null;
  }
}

export function verifyDisabled(
  snapshot: PackageSnapshot | null,
): DisableVerification {
  if (!snapshot) return "unknown";
  if (!snapshot.present) return "missing";
  return snapshot.enabled === false ? "ok" : "still_enabled";
}

export function queueStats(rows: { status: QueueStatus }[]): QueueStats {
  const total = rows.length;
  const verified = rows.filter((row) => row.status === "verified").length;
  const failed = rows.filter((row) => row.status === "failed").length;
  const cancelled = rows.filter((row) => row.status === "cancelled").length;
  return {
    total,
    completed: verified + failed + cancelled,
    verified,
    failed,
    cancelled,
  };
}
