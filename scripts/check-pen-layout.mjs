#!/usr/bin/env node
// Refuse a design canvas whose boards are misnamed or out of place.
//
//   node scripts/check-pen-layout.mjs
//
// The canvas has no pages, so a board's name prefix and its band position are
// the whole of its organisation. A board with no prefix cannot be found by the
// scheme; a board dragged out of its band reads as belonging to another one.

import fs from 'node:fs';
import path from 'node:path';
import {CANVAS, BANDS, layout, serialize} from './pen-bands.mjs';

const file = path.join(process.cwd(), CANVAS);
const before = fs.readFileSync(file, 'utf8');
const {document, unknown} = layout(JSON.parse(before));
const failures = [];

if (unknown.length) {
  failures.push(`${unknown.length} board(s) carry no band prefix:\n` +
    unknown.map(name => `    ${name}`).join('\n') +
    `\n  Name each with one of: ${BANDS.map(band => `\`${band.prefix}\``).join(', ')}.`);
}
if (!unknown.length && before !== serialize(document)) {
  failures.push(`${CANVAS} is not laid out by band; run node scripts/gen-pen-layout.mjs`);
}

if (failures.length) {
  console.error('Design canvas layout is out of step:\n');
  for (const failure of failures) console.error(`  ${failure}\n`);
  process.exit(1);
}

const counts = BANDS.map(band => `${band.prefix.replace(' /', '')} ${document.children.filter(node => node.name.startsWith(band.prefix)).length}`);
console.log(`Design canvas is laid out by band: ${counts.join(', ')}.`);
