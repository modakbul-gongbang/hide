// The text a drag selection copies.
//
// Herdr's terminal frames redraw every row in place and padded to the full
// width (`CSI row;1H` then `width` cells), so xterm.js never soft-wraps a row
// itself: every padding space is a written cell its own `getSelection()`
// keeps, and no row is ever `isWrapped`. The frame record (`terminal.frame`:
// seq, encoding, width, height, full, bytes) carries no continuation flag
// either, so the copy is assembled here the way a native terminal would:
// trailing whitespace trimmed from every row, and a row whose last cell is
// non-blank read as continuing onto the next row, joined with no break.
// A hard line that exactly fills the width is joined too; that is the gap a
// wrap flag in the frame would close.

/**
 * One row as xterm reports its cells: the chars of each column, `""` for a
 * cell nothing was written to, and `null` for the trailing half of a wide
 * glyph.
 */
export type CellRow = (string | null)[];

function rowText(cells: CellRow, from: number, to: number): string {
  let text = "";
  for (let column = from; column < to && column < cells.length; column += 1) {
    const chars = cells[column];
    if (chars === null) continue;
    text += chars === "" ? " " : chars;
  }
  return text.trimEnd();
}

/** Whether the row's content reaches its last column, so the next row continues it. */
export function rowContinues(cells: CellRow): boolean {
  for (let column = cells.length - 1; column >= 0; column -= 1) {
    const chars = cells[column];
    if (chars === null) continue;
    return chars !== undefined && chars.trim() !== "";
  }
  return false;
}

/**
 * The copied text for `rows`, the selected rows top to bottom, taking the
 * first row from `startColumn` and the last row up to `endColumn`
 * (exclusive); a row between is taken whole. A rectangular selection is
 * not represented.
 */
export function selectionToText(rows: CellRow[], startColumn: number, endColumn: number): string {
  let text = "";
  rows.forEach((cells, index) => {
    const from = index === 0 ? startColumn : 0;
    const to = index === rows.length - 1 ? endColumn : cells.length;
    if (index > 0 && !rowContinues(rows[index - 1] ?? [])) text += "\n";
    text += rowText(cells, from, to);
  });
  return text;
}
