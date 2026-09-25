#!/usr/bin/env node
// Read design/tokens.json and write web/src/tokens.css.
//
// Colors carry a Dark value and a Light value: `:root` (and `.light`) holds
// Light and `.dark` holds Dark, the way shadcn themes switch, and an alias names another token so
// `--ring` follows `--primary` whichever theme is on. The Tailwind v4 theme maps
// the utility namespaces onto these variables and resets Tailwind's own color,
// text and radius scales, so only a token can reach a class.

import fs from 'node:fs';
import path from 'node:path';

export const TOKENS = 'design/tokens.json';
export const CSS = 'web/src/tokens.css';
export const ACCENTS = 'web/src/generated/accents.ts';

/** The interface text sizes Settings > Appearance > Interface font scales; terminal and editor text do not. */
export const INTERFACE_TEXT = ['--text-micro', '--text-caption', '--text-body', '--text-subhead', '--text-title', '--text-headline'];

export function readTokens(root = process.cwd()) {
  return JSON.parse(fs.readFileSync(path.join(root, TOKENS), 'utf8')).tokens;
}

function plain(token) {
  if (token.type === 'scalar') return String(token.value);
  if (token.type === 'number') return `${token.value}px`;
  throw new Error(`Not a numeric token: ${JSON.stringify(token)}`);
}

function color(value) {
  if (!/^#[0-9A-Fa-f]{6}$/.test(value)) throw new Error(`Not a hex color: ${value}`);
  return value.toLowerCase();
}

export function generate(root = process.cwd()) {
  const tokens = readTokens(root);
  const theme = [], mapped = [], light = [], dark = [], rest = [];
  for (const [name, token] of Object.entries(tokens)) {
    if (token.type === 'color') {
      if (!token.light) throw new Error(`${name} has no Light value`);
      light.push(`  ${name}: ${color(token.light)};`);
      dark.push(`  ${name}: ${color(token.value)};`);
      mapped.push(`  --color-${name.slice(2)}: var(${name});`);
    } else if (token.type === 'alias') {
      if (!tokens[token.value]) throw new Error(`${name} aliases unknown ${token.value}`);
      // A custom property resolves its var() where it is declared, so each
      // theme block carries the aliases again for its own values.
      light.push(`  ${name}: var(${token.value});`);
      dark.push(`  ${name}: var(${token.value});`);
      mapped.push(`  --color-${name.slice(2)}: var(${name});`);
    } else if (INTERFACE_TEXT.includes(name)) {
      theme.push(`  ${name}: calc(${plain(token)} * var(--interface-scale, 1));`);
    } else if (name.startsWith('--spacing-') || name.startsWith('--radius-')) {
      theme.push(`  ${name}: ${plain(token)};`);
    } else {
      rest.push(`  ${name}: ${plain(token)};`);
    }
  }
  const css = [
    '/* Generated from design/tokens.json by scripts/gen-tokens.mjs. Do not edit. */',
    '@theme static {',
    '  --color-*: initial;',
    '  --text-*: initial;',
    '  --radius-*: initial;',
    ...theme,
    '}',
    '',
    '@theme inline {',
    ...mapped,
    '}',
    '',
    ':root {',
    ...rest,
    '}',
    '',
    '/* `.light` and `.dark` also scope a subtree, so one page can show both (the dev gallery). */',
    ':root,',
    '.light {',
    ...light,
    '}',
    '',
    '.dark {',
    ...dark,
    '}',
    '',
  ].join('\n');
  // The stored accent is the choice's Dark value (the value the core has
  // always stored); the shell maps it back to the choice so each theme draws
  // that choice's own value.
  const accents = Object.entries(tokens)
    .filter(([name]) => name.startsWith('--accent-choice-'))
    .map(([name, token]) => `  ${JSON.stringify(name.slice('--accent-choice-'.length))}: ${JSON.stringify(color(token.value))},`);
  const ts = [
    '/* Generated from design/tokens.json by scripts/gen-tokens.mjs. Do not edit. */',
    '',
    '/** Each accent choice by name, with the value the core stores for it. */',
    'export const ACCENT_STORED_HEX = {',
    ...accents,
    '} as const;',
    '',
  ].join('\n');
  return { css, ts, tokens };
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === 'gen-tokens.mjs';
if (invoked) {
  const root = process.cwd();
  const { css, ts } = generate(root);
  fs.writeFileSync(path.join(root, CSS), css);
  fs.writeFileSync(path.join(root, ACCENTS), ts);
  console.log(`wrote ${CSS} and ${ACCENTS} from ${TOKENS}`);
}
