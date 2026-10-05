import { ESLint } from "eslint";
import path from "node:path";
import { describe, expect, it } from "vitest";

// The recurrence guard of docs/LOCALIZATION.md lives in eslint.config.js; this
// proves it still fires, since a guard that quietly stops matching reads as clean.
const eslint = new ESLint({ cwd: path.resolve(__dirname, "../.."), overrideConfigFile: path.resolve(__dirname, "../../eslint.config.js") });

async function problems(source: string, filePath = "src/Probe.tsx"): Promise<string[]> {
  const [result] = await eslint.lintText(source, { filePath: path.resolve(__dirname, "../..", filePath) });
  return result!.messages.map((message) => message.ruleId ?? "parse");
}

describe("the hardcoded product text guard", () => {
  it("flags an English sentence a screen draws, as text and as an attribute a person reads", async () => {
    expect(await problems('export const Probe = () => <p>Nothing to show</p>;')).toEqual(["i18next/no-literal-string"]);
    expect(await problems('export const Probe = () => <input placeholder="Search projects" />;')).toEqual(["i18next/no-literal-string"]);
    expect(await problems('export const Probe = () => <button aria-label="Close sheet" />;')).toEqual(["i18next/no-literal-string"]);
  });

  it("flags Korean, Chinese and Japanese text in a string or template, in a helper as well as in a screen", async () => {
    for (const text of ["닫기", "关闭", "閉じる"]) {
      expect(await problems(`export const label = "${text}";`, "src/probe.ts")).toEqual(["no-restricted-syntax"]);
      expect(await problems(`export const label = (n: number) => \`${text} \${n}\`;`, "src/probe.ts")).toEqual(["no-restricted-syntax"]);
    }
  });

  it("lets a keycap, a product name, a symbol and a catalog lookup through", async () => {
    expect(await problems('export const Probe = () => <div><kbd>x</kbd><span>GitHub</span><span>→</span><p>{t("common.close")}</p></div>;')).toEqual([]);
  });

  it("does not apply to the catalogs themselves", async () => {
    expect(await problems('export const ko = { "common.close": "닫기" };', "src/i18n/resources/probe.ts")).toEqual([]);
  });
});
