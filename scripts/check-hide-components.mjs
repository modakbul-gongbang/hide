#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {inventory, sources as swiftSources} from './check-design-controls.mjs';
import {tokens} from './swift-source-tokens.mjs';

const root = process.argv[2] && !process.argv[2].startsWith('--') ? process.argv[2] : 'macos/Sources/HerdrMacOS';
const owners = [
  'HideTheme',
  'HideKeycap',
  'HideBalloon',
  'HideIconButton',
  'HideBadge',
  'HideFormPicker',
  'HideTextButtonStyle',
  'HideChoiceGroup',
  'HideSearchField',
  'HideInputSurface',
  'HideCheckboxStyle',
  'HideDisclosureStyle',
  'HideInteractiveButtonStyle',
  'HideEmptyState',
  'HideMenuChipLabel',
];
// The Pet surface has its own visual language. Keep this exemption explicit
// so a newly named or nested file cannot silently opt out of ownership checks.
const petFiles = new Set([
  'PetAnimation.swift',
  'PetDashboardProjection.swift',
  'PetHotkey.swift',
  'PetMenuBar.swift',
  'PetSettings.swift',
  'PetTheme.swift',
  'PetURLCommand.swift',
  'PetView.swift',
  'PetWindow.swift',
]);
function hasCall(source, name) {
  const ts = tokens(source);
  return ts.some((token, index) => token === name && ts[index + 1] === '(');
}
function hasQualifiedCall(source, qualifier, member) {
  const ts = tokens(source);
  return ts.some((token, index) => token === qualifier && ts[index + 1] === '.'
    && ts[index + 2] === member && ts[index + 3] === '(');
}
function hasModifierCall(source, member, argument) {
  const ts = tokens(source);
  return ts.some((token, index) => token === '.' && ts[index + 1] === member
    && ts[index + 2] === '(' && ts[index + 3] === argument);
}
function violations(directory, structureOnly = false) {
  const problems=[];
  const sources = new Map(swiftSources(directory).filter(({file})=>!petFiles.has(file))
    .map(({file,source})=>[file,source]));
  for(const owner of owners) {
    const file=owner+'.swift';
    const definition=new RegExp('\\b(?:struct|enum|class)\\s+'+owner+'\\b');
    if(!definition.test(sources.get(file)??'')) problems.push(`${file} must define ${owner}`);
    for(const [other,source] of sources) if(other!==file&&definition.test(source)) problems.push(`${other} duplicates ${owner}`);
  }
  for(const [file,source] of sources) {
    if(/\b(?:HideToolbarButtonStyle|HideDestructiveButtonStyle)\b/.test(source)) problems.push(`${file}: obsolete text button style; use HideTextButtonStyle and Button role`);
    if(/\bPaneHeaderButton\b/.test(source)) problems.push(`${file}: obsolete pane header icon button; use HideIconButton`);
    if(/\bHideSettingsKeycaps\b/.test(source)) problems.push(`${file}: obsolete keycaps`);
    if(/\bComposerChipLabel\b/.test(source)) problems.push(`${file}: obsolete composer menu chip; use HideMenuChipLabel`);
    if(/(?:SidebarBadge|HideBadge)\(\s*label:\s*"[⌘⌃⌥⇧]/.test(source)) problems.push(`${file}: badge used as keycap`);
    if(file!=='HideKeycap.swift'&&/Text\(\s*"[⌘⌃⌥⇧]/.test(source)) problems.push(`${file}: inline keycap drawing`);
    if(file!=='HideBalloon.swift'&&/\.background\(HideTheme\.balloon/.test(source)) problems.push(`${file}: balloon drawing outside component`);
  }
  const overview = sources.get('CheckoutOverview.swift') ?? '';
  const cleanup = sources.get('MergedWorktreeCleanup.swift') ?? '';
  const shell = sources.get('ShellView.swift') ?? '';
  const sidebar = sources.get('HideUI.swift') ?? '';
  const settings = sources.get('HideSettings.swift') ?? '';
  const overviewInventory = inventory(overview);
  const shellInventory = inventory(shell);
  const sidebarInventory = inventory(sidebar);
  const settingsInventory = inventory(settings);
  if ((overviewInventory['native-style:pickerStyle:segmented'] ?? 0) > 0) problems.push('CheckoutOverview.swift: use the shared choice control instead of stock segmented appearance');
  if ([shellInventory, sidebarInventory, settingsInventory].some(counted => (counted['native-style:pickerStyle:segmented'] ?? 0) > 0)) {
    problems.push('Known selector surface: use HideChoiceGroup instead of stock segmented appearance');
  }
  const cleanupInventory = inventory(cleanup);
  if ((cleanupInventory['native-style:toggleStyle:checkbox'] ?? 0) > 0
      || (sidebarInventory['native-style:toggleStyle:checkbox'] ?? 0) > 0) {
    problems.push('Known checkbox surface: use HideCheckboxStyle instead of stock checkbox appearance');
  }
  if ((overviewInventory['control:DisclosureGroup'] ?? 0) > 0 && !hasModifierCall(overview, 'disclosureGroupStyle', 'HideDisclosureStyle')) problems.push('CheckoutOverview.swift: apply the shared disclosure appearance');
  const requiredChoiceGroups = [
    ['CheckoutOverview.swift', overview],
    ['ShellView.swift', shell],
    ['HideUI.swift', sidebar],
    ['HideSettings.swift', settings],
  ];
  for (const [file, source] of requiredChoiceGroups) {
    if (source && !hasCall(source, 'HideChoiceGroup')) {
      problems.push(`${file}: known selector surface must use HideChoiceGroup`);
    }
  }
  const rightPanel = sources.get('RightPanel.swift') ?? '';
  if (rightPanel && !hasCall(rightPanel, 'PanelHeader')) {
    problems.push('RightPanel.swift: use the shared PanelHeader section surface');
  } else if (hasCall(rightPanel, 'PanelHeader') && !hasQualifiedCall(rightPanel, 'PanelHeader', 'Sections')) {
    problems.push('RightPanel.swift: pass its section choices through PanelHeader.Sections');
  }
  if (/\bPanelSectionPicker\b/.test(shell) && !hasCall(shell, 'HideChoiceGroup')) {
    problems.push('ShellView.swift: PanelSectionPicker must use HideChoiceGroup');
  }
  for (const [file, source] of sources) {
    const counted = inventory(source);
    const nativeInputCount = ['TextField', 'TextEditor', 'SecureField']
      .reduce((count, control) => count + (counted[`control:${control}`] ?? 0), 0);
    if (nativeInputCount > 0 && !/\.hideInputSurface\s*\(/.test(source)) {
      problems.push(`${file}: native text input must use HideInputSurface`);
    }
    const emptyCount = counted['control:ContentUnavailableView'] ?? 0;
    // PetDashboard intentionally retains one system empty state. Every other
    // shell empty/error state belongs to HideEmptyState.
    const allowedPetEmptyStates = file === 'HideUI.swift' ? 1 : 0;
    if (emptyCount > allowedPetEmptyStates) {
      problems.push(`${file}: use HideEmptyState instead of ContentUnavailableView`);
    }
  }
  if(!structureOnly) {
    // These are the baseline files with native .help call sites. Browser pane
    // controls also inherit the common icon button's tooltip implementation.
    const migrated=['HideUI.swift','HideSettings.swift','RightPanel.swift','ShellView.swift'];
    for(const file of migrated) if(!sources.get(file)?.includes('.hideTooltip(')) problems.push(`${file}: tooltip migration missing`);
    const count=[...sources.values()].reduce((n,s)=>n+(s.match(/\.hideTooltip\(/g)?.length??0),0);
    if(count<28)problems.push(`Expected at least 28 tooltip references, found ${count}`);
  }
  return problems;
}
const structureOnly=process.argv.includes('--structure-only');
const issues=violations(root,structureOnly);
if(issues.length) { console.error(issues.join('\n'));process.exitCode=1; }
else {
  const fixture=fs.mkdtempSync(path.join(os.tmpdir(),'hide-components-'));
  try {
    fs.cpSync(root,fixture,{recursive:true});
    fs.appendFileSync(path.join(fixture,'HideUI.swift'),'\nstruct HideKeycap {}\n');
    assert(violations(fixture,structureOnly).some(s=>s.includes('duplicates HideKeycap')),'Positive duplicate fixture must fail');
    fs.appendFileSync(path.join(fixture,'CheckoutOverview.swift'),'\nlet legacy = Picker("Mode") {}.pickerStyle(.segmented)\n');
    assert(violations(fixture,structureOnly).some(s=>s.includes('stock segmented appearance')),'Reintroducing the stock choice appearance must fail');
    fs.appendFileSync(path.join(fixture,'MergedWorktreeCleanup.swift'),'\nlet legacy = Toggle("Remove").toggleStyle(.checkbox)\n');
    assert(violations(fixture,structureOnly).some(s=>s.includes('Known checkbox surface')),'Reintroducing stock checkbox appearance must fail');
    fs.appendFileSync(path.join(fixture,'HideUI.swift'),'\nstruct PaneHeaderButton {}\n');
    assert(violations(fixture,structureOnly).some(s=>s.includes('obsolete pane header icon button')),'Retired button fixture must fail');
  } finally {fs.rmSync(fixture,{recursive:true,force:true});}
  console.log(`Component ownership and positive duplicate fixture: PASS${structureOnly?' (structure-only invocation)':''}`);
}
