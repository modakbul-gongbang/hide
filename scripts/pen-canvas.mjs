// The design canvas as the generators would write it.
//
// One pipeline serves both entrypoints: gen-pen.mjs writes its output and
// check-pen.mjs compares the file against it. Tokens come first, because the
// Foundations sheet is drawn from the variables the token pass has just made
// agree with HideTheme; layout comes second and rebuilds every generator-owned
// board. Both compare against exactly the text this function returns.

import fs from 'node:fs';
import {read, loadCanvas, apply, unclaimed} from './pen-tokens.mjs';
import {layout, serialize} from './pen-bands.mjs';

export function generate(root) {
  const {map, table, expected} = read(root);
  const {file, document} = loadCanvas(root);
  const orphans = unclaimed(map, table);
  const tokens = JSON.parse(apply(document, expected));
  const {document: laid, unknown} = layout(tokens, map.mapped);
  return {map, expected, file, document, orphans, unknown, before: fs.readFileSync(file, 'utf8'), after: serialize(laid)};
}

// Where a board sits is the generator's convenience, not the contract: a board
// dragged somewhere else in the pen app is still in its band by name, and the
// next gen-pen run puts it back. The check therefore compares everything but
// the top-level positions and the order they imply, so a drag never fails a
// gate on its own.
export function unplaced(text) {
  const document = JSON.parse(text);
  const children = document.children.map(({x, y, ...node}) => node).sort((a, b) => a.id.localeCompare(b.id));
  return serialize({...document, children});
}
