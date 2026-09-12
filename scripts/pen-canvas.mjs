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
