import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn, spawnSync, execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {childIndent, evaluate, rootsAligned, sharedColumns, stable} from '../design-review-rules.mjs';

const repository = fileURLToPath(new URL('../../', import.meta.url));

test('area focus follows the keyboard without changing selected tabs or geometry', () => {
  const row = (id, keyboard) => ({id, keyboard, x: id === 'a1' ? 0 : 480, y: 0, width: 480, height: 440, selected: 1, filter: 'none'});
  const measured = {areaFocus: {before: [row('a1', true), row('a2', false)], after: [row('a1', false), row('a2', true)]}};
  assert.equal(evaluate({areaFocus: true}, measured)[0].pass, true);
  measured.areaFocus.after[0].keyboard = true;
  measured.areaFocus.after[1].width -= 2;
  measured.areaFocus.after[1].filter = 'blur(1px)';
  const failed = evaluate({areaFocus: true}, measured)[0];
  assert.equal(failed.pass, false);
  assert.ok(failed.problems.some(problem => problem.includes('keyboard')));
  assert.ok(failed.problems.some(problem => problem.includes('moved or resized')));
  assert.ok(failed.problems.some(problem => problem.includes('filter')));
  assert.equal(evaluate({areaFocus: true}, {})[0].pass, null);
});

// -- the rules, with expected answers written from the rule, not the code -----

const columns = (...rows) => [{list: 'main', rows: rows.map(([pane, depth, mark, title]) => ({pane, depth, mark, title}))}];

test('a child one level down 12px right of its root meets a 12px rule and fails an 18px one', () => {
  const measured = columns(['root', 0, 100, 124], ['child', 1, 112, 136], ['grandchild', 2, 124, 148]);
  assert.deepEqual(childIndent(measured, 12).problems, []);
  const wider = childIndent(measured, 18);
  assert.equal(wider.problems.length, 4);
  assert.match(wider.problems[0], /child: mark is 12px right of the roots at depth 1, expected 18px/);
});

test('a title that does not move with its mark breaks the indent even when the mark is right', () => {
  const measured = columns(['root', 0, 100, 124], ['child', 1, 112, 130]);
  assert.deepEqual(childIndent(measured, 12).problems, ['main child: title is 6px right of the roots at depth 1, expected 12px']);
});

test('with no child row on screen the indent is not judged rather than passed', () => {
  const [result] = evaluate({childIndentPx: 12}, {columns: columns(['root', 0, 100, 124])});
  assert.equal(result.pass, null);
});

test('roots of one list start on one column', () => {
  assert.equal(rootsAligned(columns(['a', 0, 100, 1], ['b', 0, 100.3, 1])).problems.length, 0);
  assert.equal(rootsAligned(columns(['a', 0, 100, 1], ['b', 0, 104, 1])).problems.length, 1);
});

test('a row that grows under the pointer and pushes the next row is reported with both rows', () => {
  const rest = {'agent:a': {x: 0, y: 0, width: 280, height: 28}, 'agent:b': {x: 0, y: 28, width: 280, height: 28}};
  const hovered = {'agent:a': {'agent:a': {x: 0, y: 0, width: 280, height: 44}, 'agent:b': {x: 0, y: 44, width: 280, height: 28}}};
  const result = stable(rest, hovered, 'hover');
  assert.deepEqual(result.problems, ['hover agent:a: agent:a height 28 -> 44', 'hover agent:a: agent:b y 28 -> 44']);
  assert.deepEqual(stable(rest, {'agent:a': rest}, 'hover').problems, []);
});

test('two time columns or two chevron columns fail the shared columns rule', () => {
  assert.deepEqual(sharedColumns({times: [260], chevrons: [274]}).problems, []);
  assert.equal(sharedColumns({times: [255, 260], chevrons: [274]}).problems.length, 1);
});

