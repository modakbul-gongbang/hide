#!/usr/bin/env node
// Refuse a design/hide-screens.pen that a reviewer cannot trust (PRD
// web-design-system-reset B19, D-18, D-19).
//
//   node scripts/check-hide-screens.mjs
//
// Six failures, independent of one another and all reported together:
//   - a top-level node whose name does not start with `Screen / `
//   - a `Screen / ` sheet whose subtree carries no `theme: {Mode: 'Light'}`
//     node, or no `theme: {Mode: 'Dark'}` node
//   - a `$--name` variable reference the document's own `variables` block
//     does not define
//   - a `<alias>:<id>` ref, or a `<alias>:<id>` descendant-override key, whose
//     alias is not in `imports`, or whose id does not exist in the imported
//     document
//   - a cross-library ref that leaves one of the imported master's own
//     `$--token` fills or strokes un-restated (the master's own top-level
//     property, or a descendant's, present in the imported document but
//     missing, or still `$alias:token`, at the ref site) - the exact defect
//     class DESIGN_WORKFLOW.md's Pen-toolchain-limits section describes: an
//     un-restated color freezes at the library's own Light value in every
//     frame, Dark included, so this is checked structurally (is every
//     colorable id restated with a local, non-aliased value) rather than by
//     resolving actual rendered colors, which only the Pen CLI itself can do.
//   - `document.variables` differing from what `pen-screens.mjs`'s own
//     `readLocalVariables()` computes from design/tokens.json (the same
//     generator gen-screens.mjs itself calls) - the check reuses that
//     function rather than a second copy of its logic, so the two can never
//     drift from each other by definition; only the document being checked
//     can drift from it.
//
// This is a structural check on whatever document it is pointed at (the real
// file by default, or a copy under --file for the transplant proof in
// scripts/tests/hide-screens.test.mjs), so it passes equally on the
// generator's output and on a scripts/pen-transplant.mjs result. The sixth
// rule is the exception: it needs design/tokens.json and design/hide-ui.lib.pen
// at their real repository paths (via readLocalVariables(root)), so it is
// skipped when `root` does not resolve a real checkout (the transplant test's
// throwaway fixture directory) rather than failing on a missing tree.

import fs from 'node:fs';
import path from 'node:path';
import {readLocalVariables} from './pen-screens.mjs';

const SCREEN_PREFIX = 'Screen / ';
const LOCAL_VARIABLE = /^\$(--[A-Za-z0-9_-]+)$/;
const ALIASED = /^\$?([A-Za-z0-9_-]+):(--[A-Za-z0-9_-]+|[A-Za-z0-9_-]+)$/;
const THEMED_PROPS = ['fill', 'stroke'];

