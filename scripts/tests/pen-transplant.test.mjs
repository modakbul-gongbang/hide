import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {parseArgs, transplant} from '../pen-transplant.mjs';

const repository = fileURLToPath(new URL('../../', import.meta.url));

function screen(id, name, extra = {}) {
  return {id, type: 'frame', name, x: 0, y: 0, children: [], ...extra};
}

function doc(children, extra = {}) {
  return {version: '2', variables: {}, children, ...extra};
}

test('replacing a sheet keeps its position and leaves every other sheet identical', () => {
  const untouchedA = screen('sys-1', 'System / Button');
  const untouchedB = screen('sys-2', 'System / Badge');
  const oldScreen = screen('scr-home', 'Screen / Home', {x: 10, y: 20});
  const into = doc([untouchedA, oldScreen, untouchedB]);

  const newScreen = screen('scr-home', 'Screen / Home', {x: 99, y: 200, children: [{id: 'scr-home-title', type: 'text'}]});
  const from = doc([newScreen]);

  const {document} = transplant(from, into, ['scr-home']);
  assert.deepEqual(document.children, [untouchedA, newScreen, untouchedB]);
  assert.equal(document.children[1].x, 99);
  // The source sheet is not aliased into the result.
  assert.notEqual(document.children[1], newScreen);
});

test('a sheet absent from --into is appended', () => {
  const into = doc([screen('sys-1', 'System / Button')]);
  const newScreen = screen('scr-new', 'Screen / New Area');
  const from = doc([newScreen]);

  const {document} = transplant(from, into, ['scr-new']);
  assert.equal(document.children.length, 2);
  assert.equal(document.children[1].id, 'scr-new');
});

test('an id missing from --from is refused and nothing else changes', () => {
  const into = doc([screen('sys-1', 'System / Button')]);
  const from = doc([screen('scr-home', 'Screen / Home')]);
  assert.throws(() => transplant(from, into, ['scr-missing']), /not found in --from/);
});

test('a --sheet id naming a non-Screen node is refused', () => {
  const into = doc([]);
  const from = doc([screen('sys-1', 'System / Button')]);
  assert.throws(() => transplant(from, into, ['sys-1']), /not a top-level "Screen \/ " sheet/);
});

test('an undefined variable is refused, naming it', () => {
  const into = doc([], {variables: {'--color-primary': {type: 'color', value: '#fff'}}});
  const from = doc([screen('scr-home', 'Screen / Home', {children: [{id: 'a', type: 'text', fill: '$--color-missing'}]})]);
  assert.throws(() => transplant(from, into, ['scr-home']), /--color-missing/);
});

test('a $--variable --into already defines is accepted', () => {
  const into = doc([], {variables: {'--color-primary': {type: 'color', value: '#fff'}}});
  const from = doc([screen('scr-home', 'Screen / Home', {children: [{id: 'a', type: 'text', fill: '$--color-primary'}]})]);
  assert.doesNotThrow(() => transplant(from, into, ['scr-home']));
});

test('a ref targeting an import alias --into lacks is refused', () => {
  const into = doc([], {imports: {libA: {status: 'ok'}}});
  const from = doc([screen('scr-home', 'Screen / Home', {children: [{id: 'a', type: 'ref', ref: 'libB:comp-1'}]})]);
  assert.throws(() => transplant(from, into, ['scr-home']), /libB/);
});

test('a ref targeting an import alias --into has is accepted', () => {
  const into = doc([], {imports: {libA: {status: 'ok'}}});
  const from = doc([screen('scr-home', 'Screen / Home', {children: [{id: 'a', type: 'ref', ref: 'libA:comp-1'}]})]);
  assert.doesNotThrow(() => transplant(from, into, ['scr-home']));
});

test('a node id colliding with an id elsewhere in --into is refused, naming it', () => {
  const into = doc([screen('sys-1', 'System / Button', {children: [{id: 'shared', type: 'text'}]})]);
  const from = doc([screen('scr-home', 'Screen / Home', {children: [{id: 'shared', type: 'text'}]})]);
  assert.throws(() => transplant(from, into, ['scr-home']), /shared/);
});

test('a node id colliding with a node written whole in a descendants entry of --into is refused', () => {
  const instance = {id: 'inst', type: 'ref', ref: 'row-m', descendants: {act: {id: 'act', type: 'frame'}}};
  const into = doc([screen('scr-other', 'Screen / Other', {children: [instance]})]);
  const from = doc([screen('scr-home', 'Screen / Home', {children: [{id: 'act', type: 'text'}]})]);
  assert.throws(() => transplant(from, into, ['scr-home']), /act .*inst\.descendants\.act/);
});

