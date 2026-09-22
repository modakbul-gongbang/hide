import { describe, expect, it } from "vitest";
import { rowContinues, selectionToText, type CellRow } from "./selection";

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

  it("treats a cell nothing was written to as a space inside a row and as nothing at its end", () => {
    const rows: CellRow = ["a", "", "b", "", ""];
    expect(selectionToText([rows], 0, 5)).toBe("a b");
    expect(rowContinues(rows)).toBe(false);
  });
});
