import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync, execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const repository = fileURLToPath(new URL('../../', import.meta.url));
function fixture(t, git = false) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hide-design-test-'));
  t.after(() => fs.rmSync(root, {recursive: true, force: true}));
  fs.mkdirSync(path.join(root, 'scripts'));
  for (const file of ['check-design-contract.mjs', 'check-design-controls.mjs', 'check-hide-theme-literals.mjs', 'check-hide-components.mjs', 'swift-source-tokens.mjs']) fs.copyFileSync(path.join(repository, 'scripts', file), path.join(root, 'scripts', file));
  // Minimal source fixtures exercise checker CLI behavior without depending on
  // whichever product UI happens to be present or being edited in the repo.
  for (const owner of ['HideTheme', 'HideKeycap', 'HideBalloon', 'HideIconButton', 'HideBadge', 'HideFormPicker', 'HideTextButtonStyle', 'HideChoiceGroup', 'HideSearchField', 'HideCheckboxStyle', 'HideDisclosureStyle', 'HideInteractiveButtonStyle']) {
    write(root, owner + '.swift', `struct ${owner} {}`);
  }
  for (const name of ['HideUI', 'HideSettings', 'CheckoutSummaryCard', 'RightPanel', 'ShellView']) {
    write(root, name + '.swift', 'Text("Fixture").hideTooltip("Fixture")\n'.repeat(name === 'HideUI' ? 24 : 1));
  }
  write(root, 'CheckoutOverview.swift', 'Picker("Project view", selection: $mode) {}');
  fs.writeFileSync(path.join(root, 'scripts/design-control-policy.json'), JSON.stringify({version: 1, files: {
    'CheckoutOverview.swift': {kind: 'legacy', reason: 'Fixture legacy control', rules: {'control:Picker': 1}},
  }}));
  fs.cpSync(path.join(repository, '.githooks'), path.join(root, '.githooks'), {recursive: true});
  if (git) { command(root, ['init', '--quiet']); command(root, ['add', '.']); }
  return root;
}
function command(root, args) { return execFileSync('git', args, {cwd: root, encoding: 'utf8'}); }
function run(root, name, args = []) {
  return spawnSync(process.execPath, ['scripts/' + name, ...args], {cwd: root, encoding: 'utf8'});
}
function write(root, name, content) {
  const file = path.join(root, 'macos/Sources/HerdrMacOS', name);
  fs.mkdirSync(path.dirname(file), {recursive: true}); fs.writeFileSync(file, content);
}

test('new controls and styles are rejected in nested or owner-looking filenames; comments are not code', t => {
  const root = fixture(t);
  assert.equal(run(root, 'check-design-controls.mjs').status, 0);
  write(root, 'Nested/HideNewView.swift', '// Picker("ignored")\nText("DisclosureGroup(ignored)")');
  assert.equal(run(root, 'check-design-controls.mjs').status, 0);
  write(root, 'Nested/HideNewView.swift', 'SwiftUI.Picker(\n"Mode", selection: $mode) {}\nstruct NewStyle: ButtonStyle {}\nstruct QualifiedStyle: SwiftUI.ButtonStyle {}\nTextField("Raw", text: $text)\nTextEditor(text: $text)\nSecureField("Secret", text: $text)');
  const result = run(root, 'check-design-controls.mjs');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Nested\/HideNewView.swift: control:Picker/);
  assert.match(result.stderr, /implementation:ButtonStyle has 2, allowed 0/);
  for (const control of ['TextField', 'TextEditor', 'SecureField']) assert.match(result.stderr, new RegExp('control:' + control));
});

test('counted legacy allowance cannot grow and must be retired when usage disappears', t => {
  const root = fixture(t), file = path.join(root, 'macos/Sources/HerdrMacOS/CheckoutOverview.swift');
  const original = fs.readFileSync(file, 'utf8');
  fs.appendFileSync(file, '\nPicker("Another", selection: $mode) {}');
  assert.match(run(root, 'check-design-controls.mjs').stderr, /control:Picker has 2, allowed 1/);
  fs.writeFileSync(file, original.replace('Picker("Project view"', 'HideChoices("Project view"'));
  assert.match(run(root, 'check-design-controls.mjs').stderr, /retire stale allowance control:Picker/);
});

test('nested source cannot evade literal and duplicate component ownership checks', t => {
  const root = fixture(t);
  write(root, 'Nested/HideTheme.swift', 'struct HideKeycap {}\nText("bad").padding(99)');
  assert.notEqual(run(root, 'check-hide-theme-literals.mjs').status, 0);
  assert.match(run(root, 'check-hide-components.mjs').stderr, /duplicates HideKeycap/);
});

test('staged hook rejects staged violations despite a clean working copy, and ignores unstaged violations', t => {
  const root = fixture(t, true), file = 'macos/Sources/HerdrMacOS/NewView.swift';
  const hook = () => spawnSync('sh', ['.githooks/pre-commit'], {cwd: root, encoding: 'utf8'});
  write(root, 'NewView.swift', 'Picker("Bad", selection: $mode) {}'); command(root, ['add', file]);
  write(root, 'NewView.swift', 'Text("Clean working copy")');
  const before = command(root, ['write-tree']), work = fs.readFileSync(path.join(root, file));
  const failed = hook(); assert.notEqual(failed.status, 0); assert.match(failed.stderr, /control:Picker/);
  assert.equal(command(root, ['write-tree']), before);
  assert.deepEqual(fs.readFileSync(path.join(root, file)), work);
  command(root, ['add', file]); write(root, 'NewView.swift', 'Toggle("Unstaged", isOn: $value)');
  const next = command(root, ['write-tree']); const unstaged = fs.readFileSync(path.join(root, file));
  const passed = hook(); assert.equal(passed.status, 0, passed.stderr);
  assert.equal(command(root, ['write-tree']), next);
  assert.deepEqual(fs.readFileSync(path.join(root, file)), unstaged);
  const commit = spawnSync('git', ['-c', 'core.hooksPath=.githooks', '-c', 'commit.gpgsign=false', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-m', 'Check staged design inputs'], {cwd: root, encoding: 'utf8'});
  assert.equal(commit.status, 0, commit.stderr);
  assert.match(commit.stdout + commit.stderr, /Checking staged design inputs/);
  assert.equal(command(root, ['config', '--local', '--default', '', '--get', 'core.hooksPath']).trim(), '');
});
