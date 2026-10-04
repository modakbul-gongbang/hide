import { describe, expect, it } from "vitest";
import { createInterfaceI18n, TranslationError } from "./instance";
import type { InterfaceLanguage } from "./locale";

describe("interface translator", () => {
  it.each([
    ["en", "Close"],
    ["ko", "닫기"],
    ["zh-CN", "关闭"],
    ["ja", "閉じる"],
  ] as const)("reads the selected %s catalog", async (language, expected) => {
    const instance = await createInterfaceI18n(language);
    expect(instance.t("common.close")).toBe(expected);
  });

  it("uses the numeric count for plurals and formats its visible value", async () => {
    const instance = await createInterfaceI18n("en");
    expect(instance.t("cleanup.selectedCells", { count: 1 })).toBe("1 cell");
    expect(instance.t("cleanup.selectedCells", { count: 2 })).toBe("2 cells");
    expect(instance.t("cleanup.selectedCells", { count: 1234 })).toBe("1,234 cells");
    expect(instance.t("settings.processValue", { pid: "1234", schema: "14" })).toBe("pid 1234 · schema 14");
  });

  it("preserves user names, paths and translation-like input literally", async () => {
    const instance = await createInterfaceI18n("en");
    const name = "내 문서/<img> {{secret}} $t(common.close) & $&";
    expect(instance.t("devices.installTitle", { name })).toBe(`Install Hide on ${name}?`);
  });

  it("keeps independently initialized clients separate", async () => {
    const english = await createInterfaceI18n("en");
    const korean = await createInterfaceI18n("ko");
    await korean.changeLanguage("ja");
    expect(korean.t("common.close")).toBe("閉じる");
    expect(english.t("common.close")).toBe("Close");
  });

  it("refuses unknown explicit languages without changing the current language", async () => {
    await expect(createInterfaceI18n("fr" as InterfaceLanguage)).rejects.toThrow("invalid_interface_language");
    const instance = await createInterfaceI18n("ko");
    expect(() => instance.changeLanguage("fr")).toThrow("invalid_interface_language");
    expect(() => instance.changeLanguage("ko-KR")).toThrow("invalid_interface_language");
    expect(instance.language).toBe("ko");
    expect(instance.t("common.close")).toBe("닫기");
  });

  it("raises missing keys and supplied defaults instead of displaying them", async () => {
    const instance = await createInterfaceI18n("ja");
    // @ts-expect-error An untyped caller cannot bypass runtime detection.
    expect(() => instance.t("unknown.user-content-key")).toThrow("missing_translation");
    // @ts-expect-error Defaults cannot make an unknown key valid.
    expect(() => instance.t("unknown.user-content-key", "Untranslated default")).toThrow("missing_translation");
  });

  it("never falls back to another catalog for a missing translation", async () => {
    const instance = await createInterfaceI18n("ja");
    instance.removeResourceBundle("ja", "translation");
    expect(() => instance.t("common.close")).toThrow("missing_translation");
  });

  it("raises missing variables without exposing already inserted user content", async () => {
    const instance = await createInterfaceI18n("en");
    expect(() => instance.t("devices.installTitle")).toThrow("missing_interpolation");
    try {
      // @ts-expect-error The source schema also rejects missing interpolations.
      instance.t("settings.backgroundDegraded", { agent: "private-user-content" });
      expect.fail("A missing status must fail");
    } catch (error) {
      expect(error).toBeInstanceOf(TranslationError);
      expect((error as Error).message).toBe("missing_interpolation");
    }
  });

  it("refuses invalid insertion values instead of displaying an empty or coerced value", async () => {
    const instance = await createInterfaceI18n("en");
    // @ts-expect-error An untyped boundary's null is also rejected at runtime.
    expect(() => instance.t("devices.installTitle", { name: null })).toThrow("invalid_interpolation");
    // @ts-expect-error The library schema rejects objects as text insertions.
    expect(() => instance.t("devices.installTitle", { name: { private: "user-content" } })).toThrow("invalid_interpolation");
    expect(() => instance.t("cleanup.selectedCells", { count: Infinity })).toThrow("invalid_localized_number");
  });
});
