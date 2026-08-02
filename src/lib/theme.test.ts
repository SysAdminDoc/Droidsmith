import { describe, expect, it, vi } from "vitest";

import {
  applyTheme,
  loadStoredTheme,
  persistTheme,
  THEME_STORAGE_KEY,
} from "./theme";

describe("theme preference", () => {
  it("round-trips the persisted choice and applies it to the document root", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => void values.set(key, value),
    } as Storage;
    const root = {
      setAttribute: vi.fn(),
      style: { setProperty: vi.fn() },
    } as unknown as HTMLElement;
    persistTheme("light", storage);
    expect(storage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(loadStoredTheme(storage)).toBe("light");
    applyTheme("light", root);
    expect(root.setAttribute).toHaveBeenCalledWith("data-theme", "light");
  });
});
