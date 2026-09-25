#!/usr/bin/env node
// Write design/hide-screens.pen: every `Screen / <Area>` sheet (PRD
// web-design-system-reset B19, D-18, D-19).
//
//   node scripts/gen-screens.mjs
//
// Regeneration owns every `Screen /` sheet's content and this file's local token
// variables (kept in sync with design/tokens.json through the same pen-tokens.mjs
// pipeline gen-pen.mjs uses), never a sheet's placement (x, y) once drawn - the
// designer's layout on the canvas survives a rerun exactly as pen-bands.mjs
// preserves it for design/hide-ui.lib.pen.

import fs from 'node:fs';
import path from 'node:path';
import {readTokens} from './gen-tokens.mjs';
import {screenSheets, readLocalVariables, ALIAS, LIBRARY_PATH} from './pen-screens.mjs';
import {serialize} from './pen-bands.mjs';

export const FILE = 'design/hide-screens.pen';
const GRID_STRIDE_X = 1400;
const GRID_STRIDE_Y = 900;
const GRID_COLUMNS = 3;

function loadExisting(root) {
  const file = path.join(root, FILE);
  if (!fs.existsSync(file)) return null;
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

export function generate(root) {
  const tokens = readTokens(root);
  const variables = readLocalVariables(root);
  const existing = loadExisting(root);
  const generated = screenSheets(tokens, root);

  let children = existing ? [...existing.children] : [];
  let placed = 0;
  for (const {name, build} of generated) {
    const found = children.find(node => node.name === name);
    const sheet = found ? {...build(), x: found.x, y: found.y} : {...build(), x: (placed % GRID_COLUMNS) * GRID_STRIDE_X, y: Math.floor(placed / GRID_COLUMNS) * GRID_STRIDE_Y};
    if (!found) placed++;
    children = found ? children.map(node => node === found ? sheet : node) : [...children, sheet];
  }

  const document = {version: '2.18', themes: {Mode: ['Light', 'Dark']}, imports: {[ALIAS]: LIBRARY_PATH}, variables, children};
  const before = existing ? serialize(existing) : null;
  const after = serialize(document);
  return {file: path.join(root, FILE), before, after, sheetCount: generated.length};
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === 'gen-screens.mjs';
if (invoked) {
  const {file, before, after, sheetCount} = generate(process.cwd());
  if (before === after) {
    console.log(`${FILE} is already what the generator writes: ${sheetCount} screen sheets.`);
  } else {
    fs.writeFileSync(file, after);
    console.log(`${FILE} ${before === null ? 'created' : 'updated'}: ${sheetCount} screen sheets written; sheet placement preserved.`);
  }
}
