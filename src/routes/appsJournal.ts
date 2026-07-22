import type { JournalEntry } from "../lib/tauri";

export type JournalEntryStatus =
  | "pending"
  | "failed"
  | "interrupted"
  | "undo_interrupted"
  | "undoable"
  | "undone"
  | "undo_record"
  | "irreversible";

export function journalEntryStatus(
  entry: JournalEntry,
  linkedUndoOutcome?: JournalEntry["outcome"],
): JournalEntryStatus {
  if (entry.outcome === "pending") return "pending";
  if (entry.outcome === "failed") return "failed";
  if (entry.outcome === "interrupted") return "interrupted";
  if (entry.undoes !== null) return "undo_record";
  if (entry.undone_by !== null && linkedUndoOutcome === "interrupted") {
    return "undo_interrupted";
  }
  if (entry.undone_by !== null) return "undone";
  const kind = entry.applied.plan.request.kind;
  if (kind === "grant_permission") {
    return entry.applied.before_state === "revoked" &&
      entry.applied.after_state === "granted"
      ? "undoable"
      : "irreversible";
  }
  if (kind === "revoke_permission") {
    return entry.applied.before_state === "granted" &&
      entry.applied.after_state === "revoked"
      ? "undoable"
      : "irreversible";
  }
  if (kind === "uninstall_for_user") {
    return (entry.applied.before_state === "preinstalled_enabled" ||
      entry.applied.before_state === "preinstalled_disabled") &&
      entry.applied.after_state === "retained_preinstalled"
      ? "undoable"
      : "irreversible";
  }
  if (kind === "archive") {
    return (entry.applied.before_state === "user_installed_enabled" ||
      entry.applied.before_state === "user_installed_disabled") &&
      entry.applied.after_state === "archived"
      ? "undoable"
      : "irreversible";
  }
  if (kind === "request_unarchive") {
    return entry.applied.before_state === "archived" &&
      (entry.applied.after_state === "user_installed_enabled" ||
        entry.applied.after_state === "user_installed_disabled")
      ? "undoable"
      : "irreversible";
  }
  if (kind === "shell") {
    const context = entry.applied.plan.request.context;
    const sameDisplayStateFamily =
      (entry.applied.before_state.startsWith("density:") &&
        entry.applied.after_state.startsWith("density:")) ||
      (entry.applied.before_state.startsWith("night:") &&
        entry.applied.after_state.startsWith("night:"));
    return context?.confirmation_source === "device_control" &&
      (context.device_control_restore_argv?.length ?? 0) > 0 &&
      sameDisplayStateFamily &&
      entry.applied.before_state !== entry.applied.after_state
      ? "undoable"
      : "irreversible";
  }
  return kind === "disable" || kind === "enable" ? "undoable" : "irreversible";
}
