#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import {tokens} from './swift-source-tokens.mjs';

const root = process.argv[2] ?? 'macos/Sources/HerdrMacOS';
const excluded = name => name.startsWith('Pet') || name === 'HideTheme.swift';
const number = value => typeof value === 'string' && /^[0-9]/.test(value);
function violations(source) {
  const ts = tokens(source), issues = [];
  const at = (i, ...parts) => parts.every((p, n) => ts[i+n] === p);
  // Read one syntactic argument, not a regex spanning nested parentheses.
  function argument(start) {
    const result = [];
    let depth = 0;
    for (let i = start; i < ts.length; i++) {
      if (depth === 0 && [',', ')'].includes(ts[i])) break;
      if (['(', '[', '{'].includes(ts[i])) depth++;
      if ([')', ']', '}'].includes(ts[i])) depth--;
      result.push(ts[i]);
    }
    return result;
  }
  for (let i = 0; i < ts.length; i++) {
    if (ts[i]?.string !== undefined && /[⌘⌃⌥⇧]/.test(ts[i].string)) issues.push('modifier glyph literal');
    if (at(i, '.', 'help', '(')) issues.push('native help');
    if (at(i, 'Color', '(', 'red', ':')) issues.push('RGB color literal');
    if (at(i, 'Color', '.', 'white', '.', 'opacity', '(')
      || at(i, 'Color', '.', 'black', '.', 'opacity', '(')) issues.push('white/black opacity color');
    if (at(i, 'hideFont', '(', 'size', ':') && argument(i+4).some(number)) issues.push('numeric hideFont size');
    if (at(i, 'cornerRadius', ':') && argument(i+2).some(number)) issues.push('numeric corner radius');
    if (at(i, '.', 'opacity', '(') && argument(i+3).some(t => number(t) && ![0, 1].includes(Number(t)))) {
      issues.push('numeric opacity');
    }
    if (at(i, '.', 'padding', '(')) {
      let cursor = i+3, depth = 0;
      for (; cursor < ts.length; cursor++) {
        if (ts[cursor] === ')' && depth === 0) break;
        if (ts[cursor] === '(') depth++;
        if (ts[cursor] === ')') depth--;
        if (number(ts[cursor])) { issues.push('numeric padding'); break; }
      }
    }
  }
  return issues;
}
function scan(directory) {
  return fs.readdirSync(directory).filter(name => name.endsWith('.swift') && !excluded(name))
    .flatMap(name => violations(fs.readFileSync(path.join(directory, name), 'utf8'))
      .map(rule => name + ': ' + rule));
}
const issues = scan(root);
if (issues.length) {
  console.error(issues.join('\n'));
  process.exitCode = 1;
} else {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'hide-theme-literals-'));
  try {
    // A separate file for each planted regression proves every forbidden class,
    // including multiline calls, numeric expressions, and raw glyph strings.
    const positives = [
      'Text("x").hideFont(size: 12)', 'Text("x").padding(.horizontal, 8)',
      'RoundedRectangle(cornerRadius: size * 0.2)', 'Text("x").opacity(0.4)',
      'Color(red:\n 0.1, green: 0.2, blue: 0.3)', 'Color.white.opacity(1)',
      'Color.black.opacity(0)', 'Text("x").help("Help")', 'Text(#"⌘K"#)',
      'Text("x").padding(size * 0.16)', 'Text("x").hideFont(size: size * 0.53)',
    ];
    for (const [index, source] of positives.entries()) {
      const file = path.join(fixture, 'Violation.swift');
      fs.writeFileSync(file, source);
      assert(scan(fixture).length > 0, 'Undetected positive fixture ' + index);
      fs.unlinkSync(file);
    }
    fs.writeFileSync(path.join(fixture, 'PetExample.swift'), positives.join('\n'));
    fs.writeFileSync(path.join(fixture, 'HideTheme.swift'), positives.join('\n'));
    fs.writeFileSync(path.join(fixture, 'View.swift'),
      '// Text("⌘K").padding(12)\n/* .help("x") */\nText("body").padding(HideTheme.spacingSM).opacity(0).opacity(1)');
    assert.deepEqual(scan(fixture), [], 'Comments, named tokens, visibility, and excluded files must pass');
  } finally { fs.rmSync(fixture, {recursive: true, force: true}); }
  console.log('Theme literals: 0 violations; 11 positive regressions and exclusion fixtures PASS');
}
