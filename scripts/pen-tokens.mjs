// Resolve design/tokens.json into the design canvas's variables.
//
// tokens.json is the one source of truth for a token, and design/hide-ui.lib.pen is a
// consumer of it, the same way web/src/tokens.css is. This module answers what each
// canvas variable should be; gen-pen.mjs writes those answers into the canvas and
// check-pen.mjs fails when the canvas disagrees. Every token becomes a Pen variable
// of the SAME name: a color token carries hide's whole theme axis, `Mode: [Light,
// Dark]`, the same two values gen-tokens.mjs writes into `:root`/`.light` and `.dark`.

import fs from 'node:fs';
import path from 'node:path';
import {readTokens} from './gen-tokens.mjs';

export const CANVAS = 'design/hide-ui.lib.pen';
export const MAP = 'scripts/pen-token-map.json';
export const THEMES = {Mode: ['Light', 'Dark']};

function hex(value) {
  if (!/^#[0-9A-Fa-f]{6}$/.test(value)) throw new Error(`Not a 6-digit hex color: ${value}`);
  return value.toUpperCase();
}

// A token resolves to {type, value} in the canvas's vocabulary. An alias carries no
// value of its own and resolves through its chain to the color it names; `seen`
// breaks a cycle rather than overflowing the stack on one.
function resolveToken(name, tokens, seen) {
  if (seen.has(name)) throw new Error(`Alias cycle at ${name}`);
  seen.add(name);
  const token = tokens[name];
  if (!token) throw new Error(`No token named ${name}`);
  if (token.type === 'alias') {
    if (!tokens[token.value]) throw new Error(`${name} aliases unknown token ${token.value}`);
    return resolveToken(token.value, tokens, seen);
  }
  if (token.type === 'color') {
    if (!token.light) throw new Error(`${name} has no Light value`);
    return {type: 'color', value: [{value: hex(token.light)}, {value: hex(token.value), theme: {Mode: 'Dark'}}]};
  }
  if (token.type === 'number' || token.type === 'scalar') {
    if (typeof token.value !== 'number') throw new Error(`${name} is not numeric: ${JSON.stringify(token.value)}`);
    return {type: 'number', value: token.value};
  }
  throw new Error(`Unknown token type at ${name}: ${token.type}`);
}

export function read(root) {
  const map = JSON.parse(fs.readFileSync(path.join(root, MAP), 'utf8'));
  const tokens = readTokens(root);
  const expected = new Map();
  for (const name of Object.keys(tokens)) expected.set(name, resolveToken(name, tokens, new Set()));
  return {map, tokens, expected};
}

export function loadCanvas(root) {
  const file = path.join(root, CANVAS);
  if (!fs.existsSync(file)) throw new Error(`${CANVAS} is missing; the design canvas is the file this contract is about`);
  return {file, document: JSON.parse(fs.readFileSync(file, 'utf8'))};
}

// D-21's mechanical rename, applied once: hide's own former color names to the
// shadcn-meaning names D-03 chose. A name absent from the canvas (every run after
// the first) is simply not found below, so this stays a no-op forever after.
export const RENAME = {
  '--color-background': '--background',
  '--color-panel': '--card',
  '--color-balloon': '--popover',
  '--color-elevated': '--secondary',
  '--color-divider': '--border',
  '--color-primary': '--foreground',
  '--color-secondary': '--subtle-foreground',
  '--color-muted': '--muted-foreground',
  '--color-accent': '--primary',
  '--color-danger': '--destructive',
  '--color-sidebar': '--sidebar',
  '--color-accent-choice-lime': '--accent-choice-lime',
  '--color-accent-choice-sky': '--accent-choice-sky',
  '--color-accent-choice-violet': '--accent-choice-violet',
  '--color-accent-choice-amber': '--accent-choice-amber',
  '--color-agent-working': '--agent-working',
  '--color-file-neutral': '--file-neutral',
  '--color-file-document': '--file-document',
  '--color-file-blue': '--file-blue',
  '--color-file-green': '--file-green',
  '--color-file-orange': '--file-orange',
  '--color-file-yellow': '--file-yellow',
  '--color-file-purple': '--file-purple',
  '--color-pr-open': '--pr-open',
  '--color-pr-merged': '--pr-merged',
  '--color-pr-closed': '--pr-closed',
  '--color-pr-draft': '--pr-draft',
  '--color-diff-added': '--diff-added',
  '--color-diff-removed': '--diff-removed',
  '--color-success': '--success',
  '--color-warning': '--warning',
};

// Also rename the `ref` a component instance points at, when the master it names
// was itself renamed (a node id, not a variable, so it is a different table).
export const REF_RENAME = {};

function renameValue(value) {
  if (typeof value === 'string' && value[0] === '$' && RENAME[value.slice(1)]) return '$' + RENAME[value.slice(1)];
  return value;
}

// Rewrite every exact `$--old-name` reference and `--old-name` variable key to its
// renamed form. A structural walk, not a text substitution: a variable name is
// always a whole property value or a whole variable-dict key, never a substring of
// one, so a short old name (`--color-accent`) can never eat a longer one
// (`--color-accent-choice-lime`) the way a naive string replace could.
export function renamed(document) {
  function walk(node) {
    if (Array.isArray(node)) return node.map(walk);
    if (node && typeof node === 'object') {
      const out = {};
      for (const [key, value] of Object.entries(node)) {
        if (key === 'ref' && typeof value === 'string' && REF_RENAME[value]) out[key] = REF_RENAME[value];
        else out[key] = walk(value);
      }
      return out;
    }
    return renameValue(node);
  }
  const children = document.children.map(walk);
  const variables = {};
  for (const [name, value] of Object.entries(document.variables)) variables[RENAME[name] ?? name] = value;
  return {...document, children, variables};
}

// A canvas variable must be generated from tokens.json or excused as design-authored
// (a font name, a derived wash, proposal/as-built geometry). Unclaimed is the
// failure this check exists to catch: a variable the design carries that neither
// side accounts for, drifting silently from both.
export function unclaimed(map, document, expected) {
  const excused = new Set(Object.keys(map.authored));
  return Object.keys(document.variables).filter(name => !expected.has(name) && !excused.has(name));
}

// The generated document, as text. Both entrypoints go through this one function so
// the check compares against exactly what the generator would have written.
// Existing variables keep their position, because the rest of the file is the
// designer's and a reordered diff hides the change that matters.
export function apply(document, expected) {
  const variables = {};
  for (const [name, variable] of Object.entries(document.variables)) {
    variables[name] = expected.has(name) ? expected.get(name) : variable;
  }
  for (const name of [...expected.keys()].filter(name => !(name in variables)).sort()) {
    variables[name] = expected.get(name);
  }
  return JSON.stringify({...document, themes: THEMES, variables}, null, 2);
}
