import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { NAV_ITEMS } from "../App";
import {
  applyDocumentLanguage,
  detectInitialLanguage,
  formatDateTime,
  formatNumber,
  LANGUAGE_STORAGE_KEY,
  languageMetadata,
} from "./i18n";
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

  it("prefers a persisted language over browser detection", () => {
    const storage = memoryStorage({ [LANGUAGE_STORAGE_KEY]: "ru" });

    expect(detectInitialLanguage({ storage, browserLanguage: "en-US" })).toBe(
      "ru",
    );
  });

  it("falls back from invalid persisted values to browser language", () => {
    const storage = memoryStorage({ [LANGUAGE_STORAGE_KEY]: "jp" });

    expect(detectInitialLanguage({ storage, browserLanguage: "ru-RU" })).toBe(
      "ru",
    );
  });

  it("propagates normalized language metadata to the document root", () => {
    const root = { lang: "", dir: "" };

    applyDocumentLanguage("ru-RU", root);

    expect(root).toEqual({ lang: "ru", dir: "ltr" });
    expect(languageMetadata("unsupported").locale).toBe("en-US");
  });

  it("resolves every static t(\"literal\") key used in the app", () => {
    const srcRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
    const missing: string[] = [];
    // Matches t("key") / t('key') / t(`key`) with a plain (interpolation-free)
    // literal key. Keys built from variables/template expressions are skipped
    // because they can't be resolved statically.
    const callPattern = /\bt\(\s*(["'`])([A-Za-z0-9_.]+)\1/gu;
    for (const file of sourceFiles(srcRoot)) {
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(callPattern)) {
        const key = match[2];
        if (!hasResolvableKey(en, key)) missing.push(`${key} (${file})`);
      }
    }
    expect(missing).toEqual([]);
  });

  it("formats dates and numbers with the selected locale", () => {
    expect(formatNumber(1234, "en")).toBe("1,234");
    expect(formatNumber(1234, "ru")).toMatch(/^1\s234$/u);
    expect(formatDateTime("not-a-date", "ru")).toBe("not-a-date");
    expect(formatDateTime("2026-06-29T10:05:00Z", "ru")).not.toBe(
      formatDateTime("2026-06-29T10:05:00Z", "en"),
    );
  });
});

function sourceFiles(root: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const full = join(root, entry.name);
    if (entry.isDirectory()) {
      files.push(...sourceFiles(full));
    } else if (
      /\.tsx?$/u.test(entry.name) &&
      !/\.test\.tsx?$/u.test(entry.name)
    ) {
      files.push(full);
    }
  }
  return files;
}

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem(key: string) {
      return values.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      values.set(key, value);
    },
  };
}

function flattenKeys(tree: LocaleTree, prefix = ""): string[] {
  return Object.entries(tree)
    .flatMap(([key, value]) => {
      const path = prefix ? `${prefix}.${key}` : key;
      return isLocaleTree(value) ? flattenKeys(value, path) : [path];
    })
    .sort();
}

// A key resolves if it exists directly or, for pluralized lookups, via any of
// its CLDR count suffixes (`t("x.count", { count })` -> `x.count_one`, ...).
function hasResolvableKey(tree: LocaleTree, path: string): boolean {
  if (hasKey(tree, path)) return true;
  return ["_zero", "_one", "_two", "_few", "_many", "_other"].some((suffix) =>
    hasKey(tree, `${path}${suffix}`),
  );
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
