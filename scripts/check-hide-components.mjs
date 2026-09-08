#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {sources as swiftSources} from './check-design-controls.mjs';

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
  'HideCheckboxStyle',
  'HideDisclosureStyle',
  'HideInteractiveButtonStyle',
];
function violations(directory, structureOnly = false) {
  const problems=[];
  const sources = new Map(swiftSources(directory).filter(({file})=>!path.basename(file).startsWith('Pet'))
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
    if(/(?:SidebarBadge|HideBadge)\(\s*label:\s*"[⌘⌃⌥⇧]/.test(source)) problems.push(`${file}: badge used as keycap`);
    if(file!=='HideKeycap.swift'&&/Text\(\s*"[⌘⌃⌥⇧]/.test(source)) problems.push(`${file}: inline keycap drawing`);
    if(file!=='HideBalloon.swift'&&/\.background\(HideTheme\.balloon/.test(source)) problems.push(`${file}: balloon drawing outside component`);
  }
  const overview = sources.get('CheckoutOverview.swift') ?? '';
  const cleanup = sources.get('MergedWorktreeCleanup.swift') ?? '';
  if (/\.pickerStyle\(\.segmented\)/.test(overview)) problems.push('CheckoutOverview.swift: use the shared choice control instead of stock segmented appearance');
  if (/\.toggleStyle\(\.checkbox\)/.test(cleanup)) problems.push('MergedWorktreeCleanup.swift: use the shared checkbox appearance');
  if (/DisclosureGroup\(/.test(overview) && !/\.disclosureGroupStyle\(HideDisclosureStyle\(\)\)/.test(overview)) problems.push('CheckoutOverview.swift: apply the shared disclosure appearance');
  if(!structureOnly) {
    // These are the baseline files with native .help call sites. Browser pane
    // controls also inherit the common icon button's tooltip implementation.
    const migrated=['HideUI.swift','HideSettings.swift','CheckoutSummaryCard.swift','RightPanel.swift','ShellView.swift'];
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
    assert(violations(fixture,structureOnly).some(s=>s.includes('shared checkbox appearance')),'Reintroducing stock checkbox appearance must fail');
    fs.appendFileSync(path.join(fixture,'HideUI.swift'),'\nstruct PaneHeaderButton {}\n');
    assert(violations(fixture,structureOnly).some(s=>s.includes('obsolete pane header icon button')),'Retired button fixture must fail');
  } finally {fs.rmSync(fixture,{recursive:true,force:true});}
  console.log(`Component ownership and positive duplicate fixture: PASS${structureOnly?' (T2 structure only; T5 migration pending)':''}`);
}
