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
  return kind === "disable" || kind === "enable" ? "undoable" : "irreversible";
}
