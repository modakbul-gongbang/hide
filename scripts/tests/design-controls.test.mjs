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
  for (const owner of ['HideTheme', 'HideKeycap', 'HideBalloon', 'HideIconButton', 'HideBadge', 'HideFormPicker', 'HideTextButtonStyle', 'HideChoiceGroup', 'HideSearchField', 'HideInputSurface', 'HideCheckboxStyle', 'HideDisclosureStyle', 'HideInteractiveButtonStyle', 'HideEmptyState', 'HideMenuChipLabel']) {
    write(root, owner + '.swift', `struct ${owner} {}`);
  }
  for (const name of ['HideUI', 'HideSettings']) {
    write(root, name + '.swift', 'Text("Fixture").hideTooltip("Fixture")\n'.repeat(name === 'HideUI' ? 24 : 1));
  }
  write(root, 'RightPanel.swift', 'PanelHeader(sections: PanelHeader.Sections(active: section, select: select))\nText("Fixture").hideTooltip("Fixture")');
  write(root, 'ShellView.swift', 'HideChoiceGroup(label: "Right panel section")\n' + 'Text("Fixture").hideTooltip("Fixture")');
  write(root, 'HideUI.swift', 'HideChoiceGroup(label: "Sidebar view")\n' + 'Text("Fixture").hideTooltip("Fixture")'.repeat(24));
  write(root, 'HideSettings.swift', 'HideChoiceGroup(label: "Settings section")\n' + 'Text("Fixture").hideTooltip("Fixture")');
  write(root, 'CheckoutOverview.swift', 'HideChoiceGroup(label: "Project view")\nPicker("Project view", selection: $mode) {}\nText("Fixture").hideTooltip("Fixture")');
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

test('migrated selector, input, checkbox and empty-state surfaces fail on real caller regressions', t => {
  const baseline = fixture(t);
  assert.equal(run(baseline, 'check-hide-components.mjs').status, 0);

  const selector = fixture(t);
  const selectorFile = path.join(selector, 'macos/Sources/HerdrMacOS/ShellView.swift');
  fs.writeFileSync(selectorFile, fs.readFileSync(selectorFile, 'utf8').replace('HideChoiceGroup', 'LocalChoiceGroup'));
  const selectorResult = run(selector, 'check-hide-components.mjs');
  assert.notEqual(selectorResult.status, 0);
  assert.match(selectorResult.stderr, /ShellView\.swift: known selector surface must use HideChoiceGroup/);

  const panel = fixture(t);
  const panelFile = path.join(panel, 'macos/Sources/HerdrMacOS/RightPanel.swift');
  fs.writeFileSync(panelFile, fs.readFileSync(panelFile, 'utf8').replace('sections: PanelHeader.Sections', 'title: "Local tabs"'));
  const panelResult = run(panel, 'check-hide-components.mjs');
  assert.notEqual(panelResult.status, 0);
  assert.match(panelResult.stderr, /RightPanel\.swift: pass its section choices through PanelHeader\.Sections/);

  const input = fixture(t);
  const inputFile = path.join(input, 'macos/Sources/HerdrMacOS/BrowserPaneView.swift');
  write(input, 'BrowserPaneView.swift', 'TextField("Address", text: $query)\n    .hideInputSurface(compact: true)');
  fs.writeFileSync(inputFile, fs.readFileSync(inputFile, 'utf8').replace(/\n\s*\.hideInputSurface\([^\n]*\)/, ''));
  const inputResult = run(input, 'check-hide-components.mjs');
  assert.notEqual(inputResult.status, 0);
  assert.match(inputResult.stderr, /BrowserPaneView\.swift: native text input must use HideInputSurface/);

  const checkbox = fixture(t);
  fs.appendFileSync(path.join(checkbox, 'macos/Sources/HerdrMacOS/HideUI.swift'), '\nToggle("Legacy checkbox", isOn: $value).toggleStyle(.checkbox)\n');
  const checkboxResult = run(checkbox, 'check-hide-components.mjs');
  assert.notEqual(checkboxResult.status, 0);
  assert.match(checkboxResult.stderr, /Known checkbox surface: use HideCheckboxStyle/);

  const empty = fixture(t);
  fs.appendFileSync(path.join(empty, 'macos/Sources/HerdrMacOS/RightPanel.swift'), '\nContentUnavailableView("Legacy empty", systemImage: "xmark")\n');
  const emptyResult = run(empty, 'check-hide-components.mjs');
  assert.notEqual(emptyResult.status, 0);
  assert.match(emptyResult.stderr, /RightPanel\.swift: use HideEmptyState instead of ContentUnavailableView/);
});

test('plain Button remains allowed when it is not a migrated shared control', t => {
  const root = fixture(t);
  write(root, 'AllowedButton.swift', 'Button("Menu action") {}.buttonStyle(.plain)');
  const result = run(root, 'check-hide-components.mjs');
  assert.equal(result.status, 0, result.stderr);
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
