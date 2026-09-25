#!/usr/bin/env node
// Move a branch's own `Screen / <Area>` sheets into main's copy of
// design/hide-screens.pen, node by node, so parallel screen PRs never line-merge
// the same JSON file (PRD web-design-system-reset D-19, B20).
//
//   node scripts/pen-transplant.mjs --from <branch.pen> --into <main.pen> \
//     --sheet <id> [--sheet <id> ...] [--out <path>]
//
// Refuses, writing nothing, when:
//   - a --sheet id does not name a top-level `Screen /` sheet in --from
//   - --from and --into disagree on `version`
//   - a transplanted sheet uses a `$--variable` --into does not define
//   - a transplanted sheet's `ref` targets an import alias --into's `imports` lacks
//   - a node id inside a transplanted sheet collides with an id living
//     elsewhere in --into (outside the sheet(s) being replaced)
//   - the same --sheet id is given more than once
//
// A named sheet already present in --into is replaced in place, at its
// existing position, with the branch's node whole (including its placement:
// the sheet's layout belongs to the branch that drew it). A named sheet
// absent from --into is appended. Every other top-level node of --into is
// untouched.

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {serialize} from './pen-bands.mjs';
import {duplicateIds} from './pen-canvas.mjs';

const SCREEN_PREFIX = 'Screen / ';
const LOCAL_VARIABLE = /^\$(--[A-Za-z0-9_-]+)$/;
const IMPORTED_VARIABLE = /^\$([A-Za-z0-9_-]+):(--[A-Za-z0-9_-]+)$/;

/** Parse `--from <path> --into <path> --sheet <id> ... [--out <path>]`. */
export function parseArgs(argv) {
  const args = {from: undefined, into: undefined, sheets: [], out: undefined};
  for (let i = 0; i < argv.length; i++) {
    const token = argv[i];
    if (token === '--from') args.from = argv[++i];
    else if (token === '--into') args.into = argv[++i];
    else if (token === '--sheet') args.sheets.push(argv[++i]);
    else if (token === '--out') args.out = argv[++i];
    else throw new Error(`Unknown argument: ${token}`);
  }
  if (!args.from) throw new Error('--from is required');
  if (!args.into) throw new Error('--into is required');
  if (args.sheets.length === 0) throw new Error('at least one --sheet is required');
  return args;
}

/** Every `$--name` and `$alias:--name` string value anywhere under `node`. */
function collectVariableRefs(node, into = {local: new Set(), aliases: new Set()}) {
  if (typeof node === 'string') {
    const local = LOCAL_VARIABLE.exec(node);
    if (local) into.local.add(local[1]);
    const imported = IMPORTED_VARIABLE.exec(node);
    if (imported) into.aliases.add(imported[1]);
    return into;
  }
  if (Array.isArray(node)) { for (const item of node) collectVariableRefs(item, into); return into; }
  if (node && typeof node === 'object') { for (const value of Object.values(node)) collectVariableRefs(value, into); return into; }
  return into;
}

/** Every import alias a `ref` node's target qualifies with (`<alias>:<component-id>`). */
function collectRefAliases(node, into = new Set()) {
  if (node && typeof node === 'object') {
    if (node.type === 'ref' && typeof node.ref === 'string' && node.ref.includes(':')) {
      into.add(node.ref.slice(0, node.ref.indexOf(':')));
    }
    for (const child of node.children ?? []) collectRefAliases(child, into);
  }
  return into;
}

/**
 * Transplant `ids` from `fromDoc`'s top-level `Screen /` sheets into
 * `intoDoc`, whole and in place. Returns `{document}`, or throws an Error
 * naming the refusal; nothing about `intoDoc` is mutated.
 */
export function transplant(fromDoc, intoDoc, ids) {
  const duplicated = [...new Set(ids.filter((id, index) => ids.indexOf(id) !== index))];
  if (duplicated.length) throw new Error(`--sheet given more than once: ${duplicated.join(', ')}`);

  if (fromDoc.version !== intoDoc.version) {
    throw new Error(`--from is version ${JSON.stringify(fromDoc.version)} but --into is version ${JSON.stringify(intoDoc.version)}`);
  }

  const missing = [];
  const notScreen = [];
  const sheets = new Map();
  for (const id of ids) {
    const node = (fromDoc.children ?? []).find(child => child.id === id);
    if (!node) { missing.push(id); continue; }
    if (!(node.name ?? '').startsWith(SCREEN_PREFIX)) { notScreen.push(`${id} (${JSON.stringify(node.name ?? '')})`); continue; }
    sheets.set(id, node);
  }
  if (missing.length) throw new Error(`sheet id(s) not found in --from: ${missing.join(', ')}`);
  if (notScreen.length) throw new Error(`sheet id(s) are not a top-level "${SCREEN_PREFIX}" sheet in --from: ${notScreen.join(', ')}`);

  const definedVariables = new Set(Object.keys(intoDoc.variables ?? {}));
  const definedAliases = new Set(intoDoc.imports ? Object.keys(intoDoc.imports) : []);

  for (const [id, node] of sheets) {
    const refs = collectVariableRefs(node);
    const undefinedVariables = [...refs.local].filter(name => !definedVariables.has(name));
    if (undefinedVariables.length) throw new Error(`sheet ${id} references variable(s) --into does not define: ${undefinedVariables.join(', ')}`);

    const aliases = new Set([...refs.aliases, ...collectRefAliases(node)]);
    const missingAliases = [...aliases].filter(alias => !definedAliases.has(alias));
    if (missingAliases.length) throw new Error(`sheet ${id} references import alias(es) --into lacks: ${missingAliases.join(', ')}`);
  }

  const children = [...(intoDoc.children ?? [])];
  for (const id of ids) {
    const node = structuredClone(sheets.get(id));
    const index = children.findIndex(child => child.id === id);
    if (index === -1) children.push(node);
    else children[index] = node;
  }
  // The result has to load in Pen: one id per node, counting a node written
  // whole inside a ref's `descendants` (the replaced sheets' own ids are gone).
  const collisions = duplicateIds(children);
  if (collisions.length) {
    throw new Error(`the transplanted sheet(s) leave node id(s) colliding with --into: ${collisions.map(([id, paths]) => `${id} (${paths.join(', ')})`).join('; ')}`);
  }
  return {document: {...intoDoc, children}};
}

function readDoc(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

// Write beside the target and rename over it, so a crash mid-write never
// leaves --into truncated or half-transplanted.
function writeAtomic(file, text) {
  const tmp = path.join(path.dirname(file), `.${path.basename(file)}.${crypto.randomUUID()}.tmp`);
  fs.writeFileSync(tmp, text);
  fs.renameSync(tmp, file);
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === 'pen-transplant.mjs';
if (invoked) {
  try {
    const args = parseArgs(process.argv.slice(2));
    const {document} = transplant(readDoc(args.from), readDoc(args.into), args.sheets);
    const out = args.out ?? args.into;
    writeAtomic(out, serialize(document));
    console.log(`Transplanted ${args.sheets.length} sheet(s) into ${out}`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
