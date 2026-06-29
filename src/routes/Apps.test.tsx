import { describe, expect, it } from "vitest";

import type { ActionKind, JournalEntry } from "../lib/tauri";
import { journalEntryStatus } from "./appsJournal";

function entry(
  kind: ActionKind,
  overrides: Partial<Pick<JournalEntry, "undone_by" | "undoes">> = {},
): JournalEntry {
  return {
    id: 1,
    applied: {
      plan: {
        request: {
          serial: "device-1",
          package: "com.example.app",
          kind,
        },
        args: ["pm", kind, "com.example.app"],
        description: `${kind} com.example.app`,
      },
      stdout: "ok",
      applied_at: "2026-06-28T12:00:00Z",
    },
    undone_by: null,
    undoes: null,
    ...overrides,
  };
}

describe("journalEntryStatus", () => {
  it("enables undo for original disable and enable entries", () => {
    expect(journalEntryStatus(entry("disable"))).toBe("undoable");
    expect(journalEntryStatus(entry("enable"))).toBe("undoable");
  });

  it("marks already-undone entries and undo records as non-actionable", () => {
    expect(journalEntryStatus(entry("disable", { undone_by: 2 }))).toBe(
      "undone",
    );
    expect(journalEntryStatus(entry("enable", { undoes: 1 }))).toBe(
      "undo_record",
    );
  });

  it("marks destructive or one-shot actions as irreversible", () => {
    expect(journalEntryStatus(entry("uninstall_for_user"))).toBe(
      "irreversible",
    );
    expect(journalEntryStatus(entry("clear_data"))).toBe("irreversible");
    expect(journalEntryStatus(entry("force_stop"))).toBe("irreversible");
  });
});
