// Resolve HideTheme's constants into the design canvas's variable values.
//
// HideTheme.swift is the one source of truth for a token, and design/hide.pen is a
// consumer of it, the same way a Swift view is. This module reads the Swift and
// answers what each mapped variable should be; gen-pen.mjs writes those into the
// canvas and check-pen.mjs fails when the canvas disagrees.

import fs from 'node:fs';
import path from 'node:path';

export const THEME = 'macos/Sources/HerdrMacOS/HideTheme.swift';
export const CANVAS = 'design/hide.pen';
export const MAP = 'scripts/pen-token-map.json';

// --- Swift ------------------------------------------------------------------

// Drop a trailing line comment, but only one that starts outside a string literal,
// so a `//` inside a quoted value survives.
function strip(line) {
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    if (line[i] === '"' && line[i - 1] !== '\\') quoted = !quoted;
    else if (!quoted && line[i] === '/' && line[i + 1] === '/') return line.slice(0, i);
  }
  return line;
}

// Constants keyed by dotted path from HideTheme: `spacingMD`, `Typography.micro`.
// Namespaces nest one level in this file and the parser assumes no more; a deeper
// nesting would silently flatten, so it refuses instead.
export function constants(source) {
  const found = new Map();
  const lines = source.split('\n').map(line => strip(line).replace(/\s+$/, ''));
  const stack = [];
  let depth = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const open = /^\s*(?:@MainActor\s+)?(?:enum|struct)\s+([A-Z]\w*)/.exec(line);
    const decl = /^\s*(?:@MainActor\s+)?(?:private\s+)?static\s+let\s+(\w+)(?:\s*:\s*[\w<>\[\], .]+)?\s*=\s*(.*)$/.exec(line);
    if (decl) {
      // A declaration wraps when its value sits on the next line, or when the next
      // line opens with an operator continuing the expression.
      let expression = decl[2].trim();
      while (i + 1 < lines.length && (expression === '' || /^\s*[+\-*/]/.test(lines[i + 1]))) {
        expression = (expression + ' ' + lines[++i].trim()).trim();
      }
      const scope = stack.slice(1);
      if (scope.length > 1) throw new Error(`Namespace nested deeper than the parser handles: ${[...stack, decl[1]].join('.')}`);
      found.set([...scope, decl[1]].join('.'), expression);
    }
    if (open) stack.push(open[1]);
    depth += (line.match(/\{/g) || []).length - (line.match(/\}/g) || []).length;
    while (stack.length > depth) stack.pop();
  }
  return found;
}

const clamp = n => Math.max(0, Math.min(255, Math.round(n * 255)));
const hex = n => clamp(n).toString(16).toUpperCase().padStart(2, '0');

// A derived token is still a token: `lineageIndent` is arithmetic over three other
// constants and the canvas has to carry its result, not a number somebody retyped.
// So numbers are an expression grammar over + - * / ( ) with identifiers resolved
// through the same table, and nothing else. Anything richer is a value we have no
// business guessing at, and says so rather than guessing.
function number(expression, table, seen) {
  const tokens = expression.match(/\d+(?:\.\d+)?|[A-Za-z_]\w*(?:\.\w+)*|[-+*/()]/g);
  if (!tokens || tokens.join('') !== expression.replace(/\s+/g, '')) return null;

  let at = 0;
  const peek = () => tokens[at];
  const take = () => tokens[at++];

  function primary() {
    const token = take();
    if (token === '(') {
      const inner = additive();
      if (take() !== ')') throw new Error(`Unbalanced parentheses in ${expression}`);
      return inner;
    }
    if (token === '-') return -primary();
    if (/^\d/.test(token)) return Number(token);
    if (/^[A-Za-z_]/.test(token)) {
      if (!table) return NaN;
      const resolved = resolve(token, table, new Set(seen));
      if (resolved.type !== 'number') throw new Error(`${token} is not a number`);
      return resolved.value;
    }
    throw new Error(`Unexpected ${token} in ${expression}`);
  }
  function multiplicative() {
    let left = primary();
    while (peek() === '*' || peek() === '/') {
      const operator = take();
      const right = primary();
      left = operator === '*' ? left * right : left / right;
    }
    return left;
  }
  function additive() {
    let left = multiplicative();
    while (peek() === '+' || peek() === '-') {
      const operator = take();
      const right = multiplicative();
      left = operator === '+' ? left + right : left - right;
    }
    return left;
  }

  let result;
  try { result = additive(); } catch { return null; }
  if (at !== tokens.length || !Number.isFinite(result)) return null;
  return result;
}

