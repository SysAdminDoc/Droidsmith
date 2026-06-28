import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "../locales/en.json";
import ru from "../locales/ru.json";

const resources = {
  en: { translation: en },
  ru: { translation: ru },
};

const browserLanguage =
  typeof navigator === "undefined" ? "en" : navigator.language;

i18n.use(initReactI18next).init({
  resources,
  lng: browserLanguage.startsWith("ru") ? "ru" : "en",
  fallbackLng: "en",
  interpolation: {
    escapeValue: false,
  },
});

export { resources };
export default i18n;
