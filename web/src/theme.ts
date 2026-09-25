// The web shell's theme and accent, decided from the core's ui_state (PRD
// web-design-system-reset D-11, D-14, D-15). Nothing here touches the DOM, so
// every rule is tested without a browser; `App.tsx` applies the result.

import { ACCENT_STORED_HEX } from "./generated/accents";

export type ThemeChoice = "system" | "light" | "dark";
export type Theme = "light" | "dark";

export const THEME_CHOICES: readonly { id: ThemeChoice; label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

/**
 * The stored choice when the page knows it. Absent (a core older than the
 * setting, or no snapshot yet) is Dark with nothing to report; a value the
 * page does not know is Dark and worth a diagnostic.
 */
export function readTheme(value: unknown): { choice: ThemeChoice; unknown: boolean } {
  if (value === undefined || value === null) return { choice: "dark", unknown: false };
  if (value === "system" || value === "light" || value === "dark") return { choice: value, unknown: false };
  return { choice: "dark", unknown: true };
}

/** What the page draws: System follows the OS appearance. */
export function resolveTheme(choice: ThemeChoice, prefersDark: boolean): Theme {
  if (choice === "system") return prefersDark ? "dark" : "light";
  return choice;
}

export type AccentName = keyof typeof ACCENT_STORED_HEX;

/** The accent choice a stored hex names, or null for a value no choice stores. */
export function accentNameOf(hex: string | null): AccentName | null {
  if (!hex) return null;
  const lower = hex.toLowerCase();
  const found = (Object.entries(ACCENT_STORED_HEX) as [AccentName, string][]).find(([, stored]) => stored === lower);
  return found ? found[0] : null;
}

/**
 * The value `--primary` takes: a known choice follows its per-theme token, so
 * Light draws Light's Lime; any other stored hex is drawn as it is in both
 * themes, as before the Light theme existed; no stored accent keeps the default.
 */
export function primaryValue(hex: string | null): string | null {
  if (!hex) return null;
  const name = accentNameOf(hex);
  return name ? `var(--accent-choice-${name})` : hex;
}
