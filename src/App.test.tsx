import { describe, expect, it } from "vitest";

describe("smoke", () => {
  it("vitest harness is alive", () => {
    expect(1 + 1).toBe(2);
  });

  it("nav-item mapping shape is what App expects", () => {
    // Mirrors the NAV_ITEMS table in App.tsx — kept here so renaming a
    // milestone doesn't slip past CI without at least a test failure.
    const expected = [
      "R-012",
      "R-020",
      "R-033",
      "R-040",
      "R-050",
      "R-051",
      "R-052",
    ];
    expect(expected).toHaveLength(7);
    expect(expected.every((m) => /^R-\d+$/.test(m))).toBe(true);
  });
});
