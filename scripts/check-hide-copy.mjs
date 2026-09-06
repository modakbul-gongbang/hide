#!/usr/bin/env node
// AC19 inventories copy, not shortcut notation. Observer resolution: a string
// consisting of modifier glyphs plus a key (including an interpolated number)
// is excluded from BOTH sides; AC6 compares those values with the registry.
// Chord suffixes in old help strings are likewise not part of the label.
import {execFileSync} from 'node:child_process';
import {readFileSync, readdirSync} from 'node:fs';
import assert from 'node:assert/strict';
import {quoted, tokens} from './swift-source-tokens.mjs';

const directory = 'macos/Sources/HerdrMacOS';
const base = process.argv[process.argv.indexOf('--base') + 1];
if (!process.argv.includes('--base') || !/^[a-f0-9]{7,40}$/.test(base)) {
  throw new Error('Supply the immutable pre-change commit with --base <sha>');
}

function shortcutOnly(value) {
  return /^[⌘⌃⌥⇧↩]+(?:[A-Za-z0-9↩=+\-]|\\\([^\n]+\))?$/.test(value);
}
function label(value) {
  return value.replace(/ \([⌘⌃⌥⇧]+[^)]*\)$/, '')
    .replace(/ \(\\\([^\n]*\.(?:displayShortcut|displayString)\)\)$/, '');
}
function inventory(files) {
  const rows = [];
  for (const [file, source] of files) {
    if (/\/Pet[^/]*\.swift$/.test(file)) continue;
    const ts = tokens(source);
    for (let i = 0; i < ts.length; i++) {
      if (!ts[i]?.string && ts[i]?.string !== '') continue;
      const value = ts[i].string;
      const call = ts[i-2];
      const direct = ts[i-1] === '(';
      let kind;
      if (direct && ['Text', 'accessibilityIdentifier', 'accessibilityLabel'].includes(call)) kind = call;
      if (direct && ['help', 'hideTooltip'].includes(call)) kind = 'help';
      if (ts[i-1] === ':' && ts[i-2] === 'help') kind = 'help';
      if (!kind || shortcutOnly(value)) continue;
      rows.push({kind, value: kind === 'help' ? label(value) : value});
    }
  }
  return rows.sort((a,b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}

assert(shortcutOnly('⌘K'));
assert(shortcutOnly('⌘\\(n)'));
assert(!shortcutOnly('Press ⌘K to search'));
assert.equal(quoted('"Close \\(item.name ?? "Untitled")"', 0).value, 'Close \\(item.name ?? "Untitled")');
assert.deepEqual(inventory([['view.swift', '// Text("ignore")\nText("Search")\nText("⌘K")']]), [{kind:'Text',value:'Search'}]);
assert.equal(label('New agent (⌘N)'), 'New agent');

const names = execFileSync('git', ['ls-tree', '-r', '--name-only', base, directory], {encoding:'utf8'})
  .trim().split('\n').filter(file => file.endsWith('.swift'));
const before = inventory(names.map(file => [file, execFileSync('git', ['show', `${base}:${file}`], {encoding:'utf8'})]));
const after = inventory(readdirSync(directory).filter(file => file.endsWith('.swift'))
  .map(file => [`${directory}/${file}`, readFileSync(`${directory}/${file}`, 'utf8')]));
if (process.argv.includes('--inventory')) {
  process.stdout.write(JSON.stringify({base, exclusion:'Shortcut-only glyph strings are checked through registry equality in AC6', rows:before}, null, 2)+'\n');
} else {
  assert.deepEqual(after, before, 'User-visible copy or accessibility strings changed');
  console.log(`AC19 copy inventory: ${before.length} entries unchanged; shortcut-only strings excluded on both sides`);
}