test('a rule the command cannot measure is listed as not judged', () => {
  const [result] = evaluate({colorContrast: 4.5}, {});
  assert.equal(result.pass, null);
  assert.match(result.measured, /does not know how to measure/);
});

// -- baseline and show, against a fake Pen at the process boundary ------------

// Writes a PNG header of the requested size for every node an Export names;
// FAKE_PEN_MODE selects a failure. Nothing of ours is replaced.
const fakePen = `#!${process.execPath}
const fs = require('node:fs');
const args = process.argv.slice(2);
const mode = process.env.FAKE_PEN_MODE ?? '';
if (args[0] === 'version') { console.log('pen 0.3.8'); process.exit(); }
if (mode === 'logged-out') { console.error('[ERROR] Authentication required. Run "pen login" or set PEN_CLI_KEY environment variable.'); process.exit(1); }
if (mode === 'hang') { fs.appendFileSync(process.env.FAKE_PEN_LOG, JSON.stringify({hang: process.pid}) + '\\n'); setInterval(() => {}, 1000); }
let input = '';
process.stdin.on('data', chunk => { input += chunk; });
process.stdin.on('end', () => {
  const file = args[args.indexOf('-i') + 1];
  fs.appendFileSync(process.env.FAKE_PEN_LOG, JSON.stringify({file, lib: fs.existsSync(require('node:path').join(require('node:path').dirname(file), 'lib.pen'))}) + '\\n');
  const call = JSON.parse(input.slice(input.indexOf('input: ') + 7, input.lastIndexOf(' })')));
  const close = call.indexOf(']');
  const nodes = JSON.parse(call.slice(call.indexOf('['), close + 1));
  const directory = JSON.parse(call.slice(close + 1).split(', ')[2]);
  for (const node of nodes) {
    if (mode === 'missing-node' && node === nodes[0]) continue;
    const png = Buffer.alloc(24);
    png.write('.PNG', 0, 'latin1');
    png.writeUInt32BE(mode === 'wide' ? 600 : 584, 16);
    png.writeUInt32BE(1000, 20);
    fs.writeFileSync(require('node:path').join(directory, node + '.png'), png);
  }
  console.log('Exported');
});
`;

function fixture(t) {
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'hide-design-review-test-')));
  t.after(() => fs.rmSync(directory, {recursive: true, force: true}));
  const root = path.join(directory, 'checkout');
  for (const dir of ['scripts', 'design', 'web/src/assets']) fs.mkdirSync(path.join(root, dir), {recursive: true});
  fs.mkdirSync(path.join(directory, 'bin'));
  for (const name of ['design-review.mjs', 'design-review-rules.mjs', 'pen-cli.mjs']) fs.copyFileSync(path.join(repository, 'scripts', name), path.join(root, 'scripts', name));
  fs.writeFileSync(path.join(root, '.gitignore'), '/agents/\n');
  fs.writeFileSync(path.join(root, 'design/review-targets.json'), JSON.stringify({
    sample: {file: 'design/screens.pen', sheet: 's', scene: 'projects-sidebar', selector: 'nav', frames: [{node: 'f-l', theme: 'light', width: 292, scale: 1, content: 'reference', state: 'rest'}, {node: 'f-d', theme: 'dark', width: 292, scale: 1, content: 'reference', state: 'rest'}], conditions: {themes: ['light'], widths: [292], contents: ['reference'], scales: [1]}, states: {rest: 'at rest'}, rules: {childIndentPx: 12}, judgment: []},
  }));
  fs.writeFileSync(path.join(root, 'design/lib.pen'), JSON.stringify({version: '2', children: [{id: 'm', url: '../web/src/assets/mark.png'}]}));
  fs.writeFileSync(path.join(root, 'design/screens.pen'), JSON.stringify({version: '2', imports: {ui: './lib.pen'}, children: [{id: 'f-l', children: [{id: 'img', fill: {type: 'image', url: '../web/src/assets/mark.png'}}]}]}));
  fs.writeFileSync(path.join(root, 'web/src/assets/mark.png'), 'mark bytes');
  fs.writeFileSync(path.join(directory, 'bin/pen'), fakePen, {mode: 0o755});
  execFileSync('git', ['init', '--quiet'], {cwd: root});
  execFileSync('git', ['add', '.'], {cwd: root});
  execFileSync('git', ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false', 'commit', '--quiet', '-m', 'Fixture'], {cwd: root});
  const log = path.join(directory, 'pen.log');
  fs.writeFileSync(log, '');
  const env = {...process.env, PATH: path.join(directory, 'bin') + path.delimiter + process.env.PATH, FAKE_PEN_LOG: log};
  return {root, directory, env, log};
}

