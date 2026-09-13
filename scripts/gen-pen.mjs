#!/usr/bin/env node
// Write the generated parts of the design canvas.
//
//   node scripts/gen-pen.mjs
//
// HideTheme's values go into the mapped variables, every board is placed at
// its band, and the band labels and the Foundations sheet are redrawn. Every
// other node, and every variable the design authored for itself, is left
// alone. A HideTheme constant no list claims, or a board no band claims, is a
// failure rather than a guess: the first is a token the design never received,
// the second a board the scheme cannot find.

import fs from 'node:fs';
import {CANVAS, MAP} from './pen-tokens.mjs';
import {BANDS} from './pen-bands.mjs';
import {generate} from './pen-canvas.mjs';

const {orphans, unknown, file, before, after, expected} = generate(process.cwd());

if (orphans.length) {
  console.error(`${orphans.length} HideTheme constant(s) are in neither list of ${MAP}:`);
  for (const name of orphans) console.error(`  ${name}`);
  console.error('Map each to a canvas variable, or record why it stays out.');
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
  console.log(`${CANVAS} updated: ${expected.size} token values written, boards laid out across ${BANDS.length} bands.`);
}
