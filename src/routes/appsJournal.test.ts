import { describe, expect, it } from "vitest";

import type { ActionKind, JournalEntry } from "../lib/tauri";
import { journalEntryStatus } from "./appsJournal";

function entry(
  kind: ActionKind,
  overrides: Partial<Pick<JournalEntry, "undone_by" | "undoes">> = {},
): JournalEntry {
  const permissionStates =
    kind === "grant_permission"
      ? { before_state: "revoked", after_state: "granted" }
      : kind === "revoke_permission"
        ? { before_state: "granted", after_state: "revoked" }
        : {
            before_state: "installed_enabled",
            after_state: "installed_disabled",
          };
  return {
    id: 1,
    applied: {
      plan: {
        request: {
          serial: "device-1",
          target: {
            serial: "device-1",
            transport_id: 1,
            connection_generation: 2,
            transport_kind: "usb",
            untrusted_transport_override: false,
            model: null,
            product: null,
            device: null,
            build_fingerprint: "build/test",
          },
          package: "com.example.app",
          kind,
          user_id: 0,
        },
        args: ["pm", kind, "com.example.app"],
        description: `${kind} com.example.app`,
        incident_id: "op-test-1",
        before_state: "installed_enabled",
      },
      stdout: "ok",
      ...permissionStates,
      applied_at: "2026-06-28T12:00:00Z",
    },
    undone_by: null,
    undoes: null,
    outcome: "succeeded",
    failure: null,
    ...overrides,
  };
}

describe("journalEntryStatus", () => {
  it("enables undo for original disable and enable entries", () => {
    expect(journalEntryStatus(entry("disable"))).toBe("undoable");
    expect(journalEntryStatus(entry("enable"))).toBe("undoable");
    expect(journalEntryStatus(entry("grant_permission"))).toBe("undoable");
    expect(journalEntryStatus(entry("revoke_permission"))).toBe("undoable");
  });

  it("does not undo a permission no-op with an unknown prior state", () => {
    expect(
      journalEntryStatus({
        ...entry("grant_permission"),
        applied: {
          ...entry("grant_permission").applied,
          before_state: "unknown",
        },
      }),
    ).toBe("irreversible");
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

  it("surfaces pending, failed, and interrupted operation records", () => {
    expect(
      journalEntryStatus({ ...entry("disable"), outcome: "pending" }),
    ).toBe("pending");
    expect(journalEntryStatus({ ...entry("disable"), outcome: "failed" })).toBe(
      "failed",
    );
    expect(
      journalEntryStatus({ ...entry("disable"), outcome: "interrupted" }),
    ).toBe("interrupted");
    expect(
      journalEntryStatus({ ...entry("disable"), undone_by: 2 }, "interrupted"),
    ).toBe("undo_interrupted");
  });
});
