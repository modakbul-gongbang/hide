import { describe, expect, it } from "vitest";
import type { CellRow } from "./selection";
import { linkCandidates, osc8Target, parseToken, type LinkCandidate } from "./terminalLinks";

/** A row of `width` cells holding `text`, padded with written spaces the way a Herdr frame draws it. */
function row(text: string, width: number): CellRow {
  const cells: CellRow = [];
  for (const char of text) {
    cells.push(char);
    // Hangul takes two columns; xterm keeps an empty trailing half.
    if (/\p{Script=Hangul}/u.test(char)) cells.push(null);
  }
  while (cells.length < width) cells.push(" ");
  return cells;
}

const texts = (groups: LinkCandidate[][]) => groups.map((group) => group.map((candidate) => candidate.text));

describe("parseToken", () => {
  it("reads a URL and drops the punctuation around it", () => {
    expect(parseToken("(https://example.com/a_(b)).")).toMatchObject({ text: "https://example.com/a_(b)", lead: 1, trail: 2, target: { kind: "url", url: "https://example.com/a_(b)" } });
    expect(parseToken("<https://example.com/x>,")?.target).toEqual({ kind: "url", url: "https://example.com/x" });
  });

  it("reads a path with the location compilers and agents write after it", () => {
    expect(parseToken("src/main.rs:10:5")?.target).toEqual({ kind: "path", path: "src/main.rs", line: 10, column: 5 });
    expect(parseToken("web/src/a.ts:42:")?.target).toEqual({ kind: "path", path: "web/src/a.ts", line: 42, column: null });
    expect(parseToken("src/a.ts(12,3):")?.target).toEqual({ kind: "path", path: "src/a.ts", line: 12, column: 3 });
    expect(parseToken("docs/README.md#L7-L9")?.target).toEqual({ kind: "path", path: "docs/README.md", line: 7, column: null });
    expect(parseToken("`README.md:3`")?.target).toEqual({ kind: "path", path: "README.md", line: 3, column: null });
  });

  it("takes a tool header's argument out of its parentheses", () => {
    expect(parseToken("Update(web/src/terminals.ts)")).toMatchObject({ text: "web/src/terminals.ts", lead: 7, trail: 1 });
  });

  it("reads absolute, home, relative and bare file names, Hangul included", () => {
    expect(parseToken("/tmp/out.png")?.target).toMatchObject({ kind: "path", path: "/tmp/out.png" });
    expect(parseToken("~/notes/할일.md")?.target).toMatchObject({ kind: "path", path: "~/notes/할일.md" });
    expect(parseToken("../x/y")?.target).toMatchObject({ kind: "path", path: "../x/y" });
    expect(parseToken(".gitignore")?.target).toMatchObject({ kind: "path", path: ".gitignore" });
    expect(parseToken("Cargo.lock")?.target).toMatchObject({ kind: "path", path: "Cargo.lock" });
  });

  it("refuses what only looks like a path: versions, shortened paths, schemes, prose", () => {
    for (const token of ["v1.2.3", "1.5", "…/terminals.ts", "mailto:a@b.c", "/", "and", "--flag", "ssh://host/x"]) {
      expect(parseToken(token), token).toBeNull();
    }
  });
});

describe("osc8Target", () => {
  it("reads http(s), file and the vscode://file links Codex writes", () => {
    expect(osc8Target("https://example.com")).toEqual({ kind: "url", url: "https://example.com/" });
    expect(osc8Target("file:///tmp/a%20b.txt")).toEqual({ kind: "path", path: "/tmp/a b.txt", line: null, column: null });
    expect(osc8Target("vscode://file/Users/example/p/src/a.ts:12:4")).toEqual({ kind: "path", path: "/Users/example/p/src/a.ts", line: 12, column: 4 });
  });

  it("leaves any other address alone and refuses a file on another host", () => {
    expect(osc8Target("mailto:a@example.com")).toBeUndefined();
    expect(osc8Target("cursor://file/x")).toBeUndefined();
    expect(osc8Target("file://server/share/x")).toBeNull();
  });
});