test('a node id matching the sheet being replaced is not a collision', () => {
  const into = doc([screen('scr-home', 'Screen / Home', {children: [{id: 'inner', type: 'text'}]})]);
  const from = doc([screen('scr-home', 'Screen / Home', {children: [{id: 'inner', type: 'text', fill: 'red'}]})]);
  assert.doesNotThrow(() => transplant(from, into, ['scr-home']));
});

test('duplicate --sheet ids are refused', () => {
  const into = doc([]);
  const from = doc([screen('scr-home', 'Screen / Home')]);
  assert.throws(() => transplant(from, into, ['scr-home', 'scr-home']), /given more than once/);
});

test('--from and --into at different versions are refused', () => {
  const into = doc([], {version: '3'});
  const from = doc([screen('scr-home', 'Screen / Home')], {version: '2'});
  assert.throws(() => transplant(from, into, ['scr-home']), /version/);
});

test('parseArgs collects repeated --sheet flags and defaults --out', () => {
  const args = parseArgs(['--from', 'a.pen', '--into', 'b.pen', '--sheet', 'x', '--sheet', 'y']);
  assert.deepEqual(args, {from: 'a.pen', into: 'b.pen', sheets: ['x', 'y'], out: undefined});
});

test('parseArgs requires --from, --into and at least one --sheet', () => {
  assert.throws(() => parseArgs(['--into', 'b.pen', '--sheet', 'x']), /--from is required/);
  assert.throws(() => parseArgs(['--from', 'a.pen', '--sheet', 'x']), /--into is required/);
  assert.throws(() => parseArgs(['--from', 'a.pen', '--into', 'b.pen']), /at least one --sheet/);
});

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pen-transplant-'));
  t.after(() => fs.rmSync(root, {recursive: true, force: true}));
  return root;
}

function cli(root, args) {
  return spawnSync(process.execPath, [path.join(repository, 'scripts/pen-transplant.mjs'), ...args], {cwd: root, encoding: 'utf8', timeout: 10_000});
}

test('CLI end to end: writes the transplanted document to --out', t => {
  const root = fixture(t);
  const fromPath = path.join(root, 'branch.pen');
  const intoPath = path.join(root, 'main.pen');
  const outPath = path.join(root, 'out.pen');
  fs.writeFileSync(fromPath, JSON.stringify(doc([screen('scr-home', 'Screen / Home', {x: 5, y: 5})])));
  fs.writeFileSync(intoPath, JSON.stringify(doc([screen('sys-1', 'System / Button')])));

  const result = cli(root, ['--from', fromPath, '--into', intoPath, '--sheet', 'scr-home', '--out', outPath]);
  assert.equal(result.status, 0, result.stderr);
  const written = JSON.parse(fs.readFileSync(outPath, 'utf8'));
  assert.equal(written.children.length, 2);
  assert.ok(written.children.some(child => child.id === 'scr-home'));
  // --into on disk is untouched when --out names a separate file.
  const untouchedInto = JSON.parse(fs.readFileSync(intoPath, 'utf8'));
  assert.equal(untouchedInto.children.length, 1);
});

test('CLI end to end: defaults --out to --into and writes in place', t => {
  const root = fixture(t);
  const fromPath = path.join(root, 'branch.pen');
  const intoPath = path.join(root, 'main.pen');
  fs.writeFileSync(fromPath, JSON.stringify(doc([screen('scr-home', 'Screen / Home')])));
  fs.writeFileSync(intoPath, JSON.stringify(doc([])));

  const result = cli(root, ['--from', fromPath, '--into', intoPath, '--sheet', 'scr-home']);
  assert.equal(result.status, 0, result.stderr);
  const written = JSON.parse(fs.readFileSync(intoPath, 'utf8'));
  assert.equal(written.children.length, 1);
});

test('CLI end to end: a refusal exits nonzero and writes nothing', t => {
  const root = fixture(t);
  const fromPath = path.join(root, 'branch.pen');
  const intoPath = path.join(root, 'main.pen');
  const outPath = path.join(root, 'out.pen');
  fs.writeFileSync(fromPath, JSON.stringify(doc([screen('sys-1', 'System / Button')])));
  const intoText = JSON.stringify(doc([]));
  fs.writeFileSync(intoPath, intoText);

  const result = cli(root, ['--from', fromPath, '--into', intoPath, '--sheet', 'sys-1', '--out', outPath]);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /not a top-level/);
  assert.equal(fs.existsSync(outPath), false);
  assert.equal(fs.readFileSync(intoPath, 'utf8'), intoText);
});
