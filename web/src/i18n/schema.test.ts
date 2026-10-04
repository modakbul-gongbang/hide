import { describe, expect, it } from "vitest";
import { validateCatalogs, type Catalogs } from "./schema";

const en = { open: "Open {{name}}", panes_one: "{{count}} pane", panes_other: "{{count}} panes", close: "Close" } as const;
const catalogs = {
  en,
  ko: { open: "{{name}} 열기", panes_one: "pane {{count}}개", panes_other: "pane {{count}}개", close: "닫기" },
  "zh-CN": { open: "打开 {{name}}", panes_one: "{{count}} 个窗格", panes_other: "{{count}} 个窗格", close: "关闭" },
  ja: { open: "{{name}}を開く", panes_one: "{{count}} 個のペイン", panes_other: "{{count}} 個のペイン", close: "閉じる" },
} satisfies Catalogs<typeof en>;

describe("four-language catalog boundary", () => {
  it("accepts a complete catalog with the same interpolation variables", () => {
    expect(() => validateCatalogs(en, catalogs)).not.toThrow();
  });

  it("refuses a missing or empty translation instead of showing English", () => {
    const missing = structuredClone(catalogs) as unknown as Record<string, Record<string, unknown>>;
    delete missing.ja!.close;
    expect(() => validateCatalogs(en, missing as unknown as Catalogs<typeof en>)).toThrow("catalog_keys:ja");
    const empty = structuredClone(catalogs);
    empty.ko.close = "";
    expect(() => validateCatalogs(en, empty)).toThrow("message_empty:ko:close");
  });

  it("refuses an extra key in any language", () => {
    const extra = { ...catalogs, en: { ...en, unexpected: "Unexpected" } };
    expect(() => validateCatalogs(en, extra)).toThrow("catalog_keys:en");
  });

  it("refuses a placeholder changed by a translation", () => {
    const mismatch = structuredClone(catalogs);
    mismatch["zh-CN"].open = "打开 {{title}}";
    expect(() => validateCatalogs(en, mismatch)).toThrow("message_placeholders:zh-CN:open");
  });

  it("refuses malformed interpolation syntax before rendering", () => {
    const malformed = structuredClone(catalogs);
    malformed.ja.open = "{{name}を開く";
    expect(() => validateCatalogs(en, malformed)).toThrow("message_placeholders:ja:open");
  });

  it("requires all four supported languages", () => {
    const { ko: _omitted, ...missing } = catalogs;
    void _omitted;
    expect(() => validateCatalogs(en, missing as unknown as Catalogs<typeof en>)).toThrow("catalog_languages");
  });
});
