import { describe, expect, it } from "vitest";

import {
  BUILTIN_QUERIES,
  DEFAULT_QUERY,
  matchesLine,
  parseImportedQueries,
  parseLogcatLine,
  regexError,
  serializeQueries,
  validateQuery,
  type WorkingQuery,
} from "./logcatQueries";

function query(overrides: Partial<WorkingQuery>): WorkingQuery {
  return { ...DEFAULT_QUERY, id: "q1", name: "Test", ...overrides };
}

const nowMs = Date.UTC(2026, 6, 16, 12, 0, 0);

describe("logcat query regex guard", () => {
  it("rejects catastrophic and unsupported constructs", () => {
    expect(regexError("(a+)+")).toBe("nestedQuantifier");
    expect(regexError("(\\w)\\1")).toBe("backreference");
    expect(regexError("(?=foo)")).toBe("lookaround");
    expect(regexError("(unterminated")).toBe("syntax");
  });

  it("accepts a bounded alternation", () => {
    expect(regexError("FATAL EXCEPTION|ANR in ")).toBeNull();
  });
});

describe("logcat query validation", () => {
  it("flags an empty name and an invalid pid", () => {
    expect(validateQuery(query({ name: " " }))?.field).toBe("name");
    expect(validateQuery(query({ pidFilter: "12a" }))?.field).toBe("pidFilter");
    expect(
      validateQuery(query({ useRegex: true, messageFilter: "(a+)+" }))?.field,
    ).toBe("messageFilter");
    expect(validateQuery(query({ tagFilter: "AM" }))).toBeNull();
  });
});

describe("threadtime parsing and matching", () => {
  const line = parseLogcatLine(
    "07-16 11:59:30.500  1234  1250 E ActivityManager: FATAL EXCEPTION: main",
    nowMs,
  );

  it("extracts level, tag, pid, message, and a timestamp", () => {
    expect(line.level).toBe("E");
    expect(line.tag).toBe("ActivityManager");
    expect(line.pid).toBe("1234");
    expect(line.message).toBe("FATAL EXCEPTION: main");
    expect(line.timeMs).not.toBeNull();
  });

  it("still parses legacy brief lines without a timestamp", () => {
    const brief = parseLogcatLine("I/Tag( 42): hello");
    expect(brief.tag).toBe("Tag");
    expect(brief.pid).toBe("42");
    expect(brief.timeMs).toBeNull();
  });

  it("applies level, pid, negation, and age filters", () => {
    expect(matchesLine(line, query({ minLevel: "E" }), nowMs)).toBe(true);
    expect(matchesLine(line, query({ minLevel: "F" }), nowMs)).toBe(false);
    expect(matchesLine(line, query({ pidFilter: "1234" }), nowMs)).toBe(true);
    expect(
      matchesLine(line, query({ pidFilter: "1234", negatePid: true }), nowMs),
    ).toBe(false);
    // Anchor age to the line's own timestamp so the assertion is timezone-safe.
    const base = line.timeMs!;
    expect(matchesLine(line, query({ maxAgeSeconds: 60 }), base + 10_000)).toBe(
      true,
    );
    expect(matchesLine(line, query({ maxAgeSeconds: 5 }), base + 60_000)).toBe(
      false,
    );
  });

  it("resolves package/process filters via the PID map and surfaces unmapped PIDs", () => {
    const map = new Map([["1234", "com.example.app:remote"]]);
    // Process filter matches the full ps name.
    expect(
      matchesLine(line, query({ processFilter: "app:remote" }), nowMs, map),
    ).toBe(true);
    expect(
      matchesLine(line, query({ processFilter: "other" }), nowMs, map),
    ).toBe(false);
    // Package filter strips the ":component" suffix.
    expect(
      matchesLine(
        line,
        query({ packageFilter: "com.example.app" }),
        nowMs,
        map,
      ),
    ).toBe(true);
    expect(
      matchesLine(
        line,
        query({ packageFilter: "com.example.app", negatePackage: true }),
        nowMs,
        map,
      ),
    ).toBe(false);
    // An unmapped PID is never dropped by a package/process filter.
    expect(
      matchesLine(
        line,
        query({ packageFilter: "com.other" }),
        nowMs,
        new Map(),
      ),
    ).toBe(true);
  });
});

describe("built-in presets match Android Studio semantics", () => {
  const crash = BUILTIN_QUERIES.find((q) => q.id === "builtin-crash")!;
  const stacktrace = BUILTIN_QUERIES.find(
    (q) => q.id === "builtin-stacktrace",
  )!;

  const lineWith = (
    level: string,
    message: string,
  ): ReturnType<typeof parseLogcatLine> => ({
    raw: message,
    level,
    tag: "T",
    pid: "1",
    message,
    timeMs: nowMs,
  });

  it("built-in regexes stay inside the linear-time subset", () => {
    expect(regexError(crash.messageFilter)).toBeNull();
    expect(regexError(stacktrace.messageFilter)).toBeNull();
  });

  it("crash preset matches fatal exceptions and ANRs, not routine errors", () => {
    expect(
      matchesLine(lineWith("E", "FATAL EXCEPTION: main"), crash, nowMs),
    ).toBe(true);
    expect(matchesLine(lineWith("E", "ANR in com.example"), crash, nowMs)).toBe(
      true,
    );
    expect(
      matchesLine(lineWith("E", "Fatal signal 11 (SIGSEGV)"), crash, nowMs),
    ).toBe(true);
    expect(
      matchesLine(lineWith("E", "failed to load asset"), crash, nowMs),
    ).toBe(false);
    // Below-Error severity is excluded regardless of message.
    expect(
      matchesLine(lineWith("W", "FATAL EXCEPTION: main"), crash, nowMs),
    ).toBe(false);
  });

  it("stacktrace preset matches stack-trace shaped lines", () => {
    expect(
      matchesLine(
        lineWith("I", "\tat com.example.Foo.bar(Foo.java:42)"),
        stacktrace,
        nowMs,
      ),
    ).toBe(true);
    expect(
      matchesLine(
        lineWith("I", "Caused by: java.lang.NullPointerException"),
        stacktrace,
        nowMs,
      ),
    ).toBe(true);
    expect(matchesLine(lineWith("I", "... 12 more"), stacktrace, nowMs)).toBe(
      true,
    );
    expect(
      matchesLine(lineWith("I", "user tapped settings"), stacktrace, nowMs),
    ).toBe(false);
  });
});

describe("import/export round trip", () => {
  it("re-imports serialized queries and drops invalid payloads", () => {
    const exported = serializeQueries([query({ id: "keep", name: "Keeper" })]);
    const parsed = parseImportedQueries(exported);
    expect(parsed.ok).toBe(true);
    if (parsed.ok) {
      expect(parsed.queries).toHaveLength(1);
      expect(parsed.queries[0]!.name).toBe("Keeper");
    }
    expect(parseImportedQueries("not json").ok).toBe(false);
    expect(parseImportedQueries("[]").ok).toBe(false);
  });
});
