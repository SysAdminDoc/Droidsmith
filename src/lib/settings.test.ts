import { beforeEach, describe, expect, it, vi } from "vitest";

// Overridable Tauri backend: the retry-after-failure test needs the desktop
// initialization path (browser fallback never rejects). Defaults pass through
// to the real module so the browser-fallback tests stay untouched.
const backend = vi.hoisted(() => ({
  forceTauri: false,
  initialize: vi.fn<(legacy: unknown) => Promise<unknown>>(),
}));

vi.mock(
  "./tauri",
  async (importOriginal: () => Promise<typeof import("./tauri")>) => {
    const actual = await importOriginal();
    return {
      ...actual,
      inTauri: () => backend.forceTauri || actual.inTauri(),
      callInitializeSettings: ((legacy: unknown) =>
        backend.initialize(legacy)) as typeof actual.callInitializeSettings,
    };
  },
);

import {
  collectLegacySettings,
  initializeSettings,
  loadStoredMirrorPreset,
  resetSettingsInitializationForTests,
  resetStoredSettings,
  saveStoredMirrorPreset,
  setStoredLanguage,
} from "./settings";
import { LANGUAGE_STORAGE_KEY } from "./i18n";
import {
  DEFAULT_MIRROR_PRESET,
  presetStorageKey,
} from "../routes/mirrorPresets";

describe("typed settings renderer migration", () => {
  beforeEach(() => {
    resetSettingsInitializationForTests();
    backend.forceTauri = false;
    backend.initialize.mockReset();
  });

  it("retries initialization after a transient startup failure", async () => {
    backend.forceTauri = true;
    const loaded = {
      settings: { version: "1", language: "en", mirrorPresetCount: 0 },
      recovery: "clean",
      legacyCleanupAllowed: false,
    };
    backend.initialize
      .mockRejectedValueOnce(new Error("settings store locked"))
      .mockResolvedValue(loaded);
    const storage = memoryStorage();

    // The rejection must not be cached: the next call retries from scratch
    // instead of failing every settings operation for the whole session.
    await expect(initializeSettings(storage)).rejects.toThrow(
      "settings store locked",
    );
    await expect(initializeSettings(storage)).resolves.toEqual(loaded);
    expect(backend.initialize).toHaveBeenCalledTimes(2);

    // A successful result stays cached — no third backend call.
    await expect(initializeSettings(storage)).resolves.toEqual(loaded);
    expect(backend.initialize).toHaveBeenCalledTimes(2);
  });

  it("collects legacy language and per-device presets deterministically", () => {
    const storage = memoryStorage({
      unrelated: "keep",
      [presetStorageKey("device-b")]: JSON.stringify({ bitRate: "16M" }),
      [LANGUAGE_STORAGE_KEY]: "ru",
      [presetStorageKey("device-a")]: "{corrupt",
    });

    const collection = collectLegacySettings(storage);

    expect(collection.legacy.language).toBe("ru");
    expect(
      collection.legacy.mirrorPresets?.map((entry) => entry.deviceIdentity),
    ).toEqual(["device-a", "device-b"]);
    expect(collection.cleanupKeys).toEqual([
      LANGUAGE_STORAGE_KEY,
      presetStorageKey("device-a"),
      presetStorageKey("device-b"),
    ]);
  });

  it("keeps browser development fallback typed and scoped", async () => {
    const storage = memoryStorage();
    await setStoredLanguage("ru", storage);
    await saveStoredMirrorPreset(
      "device-a",
      {
        ...DEFAULT_MIRROR_PRESET,
        bitRate: "16M",
      },
      storage,
    );

    expect((await initializeSettings(storage)).settings.language).toBe("ru");
    expect((await loadStoredMirrorPreset("device-a", storage)).bitRate).toBe(
      "16M",
    );
    expect(await loadStoredMirrorPreset("device-b", storage)).toEqual(
      DEFAULT_MIRROR_PRESET,
    );

    await resetStoredSettings("language", storage);
    expect(storage.getItem(LANGUAGE_STORAGE_KEY)).toBeNull();
    expect(storage.getItem(presetStorageKey("device-a"))).not.toBeNull();
  });

  it("falls back from corrupt browser preset JSON", async () => {
    const storage = memoryStorage({
      [presetStorageKey("device-a")]: "{broken",
    });
    expect(await loadStoredMirrorPreset("device-a", storage)).toEqual(
      DEFAULT_MIRROR_PRESET,
    );
  });
});

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    get length() {
      return values.size;
    },
    getItem(key: string) {
      return values.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      values.set(key, value);
    },
    removeItem(key: string) {
      values.delete(key);
    },
    key(index: number) {
      return [...values.keys()][index] ?? null;
    },
  };
}
