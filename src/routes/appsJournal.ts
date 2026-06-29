import type { JournalEntry } from "../lib/tauri";

export type JournalEntryStatus =
  | "undoable"
  | "undone"
  | "undo_record"
  | "irreversible";

export function journalEntryStatus(entry: JournalEntry): JournalEntryStatus {
  if (entry.undoes !== null) return "undo_record";
  if (entry.undone_by !== null) return "undone";
  const kind = entry.applied.plan.request.kind;
  return kind === "disable" || kind === "enable" ? "undoable" : "irreversible";
}
