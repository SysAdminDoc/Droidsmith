import type { PackEntry } from "../../lib/tauri";
import type { PackageSnapshot, QueueStatus } from "../debloatQueue";

export type DebloatQueueRow = {
  entry: PackEntry;
  status: QueueStatus;
  attempts: number;
  before: PackageSnapshot | null;
  after: PackageSnapshot | null;
  journalId: number | null;
  error: string | null;
};

export type QuirkDeviceContext = {
  manufacturer: string | null;
  rom: string | null;
};

export function makeQueueRows(entries: PackEntry[]): DebloatQueueRow[] {
  return entries.map((entry) => ({
    entry,
    status: "pending",
    attempts: 0,
    before: null,
    after: null,
    journalId: null,
    error: null,
  }));
}

export function patchQueueRow(
  rows: DebloatQueueRow[],
  entryId: string,
  patch: (row: DebloatQueueRow) => DebloatQueueRow,
): DebloatQueueRow[] {
  return rows.map((row) => (row.entry.id === entryId ? patch(row) : row));
}
