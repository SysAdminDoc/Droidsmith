export type DroidsmithTheme = "dark" | "light";

export const THEME_STORAGE_KEY = "droidsmith.theme";

export function loadStoredTheme(
  storage: Storage | null = typeof window === "undefined"
    ? null
    : window.localStorage,
): DroidsmithTheme {
  return storage?.getItem(THEME_STORAGE_KEY) === "light" ? "light" : "dark";
}

export function applyTheme(
  theme: DroidsmithTheme,
  root: HTMLElement | null = typeof document === "undefined"
    ? null
    : document.documentElement,
): void {
  root?.setAttribute("data-theme", theme);
  root?.style.setProperty("color-scheme", theme);
}

export function persistTheme(
  theme: DroidsmithTheme,
  storage: Storage | null = typeof window === "undefined"
    ? null
    : window.localStorage,
): void {
  try {
    storage?.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Private browsing or a disabled storage backend should not block the UI.
  }
}
