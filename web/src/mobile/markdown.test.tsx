import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { renderMarkdown } from "./markdown";

const html = (text: string) => renderToStaticMarkup(<>{renderMarkdown(text)}</>);

describe("renderMarkdown", () => {
  it("draws the syntax an agent writes and drops its marks", () => {
    const out = html("## 결과\n\n**굵게**, *기울임*, `code`, ~~취소~~\n\n- 하나\n- 둘\n\n1. 첫째\n2. 둘째\n\n> 인용\n\n```ts\nconst a = 1;\n```");
    expect(out).toContain('<p class="whitespace-pre-line font-semibold">결과</p>');
    expect(out).toContain('<strong class="font-semibold">굵게</strong>');
    expect(out).toContain("<em>기울임</em>");
    expect(out).toContain('<code class="rounded-xs bg-secondary px-xxs font-mono">code</code>');
    expect(out).toContain("<s>취소</s>");
    expect(out).toMatch(/<ul[^>]*><li[^>]*><p[^>]*>하나<\/p><\/li><li[^>]*><p[^>]*>둘<\/p><\/li><\/ul>/);
    expect(out).toMatch(/<ol start="1"[^>]*>/);
    expect(out).toContain("인용</p></blockquote>");
    expect(out).toMatch(/<pre[^>]*>const a = 1;<\/pre>/);
    expect(out).not.toMatch(/\*\*|##|```|~~/);
  });

  it("keeps a link and an image as text and never a target", () => {
    const out = html("[문서](https://example.com/docs) ![그림](a.png) <https://example.com/auto> https://example.com/bare");
    expect(out).toContain("<span>문서</span>");
    expect(out).toContain("<span>그림</span>");
    expect(out).toContain("https://example.com/auto");
    expect(out).toContain("https://example.com/bare");
    expect(out).not.toContain("example.com/docs");
    expect(out).not.toContain("<a");
    expect(out).not.toContain("<img");
  });

  it("draws a table as rows of cells and escapes raw HTML", () => {
    const out = html("| 이름 | 값 |\n| --- | --- |\n| a | **1** |\n\n<script>alert(1)</script>");
    expect(out).toMatch(/<th[^>]*>이름<\/th><th[^>]*>값<\/th>/);
    expect(out).toMatch(/<td[^>]*>a<\/td><td[^>]*><strong[^>]*>1<\/strong><\/td>/);
    expect(out).not.toContain("---");
    expect(out).toContain("&lt;script&gt;");
  });
});
