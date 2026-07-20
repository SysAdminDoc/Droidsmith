// Shared local state types for the Apps route and its extracted sub-panels
// (IMP-67). Kept separate so the god-file split does not duplicate them.

import type {
  AppPackage,
  BatchActionPlan,
  InstallPackageResult,
  JournalEntry,
  PackageArchiveCapability,
  PackageBackupPreflight,
  PlannedAction,
  RecoveryBaselineDiff,
} from "../../lib/tauri";

export type PackagesState =
  | { kind: "idle" }
  | { kind: "loading" }
  | {
      kind: "ok";
      packages: AppPackage[];
      archive: PackageArchiveCapability;
    }
  | { kind: "error"; message: string };

export type ActionState =
  | { kind: "idle" }
  | { kind: "confirming"; plan: PlannedAction }
  | { kind: "applying"; plan: PlannedAction }
  | { kind: "confirming_batch"; plan: BatchActionPlan }
  | { kind: "applying_batch"; plan: BatchActionPlan }
  | { kind: "success"; message: string; details?: string[] }
  | { kind: "error"; message: string };

export type JournalState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ok"; entries: JournalEntry[] }
  | { kind: "error"; message: string };

export type BackupNotice = {
  title: string;
  message: string;
  tone: "neutral" | "info" | "success" | "warning" | "danger";
  path?: string;
  output?: string;
  sizeBytes?: number | null;
  showLimitations?: boolean;
  operationId?: string;
  progress?: string;
  evidence?: PackageBackupPreflight["evidence"];
  pendingLegacy?: {
    package: string;
    preflight: PackageBackupPreflight;
  };
};

export type InstallState =
  | { kind: "idle" }
  | { kind: "choosing" }
  | {
      kind: "running";
      operationId: string;
      progress: string;
      output: string;
    }
  | { kind: "result"; localPath: string; result: InstallPackageResult }
  | { kind: "error"; message: string }
  | {
      kind: "confirming_override";
      localPath: string;
      result: InstallPackageResult;
    };

export type RecoveryState =
  | { kind: "idle" }
  | { kind: "busy"; message: string }
  | { kind: "saved"; path: string; sha256: string }
  | { kind: "review"; diff: RecoveryBaselineDiff }
  | {
      kind: "result";
      diff: RecoveryBaselineDiff;
      applied: number;
      failures: string[];
    }
  | { kind: "error"; message: string };
