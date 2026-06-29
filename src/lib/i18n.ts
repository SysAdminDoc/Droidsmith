import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "../locales/en.json";
import ru from "../locales/ru.json";

export const LANGUAGE_STORAGE_KEY = "droidsmith.language";
export const SUPPORTED_LANGUAGES = [
  { code: "en", labelKey: "language.english" },
  { code: "ru", labelKey: "language.russian" },
] as const;

export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number]["code"];
type LanguageStorage = Pick<Storage, "getItem" | "setItem">;

const resources = {
  en: { translation: en },
  ru: { translation: ru },
};

i18n.use(initReactI18next).init({
  resources,
  lng: detectInitialLanguage(),
  fallbackLng: "en",
  supportedLngs: SUPPORTED_LANGUAGES.map((language) => language.code),
  interpolation: {
    escapeValue: false,
  },
});

export function normalizeLanguage(
  value: string | null | undefined,
): SupportedLanguage | null {
  if (!value) return null;
  const base = value.toLowerCase().split(/[-_]/)[0];
  return SUPPORTED_LANGUAGES.some((language) => language.code === base)
    ? (base as SupportedLanguage)
    : null;
}

export function readStoredLanguage(
  storage: LanguageStorage | null = getLanguageStorage(),
): SupportedLanguage | null {
  try {
    return normalizeLanguage(storage?.getItem(LANGUAGE_STORAGE_KEY));
  } catch {
    return null;
  }
}

export function persistLanguage(
  language: SupportedLanguage,
  storage: LanguageStorage | null = getLanguageStorage(),
): SupportedLanguage {
  try {
    storage?.setItem(LANGUAGE_STORAGE_KEY, language);
  } catch {
    // Persistence is best-effort; changing the runtime language still works.
  }
  return language;
}

export function detectInitialLanguage({
  storage = getLanguageStorage(),
  browserLanguage = typeof navigator === "undefined"
    ? undefined
    : navigator.language,
}: {
  storage?: LanguageStorage | null;
  browserLanguage?: string;
} = {}): SupportedLanguage {
  return (
    readStoredLanguage(storage) ?? normalizeLanguage(browserLanguage) ?? "en"
  );
}

function getLanguageStorage(): LanguageStorage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

export { resources };
export default i18n;
