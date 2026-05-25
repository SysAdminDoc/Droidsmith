import { describe, expect, it } from "vitest";

import { NAV_ITEMS } from "./App";

describe("NAV_ITEMS", () => {
  it("has the seven planned panes", () => {
    expect(NAV_ITEMS).toHaveLength(7);
  });

  it("every item references a valid R-NNN milestone", () => {
    for (const item of NAV_ITEMS) {
      expect(item.milestone).toMatch(/^R-\d{3}$/);
    }
  });

  it("every item has a description so the placeholder pane is never bare", () => {
    for (const item of NAV_ITEMS) {
      expect(item.description.length).toBeGreaterThan(20);
    }
  });

  it("labels are unique (active-pane lookup depends on it)", () => {
    const labels = new Set(NAV_ITEMS.map((i) => i.label));
    expect(labels.size).toBe(NAV_ITEMS.length);
  });

  it("milestones are sorted ascending — keeps the sidebar in roadmap order", () => {
    const numbers = NAV_ITEMS.map((i) => Number(i.milestone.slice(2)));
    const sorted = [...numbers].sort((a, b) => a - b);
    expect(numbers).toEqual(sorted);
  });
});
