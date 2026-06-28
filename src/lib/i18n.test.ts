import { describe, expect, it } from "vitest";

import { NAV_ITEMS } from "../App";
import en from "../locales/en.json";
import ru from "../locales/ru.json";

type LocaleTree = Record<string, unknown>;

describe("i18n resources", () => {
  it("keeps Russian keys aligned with English keys", () => {
    expect(flattenKeys(ru)).toEqual(flattenKeys(en));
  });

  it("covers every navigation item label and description", () => {
    for (const item of NAV_ITEMS) {
      expect(hasKey(en, item.labelKey)).toBe(true);
      expect(hasKey(en, item.descriptionKey)).toBe(true);
      expect(hasKey(ru, item.labelKey)).toBe(true);
      expect(hasKey(ru, item.descriptionKey)).toBe(true);
    }
  });
});

function flattenKeys(tree: LocaleTree, prefix = ""): string[] {
  return Object.entries(tree)
    .flatMap(([key, value]) => {
      const path = prefix ? `${prefix}.${key}` : key;
      return isLocaleTree(value) ? flattenKeys(value, path) : [path];
    })
    .sort();
}

function hasKey(tree: LocaleTree, path: string): boolean {
  let cursor: unknown = tree;
  for (const segment of path.split(".")) {
    if (!isLocaleTree(cursor) || !(segment in cursor)) {
      return false;
    }
    cursor = cursor[segment];
  }
  return typeof cursor === "string";
}

function isLocaleTree(value: unknown): value is LocaleTree {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
