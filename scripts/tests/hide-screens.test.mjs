import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {fileURLToPath} from 'node:url';
import {check} from '../check-hide-screens.mjs';
import {transplant} from '../pen-transplant.mjs';

const repository = fileURLToPath(new URL('../../', import.meta.url));

function screen(id, name, extra = {}) {
  return {id, type: 'frame', name, x: 0, y: 0, children: [], ...extra};
}

function themed(id, mode, children = []) {
  return {id, type: 'frame', name: mode, theme: {Mode: mode}, children};
}

function doc(children, extra = {}) {
  return {version: '2.18', variables: {}, imports: {}, children, ...extra};
}

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hide-screens-check-'));
  t.after(() => fs.rmSync(root, {recursive: true, force: true}));
  return root;
}

function writeLibrary(root, ids) {
  const file = path.join(root, 'lib.pen');
  fs.writeFileSync(file, JSON.stringify({version: '2.18', variables: {}, children: ids.map(id => ({id, type: 'frame', name: id, children: []}))}));
  return 'lib.pen';
}

// A library whose master carries its own themed fill, and a themed-fill descendant,
// for the un-restated-cross-library-color tests below.
function writeColorLibrary(root) {
  const file = path.join(root, 'lib.pen');
  const master = {id: 'btn-m', type: 'frame', name: 'Button', fill: '$--primary', children: [
    {id: 'btn-lb', type: 'text', name: 'Label', fill: '$--primary-foreground', children: []},
  ]};
  fs.writeFileSync(file, JSON.stringify({version: '2.18', variables: {}, children: [master]}));
  return 'lib.pen';
}

test('a top-level node not named "Screen / " is refused, naming it', t => {
  const root = fixture(t);
  const document = doc([screen('sys-1', 'System / Button', {children: [themed('a-l', 'Light'), themed('a-d', 'Dark')]})]);
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('sys-1') && f.includes('System / Button')));
});

test('an id carried by a descendants entry as well as its node is refused, naming both places', t => {
  const root = fixture(t);
  const button = {id: 'act-1', type: 'frame', name: 'Action', children: []};
  const instance = {id: 'inst-1', type: 'ref', ref: 'row-m', descendants: {'act-1': {id: 'act-1', type: 'frame', name: 'Action'}}};
  const document = doc([screen('scr-1', 'Screen / Main', {children: [themed('scr-1-l', 'Light', [button, instance]), themed('scr-1-d', 'Dark')]})]);
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('act-1') && f.includes('inst-1.descendants.act-1')));
  delete instance.descendants['act-1'].id;
  assert.ok(!check(document, path.join(root, 'hide-screens.pen')).some(f => f.includes('more than one node')));
});

test('a Screen sheet missing a Dark frame is refused, naming it', t => {
  const root = fixture(t);
  const document = doc([screen('scr-1', 'Screen / Main', {children: [themed('scr-1-l', 'Light')]})]);
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('scr-1') && f.includes('Dark')));
});

test('a Screen sheet missing a Light frame is refused, naming it', t => {
  const root = fixture(t);
  const document = doc([screen('scr-1', 'Screen / Main', {children: [themed('scr-1-d', 'Dark')]})]);
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('scr-1') && f.includes('Light')));
});

test('a Screen sheet with both frames passes', t => {
  const root = fixture(t);
  const document = doc([screen('scr-1', 'Screen / Main', {children: [themed('scr-1-l', 'Light'), themed('scr-1-d', 'Dark')]})]);
  assert.deepEqual(check(document, path.join(root, 'hide-screens.pen')), []);
});

test('a local $--variable the document does not define is refused, naming it', t => {
  const root = fixture(t);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'text', fill: '$--missing'}]), themed('scr-1-d', 'Dark')],
  })]);
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('--missing')));
});

test('a local $--variable the document defines is accepted', t => {
  const root = fixture(t);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'text', fill: '$--card'}]), themed('scr-1-d', 'Dark')],
  })], {variables: {'--card': {type: 'color', value: []}}});
  assert.deepEqual(check(document, path.join(root, 'hide-screens.pen')), []);
});

test('a ref whose alias is not in imports is refused, naming it', t => {
  const root = fixture(t);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'ref', ref: 'hideui:btn-m'}]), themed('scr-1-d', 'Dark')],
  })]);
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('hideui:btn-m') && f.includes('imports')));
});

test('a ref whose id does not exist in the imported library is refused, naming it', t => {
  const root = fixture(t);
  const libraryPath = writeLibrary(root, ['btn-m']);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'ref', ref: 'hideui:missing-m'}]), themed('scr-1-d', 'Dark')],
  })], {imports: {hideui: `./${libraryPath}`}});
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('hideui:missing-m')));
});

test('a ref whose id exists in the imported library is accepted', t => {
  const root = fixture(t);
  const libraryPath = writeLibrary(root, ['btn-m']);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'ref', ref: 'hideui:btn-m'}]), themed('scr-1-d', 'Dark')],
  })], {imports: {hideui: `./${libraryPath}`}});
  assert.deepEqual(check(document, path.join(root, 'hide-screens.pen')), []);
});

test('a descendant override key whose id does not exist in the imported library is refused', t => {
  const root = fixture(t);
  const libraryPath = writeLibrary(root, ['btn-m', 'btn-lb']);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'ref', ref: 'hideui:btn-m', descendants: {'hideui:missing-lb': {content: 'x'}}}]), themed('scr-1-d', 'Dark')],
  })], {imports: {hideui: `./${libraryPath}`}});
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('hideui:missing-lb')));
});

