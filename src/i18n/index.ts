import { createI18n } from "vue-i18n";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";

export const supportedLocales = [
  { code: "en", nativeName: "English", shortName: "EN" },
  { code: "zh-CN", nativeName: "简体中文", shortName: "中" },
] as const;

export type SupportedLocale = (typeof supportedLocales)[number]["code"];

const localeStorageKey = "routepilot.locale";

function isSupportedLocale(locale: string | null): locale is SupportedLocale {
  return supportedLocales.some((option) => option.code === locale);
}

function initialLocale(): SupportedLocale {
  if (typeof window !== "undefined") {
    try {
      const storedLocale = window.localStorage.getItem(localeStorageKey);
      if (isSupportedLocale(storedLocale)) return storedLocale;
    } catch {
      // Local storage can be unavailable in hardened browser contexts.
    }

    if (window.navigator.languages.some((locale) => locale.toLowerCase().startsWith("zh"))) {
      return "zh-CN";
    }
  }

  return "en";
}

export const i18n = createI18n({
  legacy: false,
  locale: initialLocale(),
  fallbackLocale: "en",
  messages: {
    en,
    "zh-CN": zhCN,
  },
});

export function setLocale(locale: SupportedLocale): void {
  i18n.global.locale.value = locale;

  if (typeof document !== "undefined") {
    document.documentElement.lang = locale;
  }

  if (typeof window !== "undefined") {
    try {
      window.localStorage.setItem(localeStorageKey, locale);
    } catch {
      // The in-memory choice still applies when persistence is unavailable.
    }
  }
}

export function translate(key: string, parameters?: Record<string, unknown>): string {
  return i18n.global.t(key, parameters ?? {});
}

const runtimeErrorKeys: Record<string, string> = {
  "The OpenVPN connection ended unexpectedly.": "errors.runtime.connectionEnded",
  "OpenVPN exited before the connection was established.": "errors.runtime.exitedEarly",
  "RoutePilot could not read the OpenVPN process status.": "errors.runtime.processStatus",
  "Authentication failed. This VPN client certificate was rejected by the server.":
    "errors.runtime.authentication",
};

export function translateRuntimeError(message: string | null): string | null {
  if (!message) return null;
  const key = runtimeErrorKeys[message];
  return key ? translate(key) : message;
}

setLocale(i18n.global.locale.value);