describe("linkCandidates", () => {
  it("finds each token of the hovered row with its columns", () => {
    const rows = [row("see src/a.ts:3 and https://example.com.", 60)];
    const groups = linkCandidates(rows, 0);
    expect(texts(groups)).toEqual([["src/a.ts:3", "src/a.ts:3"], ["https://example.com"]]);
    expect(groups[0]![0]!.spans).toEqual([{ row: 0, start: 4, end: 14 }]);
    expect(groups[1]![0]!.spans).toEqual([{ row: 0, start: 19, end: 38 }]);
  });

  it("joins a path the terminal wrapped at its last column, from either row", () => {
    const rows = [row("open web/src/termi", 18), row("nals.ts now", 18)];
    const fromTop = linkCandidates(rows, 0);
    const fromBottom = linkCandidates(rows, 1);
    expect(texts(fromTop)).toEqual([["web/src/terminals.ts", "web/src/termi"]]);
    // The lone second half is offered too, for when the joined path does not exist.
    expect(texts(fromBottom)[0]).toEqual(["web/src/terminals.ts", "nals.ts"]);
    expect(fromTop[0]![0]!.spans).toEqual([
      { row: 0, start: 5, end: 18 },
      { row: 1, start: 0, end: 7 },
    ]);
  });

  it("joins a path a TUI broke at its own margin and indented, and leaves the check to decide", () => {
    const rows = [row("⏺ Edited /Users/example/proj/web/src/ter", 50), row("  minalLinks.ts", 50)];
    expect(texts(linkCandidates(rows, 1))).toEqual([["/Users/example/proj/web/src/terminalLinks.ts", "minalLinks.ts"]]);
  });

  it("keeps a row that ends short apart from a next row at the left edge, as `ls` prints them", () => {
    const rows = [row("src/", 20), row("main.rs", 20)];
    expect(texts(linkCandidates(rows, 0))).toEqual([["src/"]]);
    expect(texts(linkCandidates(rows, 1))).toEqual([["main.rs"]]);
  });

  it("places a tool header's argument on the row it lands on when the wrap cut the header", () => {
    const rows = [row("see Upd", 7), row("ate(web/src/a.ts)", 30)];
    const [group] = linkCandidates(rows, 1);
    const argument = group!.find((candidate) => candidate.text === "web/src/a.ts")!;
    expect(argument.spans).toEqual([{ row: 1, start: 4, end: 16 }]);
  });

  it("joins a URL only across a row that reaches the last column", () => {
    const full = [row("https://example.com/very/lo", 27), row("ng/path", 27)];
    expect(texts(linkCandidates(full, 0))[0]).toEqual(["https://example.com/very/long/path", "https://example.com/very/lo"]);
    const margin = [row("https://example.com/a", 40), row("  next", 40)];
    expect(texts(linkCandidates(margin, 0))).toEqual([["https://example.com/a"]]);
  });

  it("reads a wide glyph as one character and never splits it", () => {
    const rows = [row("열기 docs/한글.md 끝", 30)];
    const [group] = linkCandidates(rows, 0);
    expect(group![0]!.text).toBe("docs/한글.md");
    expect(group![0]!.spans).toEqual([{ row: 0, start: 5, end: 17 }]);
  });

  it("stops at the frames and marks a TUI draws around text", () => {
    const rows = [row("│ ⎿  src/a.ts │", 20)];
    expect(texts(linkCandidates(rows, 0))).toEqual([["src/a.ts"]]);
  });

  it("offers nothing for a row of prose", () => {
    expect(linkCandidates([row("nothing to open here", 30)], 0)).toEqual([]);
  });
});


