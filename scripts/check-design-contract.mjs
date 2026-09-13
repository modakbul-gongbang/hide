#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync, spawnSync} from 'node:child_process';

const commands = ['check-hide-theme-literals.mjs', 'check-hide-components.mjs', 'check-design-controls.mjs', 'check-pen.mjs'];
function run(root) {
  for (const command of commands) {
    const result = spawnSync(process.execPath, [path.join(root, 'scripts', command)], {cwd: root, stdio: 'inherit'});
    if (result.error) throw result.error;
    if (result.status !== 0) return result.status ?? 1;
  }
  return 0;
}
let temporary;
try {
  const root = execFileSync('git', ['rev-parse', '--show-toplevel'], {encoding: 'utf8'}).trim();
  const args = process.argv.slice(2);
  if (args.length && (args.length !== 1 || args[0] !== '--staged')) throw new Error('Usage: node scripts/check-design-contract.mjs [--staged]');
  if (args[0] === '--staged') {
    temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'hide-staged-design-'));
    const entries = execFileSync('git', ['ls-files', '--stage', '-z'], {cwd: root, encoding: 'utf8'}).split('\0').filter(Boolean);
    for (const entry of entries) {
      const tab = entry.indexOf('\t'), [mode, object, stage] = entry.slice(0, tab).split(' '), file = entry.slice(tab + 1);
      const input = (file.startsWith('macos/Sources/HerdrMacOS/') && file.endsWith('.swift'))
        || file === 'design/hide.pen'
        || [...commands, 'swift-source-tokens.mjs', 'design-control-policy.json', 'pen-tokens.mjs', 'pen-token-map.json', 'pen-bands.mjs', 'pen-foundations.mjs', 'pen-canvas.mjs'].some(name => file === 'scripts/' + name);
      if (!input) continue;
      if (stage !== '0') throw new Error(`Resolve staged conflict before design check: ${file}`);
      if (!['100644', '100755'].includes(mode)) throw new Error(`Design inputs must be ordinary files: ${file}`);
      const output = path.resolve(temporary, file);
      if (!output.startsWith(temporary + path.sep)) throw new Error('Staged design input escaped fixture root');
      fs.mkdirSync(path.dirname(output), {recursive: true});
      fs.writeFileSync(output, execFileSync('git', ['cat-file', 'blob', object], {cwd: root, maxBuffer: 32 * 1024 * 1024}));
    }
    console.log('Checking staged design inputs; index and working tree are not modified.');
    process.exitCode = run(temporary);
  } else process.exitCode = run(root);
} catch (error) {
  console.error(`Design contract failed: ${error.message}`);
  process.exitCode = 1;
} finally {
  if (temporary) fs.rmSync(temporary, {recursive: true, force: true});
}
