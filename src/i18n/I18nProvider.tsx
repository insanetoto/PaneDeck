import {
  createContext,
  type PropsWithChildren,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { enMessages, type MessageKey, zhCNMessages } from "./messages";

export type Language = "en" | "zh-CN";

export const LANGUAGE_STORAGE_KEY = "panedeck.language";

const catalogs: Record<Language, Record<MessageKey, string>> = {
  en: enMessages,
  "zh-CN": zhCNMessages,
};

interface I18nContextValue {
  language: Language;
  setLanguage: (language: Language) => void;
  t: (key: MessageKey) => string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

export function resolvePreferredLanguage(languages: readonly string[]): Language {
  return languages.some((language) => language.toLowerCase().startsWith("zh")) ? "zh-CN" : "en";
}

function readInitialLanguage(): Language {
  try {
    const savedLanguage = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
    if (savedLanguage === "en" || savedLanguage === "zh-CN") {
      return savedLanguage;
    }
  } catch {
    // The app remains usable when WebView storage is unavailable.
  }

  return resolvePreferredLanguage(window.navigator.languages);
}

export function I18nProvider({ children }: PropsWithChildren) {
  const [language, setLanguage] = useState<Language>(readInitialLanguage);
  const messages = catalogs[language];

  useEffect(() => {
    document.documentElement.lang = language;
    try {
      window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
    } catch {
      // Language switching must still work for the active session.
    }
  }, [language]);

  const t = useCallback((key: MessageKey) => messages[key], [messages]);
  const value = useMemo(() => ({ language, setLanguage, t }), [language, t]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const value = useContext(I18nContext);
  if (!value) {
    throw new Error("useI18n must be used inside I18nProvider");
  }
  return value;
}