function run(f, args, extraEnv = {}) {
  return spawnSync(process.execPath, [path.join(f.root, 'scripts/design-review.mjs'), ...args], {cwd: f.root, env: {...f.env, ...extraEnv}, encoding: 'utf8', timeout: 20_000});
}

const baselineArgs = (name = 'one') => ['baseline', 'task', '--target', 'sample', '--approval', 'delegated', '--reference', 'PRD D-08', '--name', name];
const bundleOf = (f, name = 'one') => path.join(f.root, 'agents/runs/task/design/baseline', name);

test('a baseline carries the Pen file, its library and images, exports from itself, and records what it is', t => {
  const f = fixture(t);
  const result = run(f, baselineArgs());
  assert.equal(result.status, 0, result.stderr);
  const bundle = bundleOf(f);
  const manifest = JSON.parse(fs.readFileSync(path.join(bundle, 'manifest.json'), 'utf8'));
  assert.equal(manifest.approval.kind, 'delegated');
  assert.equal(manifest.approval.reference, 'PRD D-08');
  assert.deepEqual(manifest.frames.map(frame => [frame.node, frame.theme, frame.width, frame.png]), [['f-l', 'light', 292, 'png/f-l.png'], ['f-d', 'dark', 292, 'png/f-d.png']]);
  assert.deepEqual(manifest.rules, {childIndentPx: 12});
  assert.equal(manifest.source.file, 'design/screens.pen');
  // The copy names its library beside it and its image inside the bundle.
  const screens = JSON.parse(fs.readFileSync(path.join(bundle, 'screens.pen'), 'utf8'));
  assert.deepEqual(screens.imports, {ui: './lib.pen'});
  const image = screens.children[0].children[0].fill.url;
  assert.match(image, /^assets\/[0-9a-f]{12}-mark\.png$/);
  assert.equal(fs.readFileSync(path.join(bundle, image), 'utf8'), 'mark bytes');
  assert.equal(JSON.parse(fs.readFileSync(path.join(bundle, 'lib.pen'), 'utf8')).children[0].url, image);
  // Pen rendered the bundle's own copy, with its library next to it.
  const [call] = fs.readFileSync(f.log, 'utf8').trim().split('\n').map(line => JSON.parse(line));
  assert.ok(call.file.includes(`${path.sep}.one-`));
  assert.equal(call.lib, true);
  // Every file is listed with its hash, and show reads it back without Pen.
  assert.deepEqual(Object.keys(manifest.files).sort(), [image, 'lib.pen', 'png/f-d.png', 'png/f-l.png', 'screens.pen'].sort());
  const shown = spawnSync(process.execPath, [path.join(f.root, 'scripts/design-review.mjs'), 'show', bundle], {cwd: f.root, encoding: 'utf8', env: {...process.env, PATH: '/usr/bin:/bin'}});
  assert.equal(shown.status, 0, shown.stderr);
  assert.match(shown.stdout, /delegated \(PRD D-08\)/);
  assert.match(shown.stdout, /every file matches its hash/);
});

