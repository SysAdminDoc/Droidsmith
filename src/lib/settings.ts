import {
  callApplySettingsImport,
  callExportSettings,
  callGetSettingsMirrorPreset,
  callHasSettingsImportBackup,
  callInitializeSettings,
  callPreviewSettingsImport,
  callResetSettings,
  callResetSettingsMirrorPreset,
  callRestoreSettingsImportBackup,
  callSelectHostPath,
  callSetSettingsLanguage,
  callSetSettingsMirrorPreset,
  inTauri,
  type LegacySettingsImport,
  type SettingsExportResult,
  type SettingsImportMode,
  type SettingsImportPreview,
  type SettingsImportResult,
  type SettingsLanguage,
  type SettingsLoadResult,
  type SettingsMirrorPreset,
  type SettingsScope,
  type SettingsSnapshot,
} from "./tauri";
import {
  LANGUAGE_STORAGE_KEY,
  normalizeLanguage,
  type SupportedLanguage,
} from "./i18n";
import {
  DEFAULT_MIRROR_PRESET,
  LEGACY_MIRROR_PRESET_PREFIX,
  normalizePreset,
  presetStorageKey,
  type MirrorPreset,
} from "../routes/mirrorPresets";

type LegacyStorage = Pick<
  Storage,
  "getItem" | "setItem" | "removeItem" | "key" | "length"
>;

type LegacyCollection = {
  legacy: LegacySettingsImport;
  cleanupKeys: string[];
};

let initialization: Promise<SettingsLoadResult> | null = null;

export function initializeSettings(
  storage: LegacyStorage | null = getLegacyStorage(),
): Promise<SettingsLoadResult> {
  initialization ??= initializeSettingsOnce(storage);
  return initialization;
}

export async function setStoredLanguage(
  language: SupportedLanguage,
  storage: LegacyStorage | null = getLegacyStorage(),
): Promise<SettingsSnapshot | null> {
  if (!inTauri()) {
    storage?.setItem(LANGUAGE_STORAGE_KEY, language);
    return null;
  }
  await initializeSettings(storage);
  return callSetSettingsLanguage(language satisfies SettingsLanguage);
}

export async function loadStoredMirrorPreset(
  deviceIdentity: string,
  storage: LegacyStorage | null = getLegacyStorage(),
): Promise<MirrorPreset> {
  if (!inTauri()) {
    return readBrowserMirrorPreset(storage, deviceIdentity);
  }
  await initializeSettings(storage);
  const preset = await callGetSettingsMirrorPreset(deviceIdentity);
  return preset ? normalizePreset(preset) : DEFAULT_MIRROR_PRESET;
}

export async function saveStoredMirrorPreset(
  deviceIdentity: string,
  preset: MirrorPreset,
  storage: LegacyStorage | null = getLegacyStorage(),
): Promise<void> {
  const normalized = normalizePreset(preset);
  if (!inTauri()) {
    storage?.setItem(
      presetStorageKey(deviceIdentity),
      JSON.stringify(normalized),
    );
    return;
  }
  await initializeSettings(storage);
  await callSetSettingsMirrorPreset(
    deviceIdentity,
    normalized satisfies SettingsMirrorPreset,
  );
}

export async function resetStoredMirrorPreset(
  deviceIdentity: string,
  storage: LegacyStorage | null = getLegacyStorage(),
): Promise<void> {
  if (!inTauri()) {
    storage?.removeItem(presetStorageKey(deviceIdentity));
    return;
  }
  await initializeSettings(storage);
  await callResetSettingsMirrorPreset(deviceIdentity);
}

export async function resetStoredSettings(
  scope: SettingsScope,
  storage: LegacyStorage | null = getLegacyStorage(),
): Promise<SettingsSnapshot | null> {
  if (!inTauri()) {
    if (scope === "all" || scope === "language") {
      storage?.removeItem(LANGUAGE_STORAGE_KEY);
    }
    if (scope === "all" || scope === "mirror_presets") {
      for (const key of collectLegacySettings(storage).cleanupKeys) {
        if (key.startsWith(LEGACY_MIRROR_PRESET_PREFIX))
          storage?.removeItem(key);
      }
    }
    return null;
  }
  await initializeSettings(storage);
  return callResetSettings(scope);
}

