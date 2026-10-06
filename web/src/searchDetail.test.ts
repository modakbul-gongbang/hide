import { describe, expect, it } from "vitest";
import { READ_AT, RICH } from "./gallery/cmdkSceneData";
import { initializeInterfaceI18n } from "./i18n/instance";
import { searchEntries } from "./search";
import { detailOf } from "./searchDetail";

const now = READ_AT + 5 * 60_000;

describe("the palette's detail pane in the operator's language", () => {
  it("words a checkout's facts and action", () => {
    const { t } = initializeInterfaceI18n("ko");
    const entry = searchEntries(RICH, t).find((row) => row.id === "checkout:c-sand")!;
    const detail = detailOf(RICH, entry, now, t, "ko");
    expect(detail.kind).toBe("checkout");
    expect(detail.facts).toEqual([
      ["변경", "base 이후 커밋 1개 · 변경 파일 14개"],
      ["PR", "#275"],
      ["이슈", "#273"],
    ]);
    expect(detail.action).toBe("checkout 열기");
  });

  it("words a pull request's state, facts and action", () => {
    const { t } = initializeInterfaceI18n("ja");
    const entry = searchEntries(RICH, t).find((row) => row.id === "pr:w1:275")!;
    const detail = detailOf(RICH, entry, now, t, "ja");
    expect(detail.kind).toBe("プルリクエスト");
    expect(detail.facts.map(([label]) => label)).toEqual(["ブランチ", "閉じる課題", "読み込み"]);
    expect(detail.facts.at(-1)?.[1]).toBe("5 分前に読み込み");
    expect(detail.action).toBe("PR ビューで開く");
  });

  it("counts a pull request's recorded sessions and leaves the line out at none", () => {
    const { t } = initializeInterfaceI18n("ko");
    const entry = searchEntries(RICH, t).find((row) => row.id === "pr:w1:275")!;
    const links = { projects: { w1: { prs: { "275": 3 } } }, filling: false };
    expect(detailOf(RICH, entry, now, t, "ko", links).facts).toContainEqual(["세션", "3"]);
    expect(detailOf(RICH, entry, now, t, "ko", { projects: {}, filling: false }).facts.map(([label]) => label)).not.toContain("세션");
  });
});
