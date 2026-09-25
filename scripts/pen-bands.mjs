// The library holds foundations and System/Component sheets only.
// Screen proposals belong in ignored agents/runs/<task>/ documents.
// Regeneration owns Foundations, every System / <Part> sheet's content, and the two
// Component / Panel Tab and Component / Keycap sheets that carry the folded legacy
// masters (they are generated content, not hand-authored, the same way a System
// sheet is) - never the designer's placement of any sheet, and never any other
// Component / content.

import {foundations} from './pen-foundations.mjs';
import {systemSheets, componentFoldSheets, RETIRED_SHEETS} from './pen-system.mjs';

export const FOUNDATIONS = 'System / Foundations';
export const BANDS = [{prefix: 'System /'}, {prefix: 'Component /'}];

// A brand-new generated sheet needs somewhere to land before a person drags it;
// this staggers them on a grid instead of stacking every one at the origin.
const GRID_STRIDE_X = 1440;
const GRID_STRIDE_Y = 1000;
const GRID_COLUMNS = 5;

export function layout(document, tokens, legacy) {
  const retired = new Set(RETIRED_SHEETS);
  const kept = document.children.filter(node => !retired.has(node.name));

  const unknown = kept
    .filter(node => !BANDS.some(band => (node.name ?? '').startsWith(band.prefix)))
    .map(node => node.name || `<${node.type} ${node.id}>`);

  const generated = [{name: FOUNDATIONS, build: () => foundations(document.variables)}, ...systemSheets(tokens, legacy), ...componentFoldSheets(legacy)];

  let children = kept;
  let placed = 0;
  for (const {name, build} of generated) {
    const existing = children.find(node => node.name === name);
    const sheet = existing
      ? {...build(), x: existing.x, y: existing.y}
      : {...build(), x: (placed % GRID_COLUMNS) * GRID_STRIDE_X, y: Math.floor(placed / GRID_COLUMNS) * GRID_STRIDE_Y};
    if (!existing) placed++;
    children = existing ? children.map(node => node === existing ? sheet : node) : [...children, sheet];
  }
  return {document: {...document, children}, unknown};
}

export function serialize(document) {
  return JSON.stringify(document, null, 2);
}