export async function exportStoredSettings(
  scope: SettingsScope,
): Promise<SettingsExportResult | null> {
  if (!inTauri()) {
    throw new Error("Settings export requires the Droidsmith desktop runtime.");
  }
  await initializeSettings();
  const pathGrant = await callSelectHostPath(
    "settings_export",
    `droidsmith-settings-${scope.replaceAll("_", "-")}.json`,
  );
  if (!pathGrant) return null;
  return callExportSettings(scope, pathGrant.id);
}

export async function previewStoredSettingsImport(): Promise<SettingsImportPreview | null> {
  if (!inTauri()) {
    throw new Error("Settings import requires the Droidsmith desktop runtime.");
  }
  await initializeSettings();
  const pathGrant = await callSelectHostPath("settings_import");
  if (!pathGrant) return null;
  return callPreviewSettingsImport(pathGrant.id);
}

export async function applyStoredSettingsImport(
  importId: string,
  mode: SettingsImportMode,
): Promise<SettingsImportResult> {
  if (!inTauri()) {
    throw new Error("Settings import requires the Droidsmith desktop runtime.");
  }
  await initializeSettings();
  return callApplySettingsImport(importId, mode);
}

export async function restoreStoredSettingsImportBackup(): Promise<SettingsSnapshot> {
  if (!inTauri()) {
    throw new Error(
      "Settings restore requires the Droidsmith desktop runtime.",
    );
  }
  await initializeSettings();
  return callRestoreSettingsImportBackup();
}

export async function hasStoredSettingsImportBackup(): Promise<boolean> {
  if (!inTauri()) return false;
  await initializeSettings();
  return callHasSettingsImportBackup();
}

export function collectLegacySettings(
  storage: LegacyStorage | null,
): LegacyCollection {
  if (!storage) {
    return { legacy: { language: null, mirrorPresets: [] }, cleanupKeys: [] };
  }
  try {
    const language = storage.getItem(LANGUAGE_STORAGE_KEY);
    const cleanupKeys = language === null ? [] : [LANGUAGE_STORAGE_KEY];
    const mirrorPresets: NonNullable<LegacySettingsImport["mirrorPresets"]> =
      [];
    for (let index = 0; index < storage.length; index += 1) {
      const key = storage.key(index);
      if (!key?.startsWith(LEGACY_MIRROR_PRESET_PREFIX)) continue;
      const deviceIdentity = key.slice(LEGACY_MIRROR_PRESET_PREFIX.length);
      const rawValue = storage.getItem(key);
      if (!deviceIdentity || rawValue === null) continue;
      mirrorPresets.push({ deviceIdentity, rawValue });
      cleanupKeys.push(key);
    }
    mirrorPresets.sort((left, right) =>
      left.deviceIdentity.localeCompare(right.deviceIdentity),
    );
    cleanupKeys.sort();
    return {
      legacy: { language, mirrorPresets },
      cleanupKeys,
    };
  } catch {
    return { legacy: { language: null, mirrorPresets: [] }, cleanupKeys: [] };
  }
}

function readBrowserMirrorPreset(
  storage: LegacyStorage | null,
  deviceIdentity: string,
): MirrorPreset {
  try {
    const raw = storage?.getItem(presetStorageKey(deviceIdentity));
    if (!raw) return DEFAULT_MIRROR_PRESET;
    const parsed: unknown = JSON.parse(raw);
    return typeof parsed === "object" && parsed !== null
      ? normalizePreset(parsed as Partial<MirrorPreset>)
      : DEFAULT_MIRROR_PRESET;
  } catch {
    return DEFAULT_MIRROR_PRESET;
  }
}

async function initializeSettingsOnce(
  storage: LegacyStorage | null,
): Promise<SettingsLoadResult> {
  const collection = collectLegacySettings(storage);
  if (!inTauri()) {
    return {
      settings: {
        version: "1",
        language: normalizeLanguage(collection.legacy.language),
        mirrorPresetCount: collection.legacy.mirrorPresets?.length ?? 0,
      },
      recovery: "clean",
      legacyCleanupAllowed: false,
    };
  }
  const result = await callInitializeSettings(collection.legacy);
  if (result.legacyCleanupAllowed) {
    for (const key of collection.cleanupKeys) {
      try {
        storage?.removeItem(key);
      } catch {
        // Import is already durable; stale renderer keys can be retried later.
      }
    }
  }
  return result;
}

function getLegacyStorage(): LegacyStorage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

export function resetSettingsInitializationForTests(): void {
  initialization = null;
}