test('the bundle opens from another directory with nothing outside it', t => {
  const f = fixture(t);
  assert.equal(run(f, baselineArgs()).status, 0);
  const moved = path.join(f.directory, 'elsewhere');
  fs.cpSync(bundleOf(f), moved, {recursive: true});
  fs.rmSync(path.join(f.root, 'web'), {recursive: true});
  fs.rmSync(path.join(f.root, 'design/lib.pen'));
  const shown = spawnSync(process.execPath, [path.join(f.root, 'scripts/design-review.mjs'), 'show', moved], {cwd: f.root, encoding: 'utf8'});
  assert.equal(shown.status, 0, shown.stdout + shown.stderr);
});

test('a rerun never overwrites a bundle', t => {
  const f = fixture(t);
  const first = run(f, baselineArgs());
  assert.equal(first.status, 0, first.stderr);
  const before = fs.readFileSync(path.join(bundleOf(f), 'manifest.json'), 'utf8');
  const again = run(f, baselineArgs());
  assert.notEqual(again.status, 0);
  assert.match(again.stderr, /already exists and is never overwritten/);
  assert.equal(fs.readFileSync(path.join(bundleOf(f), 'manifest.json'), 'utf8'), before);
});

test('show reports a bundle whose file changed after it was made', t => {
  const f = fixture(t);
  assert.equal(run(f, baselineArgs()).status, 0);
  fs.writeFileSync(path.join(bundleOf(f), 'png/f-l.png'), 'edited');
  const shown = run(f, ['show', bundleOf(f)]);
  assert.equal(shown.status, 1);
  assert.match(shown.stdout, /png\/f-l\.png changed since the bundle was made/);
});

for (const [mode, env, pattern, status] of [
  ['no pen on PATH', {PATH: '/usr/bin:/bin'}, /Pen CLI is not installed/, 3],
  ['a logged-out Pen', {FAKE_PEN_MODE: 'logged-out'}, /Pen is not logged in\.\nRun: pen login/, 3],
  ['a frame Pen did not export', {FAKE_PEN_MODE: 'missing-node'}, /exported no image for f-l/, 1],
  ['a frame wider than its declared condition', {FAKE_PEN_MODE: 'wide'}, /f-l is 300px wide, but the target declares 292px/, 1],
]) test(`${mode} leaves no bundle and says why`, t => {
  const f = fixture(t);
  const result = run(f, baselineArgs(), env);
  assert.equal(result.status, status, result.stderr);
  assert.match(result.stderr, pattern);
  const parent = path.dirname(bundleOf(f));
  assert.deepEqual(fs.existsSync(parent) ? fs.readdirSync(parent) : [], []);
});

test('an approval that is neither user nor delegated, or no decision reference, is refused before anything runs', t => {
  const f = fixture(t);
  const args = baselineArgs();
  args[args.indexOf('delegated')] = 'approved';
  assert.equal(run(f, args).status, 2);
  const noReference = baselineArgs();
  noReference[noReference.indexOf('PRD D-08')] = ' ';
  assert.equal(run(f, noReference).status, 2);
  assert.equal(fs.readFileSync(f.log, 'utf8'), '');
  assert.equal(fs.existsSync(path.join(f.root, 'agents')), false);
});

test('two different libraries with one file name are refused rather than one overwriting the other', t => {
  const f = fixture(t);
  for (const dir of ['a', 'b']) {
    fs.mkdirSync(path.join(f.root, 'design', dir));
    fs.writeFileSync(path.join(f.root, 'design', dir, 'lib.pen'), JSON.stringify({version: '2', children: [{id: dir}]}));
  }
  fs.writeFileSync(path.join(f.root, 'design/screens.pen'), JSON.stringify({version: '2', imports: {a: './a/lib.pen', b: './b/lib.pen'}, children: [{id: 'f-l'}]}));
  const result = run(f, baselineArgs());
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /Two Pen files would both be lib\.pen in the bundle/);
  assert.deepEqual(fs.readdirSync(path.dirname(bundleOf(f))), []);
});

