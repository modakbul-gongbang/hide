#!/usr/bin/env node
// Refuse a design canvas that disagrees with HideTheme.
//
// Two failures, and the second is the one worth having. A value that drifted is
// visible: someone changed a colour in Swift and the canvas still shows the old
// one. A HideTheme constant claimed by neither list of the map is invisible: a
// token reaches the shell and never reaches the design, and nothing says so.
//
//   node scripts/check-pen-tokens.mjs

import fs from 'node:fs';
import {CANVAS, MAP, read, loadCanvas, apply, unclaimed} from './pen-tokens.mjs';

const root = process.cwd();
const {map, table, expected} = read(root);
const failures = [];

const orphans = unclaimed(map, table);
if (orphans.length) {
  failures.push(`${orphans.length} HideTheme constant(s) claimed by neither list of ${MAP}:\n` +
    orphans.map(name => `    ${name}`).join('\n') +
    `\n  Map each to a canvas variable, or record why it stays out.`);
}

const {file, document} = loadCanvas(root);
for (const [name, want] of expected) {
  const got = document.variables[name];
  if (got === undefined) {
    failures.push(`${CANVAS} is missing ${name} (${want.type} ${want.value} from ${map.mapped[name]})`);
  } else if (got.type !== want.type || String(got.value) !== String(want.value)) {
    failures.push(`${CANVAS} carries ${name} = ${got.type} ${got.value}; HideTheme.${map.mapped[name]} is ${want.type} ${want.value}`);
  }
}

if (!failures.length && fs.readFileSync(file, 'utf8') !== apply(document, expected)) {
  failures.push(`${CANVAS} is not what the generator writes; run node scripts/gen-pen-tokens.mjs`);
}

if (failures.length) {
  console.error('Design canvas is out of step with HideTheme:\n');
  for (const failure of failures) console.error(`  ${failure}\n`);
  console.error('Run node scripts/gen-pen-tokens.mjs to bring the canvas forward.');
  process.exit(1);
}

console.log(`Design canvas agrees with HideTheme: ${expected.size} generated values, ${Object.keys(document.variables).length - expected.size} design-authored.`);
