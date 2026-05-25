import { describe, expect, it } from "vitest";

import { inTauri, summarizeState } from "./tauri";

describe("summarizeState", () => {
  it("formats known snake-case states for display", () => {
    expect(summarizeState("no_permissions")).toBe("no permissions");
    expect(summarizeState("unauthorized")).toBe("unauthorized");
  });

  it("handles current and legacy serialized Other variants", () => {
    expect(summarizeState({ other: "rescue" })).toBe("other (rescue)");
    expect(summarizeState({ Other: "legacy" })).toBe("other (legacy)");
  });
});

describe("inTauri", () => {
  it("is false in a plain test runtime", () => {
    expect(inTauri()).toBe(false);
  });
});
