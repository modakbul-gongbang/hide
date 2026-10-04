import { useLayoutEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useShellStore } from "../store";
import { initializeInterfaceI18n } from "./instance";
import { resolveInterfaceLanguage } from "./locale";

// One bounded, in-memory translator per page. Only the core owns a choice;
// neither the resolved system language nor the preference is cached locally.
const clientI18n = initializeInterfaceI18n(resolveInterfaceLanguage(null, navigator.language).language);

export function useInterfaceTranslation() {
  return useTranslation("translation", { i18n: clientI18n, useSuspense: false });
}

/** Follows confirmed snapshots, including reconnects and another client's edit. */
export function InterfaceLanguageBoundary() {
  const preference = useShellStore((state) => state.rest?.ui_state?.interface_language);
  const [systemLanguage, setSystemLanguage] = useState(navigator.language);
  useLayoutEffect(() => {
    const changed = () => setSystemLanguage(navigator.language);
    window.addEventListener("languagechange", changed);
    return () => window.removeEventListener("languagechange", changed);
  }, []);
  const language = resolveInterfaceLanguage(preference, systemLanguage).language;
  useLayoutEffect(() => {
    if (clientI18n.language !== language) void clientI18n.changeLanguage(language);
    document.documentElement.lang = language;
  }, [language]);
  return null;
}
