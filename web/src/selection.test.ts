import { describe, expect, it } from "vitest";
import { dedent, rowContinues, selectionToText, type CellRow } from "./selection";

/** A row of `width` cells holding `text`, padded with written spaces the way a Herdr frame draws it. */
function row(text: string, width: number): CellRow {
  const cells: CellRow = [];
  for (const char of text) cells.push(char);
  while (cells.length < width) cells.push(" ");
  return cells;
}

describe("selectionToText", () => {
  it("trims the padding a frame writes to the column edge", () => {
    expect(selectionToText([row("short", 12)], 0, 12)).toBe("short");
  });

  it("joins a row that fills its width onto the next without a break", () => {
    const rows = [row("abcdefghijkl", 12), row("mnop", 12), row("next", 12)];
    expect(selectionToText(rows, 0, 12)).toBe("abcdefghijklmnop\nnext");
  });

  it("keeps an empty row as a blank line", () => {
    const rows = [row("one", 8), row("", 8), row("two", 8)];
    expect(selectionToText(rows, 0, 8)).toBe("one\n\ntwo");
  });

  it("takes the first row from the start column and the last row up to the end column", () => {
    const rows = [row("hello world", 12), row("second line", 12)];
    expect(selectionToText(rows, 6, 6)).toBe("world\nsecond");
  });

  it("reads a wide glyph in the last column as a full row and never copies its trailing half", () => {
    const wide: CellRow = ["a", "b", "가", null];
    expect(rowContinues(wide)).toBe(true);
    expect(selectionToText([wide, row("c", 4)], 0, 4)).toBe("ab가c");
  });

  it("removes the margin every line shares and keeps the indentation between lines", () => {
    const rows = [row("  fn main() {", 20), row("      body();", 20), row("", 20), row("  }", 20)];
    expect(selectionToText(rows, 0, 20)).toBe("fn main() {\n    body();\n\n}");
  });

  it("strips a single line's leading spaces entirely", () => {
    expect(selectionToText([row("    indented", 16)], 0, 16)).toBe("indented");
  });

  it("dedents by the other lines when the drag starts past the first line's margin", () => {
    const rows = [row("  fn main() {", 20), row("      body();", 20), row("  }", 20)];
    expect(selectionToText(rows, 2, 20)).toBe("fn main() {\n    body();\n}");
    expect(selectionToText(rows, 5, 20)).toBe("main() {\n    body();\n}");
  });

  it("takes only what the cut first line still has of the margin", () => {
    const rows = [row("  fn main() {", 20), row("      body();", 20), row("  }", 20)];
    expect(selectionToText(rows, 1, 20)).toBe("fn main() {\n    body();\n}");
  });

  it("treats a cut first row that continues onto the next as one cut line", () => {
    const rows = [row("  abcdefghij", 12), row("kl", 12), row("  end", 12)];
    expect(selectionToText(rows, 4, 12)).toBe("cdefghijkl\nend");
  });

  it("strips a lone cut line's leading spaces entirely", () => {
    expect(selectionToText([row("    indented", 16)], 2, 16)).toBe("indented");
  });

  it("keeps a selection whose lines share no margin as it is", () => {
    expect(dedent("a\n  b")).toBe("a\n  b");
    expect(dedent("")).toBe("");
  });

  it("treats a cell nothing was written to as a space inside a row and as nothing at its end", () => {
    const rows: CellRow = ["a", "", "b", "", ""];
    expect(selectionToText([rows], 0, 5)).toBe("a b");
    expect(rowContinues(rows)).toBe(false);
  });
});
