#!/usr/bin/env node
// Refuse a System library and a gallery that name different parts or states.
//
//   node scripts/check-pen-gallery.mjs [root]
//
// Every `System / <Name>` sheet in design/hide-ui.lib.pen except Foundations is
// one shadcn part, and its `Light` and `Dark` frames hold one node per state.
// web/src/gallery/manifest.ts lists the same parts and states, and the gallery
// renders each from it. A part or state on one side only means Pen and the
// code have stopped describing the same thing (DESIGN_WORKFLOW.md).

import fs from 'node:fs';
import path from 'node:path';

export const MANIFEST = 'web/src/gallery/manifest.ts';
export const LIBRARY = 'design/hide-ui.lib.pen';
const PREFIX = 'System / ';
const FOUNDATIONS = 'System / Foundations';

/** The manifest's `GALLERY` literal, read as JSON rather than executed. */
export function readManifest(root) {
  const source = fs.readFileSync(path.join(root, MANIFEST), 'utf8');
  const match = /export const GALLERY = (\{[\s\S]*?\n\}) as const;/.exec(source);
  if (!match) throw new Error(`${MANIFEST} has no \`export const GALLERY = {...} as const;\` literal`);
  return JSON.parse(match[1]);
}

/** Each System sheet's states per theme frame, from the library. */
export function readLibrary(root) {
  const document = JSON.parse(fs.readFileSync(path.join(root, LIBRARY), 'utf8'));
  const sheets = {};
  for (const sheet of document.children) {
    const name = sheet.name ?? '';
    if (!name.startsWith(PREFIX) || name === FOUNDATIONS) continue;
    const frames = Object.fromEntries((sheet.children ?? []).filter(node => node.name === 'Light' || node.name === 'Dark').map(node => [node.name, (node.children ?? []).map(child => child.name)]));
    sheets[name.slice(PREFIX.length)] = frames;
  }
  return sheets;
}

export function compare(manifest, library) {
  const failures = [];
  for (const part of Object.keys(manifest)) {
    if (!library[part]) failures.push(`gallery section \`${part}\` has no \`${PREFIX}${part}\` sheet in ${LIBRARY}`);
  }
  for (const part of Object.keys(library)) {
    if (!manifest[part]) {
      failures.push(`\`${PREFIX}${part}\` has no gallery section in ${MANIFEST}`);
      continue;
    }
    for (const theme of ['Light', 'Dark']) {
      const drawn = library[part][theme];
      if (!drawn) {
        failures.push(`\`${PREFIX}${part}\` has no \`${theme}\` frame`);
        continue;
      }
      const listed = manifest[part];
      const missing = listed.filter(state => !drawn.includes(state));
      const extra = drawn.filter(state => !listed.includes(state));
      if (missing.length) failures.push(`\`${PREFIX}${part}\` ${theme} does not draw: ${missing.join(', ')}`);
      if (extra.length) failures.push(`\`${PREFIX}${part}\` ${theme} draws states the gallery does not list: ${extra.join(', ')}`);
    }
  }
  return failures;
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === 'check-pen-gallery.mjs';
if (invoked) {
  const root = process.argv[2] ?? process.cwd();
  const failures = compare(readManifest(root), readLibrary(root));
  if (failures.length) {
    console.error('Pen System sheets and the gallery disagree:\n' + failures.map(line => `  ${line}`).join('\n'));
    process.exit(1);
  }
  console.log(`Pen System sheets and the gallery agree: ${Object.keys(readManifest(root)).length} parts.`);
}
