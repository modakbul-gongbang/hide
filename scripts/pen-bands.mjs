// The library holds foundations and reusable component sheets only.
// Screen proposals belong in ignored agents/runs/<task>/ documents.
// Regeneration owns Foundations content, never the designer's placement.

import {foundations} from './pen-foundations.mjs';

export const FOUNDATIONS = 'System / Foundations';
export const BANDS = [{prefix: 'System /'}, {prefix: 'Component /'}];

export function layout(document, mapped) {
  const unknown = document.children
    .filter(node => !BANDS.some(band => (node.name ?? '').startsWith(band.prefix)))
    .map(node => node.name || `<${node.type} ${node.id}>`);
  const existing = document.children.find(node => node.name === FOUNDATIONS);
  const sheet = {...foundations(document.variables, mapped), x: existing?.x ?? 0, y: existing?.y ?? 0};
  const children = existing
    ? document.children.map(node => node === existing ? sheet : node)
    : [sheet, ...document.children];
  return {document: {...document, children}, unknown};
}

export function serialize(document) {
  return JSON.stringify(document, null, 2);
}
