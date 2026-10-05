// The language the host draws its own words in (the app menu, the status
// page, the native dialogs). The core owns the one explicit choice
// (docs/LOCALIZATION.md); the host keeps only the last one the shell
// confirmed, in its profile, so the status page and the menu are right
// before any daemon answers, and resolves "no choice" on its own system.

import path from "node:path";
import type { i18n, TFunction } from "i18next";
import { initializeInterfaceI18n } from "../../../web/src/i18n/instance";
import { isInterfaceLanguage, resolveInterfaceLanguage, type InterfaceLanguage } from "../../../web/src/i18n/locale";
import { readJsonFile, writeJsonFile } from "./jsonFile";
import type { HostLog } from "./log";

export const LANGUAGE_SCHEMA = 1;

export function languageFile(userDataDir: string): string {
  return path.join(userDataDir, "interface-language.json");
}

/** The stored explicit choice; null when there is none or the file cannot be used (the log says which). */
function readChoice(file: string, log: HostLog): InterfaceLanguage | null {
  const stored = readJsonFile(file);
  if (stored === null) return null;
  const record = stored as { unreadable?: string; schema?: unknown; interface_language?: unknown };
  if (typeof record !== "object" || record.unreadable !== undefined || record.schema !== LANGUAGE_SCHEMA) {
    log.event("language.stored_unusable", { reason: typeof record === "object" && record.unreadable !== undefined ? "unreadable" : "schema" });
    return null;
  }
  if (record.interface_language === null) return null;
  if (!isInterfaceLanguage(record.interface_language)) {
    log.event("language.stored_unusable", { reason: "value" });
    return null;
  }
  return record.interface_language;
}

export type LanguageReport = "changed" | "unchanged" | "refused";

export class HostLanguage {
  private choice: InterfaceLanguage | null;
  private translator: i18n | null = null;

  constructor(
    private readonly file: string,
    /** The operating system's primary language, read when a word is drawn (it needs the app ready). */
    private readonly systemLanguage: () => string | undefined,
    private readonly log: HostLog,
  ) {
    this.choice = readChoice(file, log);
  }

  get language(): InterfaceLanguage {
    return resolveInterfaceLanguage(this.choice, this.systemLanguage()).language;
  }

  /** A translator in the language now in effect; ask again after a change. */
  get t(): TFunction<"translation"> {
    const language = this.language;
    if (this.translator === null) this.translator = initializeInterfaceI18n(language);
    else if (this.translator.language !== language) void this.translator.changeLanguage(language);
    return this.translator.getFixedT(null, "translation");
  }

  /**
   * What the shell confirmed: one of the four languages, or null for the
   * system's. Anything else is refused and changes nothing.
   */
  report(value: unknown): LanguageReport {
    if (value !== null && !isInterfaceLanguage(value)) return "refused";
    const before = this.language;
    if (value !== this.choice) {
      this.choice = value;
      try {
        writeJsonFile(this.file, { schema: LANGUAGE_SCHEMA, interface_language: value });
      } catch (error) {
        this.log.event("language.write_failed", { detail: String(error) });
      }
    }
    return before === this.language ? "unchanged" : "changed";
  }
}
