import { describe, expect, it } from "vitest";

import { NAV_ITEMS } from "./App";

describe("NAV_ITEMS", () => {
  it("has the ten planned panes", () => {
    expect(NAV_ITEMS).toHaveLength(10);
  });

  it("every item references a valid R-NNN milestone", () => {
    for (const item of NAV_ITEMS) {
      expect(item.milestone).toMatch(/^R-\d{3}$/);
    }
  });

  it("every item has translation keys for the label and description", () => {
    for (const item of NAV_ITEMS) {
      expect(item.labelKey).toMatch(/^nav\./);
      expect(item.descriptionKey).toMatch(/^[a-z]+\./);
    }
  });

  it("ids are unique (active-pane lookup depends on them)", () => {
    const ids = new Set(NAV_ITEMS.map((i) => i.id));
    expect(ids.size).toBe(NAV_ITEMS.length);
  });

  it("milestones are sorted ascending — keeps the sidebar in roadmap order", () => {
    const numbers = NAV_ITEMS.map((i) => Number(i.milestone.slice(2)));
    const sorted = [...numbers].sort((a, b) => a - b);
    expect(numbers).toEqual(sorted);
  });
});
