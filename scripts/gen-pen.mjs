#!/usr/bin/env node
// Write the generated parts of the design canvas.
//
//   node scripts/gen-pen.mjs
//
// design/tokens.json's values go into the same-named variables, D-21's rename runs
// first, and Foundations plus every System / <Part> sheet are redrawn without
// moving any sheet. Declared opacity bindings are materialized. Every other node,
// and every variable the library authored for itself, is left alone. A canvas
// variable neither generated nor excused, or a board no band claims, is a failure
// rather than a guess: the first is a variable that reaches the design and never
// tokens.json, the second a board the scheme cannot find.

import fs from 'node:fs';
import {CANVAS, MAP} from './pen-tokens.mjs';
import {BANDS} from './pen-bands.mjs';
import {generate} from './pen-canvas.mjs';

const {orphans, unknown, file, before, after, expected} = generate(process.cwd());

if (orphans.length) {
  console.error(`${orphans.length} canvas variable(s) are in neither list of ${MAP}:`);
  for (const name of orphans) console.error(`  ${name}`);
  console.error('Rename each to a design/tokens.json name, or record why it stays design-authored.');
  process.exit(1);
}
if (unknown.length) {
  console.error(`${unknown.length} board(s) in ${CANVAS} carry no band prefix:`);
  for (const name of unknown) console.error(`  ${name}`);
  console.error(`Name each with one of: ${BANDS.map(band => `\`${band.prefix}\``).join(', ')}.`);
  process.exit(1);
}

if (before === after) {
  console.log(`${CANVAS} is already what the generator writes: ${expected.size} token values, ${BANDS.length} bands.`);
} else {
  fs.writeFileSync(file, after);
  console.log(`${CANVAS} updated: ${expected.size} token values written, Foundations and every System part refreshed; sheet placement preserved.`);
}