function readDoc(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

/** Every id in `node`'s own subtree, via `children` only. */
function collectIds(node, into = new Set()) {
  if (node && typeof node === 'object' && typeof node.id === 'string') into.add(node.id);
  for (const child of node?.children ?? []) collectIds(child, into);
  return into;
}

/** `node`'s own subtree, via `children` only (matches collectIds's reach). */
function findMaster(node, id) {
  if (node && typeof node === 'object') {
    if (node.id === id) return node;
    for (const child of node.children ?? []) {
      const found = findMaster(child, id);
      if (found) return found;
    }
  }
  return null;
}

/**
 * Every `$--token` fill/stroke `masterId` carries in `importedDocument`: its
 * own top-level property names, and each descendant id's property names (any
 * depth, via `children` only - a nested same-library `ref`'s own internal
 * `descendants` map is not walked into, matching pen-screens.mjs's
 * themedOverrides()).
 */
function colorRequirements(importedDocument, masterId) {
  const master = findMaster({children: importedDocument.children}, masterId);
  if (!master) return null;
  const top = new Set();
  for (const prop of THEMED_PROPS) if (LOCAL_VARIABLE.test(master[prop])) top.add(prop);
  const descendants = new Map();
  (function walk(node) {
    if (!node || typeof node !== 'object') return;
    if (node.id !== masterId) {
      const props = new Set();
      for (const prop of THEMED_PROPS) if (LOCAL_VARIABLE.test(node[prop])) props.add(prop);
      if (props.size) descendants.set(node.id, props);
    }
    for (const child of node.children ?? []) walk(child);
  })(master);
  return {top, descendants};
}

/** Every `ref` node anywhere under `document`, with its alias and target master id (aliased refs only). */
function collectAliasedRefs(document, into = []) {
  (function walk(node) {
    if (!node || typeof node !== 'object') return;
    if (node.type === 'ref' && typeof node.ref === 'string' && node.ref.includes(':')) {
      const colon = node.ref.indexOf(':');
      into.push({node, alias: node.ref.slice(0, colon), masterId: node.ref.slice(colon + 1)});
    }
    for (const child of node.children ?? []) walk(child);
  })(document);
  return into;
}

/** True if `value` is a local (non-aliased) override: any non-empty string that is not `$alias:token`. */
function isLocalOverride(value) {
  return typeof value === 'string' && value.length > 0 && !ALIASED.test(value);
}

/** The descendant-override entry for `id` in a ref's `descendants` map, keyed bare or alias-prefixed. */
function descendantEntry(descendants, id) {
  if (!descendants) return undefined;
  if (descendants[id]) return descendants[id];
  for (const [key, value] of Object.entries(descendants)) if (key === id || key.endsWith(`:${id}`)) return value;
  return undefined;
}

/** True if `node`'s subtree contains a frame tagged `theme: {Mode: mode}`. */
function hasThemeFrame(node, mode) {
  if (node && typeof node === 'object') {
    if (node.theme && node.theme.Mode === mode) return true;
    for (const child of node.children ?? []) if (hasThemeFrame(child, mode)) return true;
  }
  return false;
}

// `content` and `name` are freeform display text (a pane id like "w2:p1", a
// path, a label) - text() even copies `content` into `name` - so both can
// coincidentally match the `alias:id` shape without being one; every other
// property (fill, stroke, ref...) is never arbitrary user-facing text, so
// only those are tested against ALIASED.
const FREEFORM_TEXT_KEYS = new Set(['content', 'name']);

/** Every `$--name` local variable reference, and every `alias:id` string (ref target or descendant-override key), anywhere under `node`. */
function collectReferences(node, into = {local: new Set(), aliased: new Set()}, key) {
  if (typeof node === 'string') {
    const local = LOCAL_VARIABLE.exec(node);
    if (local) into.local.add(local[1]);
    if (!FREEFORM_TEXT_KEYS.has(key)) {
      const aliased = ALIASED.exec(node);
      if (aliased) into.aliased.add(`${aliased[1]}:${aliased[2]}`);
    }
    return into;
  }
  if (Array.isArray(node)) { for (const item of node) collectReferences(item, into, key); return into; }
  if (node && typeof node === 'object') {
    if (node.type === 'ref' && typeof node.ref === 'string' && node.ref.includes(':')) into.aliased.add(node.ref);
    for (const [key, value] of Object.entries(node)) {
      if (key === 'descendants' && value && typeof value === 'object') {
        for (const descendantKey of Object.keys(value)) if (descendantKey.includes(':')) into.aliased.add(descendantKey);
      }
      collectReferences(value, into, key);
    }
  }
  return into;
}

/**
 * Check `document` (loaded from `file`, whose directory resolves `imports`
 * paths) against the six rules above. Returns an array of failure strings;
 * empty means the document passes.
 */
export function check(document, file) {
  const failures = [];
  const root = path.dirname(file);

  const notScreen = (document.children ?? []).filter(node => !(node.name ?? '').startsWith(SCREEN_PREFIX));
  if (notScreen.length) {
    failures.push(`${notScreen.length} top-level node(s) are not a "${SCREEN_PREFIX}" sheet:\n` +
      notScreen.map(node => `    ${node.id} (${JSON.stringify(node.name ?? '')})`).join('\n'));
  }

  const missingFrames = [];
  for (const sheet of document.children ?? []) {
    if (!(sheet.name ?? '').startsWith(SCREEN_PREFIX)) continue;
    const missing = [];
    if (!hasThemeFrame(sheet, 'Light')) missing.push('Light');
    if (!hasThemeFrame(sheet, 'Dark')) missing.push('Dark');
    if (missing.length) missingFrames.push(`${sheet.id} (${JSON.stringify(sheet.name)}) is missing a ${missing.join(' and a ')} frame`);
  }
  if (missingFrames.length) failures.push(`${missingFrames.length} sheet(s) missing a required theme frame:\n` + missingFrames.map(line => `    ${line}`).join('\n'));

  const refs = collectReferences(document);
  const definedVariables = new Set(Object.keys(document.variables ?? {}));
  const undefinedVariables = [...refs.local].filter(name => !definedVariables.has(name));
  if (undefinedVariables.length) {
    failures.push(`${undefinedVariables.length} local variable reference(s) design/hide-screens.pen does not define:\n` + undefinedVariables.map(name => `    $${name}`).join('\n'));
  }

  const imports = document.imports ?? {};
  const importedIds = new Map();
  const unresolved = [];
  for (const reference of refs.aliased) {
    const colon = reference.indexOf(':');
    const alias = reference.slice(0, colon);
    const id = reference.slice(colon + 1);
    const importPath = imports[alias];
    if (importPath === undefined) { unresolved.push(`${reference} (no "${alias}" entry in imports)`); continue; }
    if (typeof importPath !== 'string') continue; // an import that is not a path (e.g. the transplant test's {status:'ok'} fixture) is out of scope here
    if (!importedIds.has(alias)) {
      const importedFile = path.resolve(root, importPath);
      importedIds.set(alias, fs.existsSync(importedFile) ? collectIds(readDoc(importedFile)) : new Set());
    }
    if (!importedIds.get(alias).has(id)) unresolved.push(`${reference} (no node "${id}" in ${importPath})`);
  }
  if (unresolved.length) {
    failures.push(`${unresolved.length} ref(s) or descendant override(s) do not resolve against the imported library:\n` + unresolved.map(line => `    ${line}`).join('\n'));
  }

  const importedDocs = new Map();
  const unrestated = [];
  for (const {node, alias, masterId} of collectAliasedRefs(document)) {
    const importPath = imports[alias];
    if (typeof importPath !== 'string') continue; // unresolved import already reported above
    const importedFile = path.resolve(root, importPath);
    if (!importedDocs.has(alias)) importedDocs.set(alias, fs.existsSync(importedFile) ? readDoc(importedFile) : null);
    const importedDocument = importedDocs.get(alias);
    if (!importedDocument) continue; // missing import already reported above
    const requirements = colorRequirements(importedDocument, masterId);
    if (!requirements) continue; // unresolved master id already reported above

    for (const prop of requirements.top) {
      if (!isLocalOverride(node[prop])) unrestated.push(`${node.id} (${node.ref}) does not restate its ${prop} (still ${JSON.stringify(node[prop] ?? null)})`);
    }
    for (const [descendantId, props] of requirements.descendants) {
      const entry = descendantEntry(node.descendants, descendantId);
      for (const prop of props) {
        if (!isLocalOverride(entry?.[prop])) unrestated.push(`${node.id} (${node.ref})'s descendant ${descendantId} does not restate its ${prop} (still ${JSON.stringify(entry?.[prop] ?? null)})`);
      }
    }
  }
  if (unrestated.length) {
    failures.push(`${unrestated.length} cross-library color(s) left un-restated, which freezes them at the library's own Light value in every theme:\n` + unrestated.map(line => `    ${line}`).join('\n'));
  }

  try {
    // readLocalVariables(root) wants the repository root (it resolves
    // scripts/pen-token-map.json and design/tokens.json itself), not
    // `root` (this document's own directory, which is design/ for the real
    // file, and an unrelated flat temp directory for a test fixture).
    const repoRoot = path.basename(root) === 'design' ? path.dirname(root) : root;
    const expected = readLocalVariables(repoRoot);
    const actual = document.variables ?? {};
    const names = new Set([...Object.keys(expected), ...Object.keys(actual)]);
    const drifted = [...names].filter(name => JSON.stringify(actual[name]) !== JSON.stringify(expected[name])).sort();
    if (drifted.length) {
      failures.push(`${drifted.length} local variable(s) differ from what design/tokens.json generates (rerun node scripts/gen-screens.mjs):\n` +
        drifted.map(name => `    ${name}: has ${JSON.stringify(actual[name] ?? null)}, tokens.json generates ${JSON.stringify(expected[name] ?? null)}`).join('\n'));
    }
  } catch (error) {
    if (error?.code !== 'ENOENT') throw error; // no real design/tokens.json under root (e.g. a transplant-test fixture): rule 6 does not apply there
  }

  return failures;
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === 'check-hide-screens.mjs';
if (invoked) {
  const args = process.argv.slice(2);
  let file = path.join(process.cwd(), 'design/hide-screens.pen');
  for (let i = 0; i < args.length; i++) if (args[i] === '--file') file = args[++i];

  const failures = check(readDoc(file), file);
  if (failures.length) {
    console.error(`${file} is out of step:\n`);
    for (const failure of failures) console.error(`  ${failure}\n`);
    process.exit(1);
  }
  console.log(`${file} agrees with its imported library: every Screen sheet carries Light and Dark, every reference resolves.`);
}
