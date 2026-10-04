// Actual owned child plus error controls, without the operator's services.
import { expect, test } from 'vitest';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { stopFixtureProcess } from '../../web/e2e/platform-fixture';
import { cleanupAfterFailure, ownUntilWorkerExit } from '../../web/e2e/worker-owned';

test('primary setup signature and stack survive a secondary cleanup failure', () => {
  const primary = new Error('original endpoint setup failure');
  const secondary = new Error('cleanup EBUSY on owned executable');
  try { cleanupAfterFailure(primary, () => { throw secondary; }); }
  catch (error) {
    expect(error).toBeInstanceOf(AggregateError);
    expect((error as AggregateError).errors).toEqual([primary, secondary]);
    expect((error as AggregateError).cause).toBe(primary);
    expect((error as AggregateError).errors[0].stack).toBe(primary.stack);
    return;
  }
  throw new Error('cleanup failure disappeared');
});

test('cleanup-only failure is raised and its ownership is retained for recovery', () => {
  let blocked = true;
  const owned = ownUntilWorkerExit(() => { if (blocked) throw new Error('owned exit unconfirmed'); });
  expect(() => owned.stop()).toThrow('owned exit unconfirmed');
  blocked = false;
  expect(() => owned.stop()).not.toThrow();
  expect(() => owned.stop()).not.toThrow();
});

test('the real owned executable exits before its home is removed', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'hide-owned-exit-'));
  const ready = path.join(root, 'ready');
  const executable = process.platform === 'win32' ? path.join(root, 'node.exe') : process.execPath;
  if (process.platform === 'win32') fs.copyFileSync(process.execPath, executable);
  const child = spawn(executable, ['-e', "require('fs').writeFileSync(process.argv[1], 'ready'); setInterval(()=>{},1000)", ready], { stdio: 'ignore', detached: process.platform !== 'win32' });
  let spawnError: Error | null = null;
  child.once('error', error => { spawnError = error; });
  try {
    const deadline = Date.now() + 2000;
    while (!fs.existsSync(ready) && Date.now() < deadline && !spawnError) await new Promise(resolve => setTimeout(resolve, 20));
    if (spawnError) throw spawnError;
    expect(fs.readFileSync(ready, 'utf8')).toBe('ready');
    if (process.platform === 'win32') expect(() => fs.rmSync(executable)).toThrow();
    stopFixtureProcess(child);
    fs.rmSync(root, { recursive: true, force: true });
    expect(fs.existsSync(root)).toBe(false);
  } finally {
    stopFixtureProcess(child);
    fs.rmSync(root, { recursive: true, force: true });
  }
}, 15000);
