import { describe, expect, it } from "vitest";

import {
  queueStats,
  snapshotPackage,
  verifyDisabled,
  type QueueStatus,
} from "./debloatQueue";

const packages = [
  {
    package: "com.example.disabled",
    enabled: false,
    system: false,
  },
  {
    package: "com.example.enabled",
    enabled: true,
    system: true,
  },
];

describe("debloat queue helpers", () => {
  it("captures package state for before/after verification", () => {
    expect(snapshotPackage(packages, "com.example.disabled")).toEqual({
      present: true,
      enabled: false,
      system: false,
    });
    expect(snapshotPackage(packages, "com.missing")).toEqual({
      present: false,
      enabled: null,
      system: null,
    });
  });

  it("verifies that debloat disable leaves the package disabled", () => {
    expect(
      verifyDisabled(snapshotPackage(packages, "com.example.disabled")),
    ).toBe("ok");
    expect(
      verifyDisabled(snapshotPackage(packages, "com.example.enabled")),
    ).toBe("still_enabled");
    expect(verifyDisabled(snapshotPackage(packages, "com.missing"))).toBe(
      "missing",
    );
  });

  it("summarizes terminal queue states for progress and retry controls", () => {
    const rows: { status: QueueStatus }[] = [
      { status: "verified" },
      { status: "failed" },
      { status: "cancelled" },
      { status: "pending" },
      { status: "running" },
    ];

    expect(queueStats(rows)).toEqual({
      total: 5,
      completed: 3,
      verified: 1,
      failed: 1,
      cancelled: 1,
    });
  });
});