// Resolve one dotted path to `{type, value}` in the canvas's own vocabulary.
// `seen` breaks an alias cycle rather than overflowing the stack on one.
export function resolve(pathName, table, seen = new Set()) {
  if (seen.has(pathName)) throw new Error(`Alias cycle at ${pathName}`);
  seen.add(pathName);

  // A CGSize is two numbers under one name; the canvas has no size type, so it
  // carries the pair and each half names the component it came from.
  const member = /^(.*)\.(width|height)$/.exec(pathName);
  if (member && !table.has(pathName)) {
    const literal = table.get(member[1]);
    if (literal === undefined) throw new Error(`No HideTheme constant named ${member[1]}`);
    const size = /^CGSize\(width:\s*(.+?),\s*height:\s*(.+?)\)$/.exec(literal.trim());
    if (!size) throw new Error(`${member[1]} is not a CGSize literal: ${literal}`);
    return value(size[member[2] === 'width' ? 1 : 2].trim(), table, seen, pathName);
  }

  const index = /^(.*)\.(\d+)$/.exec(pathName);
  if (index) {
    const literal = table.get(index[1]);
    if (literal === undefined) throw new Error(`No HideTheme constant named ${index[1]}`);
    const inner = /^\[(.*)\]$/.exec(literal.trim());
    if (!inner) throw new Error(`${index[1]} is not an array literal: ${literal}`);
    const parts = splitTop(inner[1]);
    const element = parts[Number(index[2])];
    if (element === undefined) throw new Error(`${index[1]} has no element ${index[2]}`);
    return value(element.trim(), table, seen, pathName);
  }

  const expression = table.get(pathName);
  if (expression === undefined) throw new Error(`No HideTheme constant named ${pathName}`);
  return value(expression, table, seen, pathName);
}

// Split an argument list on top-level commas, so `Color(red: 1, green: 2)` stays whole.
function splitTop(text) {
  const parts = [];
  let depth = 0, start = 0;
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '(' || text[i] === '[') depth++;
    else if (text[i] === ')' || text[i] === ']') depth--;
    else if (text[i] === ',' && depth === 0) { parts.push(text.slice(start, i)); start = i + 1; }
  }
  parts.push(text.slice(start));
  return parts;
}

function value(expression, table, seen, pathName) {
  const text = expression.trim();

  // Both spellings the file uses for a colour it writes as hex: the parsed form and
  // the bare string the icon colours keep so AppKit can read them.
  const literal = /^(?:color\(for:\s*)?"(#[0-9A-Fa-f]{6})"\)?$/.exec(text);
  if (literal) return {type: 'color', value: literal[1].toUpperCase()};

  const rgb = /^Color\(red:\s*(.+?),\s*green:\s*(.+?),\s*blue:\s*(.+?)\)$/.exec(text);
  if (rgb) {
    const parts = [rgb[1], rgb[2], rgb[3]].map(part => number(part.trim(), table, seen));
    if (parts.some(part => part === null)) throw new Error(`Unreadable colour components at ${pathName}: ${text}`);
    return {type: 'color', value: '#' + parts.map(hex).join('')};
  }

  const plain = number(text, table, seen);
  if (plain !== null) return {type: 'number', value: plain};

  const alias = /^[A-Za-z_]\w*(?:\.\w+)*$/.exec(text);
  if (alias) return resolve(text, table, seen);

  throw new Error(`Cannot resolve ${pathName}: ${text}`);
}

// --- what the canvas should carry --------------------------------------------

export function read(root) {
  const map = JSON.parse(fs.readFileSync(path.join(root, MAP), 'utf8'));
  const table = constants(fs.readFileSync(path.join(root, THEME), 'utf8'));
  const expected = new Map();
  for (const [name, swiftPath] of Object.entries(map.mapped)) {
    expected.set(name, resolve(swiftPath, table));
  }
  return {map, table, expected};
}

export function loadCanvas(root) {
  const file = path.join(root, CANVAS);
  if (!fs.existsSync(file)) throw new Error(`${CANVAS} is missing; the design canvas is the file this contract is about`);
  return {file, document: JSON.parse(fs.readFileSync(file, 'utf8'))};
}

// A HideTheme constant must be mapped or excused. Unclaimed is the failure this
// whole contract exists to catch: a token reaching the shell and never the design.
export function unclaimed(map, table) {
  const claimed = new Set(Object.values(map.mapped).map(p => p.replace(/\.(?:\d+|width|height)$/, '')));
  const excused = Object.keys(map.unmapped);
  return [...table.keys()].filter(name =>
    !claimed.has(name) && !excused.some(prefix => name === prefix || name.startsWith(prefix + '.')));
}

// The generated document, as text. Both entrypoints go through this one function so
// the check compares against exactly what the generator would have written.
// Existing variables keep their position, because the rest of the file is the
// designer's and a reordered diff hides the change that matters.
export function apply(document, expected) {
  const variables = {};
  for (const [name, variable] of Object.entries(document.variables)) {
    variables[name] = expected.has(name) ? {...variable, ...expected.get(name)} : variable;
  }
  for (const name of [...expected.keys()].filter(name => !(name in variables)).sort()) {
    variables[name] = expected.get(name);
  }
  return JSON.stringify({...document, variables}, null, 2);
}
