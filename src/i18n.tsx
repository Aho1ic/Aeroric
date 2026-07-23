import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { en } from "./i18n/en";
import { zh } from "./i18n/zh";

export type AppLanguage = "en" | "zh";

type TranslationParams = Record<string, string | number>;

interface I18nContextValue {
  language: AppLanguage;
  setLanguage: (language: AppLanguage) => void;
  t: (key: string, params?: TranslationParams) => string;
}

const LANGUAGE_STORAGE_KEY = "aeroric:language";

const translations: Record<AppLanguage, Record<string, string>> = {
  en,
  zh,
};

const I18nContext = createContext<I18nContextValue | null>(null);

function interpolate(template: string, params?: TranslationParams): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, key) => {
    const value = params[key];
    return value === undefined ? match : String(value);
  });
}

function normalizeLanguage(value: string | null | undefined): AppLanguage | null {
  if (value === "en" || value === "zh") return value;
  return null;
}

function getInitialLanguage(): AppLanguage {
  const stored = normalizeLanguage(localStorage.getItem(LANGUAGE_STORAGE_KEY));
  if (stored) return stored;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

export function I18nProvider({ children }: { children: React.ReactNode }) {
  const [language, setLanguageState] = useState<AppLanguage>(getInitialLanguage);

  const setLanguage = useCallback((nextLanguage: AppLanguage) => {
    setLanguageState(nextLanguage);
  }, []);

  useEffect(() => {
    localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
    document.documentElement.lang = language === "zh" ? "zh-CN" : "en";
  }, [language]);

  const t = useCallback(
    (key: string, params?: TranslationParams) => {
      const template = translations[language][key] ?? translations.en[key] ?? key;
      return interpolate(template, params);
    },
    [language],
  );

  const value = useMemo(() => ({ language, setLanguage, t }), [language, setLanguage, t]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error("useI18n must be used within I18nProvider");
  }
  return context;
}

export function pluralKey(singularKey: string, pluralKeyValue: string, count: number): string {
  return count === 1 ? singularKey : pluralKeyValue;
}
