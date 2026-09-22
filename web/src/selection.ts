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
// wrap flag in the frame would close. The copy is then dedented: the
// leading whitespace every non-blank line shares is a margin the program
// drew, not text, while the indentation lines have relative to one another
// is kept (user: "그 줄 앞에 공백있으면 앞에서 trim까지 해줘"). A single line
// therefore loses its leading spaces entirely. A drag that starts past
// column 0 cuts its first line short, so that line's leading whitespace is
// not a margin and does not decide one: the other lines do, and the first
// line gives up whatever part of the margin it still has.

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
  return dedent(text, startColumn > 0);
}

function leading(line: string): string {
  return line.slice(0, line.length - line.trimStart().length);
}

function sharedPrefix(a: string, b: string): string {
  let shared = 0;
  while (shared < a.length && shared < b.length && a[shared] === b[shared]) shared += 1;
  return a.slice(0, shared);
}

/**
 * Removes the leading whitespace every non-blank line shares; blank lines
 * stay blank. With `firstLinePartial` the first line was cut at the
 * selection start, so it has no say in the margin unless it is the only
 * line, and it loses only as much of the margin as it still carries.
 */
export function dedent(text: string, firstLinePartial = false): string {
  const lines = text.split("\n");
  const partial = (index: number) => firstLinePartial && index === 0;
  let margin: string | null = null;
  for (const [index, line] of lines.entries()) {
    if (line.trim() === "" || partial(index)) continue;
    margin = margin === null ? leading(line) : sharedPrefix(margin, leading(line));
    if (margin === "") break;
  }
  if (margin === null && firstLinePartial && lines[0]?.trim() !== "") margin = leading(lines[0] ?? "");
  if (!margin) return text;
  const width = margin.length;
  return lines
    .map((line, index) => {
      if (line.trim() === "") return line;
      return line.slice(partial(index) ? Math.min(width, leading(line).length) : width);
    })
    .join("\n");
}
