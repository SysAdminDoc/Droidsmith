import { describe, expect, it } from "vitest";

import { createRedactedRendererErrorSummary } from "./lib/rendererError";

describe("renderer error summaries", () => {
  it("keeps useful bounded context without host or device identifiers", () => {
    const summary = createRedactedRendererErrorSummary(
      new Error(
        "serial=QA123 token=abcdefghijklmnopqrstuvwx at C:\\Users\\Alice\\secret.txt from 192.168.1.42 alice@example.com",
      ),
      "\n    at BrokenRoute (C:/Users/Alice/app.tsx:12:3)\n    at App",
    );

    expect(summary).toContain("Name: Error");
    expect(summary).toContain("BrokenRoute > App");
    expect(summary).toContain("<redacted>");
    expect(summary).toContain("<path>");
    expect(summary).not.toContain("QA123");
    expect(summary).not.toContain("Alice");
    expect(summary).not.toContain("192.168.1.42");
    expect(summary).not.toContain("alice@example.com");
    expect(summary.length).toBeLessThan(900);
  });
});
