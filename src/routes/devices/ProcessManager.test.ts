import { describe, expect, it } from "vitest";

import { appProcessPackage } from "./common";

describe("appProcessPackage (R-090 force-stop gating)", () => {
  it("returns the package for app process names", () => {
    expect(appProcessPackage("com.android.systemui")).toBe(
      "com.android.systemui",
    );
    expect(appProcessPackage("com.google.android.gms")).toBe(
      "com.google.android.gms",
    );
  });

  it("strips a private-process suffix down to the base package", () => {
    expect(appProcessPackage("com.google.android.gms:persistent")).toBe(
      "com.google.android.gms",
    );
    expect(appProcessPackage("com.foo.bar:remote")).toBe("com.foo.bar");
  });

  it("returns null for native binaries, kernel threads, and daemons", () => {
    // No dot: not a package.
    expect(appProcessPackage("zygote64")).toBeNull();
    expect(appProcessPackage("system_server")).toBeNull();
    // Path-like native binaries.
    expect(appProcessPackage("/system/bin/surfaceflinger")).toBeNull();
    // Kernel worker threads.
    expect(appProcessPackage("kworker/0:2")).toBeNull();
    // Empty / whitespace.
    expect(appProcessPackage("")).toBeNull();
    expect(appProcessPackage("   ")).toBeNull();
  });

  it("rejects malformed package-like strings", () => {
    // Leading dot / empty segment.
    expect(appProcessPackage(".com.foo")).toBeNull();
    // Segment starting with a digit.
    expect(appProcessPackage("com.1foo.bar")).toBeNull();
  });
});