test('a library that imports the document back is copied once', t => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, 'design/lib.pen'), JSON.stringify({version: '2', imports: {screens: './screens.pen'}, children: []}));
  const result = run(f, baselineArgs());
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(bundleOf(f), 'lib.pen'), 'utf8')).imports, {screens: './screens.pen'});
});

test('a cancel while the current Pen frames export ends the whole review, not just Pen', async t => {
  const f = fixture(t);
  assert.equal(run(f, baselineArgs()).status, 0);
  fs.writeFileSync(f.log, '');
  const child = spawn(process.execPath, [path.join(f.root, 'scripts/design-review.mjs'), 'review', 'task', '--baseline', bundleOf(f)], {cwd: f.root, env: {...f.env, FAKE_PEN_MODE: 'hang'}, stdio: ['ignore', 'pipe', 'pipe']});
  let output = '';
  child.stdout.on('data', chunk => { output += chunk; });
  child.stderr.on('data', chunk => { output += chunk; });
  const exited = new Promise(resolve => child.once('exit', (code, signal) => resolve({code, signal})));
  let penPid;
  for (let i = 0; i < 200 && !penPid; i += 1) {
    await new Promise(resolve => setTimeout(resolve, 50));
    penPid = fs.readFileSync(f.log, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line)).find(entry => entry.hang)?.hang;
  }
  assert.ok(penPid, `Pen export never started: ${output}`);
  child.kill('SIGTERM');
  const timeout = setTimeout(() => child.kill('SIGKILL'), 10_000);
  const {code} = await exited;
  clearTimeout(timeout);
  assert.equal(code, 143);
  assert.throws(() => process.kill(penPid, 0), {code: 'ESRCH'});
  // It stopped instead of going on to the browser part and writing a report.
  const runs = path.join(f.root, 'agents/runs/task/design/review');
  for (const dir of fs.readdirSync(runs)) assert.equal(fs.existsSync(path.join(runs, dir, 'report.json')), false);
});

test('a bundle made from a scratch proposal is reviewed against the target\'s own Pen file, and says so', t => {
  const f = fixture(t);
  fs.mkdirSync(path.join(f.root, 'scratch'));
  fs.writeFileSync(path.join(f.root, 'scratch/proposal.pen'), JSON.stringify({version: '2', imports: {ui: '../design/lib.pen'}, children: [{id: 'f-l'}]}));
  const args = baselineArgs();
  args.push('--from', 'scratch/proposal.pen');
  assert.equal(run(f, args).status, 0);
  fs.writeFileSync(f.log, '');
  // The fixture has no static contract and no web app, so the run fails; only its Pen part is asserted.
  run(f, ['review', 'task', '--baseline', bundleOf(f)]);
  const [call] = fs.readFileSync(f.log, 'utf8').trim().split('\n').map(line => JSON.parse(line));
  assert.equal(call.file, path.join(f.root, 'design/screens.pen'));
  const runs = path.join(f.root, 'agents/runs/task/design/review');
  const [dir] = fs.readdirSync(runs);
  const report = JSON.parse(fs.readFileSync(path.join(runs, dir, 'report.json'), 'utf8'));
  assert.equal(report.pen.status, 'RENDERED');
  assert.equal(report.pen.changedSinceBaseline, null);
  assert.match(fs.readFileSync(path.join(runs, dir, 'report.md'), 'utf8'), /design\/screens\.pen, while the baseline was made from scratch\/proposal\.pen/);
});

test('one library reached through a symlinked directory is still one library', t => {
  const f = fixture(t);
  fs.symlinkSync(path.join(f.root, 'design'), path.join(f.root, 'alias'));
  fs.writeFileSync(path.join(f.root, 'design/screens.pen'), JSON.stringify({version: '2', imports: {a: './lib.pen', b: '../alias/lib.pen'}, children: [{id: 'f-l'}]}));
  const result = run(f, baselineArgs());
  assert.equal(result.status, 0, result.stderr);
});
