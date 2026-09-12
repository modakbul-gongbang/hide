#!/usr/bin/env node
// Lay the design canvas's boards out by band.
//
//   node scripts/gen-pen-layout.mjs
//
// Every top-level frame is placed at its band's y and packed left to right in
// the order it already had. A board whose name no band claims is a failure, not
// a guess: the scheme is what makes the canvas readable, and a board outside it
// is either misnamed or belongs in Scratch.

import fs from 'node:fs';
import path from 'node:path';
import {CANVAS, BANDS, layout, serialize} from './pen-bands.mjs';

const file = path.join(process.cwd(), CANVAS);
const before = fs.readFileSync(file, 'utf8');
const {document, unknown} = layout(JSON.parse(before));

if (unknown.length) {
  console.error(`${unknown.length} board(s) in ${CANVAS} carry no band prefix:`);
  for (const name of unknown) console.error(`  ${name}`);
  console.error(`Name each with one of: ${BANDS.map(band => `\`${band.prefix}\``).join(', ')}.`);
  process.exit(1);
}

const after = serialize(document);
if (before === after) {
  console.log(`${CANVAS} is already laid out by band.`);
} else {
  fs.writeFileSync(file, after);
  console.log(`${CANVAS} laid out: ${document.children.length} boards across ${BANDS.length} bands.`);
}
