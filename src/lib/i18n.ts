import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import languageContract from "../../language-contract.json";
import de from "../locales/de.json";
import en from "../locales/en.json";
import es from "../locales/es.json";
import ru from "../locales/ru.json";
import zh from "../locales/zh.json";
import type { SettingsLanguage } from "./bindings";

export const LANGUAGE_STORAGE_KEY = "droidsmith.language";
export type SupportedLanguage = SettingsLanguage;
type LanguageMetadata = {
  code: SupportedLanguage;
  labelKey: `language.${string}`;
  dir: "ltr" | "rtl";
  locale: string;
};

// language-contract.json is the runtime source used by the renderer. The
// release gate proves that its codes also match the Rust enum, isolation
// allowlist, locale resources, and selector metadata before a build can ship.
export const SUPPORTED_LANGUAGES =
  languageContract.languages as readonly LanguageMetadata[];
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
  options?: Intl.NumberFormatOptions,
): string {
  return new Intl.NumberFormat(
    languageMetadata(language).locale,
    options,
  ).format(value);
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
