/** The supported interface languages, shared by the shell and native host. */
export const INTERFACE_LANGUAGES = ["en", "ko", "zh-CN", "ja"] as const;
export type InterfaceLanguage = (typeof INTERFACE_LANGUAGES)[number];

/** Self-names remain readable when changing away from an unfamiliar language. */
export const LANGUAGE_NAMES: Readonly<Record<InterfaceLanguage, string>> = {
  en: "English",
  ko: "한국어",
  "zh-CN": "简体中文",
  ja: "日本語",
};

export type LanguageResolution = {
  language: InterfaceLanguage;
  source: "preference" | "system" | "fallback";
  /** A boundary can record this code without logging the supplied value. */
  reason?: "unsupported_system_language" | "invalid_system_language" | "invalid_language_preference";
};

export function isInterfaceLanguage(value: unknown): value is InterfaceLanguage {
  return typeof value === "string" && INTERFACE_LANGUAGES.some((language) => language === value);
}

/** A new explicit choice must be one of the offered values. */
export function requireInterfaceLanguage(value: unknown): InterfaceLanguage {
  if (!isInterfaceLanguage(value)) throw new Error("invalid_interface_language");
  return value;
}

/**
 * Read the first OS preference, rather than searching later preferences for
 * a supported language. An unsupported primary language means English.
 * Chinese supports the Simplified script only, including the OS's zh-Hans
 * spelling; zh-TW and zh-Hant do not silently become Simplified Chinese.
 */
export function systemLanguage(locale: unknown): LanguageResolution {
  if (typeof locale !== "string" || locale.trim() === "") {
    return { language: "en", source: "fallback", reason: "invalid_system_language" };
  }
  let parsed: Intl.Locale;
  try {
    parsed = new Intl.Locale(locale);
  } catch {
    return { language: "en", source: "fallback", reason: "invalid_system_language" };
  }
  const language = parsed.language;
  if (language === "en" || language === "ko" || language === "ja") {
    return { language, source: "system" };
  }
  if (language === "zh" && parsed.maximize().script === "Hans") {
    return { language: "zh-CN", source: "system" };
  }
  return { language: "en", source: "fallback", reason: "unsupported_system_language" };
}

/**
 * The core owns one explicit preference for all connected shells and
 * phones. Until that preference exists, each client follows its own primary
 * OS/browser language without saving a default into the core.
 * A stored explicit choice wins, including English on a Korean OS. Unknown persisted values
 * take the required English fallback and name the invalid state for the
 * boundary's diagnostic; they never clear the preference silently.
 */
export function resolveInterfaceLanguage(preference: unknown, locale: unknown): LanguageResolution {
  if (preference === null || preference === undefined) return systemLanguage(locale);
  if (isInterfaceLanguage(preference)) return { language: preference, source: "preference" };
  return { language: "en", source: "fallback", reason: "invalid_language_preference" };
}
