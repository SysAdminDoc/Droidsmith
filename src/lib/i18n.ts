import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import de from "../locales/de.json";
import en from "../locales/en.json";
import es from "../locales/es.json";
import ru from "../locales/ru.json";
import zh from "../locales/zh.json";

export const LANGUAGE_STORAGE_KEY = "droidsmith.language";
export const SUPPORTED_LANGUAGES = [
  { code: "de", labelKey: "language.german", dir: "ltr", locale: "de-DE" },
  { code: "en", labelKey: "language.english", dir: "ltr", locale: "en-US" },
  { code: "es", labelKey: "language.spanish", dir: "ltr", locale: "es-ES" },
  { code: "ru", labelKey: "language.russian", dir: "ltr", locale: "ru-RU" },
  { code: "zh", labelKey: "language.chinese", dir: "ltr", locale: "zh-CN" },
] as const;

export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number]["code"];
type LanguageStorage = Pick<Storage, "getItem">;

const resources = {
  de: { translation: de },
  en: { translation: en },
  es: { translation: es },
  ru: { translation: ru },
  zh: { translation: zh },
};

const initialLanguage = detectInitialLanguage();

i18n.use(initReactI18next).init({
  resources,
  lng: initialLanguage,
  fallbackLng: "en",
  supportedLngs: SUPPORTED_LANGUAGES.map((language) => language.code),
  interpolation: {
    escapeValue: false,
    alwaysFormat: true,
    format(value, _format, language) {
      return typeof value === "number"
        ? formatNumber(value, language)
        : String(value);
    },
  },
});

applyDocumentLanguage(initialLanguage);
i18n.on("languageChanged", applyDocumentLanguage);

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

export function languageMetadata(value: string | null | undefined) {
  const language = normalizeLanguage(value) ?? "en";
  return SUPPORTED_LANGUAGES.find((candidate) => candidate.code === language)!;
}

export function formatNumber(
  value: number,
  language: string | null | undefined,
): string {
  return new Intl.NumberFormat(languageMetadata(language).locale).format(value);
}

export function formatDateTime(
  value: string,
  language: string | null | undefined,
): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(languageMetadata(language).locale, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(date);
}

export function applyDocumentLanguage(
  value: string | null | undefined,
  root: Pick<HTMLElement, "lang" | "dir"> | null = getDocumentRoot(),
): void {
  if (!root) return;
  const language = languageMetadata(value);
  root.lang = language.code;
  root.dir = language.dir;
}

function getLanguageStorage(): LanguageStorage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

function getDocumentRoot(): HTMLElement | null {
  return typeof document === "undefined" ? null : document.documentElement;
}

export { resources };
export default i18n;