test('a descendant override key whose id exists in the imported library is accepted', t => {
  const root = fixture(t);
  const libraryPath = writeLibrary(root, ['btn-m', 'btn-lb']);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'ref', ref: 'hideui:btn-m', descendants: {'hideui:btn-lb': {content: 'x'}}}]), themed('scr-1-d', 'Dark')],
  })], {imports: {hideui: `./${libraryPath}`}});
  assert.deepEqual(check(document, path.join(root, 'hide-screens.pen')), []);
});

test('freeform text content shaped like "alias:id" (a pane label such as "w2:p1") is not mistaken for an unresolved ref', t => {
  const root = fixture(t);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [
      {id: 'a', type: 'text', name: 'w2:p1', content: 'w2:p1', fill: '$--foreground'},
    ]), themed('scr-1-d', 'Dark')],
  })], {variables: {'--foreground': {type: 'color', value: []}}});
  assert.deepEqual(check(document, path.join(root, 'hide-screens.pen')), []);
});

test('every failure class is reported together, not just the first', t => {
  const root = fixture(t);
  const document = doc([
    screen('sys-1', 'System / Button'),
    screen('scr-1', 'Screen / Main', {children: [themed('scr-1-l', 'Light', [{id: 'a', fill: '$--missing'}])]}),
  ]);
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.equal(failures.length, 3); // not-a-Screen-sheet, missing Dark frame, undefined variable
});

test('a ref that leaves the imported master\'s own themed fill un-restated is refused, naming the ref and the property', t => {
  const root = fixture(t);
  const libraryPath = writeColorLibrary(root);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'ref', ref: 'hideui:btn-m'}]), themed('scr-1-d', 'Dark')],
  })], {imports: {hideui: `./${libraryPath}`}});
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('a (hideui:btn-m)') && f.includes('fill')));
});

test('a ref that leaves an imported descendant\'s themed fill un-restated is refused, naming the descendant', t => {
  const root = fixture(t);
  const libraryPath = writeColorLibrary(root);
  const document = doc([screen('scr-1', 'Screen / Main', {
    // The top-level fill is restated; the descendant's is not.
    children: [themed('scr-1-l', 'Light', [{id: 'a', type: 'ref', ref: 'hideui:btn-m', fill: '$--primary'}]), themed('scr-1-d', 'Dark')],
  })], {imports: {hideui: `./${libraryPath}`}});
  const failures = check(document, path.join(root, 'hide-screens.pen'));
  assert.ok(failures.some(f => f.includes('btn-lb') && f.includes('fill')));
});

test('a ref that restates every themed color, at the ref site and at each descendant, is accepted', t => {
  const root = fixture(t);
  const libraryPath = writeColorLibrary(root);
  const document = doc([screen('scr-1', 'Screen / Main', {
    children: [themed('scr-1-l', 'Light', [{
      id: 'a', type: 'ref', ref: 'hideui:btn-m', fill: '$--primary',
      descendants: {'hideui:btn-lb': {fill: '$--primary-foreground'}},
    }]), themed('scr-1-d', 'Dark')],
  })], {imports: {hideui: `./${libraryPath}`}, variables: {'--primary': {type: 'color', value: []}, '--primary-foreground': {type: 'color', value: []}}});
  assert.deepEqual(check(document, path.join(root, 'hide-screens.pen')), []);
});

test('a document whose local variables differ from what design/tokens.json generates is refused, naming the variable', () => {
  const file = path.join(repository, 'design/hide-screens.pen');
  const original = JSON.parse(fs.readFileSync(file, 'utf8'));
  const tampered = structuredClone(original);
  const [name] = Object.keys(tampered.variables);
  tampered.variables[name] = {type: 'number', value: -999999};
  const failures = check(tampered, file);
  assert.ok(failures.some(f => f.includes('differ from what design/tokens.json generates') && f.includes(name)));
});

test('a fixture with no real design/tokens.json under it is not held to the local-variable-drift rule', t => {
  const root = fixture(t);
  const document = doc([screen('scr-1', 'Screen / Main', {children: [themed('scr-1-l', 'Light'), themed('scr-1-d', 'Dark')]})]);
  assert.deepEqual(check(document, path.join(root, 'hide-screens.pen')), []);
});

test('the real design/hide-screens.pen passes', () => {
  const document = JSON.parse(fs.readFileSync(path.join(repository, 'design/hide-screens.pen'), 'utf8'));
  const failures = check(document, path.join(repository, 'design/hide-screens.pen'));
  assert.deepEqual(failures, []);
});

test('a scripts/pen-transplant.mjs result still passes: one sheet from a modified copy transplanted into the original', () => {
  const file = path.join(repository, 'design/hide-screens.pen');
  const original = JSON.parse(fs.readFileSync(file, 'utf8'));
  const modified = structuredClone(original);
  const sheetId = modified.children.find(child => (child.name ?? '').startsWith('Screen / '))?.id;
  assert.ok(sheetId, 'fixture needs at least one Screen / sheet to modify');
  const sheet = modified.children.find(child => child.id === sheetId);
  sheet.children.push(themed(`${sheetId}-transplant-probe`, 'Light', [{id: `${sheetId}-transplant-probe-text`, type: 'text', fill: '$--card'}]));

  const {document} = transplant(modified, original, [sheetId]);
  const failures = check(document, file);
  assert.deepEqual(failures, []);
});
