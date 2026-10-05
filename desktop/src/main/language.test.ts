import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { HostLanguage, languageFile } from "./language";
import { HostLog } from "./log";

let dir: string;
let log: HostLog;
const logged = () => fs.readFileSync(log.path, "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line) as { event: string; reason?: string });

beforeEach(() => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-desktop-language-"));
  log = new HostLog(path.join(dir, "logs"));
});
afterEach(() => fs.rmSync(dir, { recursive: true, force: true }));

describe("the host's language", () => {
  it("follows the system until a choice is confirmed, then keeps the choice across launches", () => {
    const file = languageFile(dir);
    const first = new HostLanguage(file, () => "ja-JP", log);
    expect(first.language).toBe("ja");
    expect(first.t("native.menu.file")).toBe("ファイル");
    expect(first.report("ko")).toBe("changed");
    expect(first.t("native.menu.file")).toBe("파일");
    expect(first.report("ko")).toBe("unchanged");
    expect(JSON.parse(fs.readFileSync(file, "utf8"))).toEqual({ schema: 1, interface_language: "ko" });
    expect(new HostLanguage(file, () => "en-US", log).language).toBe("ko");
    // Back to the system's language, which this host resolves for itself.
    expect(first.report(null)).toBe("changed");
    expect(first.language).toBe("ja");
    expect(new HostLanguage(file, () => "zh-Hans-CN", log).language).toBe("zh-CN");
  });

  it("refuses a value that is not one of the four languages and changes nothing", () => {
    const language = new HostLanguage(languageFile(dir), () => "en-US", log);
    for (const value of ["fr", "zh-TW", "", 4, {}, undefined]) expect(language.report(value)).toBe("refused");
    expect(language.language).toBe("en");
    expect(fs.existsSync(languageFile(dir))).toBe(false);
  });

  it("reads English for an unsupported or traditional-script system language, and logs a stored file it cannot use", () => {
    expect(new HostLanguage(languageFile(dir), () => "fr-FR", log).language).toBe("en");
    expect(new HostLanguage(languageFile(dir), () => "zh-Hant-TW", log).language).toBe("en");
    expect(new HostLanguage(languageFile(dir), () => undefined, log).language).toBe("en");
    fs.writeFileSync(languageFile(dir), JSON.stringify({ schema: 1, interface_language: "klingon" }));
    expect(new HostLanguage(languageFile(dir), () => "ja-JP", log).language).toBe("ja");
    fs.writeFileSync(languageFile(dir), JSON.stringify({ schema: 2, interface_language: "ko" }));
    expect(new HostLanguage(languageFile(dir), () => "ja-JP", log).language).toBe("ja");
    fs.writeFileSync(languageFile(dir), "{not json");
    expect(new HostLanguage(languageFile(dir), () => "ja-JP", log).language).toBe("ja");
    expect(logged().map((line) => [line.event, line.reason])).toEqual([
      ["language.stored_unusable", "value"],
      ["language.stored_unusable", "schema"],
      ["language.stored_unusable", "unreadable"],
    ]);
  });
});
