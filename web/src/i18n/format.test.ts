import { describe, expect, it } from "vitest";
import { formatBytes, formatDateTime, formatGigabytes, formatNumber, formatPercent } from "./format";
import { INTERFACE_LANGUAGES } from "./locale";

describe("localized display values", () => {
  for (const language of INTERFACE_LANGUAGES) {
    it(`uses ${language} for numbers, dates and binary size values`, () => {
      expect(formatNumber(language, 12345)).toBe(new Intl.NumberFormat(language).format(12345));
      expect(formatPercent(language, 75)).toBe(new Intl.NumberFormat(language, { style: "percent", maximumFractionDigits: 0 }).format(0.75));
      const at = Date.UTC(2026, 9, 4, 10, 15);
      const options = { dateStyle: "medium", timeStyle: "short", timeZone: "UTC" } as const;
      expect(formatDateTime(language, at, options)).toBe(new Intl.DateTimeFormat(language, options).format(at));
      expect(formatBytes(language, 0)).toBe("0 B");
      expect(formatBytes(language, 1024)).toBe(`${new Intl.NumberFormat(language, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(1)} KB`);
      expect(formatBytes(language, 12 * 1024 ** 2)).toBe("12 MB");
      expect(formatBytes(language, 1008 * 1024)).toBe("1008 KB");
      expect(formatGigabytes(language, 12 * 1024 ** 3)).toBe(`${new Intl.NumberFormat(language, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(12)} GB`);
    });
  }

  it("rejects values that cannot describe a displayed size", () => {
    expect(() => formatBytes("en", -1)).toThrow("invalid_localized_size");
    expect(() => formatGigabytes("en", -1)).toThrow("invalid_localized_size");
    expect(() => formatBytes("en", Infinity)).toThrow("invalid_localized_number");
    expect(() => formatDateTime("en", 9e15, {})).toThrow("invalid_localized_date");
  });
});
