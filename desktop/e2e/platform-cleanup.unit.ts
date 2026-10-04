// Actual owned child plus error controls, without the operator's services.
import { expect, test } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnFixtureProcess, stopFixtureProcess } from '../../web/e2e/platform-fixture';
import { cleanupAfterFailure, ownUntilWorkerExit } from '../../web/e2e/worker-owned';

test('primary setup signature and stack survive a secondary cleanup failure', () => {
  const primary = new Error('original endpoint setup failure');
  const secondary = new Error('cleanup EBUSY on owned executable');
  try { cleanupAfterFailure(primary, () => { throw secondary; }); }
  catch (error) {
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toBe(primary.message);
    expect((error as Error).cause).toBe(secondary);
    expect((error as Error).stack).toBe(primary.stack);
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
  const child = spawnFixtureProcess(executable, ['-e', "require('fs').writeFileSync(process.argv[1], 'ready'); setInterval(()=>{},1000)", ready], root, { stdio: 'ignore' });
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

test('a graceful callback failure still ends the original native tree and retains its primary error', async () => {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), 'hide-owned-grace-')));
  const ready = path.join(root, 'ready');
  const child = spawnFixtureProcess(process.execPath, ['-e', "require('fs').writeFileSync(process.argv[1], String(process.pid)); setInterval(()=>{},1000)", ready], root, { stdio: 'ignore' });
  const exited = new Promise<void>(resolve => child.once('exit', () => resolve()));
  const deadline = Date.now() + 2000;
  try {
    while (!fs.existsSync(ready) && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 5));
    const target = fs.readFileSync(ready, 'utf8');
    const primary = new Error('original graceful endpoint refusal');
    let reported: unknown;
    try { stopFixtureProcess(child, () => { throw primary; }); }
    catch (error) {
      reported = error;
    }
    expect(reported).toBe(primary);
    await exited;
    const directory = path.resolve('../agents/runs/ci-fixture-owners');
    const receipts = fs.readdirSync(directory).filter(file => file.endsWith('.receipt'))
      .map(file => fs.readFileSync(path.join(directory, file), 'utf8').trimEnd().split('\t'))
      .filter(row => row[2] === target);
    expect(receipts).toHaveLength(1);
    expect(receipts[0]?.[1]).toBe('exited');
    expect(receipts[0]?.[5]).toBe('0');
    expect(fs.existsSync(root)).toBe(true);
    fs.rmSync(root, { recursive: true });
  } finally {
    if (fs.existsSync(root)) {
      // The original failure is deliberately retained on the same owner;
      // a second call must not repeat a callback or hide its failed outcome.
      try { stopFixtureProcess(child); } catch { /* asserted primary above */ }
    }
  }
}, 15000);
