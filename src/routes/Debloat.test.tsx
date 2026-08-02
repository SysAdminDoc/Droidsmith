import { describe, expect, it } from "vitest";

import { makeQueueRows, patchQueueRow } from "./debloat/queue";
import { verifyDisabled } from "./debloatQueue";

describe("Debloat route queue safety", () => {
  it("does not claim a disable succeeded until a fresh state is verified", () => {
    expect(
      verifyDisabled({ present: true, enabled: false, system: false }),
    ).toBe("ok");
    expect(
      verifyDisabled({ present: true, enabled: true, system: false }),
    ).toBe("still_enabled");
    expect(verifyDisabled(null)).toBe("unknown");
  });

  it("patches only the selected queue row", () => {
    const rows = makeQueueRows([
      { id: "a", package: "com.a", action: "disable" },
      { id: "b", package: "com.b", action: "disable" },
    ] as never);
    const next = patchQueueRow(rows, "a", (row) => ({
      ...row,
      status: "verified",
    }));
    expect(next.find((row) => row.entry.id === "a")).toMatchObject({
      status: "verified",
    });
    expect(next.find((row) => row.entry.id === "b")).toMatchObject({
      status: "pending",
    });
  });
});
