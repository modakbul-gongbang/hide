#!/usr/bin/env node
// Refuse a design canvas that is not what the generator writes.
//
//   node scripts/check-pen.mjs
//
// Five failures, in the order a person can act on them. A canvas variable claimed
// by neither list of the map is the one worth having: a variable reaches the
// design and never design/tokens.json, and nothing else says so. A value that
// drifted is visible, and named with both sides. A board with no band prefix
// cannot be found by the scheme. Anything else the generator would change - a
// stale Foundations or System sheet, a token value written by hand - is reported
// as one difference, with the command that repairs it. Where a board sits is not
// checked; see `unplaced`. An id that names two nodes - counting a node written
// whole inside a ref's `descendants`, where the key already addresses it - makes
// Pen's loader report duplicate ids and leaves which node an override reaches
// to chance.

import {CANVAS, MAP} from './pen-tokens.mjs';
import {BANDS} from './pen-bands.mjs';
import {duplicateIds, generate, unplaced} from './pen-canvas.mjs';

const {map, expected, document, orphans, unknown, before, after} = generate(process.cwd());
const failures = [];

if (orphans.length) {
  failures.push(`${orphans.length} canvas variable(s) claimed by neither list of ${MAP}:\n` +
    orphans.map(name => `    ${name}`).join('\n') +
    `\n  Rename each to a design/tokens.json name, or record why it stays design-authored.`);
}
for (const [name, want] of expected) {
  const got = document.variables[name];
  if (got === undefined) {
    failures.push(`${CANVAS} is missing ${name} (${want.type} ${JSON.stringify(want.value)} from design/tokens.json)`);
  } else if (got.type !== want.type || JSON.stringify(got.value) !== JSON.stringify(want.value)) {
    failures.push(`${CANVAS} carries ${name} = ${got.type} ${JSON.stringify(got.value)}; design/tokens.json is ${want.type} ${JSON.stringify(want.value)}`);
  }
}
const duplicates = duplicateIds(document.children);
if (duplicates.length) {
  failures.push(`${duplicates.length} id(s) name more than one node:\n` +
    duplicates.map(([id, paths]) => `    ${id}: ${paths.join(', ')}`).join('\n') +
    `\n  Give each node its own id; a descendants entry is addressed by its key and carries no id of its own.`);
}
if (unknown.length) {
  failures.push(`${unknown.length} board(s) carry no band prefix:\n` +
    unknown.map(name => `    ${name}`).join('\n') +
    `\n  Name each with one of: ${BANDS.map(band => `\`${band.prefix}\``).join(', ')}.`);
}
if (!failures.length && unplaced(before) !== unplaced(after)) {
  failures.push(`${CANVAS} is not what the generator writes; run node scripts/gen-pen.mjs`);
}

if (failures.length) {
  console.error('Design canvas is out of step:\n');
  for (const failure of failures) console.error(`  ${failure}\n`);
  // Which way to repair a drifted value depends on where the change was meant,
  // and only the person who made it knows. Naming only the generator would tell
  // a designer who deliberately changed a value on the canvas to throw it away.
  console.error('If a canvas value is the one you meant, put it in design/tokens.json first, then run node scripts/gen-pen.mjs.');
  console.error('If the canvas is simply behind, run node scripts/gen-pen.mjs.');
  process.exit(1);
}

const counts = BANDS.map(band => `${band.prefix.replace(' /', '')} ${document.children.filter(node => node.name.startsWith(band.prefix)).length}`);
console.log(`Design library agrees with design/tokens.json: ${expected.size} generated values, ${Object.keys(document.variables).length - expected.size} design-authored; ${counts.join(', ')}.`);
