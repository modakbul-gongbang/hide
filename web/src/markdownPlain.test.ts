import { describe, expect, it } from "vitest";
import { markdownPlainText } from "./markdownPlain";

describe("markdownPlainText", () => {
  it("keeps the words and drops the marks and the blank lines", () => {
    expect(markdownPlainText("## 배경\n\n출처를 **어댑터**로 나눈다.\n\n- GitHub\n- [x] `Local`\n> 인용 [링크](https://example.invalid)")).toBe(
      "배경\n출처를 어댑터로 나눈다.\nGitHub\nLocal\n인용 링크",
    );
  });
});
