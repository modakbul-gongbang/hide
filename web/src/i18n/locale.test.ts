import { describe, expect, it } from "vitest";
import { requireInterfaceLanguage, resolveInterfaceLanguage, systemLanguage } from "./locale";

describe("interface language policy", () => {
  it.each([
    ["en-GB", "en"], ["ko-KR", "ko"], ["ja-JP", "ja"],
    ["zh-CN", "zh-CN"], ["zh-Hans", "zh-CN"], ["zh-SG", "zh-CN"],
    ["zh-TW", "en"], ["zh-Hant", "en"], ["fr-FR", "en"],
  ])("resolves the primary system language %s as %s", (locale, language) => {
    expect(systemLanguage(locale).language).toBe(language);
  });

  it("keeps an explicit choice when the OS language changes", () => {
    expect(resolveInterfaceLanguage("en", "ko-KR")).toEqual({ language: "en", source: "preference" });
    expect(resolveInterfaceLanguage("ja", "en-US")).toEqual({ language: "ja", source: "preference" });
  });

  it("follows the OS only while no explicit choice is stored", () => {
    expect(resolveInterfaceLanguage(null, "ko-KR")).toEqual({ language: "ko", source: "system" });
    expect(resolveInterfaceLanguage(undefined, "ja-JP")).toEqual({ language: "ja", source: "system" });
  });

  it("uses English and names an invalid stored choice without using the OS", () => {
    expect(resolveInterfaceLanguage("fr", "ko-KR")).toEqual({ language: "en", source: "fallback", reason: "invalid_language_preference" });
    expect(() => requireInterfaceLanguage("fr")).toThrow("invalid_interface_language");
  });

  it.each(["not_a_locale", "", null, 42])("names an unusable OS value %s", (value) => {
    expect(systemLanguage(value)).toEqual({ language: "en", source: "fallback", reason: "invalid_system_language" });
  });
});
