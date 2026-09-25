import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { accentNameOf, primaryValue, readTheme, resolveTheme } from "./theme";

describe("theme", () => {
  it("opens Dark when the core has not said, and Dark with a note for a value it does not know", () => {
    expect(readTheme(undefined)).toEqual({ choice: "dark", unknown: false });
    expect(readTheme("light")).toEqual({ choice: "light", unknown: false });
    expect(readTheme("sepia")).toEqual({ choice: "dark", unknown: true });
  });

  it("follows the OS only for System", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
    expect(resolveTheme("light", true)).toBe("light");
  });
});

describe("accent", () => {
  it("maps a stored choice to its per-theme token and draws any other hex as it is", () => {
    expect(accentNameOf("#B9FF66")).toBe("lime");
    expect(primaryValue("#7dd3fc")).toBe("var(--accent-choice-sky)");
    expect(primaryValue("#123456")).toBe("#123456");
    expect(primaryValue(null)).toBeNull();
  });
});

// B5: the primary button's text reads at WCAG AA (4.5:1) on every accent, in
// both themes. The expected ratio is the WCAG formula, applied to tokens.json.
describe("primary button contrast", () => {
  const tokens = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../design/tokens.json"), "utf8")).tokens as Record<string, { type: string; value: string; light?: string }>;
  const channel = (value: number) => {
    const c = value / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const luminance = (hex: string) => {
    const c = (i: number) => channel(parseInt(hex.slice(i, i + 2), 16));
    return 0.2126 * c(1) + 0.7152 * c(3) + 0.0722 * c(5);
  };
  const ratio = (a: string, b: string) => {
    const hi = Math.max(luminance(a), luminance(b));
    const lo = Math.min(luminance(a), luminance(b));
    return (hi + 0.05) / (lo + 0.05);
  };
  const token = (name: string) => {
    const found = tokens[name];
    if (!found?.light) throw new Error(`${name} is not a two-theme color`);
    return { dark: found.value, light: found.light };
  };
  const text = token("--primary-foreground");
  for (const name of ["lime", "sky", "violet", "amber"]) {
    const accent = token(`--accent-choice-${name}`);
    it(`${name} reads in Dark and Light`, () => {
      expect(ratio(text.dark, accent.dark)).toBeGreaterThanOrEqual(4.5);
      expect(ratio(text.light, accent.light)).toBeGreaterThanOrEqual(4.5);
    });
  }
  it("the destructive button's text reads in Dark and Light", () => {
    const fg = token("--destructive-foreground");
    const bg = token("--destructive");
    expect(ratio(fg.dark, bg.dark)).toBeGreaterThanOrEqual(4.5);
    expect(ratio(fg.light, bg.light)).toBeGreaterThanOrEqual(4.5);
  });
});
