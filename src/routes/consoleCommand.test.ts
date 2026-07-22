import { describe, expect, it } from "vitest";

import {
  appendConsoleHistory,
  MAX_CONSOLE_HISTORY_ENTRIES,
  MAX_CONSOLE_HISTORY_OUTPUT_CHARS,
  parseConsoleCommand,
  type ConsoleHistoryEntry,
} from "./consoleCommand";

describe("parseConsoleCommand", () => {
  it("preserves quoted and escaped argv boundaries without invoking a shell", () => {
    expect(
      parseConsoleCommand(`dumpsys package "com.example app" 'literal value'`),
    ).toEqual({
      argv: ["dumpsys", "package", "com.example app", "literal value"],
    });
    expect(parseConsoleCommand("getprop ro.product\\ model")).toEqual({
      argv: ["getprop", "ro.product model"],
    });
  });

  it("reports incomplete or backend-unsupported arguments", () => {
    expect(parseConsoleCommand(`getprop "unfinished`)).toEqual({
      error: "unterminatedQuote",
    });
    expect(parseConsoleCommand("getprop trailing\\")).toEqual({
      error: "trailingEscape",
    });
    expect(parseConsoleCommand(`settings put key ""`)).toEqual({
      error: "emptyArgument",
    });
  });
});

describe("appendConsoleHistory", () => {
  const entry = (id: number, output = "ok"): ConsoleHistoryEntry => ({
    id,
    command: `command ${id}`,
    output,
    error: false,
    timestamp: id,
  });

  it("bounds both entry count and retained output", () => {
    let history: ConsoleHistoryEntry[] = [];
    for (let id = 0; id < MAX_CONSOLE_HISTORY_ENTRIES + 5; id += 1) {
      history = appendConsoleHistory(
        history,
        entry(id),
        "Earlier output omitted",
      );
    }
    expect(history).toHaveLength(MAX_CONSOLE_HISTORY_ENTRIES);
    expect(history[0]?.id).toBe(5);

    const oversized = `first${"x".repeat(MAX_CONSOLE_HISTORY_OUTPUT_CHARS)}tail`;
    const [bounded] = appendConsoleHistory(
      [],
      entry(1, oversized),
      "Earlier output omitted",
    );
    expect(bounded?.output.length).toBeLessThanOrEqual(
      MAX_CONSOLE_HISTORY_OUTPUT_CHARS,
    );
    expect(bounded?.output).toContain("Earlier output omitted");
    expect(bounded?.output.endsWith("tail")).toBe(true);
  });
});
