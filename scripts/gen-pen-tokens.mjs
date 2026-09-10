#!/usr/bin/env node
// Write HideTheme's values into the design canvas.
//
// HideTheme.swift owns a token; design/hide.pen consumes it. Restating a value in
// both places is how they drift, so the canvas's mapped variables are generated
// from the Swift rather than maintained beside it. Everything else in the file -
// every node, and every variable the design authored for itself - is left alone.
//
//   node scripts/gen-pen-tokens.mjs

import fs from 'node:fs';
import {CANVAS, read, loadCanvas, apply, unclaimed} from './pen-tokens.mjs';

const root = process.cwd();
const {map, table, expected} = read(root);

const orphans = unclaimed(map, table);
if (orphans.length) {
  console.error(`${orphans.length} HideTheme constant(s) are in neither list of scripts/pen-token-map.json:`);
  for (const name of orphans) console.error(`  ${name}`);
  console.error('Map each to a canvas variable, or record why it stays out.');
  process.exit(1);
}

const {file, document} = loadCanvas(root);
const before = fs.readFileSync(file, 'utf8');
const after = apply(document, expected);

if (before === after) {
  console.log(`${CANVAS} already carries all ${expected.size} generated values.`);
} else {
  fs.writeFileSync(file, after);
  console.log(`${CANVAS} updated: ${expected.size} generated values written.`);
}
