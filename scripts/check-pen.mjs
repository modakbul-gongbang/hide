#!/usr/bin/env node
// Refuse a design canvas that is not what the generator writes.
//
//   node scripts/check-pen.mjs
//
// Four failures, in the order a person can act on them. A HideTheme constant
// claimed by neither list of the map is the one worth having: a token reaches
// the shell and never reaches the design, and nothing else says so. A value
// that drifted is visible, and named with both sides. A board with no band
// prefix cannot be found by the scheme. Anything else the generator would
// change - a board dragged out of its band, a stale label or Foundations sheet
// - is reported as one difference, with the command that repairs it.

import {CANVAS, MAP} from './pen-tokens.mjs';
import {BANDS} from './pen-bands.mjs';
import {generate} from './pen-canvas.mjs';

const {map, expected, document, orphans, unknown, before, after} = generate(process.cwd());
const failures = [];

if (orphans.length) {
  failures.push(`${orphans.length} HideTheme constant(s) claimed by neither list of ${MAP}:\n` +
    orphans.map(name => `    ${name}`).join('\n') +
    `\n  Map each to a canvas variable, or record why it stays out.`);
}
for (const [name, want] of expected) {
  const got = document.variables[name];
  if (got === undefined) {
    failures.push(`${CANVAS} is missing ${name} (${want.type} ${want.value} from ${map.mapped[name]})`);
  } else if (got.type !== want.type || String(got.value) !== String(want.value)) {
    failures.push(`${CANVAS} carries ${name} = ${got.type} ${got.value}; HideTheme.${map.mapped[name]} is ${want.type} ${want.value}`);
  }
}
if (unknown.length) {
  failures.push(`${unknown.length} board(s) carry no band prefix:\n` +
    unknown.map(name => `    ${name}`).join('\n') +
    `\n  Name each with one of: ${BANDS.map(band => `\`${band.prefix}\``).join(', ')}.`);
}
if (!failures.length && before !== after) {
  failures.push(`${CANVAS} is not what the generator writes; run node scripts/gen-pen.mjs`);
}

if (failures.length) {
  console.error('Design canvas is out of step:\n');
  for (const failure of failures) console.error(`  ${failure}\n`);
  // Which way to repair a drifted value depends on where the change was meant,
  // and only the person who made it knows. Naming only the generator would tell
  // a designer who deliberately changed a value on the canvas to throw it away.
  console.error('If a canvas value is the one you meant, put it in HideTheme.swift first, then run node scripts/gen-pen.mjs.');
  console.error('If the canvas is simply behind, run node scripts/gen-pen.mjs.');
  process.exit(1);
}

const counts = BANDS.map(band => `${band.prefix.replace(' /', '')} ${document.children.filter(node => node.name.startsWith(band.prefix)).length}`);
console.log(`Design canvas agrees with HideTheme and its bands: ${expected.size} generated values, ${Object.keys(document.variables).length - expected.size} design-authored; ${counts.join(', ')}.`);
