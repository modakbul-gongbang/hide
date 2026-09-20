// The design canvas as the generators would write it.
//
// One pipeline serves both entrypoints: gen-pen.mjs writes its output and
// check-pen.mjs compares the file against it. Tokens come first, because the
// Foundations sheet is drawn from the variables the token pass has just made
// agree with HideTheme; Foundations content follows without moving any sheet.
// Both compare against exactly the text this function returns.

import fs from 'node:fs';
import {read, loadCanvas, apply, unclaimed} from './pen-tokens.mjs';
import {layout, serialize} from './pen-bands.mjs';

export function generate(root) {
  const {map, table, expected} = read(root);
  const {file, document} = loadCanvas(root);
  const orphans = unclaimed(map, table);
  const tokens = JSON.parse(apply(document, expected));
  // Pen currently renders variable-bound node opacity as invisible. Materialize
  // only the declared bindings, including instance descendant overrides, while
  // keeping the token as authority. Missing targets fail rather than going stale.
  const nodes = new Map();
  function visit(node) {
    nodes.set(node.id, node);
    for (const child of node.children ?? []) visit(child);
  }
  for (const child of tokens.children) visit(child);
  for (const {node, descendant, variable} of map.renderedOpacity ?? []) {
    const owner = nodes.get(node);
    const target = descendant ? owner?.descendants?.[descendant] : owner;
    const token = expected.get(variable);
    if (!target || token?.type !== 'number' || token.value < 0 || token.value > 1) {
      throw new Error(`Invalid rendered opacity binding: ${node}/${descendant ?? ''} -> ${variable}`);
    }
    target.opacity = token.value;
  }
  const {document: laid, unknown} = layout(tokens, map.mapped);
  return {map, expected, file, document, orphans, unknown, before: fs.readFileSync(file, 'utf8'), after: serialize(laid)};
}

// Placement and document order belong to the designer, not the generator.
// A drag never fails a gate or gets undone by regeneration.
export function unplaced(text) {
  const document = JSON.parse(text);
  const children = document.children.map(({x, y, ...node}) => node).sort((a, b) => a.id.localeCompare(b.id));
  return serialize({...document, children});
}
