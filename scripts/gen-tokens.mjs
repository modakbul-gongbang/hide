#!/usr/bin/env node
// Read design/tokens.json and write web/src/tokens.css.
// HideTheme.swift must resolve to the same values; the checker compares them.

import fs from 'node:fs';
import path from 'node:path';
import { read } from './pen-tokens.mjs';

const TOKENS = 'design/tokens.json';
const CSS = 'web/src/tokens.css';
const THEME_MARK = '// Token authority: design/tokens.json. Do not edit numeric values here; run node scripts/gen-tokens.mjs.';

const shadcn = {
  '--background': '--color-background',
  '--foreground': '--color-primary',
  '--card': '--color-panel',
  '--card-foreground': '--color-primary',
  '--popover': '--color-balloon',
  '--popover-foreground': '--color-primary',
  '--primary': '--color-accent',
  '--primary-foreground': '--color-background',
  '--secondary': '--color-elevated',
  '--secondary-foreground': '--color-primary',
  '--muted': '--color-panel',
  '--muted-foreground': '--color-muted',
  '--accent': '--color-elevated',
  '--accent-foreground': '--color-primary',
  '--destructive': '--color-danger',
  '--border': '--color-divider',
  '--input': '--color-divider',
  '--ring': '--color-accent',
  '--radius': '--radius-sm',
};

function cssValue(token) {
  if (token.type === 'color') return String(token.value).toLowerCase();
  if (typeof token.value === 'number') {
    if (token.swift.includes('Opacity') || token.swift.includes('opacity') || token.swift.includes('Ratio') || token.swift.includes('Skew') || token.swift.includes('Slant')) {
      return String(token.value);
    }
    return `${token.value}px`;
  }
  return String(token.value);
}

export function generate(root = process.cwd()) {
  const file = path.join(root, TOKENS);
  const doc = JSON.parse(fs.readFileSync(file, 'utf8'));
  const lines = [
    '/* Generated from design/tokens.json. Do not edit. */',
    ':root {',
  ];
  for (const [name, token] of Object.entries(doc.tokens)) {
    lines.push(`  ${name}: ${cssValue(token)};`);
  }
  for (const [alias, source] of Object.entries(shadcn)) {
    lines.push(`  ${alias}: var(${source});`);
  }
  lines.push('}', '');
  return { css: lines.join('\n'), doc };
}

export function hideThemeMatches(root = process.cwd()) {
  const doc = JSON.parse(fs.readFileSync(path.join(root, TOKENS), 'utf8'));
  const { expected } = read(root);
  const mismatches = [];
  for (const [name, token] of Object.entries(doc.tokens)) {
    const resolved = expected.get(name);
    if (!resolved) {
      mismatches.push(`${name} missing from HideTheme/pen-token-map`);
      continue;
    }
    const left = String(resolved.value).toLowerCase();
    const right = String(token.value).toLowerCase();
    if (resolved.type !== token.type || left !== right) {
      mismatches.push(`${name}: tokens.json ${token.type} ${token.value} vs HideTheme ${resolved.type} ${resolved.value}`);
    }
  }
  return mismatches;
}

function stampTheme(root) {
  const file = path.join(root, 'macos/Sources/HerdrMacOS/HideTheme.swift');
  const source = fs.readFileSync(file, 'utf8');
  if (source.includes('Token authority: design/tokens.json')) return;
  const next = source.replace(
    'enum HideTheme {',
    `${THEME_MARK}\nenum HideTheme {`,
  );
  fs.writeFileSync(file, next);
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === 'gen-tokens.mjs';
if (invoked) {
  const root = process.cwd();
  const { css } = generate(root);
  fs.mkdirSync(path.join(root, 'web/src'), { recursive: true });
  fs.writeFileSync(path.join(root, CSS), css);
  stampTheme(root);
  const mismatches = hideThemeMatches(root);
  if (mismatches.length) {
    console.error(mismatches.join('\n'));
    process.exit(1);
  }
  console.log(`wrote ${CSS} from ${TOKENS}`);
}
