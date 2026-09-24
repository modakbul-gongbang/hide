import { GFM, parser } from "@lezer/markdown";
import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { markdownLivePlan, MARKDOWN_LIVE_BYTE_LIMIT, type LiveRange } from "./markdownLive";

const markdown = parser.configure([GFM]);

function plan(source: string, selection = 0): LiveRange[] {
  const doc = Text.of(source.split("\n"));
  const tree = markdown.parse(source);
  return markdownLivePlan(tree, doc, { from: selection, to: selection });
}

/** The ranges as `kind:slice` strings, which is what a reader can check. */
function shown(source: string, selection = 0): string[] {
  const doc = Text.of(source.split("\n"));
  return plan(source, selection).map((range) => `${range.kind}:${doc.sliceString(range.from, range.to)}`);
}

// The caret sits on the first line in every case that wants markup hidden, so
// the block under test is elsewhere and its own line is not the touched one.
const AWAY = "intro\n\n";

describe("markdown live plan", () => {
  it("styles a heading line and hides its hashes", () => {
    expect(shown(`${AWAY}# Title`)).toEqual(["hide:#", "heading:# Title"]);
  });

  it("reveals the heading's own markup while the caret is on its line", () => {
    expect(shown("# Title", 3)).toEqual(["heading:# Title"]);
  });

  it("draws a bullet in place of the list marker", () => {
    expect(shown(`${AWAY}- one\n- two`)).toEqual(["bullet:-", "bullet:-"]);
  });

  it("leaves an ordered list's number alone", () => {
    expect(shown(`${AWAY}1. one`)).toEqual([]);
  });

  it("draws a checkbox for a task item in either state", () => {
    const ranges = plan(`${AWAY}- [ ] open\n- [x] done`);
    expect(ranges.filter((range) => range.kind === "checkbox").map((range) => range.checked)).toEqual([false, true]);
  });

  it("hides emphasis, code and link markup while the caret is elsewhere", () => {
    expect(shown(`${AWAY}a **bold** b`)).toEqual(["hide:**", "hide:**"]);
    expect(shown(`${AWAY}a \`code\` b`)).toEqual(["hide:`", "codeInline:code", "hide:`"]);
    expect(shown(`${AWAY}see [text](https://example.com) now`)).toEqual([
      "hide:[",
      "link:text",
      "hide:]",
      "hide:(",
      "hide:https://example.com",
      "hide:)",
    ]);
  });

  it("collapses a fenced code block to its body and styles those lines", () => {
    expect(shown(`${AWAY}\`\`\`rust\nfn main() {}\n\`\`\`\n`)).toEqual([
      "hide:```",
      "fenceLine:```rust",
      "hide:rust",
      "codeBlock:fn main() {}",
      "hide:```",
      "fenceLine:```",
    ]);
  });

  it("reveals the whole fenced block while the caret is inside it", () => {
    expect(shown("```rust\nfn main() {}\n```\n", 12)).toEqual([]);
  });

  it("hides a quote prefix and marks the quoted line", () => {
    expect(shown(`${AWAY}> quoted`)).toEqual(["hide:>", "quote:> quoted"]);
  });

  it("reads the live bound from the same number the editor caps at", () => {
    expect(MARKDOWN_LIVE_BYTE_LIMIT).toBe(256 * 1024);
  });
});
