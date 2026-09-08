#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {tokens} from './swift-source-tokens.mjs';

const policyPath = new URL('./design-control-policy.json', import.meta.url);
export function inventory(source) {
  const ts = tokens(source), result = {};
  const record = key => { result[key] = (result[key] ?? 0) + 1; };
  const controls = new Set(['Picker', 'Toggle', 'DisclosureGroup', 'ContentUnavailableView', 'TextField', 'TextEditor', 'SecureField']);
  const styles = new Set(['ButtonStyle', 'PrimitiveButtonStyle', 'ToggleStyle', 'TextFieldStyle', 'DisclosureGroupStyle']);
  for (let i = 0; i < ts.length; i++) {
    if (controls.has(ts[i]) && ['(', '{', '<'].includes(ts[i + 1]) && !['struct', 'class', 'enum', 'func'].includes(ts[i - 1])) record(`control:${ts[i]}`);
    const protocolStart = ts[i - 1] === '.' && ts[i - 2] === 'SwiftUI' ? i - 2 : i;
    if (styles.has(ts[i]) && [':', ','].includes(ts[protocolStart - 1])) record(`implementation:${ts[i]}`);
    if (ts[i] === '.' && ['pickerStyle', 'toggleStyle', 'buttonStyle', 'textFieldStyle', 'disclosureGroupStyle'].includes(ts[i + 1])
        && ts[i + 2] === '(' && ts[i + 3] === '.' && typeof ts[i + 4] === 'string' && ts[i + 4] !== 'plain') {
      record(`native-style:${ts[i + 1]}:${ts[i + 4]}`);
    }
  }
  return result;
}
export function sources(root, prefix = '') {
  return fs.readdirSync(root, {withFileTypes: true}).flatMap(entry => {
    const relative = path.posix.join(prefix, entry.name), full = path.join(root, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`Design source must not be a symlink: ${relative}`);
    if (entry.isDirectory()) return sources(full, relative);
    return entry.name.endsWith('.swift') ? [{file: relative, source: fs.readFileSync(full, 'utf8')}] : [];
  });
}
export function check(root, policy) {
  const issues = [], found = new Set();
  if (policy.version !== 1 || !policy.files || typeof policy.files !== 'object' || Array.isArray(policy.files)) return ['Unsupported or malformed design control policy.'];
  for (const [file, allowance] of Object.entries(policy.files)) {
    if (!allowance || !['owner', 'legacy', 'platform'].includes(allowance.kind)
        || typeof allowance.reason !== 'string' || !allowance.reason.trim()
        || !allowance.rules || typeof allowance.rules !== 'object' || Array.isArray(allowance.rules)
        || Object.values(allowance.rules).some(count => !Number.isSafeInteger(count) || count <= 0)) {
      issues.push(`${file}: policy needs a kind, reason and positive integer counts.`);
    }
  }
  if (issues.length) return issues;
  for (const {file, source} of sources(root)) {
    const observed = inventory(source), allowance = policy.files[file];
    found.add(file);
    for (const [rule, count] of Object.entries(observed)) {
      const permitted = allowance?.rules[rule] ?? 0;
      if (count > permitted) issues.push(`${file}: ${rule} has ${count}, allowed ${permitted}. Reuse the documented owner, or review an explicit policy change.`);
    }
    for (const [rule, count] of Object.entries(allowance?.rules ?? {})) {
      if ((observed[rule] ?? 0) < count) issues.push(`${file}: retire stale allowance ${rule} (${count} -> ${observed[rule] ?? 0}) in scripts/design-control-policy.json.`);
    }
  }
  for (const [file, allowance] of Object.entries(policy.files)) {
    if (!found.has(file)) issues.push(`${file}: remove allowance for missing source.`);
    if (!['owner', 'legacy', 'platform'].includes(allowance.kind) || !allowance.reason?.trim()) issues.push(`${file}: policy needs a kind and reason.`);
  }
  return issues;
}
if (process.argv[1] && fs.existsSync(process.argv[1]) && fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url))) {
  try {
    const issues = check(process.argv[2] ?? 'macos/Sources/HerdrMacOS', JSON.parse(fs.readFileSync(policyPath, 'utf8')));
    if (issues.length) { console.error(issues.join('\n')); process.exitCode = 1; }
    else console.log('Design control ownership and counted exceptions: PASS');
  } catch (error) { console.error(`Design control check failed: ${error.message}`); process.exitCode = 1; }
}
