#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { generate, hideThemeMatches } from './gen-tokens.mjs';

const root = process.argv[2] ?? process.cwd();
const failures = [];

const mismatches = hideThemeMatches(root);
if (mismatches.length) {
  failures.push('design/tokens.json does not match HideTheme.swift:\n' + mismatches.map(line => '  ' + line).join('\n'));
}

const { css } = generate(root);
const cssPath = path.join(root, 'web/src/tokens.css');
const current = fs.existsSync(cssPath) ? fs.readFileSync(cssPath, 'utf8') : '';
if (current !== css) {
  failures.push('web/src/tokens.css is stale. Run `node scripts/gen-tokens.mjs`.');
}

function walk(dir, acc = []) {
  if (!fs.existsSync(dir)) return acc;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory() && entry.name !== 'generated' && entry.name !== 'node_modules') {
      walk(full, acc);
    } else if (entry.isFile() && /\.(ts|tsx|css)$/.test(entry.name) && entry.name !== 'tokens.css') {
      acc.push(full);
    }
  }
  return acc;
}

const files = walk(path.join(root, 'web/src'));
for (const file of files) {
  const source = fs.readFileSync(file, 'utf8');
  const rel = path.relative(root, file);
  if (/\[#[0-9A-Fa-f]{3,8}\]/.test(source)) failures.push(`${rel}: Tailwind arbitrary color`);
  if (/\[[0-9.]+px\]/.test(source)) failures.push(`${rel}: Tailwind arbitrary px`);
  if (/style=\{\{[^}]*?(?:background|color|border)/.test(source)) failures.push(`${rel}: inline color style`);
  if (path.extname(file) !== '.css' && /#[0-9A-Fa-f]{3,8}\b/.test(source) && !rel.endsWith('tokens.css')) {
    failures.push(`${rel}: hex color literal`);
  }
}

if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log('web token contract ok');