describe("bounded Korean interpretations", () => {
  it("uses a finite particle boundary and keeps the original spelling", () => {
    const particles = ["에", "에서", "에게", "께", "으로", "로", "와", "과", "을", "를", "은", "는", "이", "가", "의", "도", "만", "부터", "까지"];
    for (const closer of [")", "]", "}", '"', "'", "`"]) {
      for (const particle of particles.flatMap((base) => [base, ...["도", "만", "는", "은"].map((extra) => base + extra)])) {
        const token = "docs/한글.md" + closer + particle;
        const [group] = linkCandidates([row(token, 40)], 0);
        expect(group!.length).toBeLessThanOrEqual(6);
        expect(group![0]!.target).toMatchObject({ path: token, line: null });
        expect(group!.at(-1)!.target).toMatchObject({ path: "docs/한글.md" });
        expect(group!.at(-1)!.spans).toEqual([{ row: 0, start: 0, end: 12 }]);
      }
    }
  });

  it("preserves all six composite spellings, including raw leading and closing punctuation", () => {
    const [group] = linkCandidates([row("(src/a.ts:12:5)에", 40)], 0);
    expect(group!.map((candidate) => [candidate.text, candidate.target])).toEqual([
      ["(src/a.ts:12:5)에", { kind: "path", path: "(src/a.ts:12:5)에", line: null, column: null }],
      ["src/a.ts:12:5)에", { kind: "path", path: "src/a.ts:12:5)에", line: null, column: null }],
      ["(src/a.ts:12:5)", { kind: "path", path: "(src/a.ts:12:5)", line: null, column: null }],
      ["src/a.ts:12:5)", { kind: "path", path: "src/a.ts:12:5)", line: null, column: null }],
      ["src/a.ts:12:5", { kind: "path", path: "src/a.ts:12:5", line: null, column: null }],
      ["src/a.ts:12:5", { kind: "path", path: "src/a.ts", line: 12, column: 5 }],
    ]);
    expect(group![2]!.spans).toEqual([{ row: 0, start: 0, end: 15 }]);
    expect(group![4]!.spans).toEqual([{ row: 0, start: 1, end: 14 }]);
  });

  it("does not truncate unsupported words or unbounded particle chains", () => {
    for (const token of ["docs/a.md)한국어", "docs/a.md)에서라도", "docs/a.md)에도만", "docs/a.md에서도"]) {
      const [group] = linkCandidates([row(token, 40)], 0);
      expect(group!.map((candidate) => candidate.target)).toEqual([{ kind: "path", path: token, line: null, column: null }]);
    }
  });

  it("excludes wrapping punctuation and wide particles from actual cell ranges", () => {
    const [group] = linkCandidates([row("보드 보기 (docs/README.md)에 끝", 50)], 0);
    const path = group!.find((candidate) => candidate.text === "docs/README.md")!;
    expect(path.spans).toEqual([{ row: 0, start: 11, end: 25 }]);
    const wrapped = [row("한글 (docs/한", 13), row("글.md)에서도 끝", 20)];
    const candidate = linkCandidates(wrapped, 1)[0]!.find((each) => each.text === "docs/한글.md")!;
    expect(candidate.spans).toEqual([{ row: 0, start: 6, end: 13 }, { row: 1, start: 0, end: 5 }]);
  });

  it("maps surrogate pairs and combining sequences to whole cells", () => {
    const cells: CellRow = ["앞", null, " ", "(", ..."docs/", "😀", null, "e\u0301", ".", "m", "d", ")", "에", null];
    const path = linkCandidates([cells], 0)[0]!.find((each) => each.text === "docs/😀e\u0301.md")!;
    expect(path.spans).toEqual([{ row: 0, start: 4, end: 15 }]);
  });

  it("bounds every joined spelling to six interpretations and every hovered token to sixteen joins", () => {
    const rows = Array.from({ length: 7 }, () => row("docs/a.md)에", 12));
    const groups = linkCandidates(rows, 3);
    expect(groups).toHaveLength(1);
    // All seven rows are fully occupied: four starts times four ends make
    // sixteen distinct joined spellings around the middle row.
    expect(groups[0]!.filter((candidate) => candidate.original)).toHaveLength(16);
    expect(groups[0]).toHaveLength(48); // This fixture has three distinct stages per join.
    expect(groups[0]!.length * 2).toBeLessThanOrEqual(192);
    expect(groups[0]!.filter((candidate) => !candidate.original).length * 2).toBeLessThanOrEqual(160);
  });
});
