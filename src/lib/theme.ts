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
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    // Keep the native title bar/window chrome aligned with the persisted
    // renderer preference. The guarded dynamic import keeps browser smoke and
    // unit tests independent of the Tauri runtime.
    void import("@tauri-apps/api/app")
      .then(({ setTheme }) => setTheme(theme))
      .catch(() => {
        // A browser preview or an older native runtime may not expose the
        // app-level theme API; the renderer theme remains authoritative.
      });
  }
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
