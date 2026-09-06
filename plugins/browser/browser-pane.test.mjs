import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { parseOpen, bindingTokens } from './browser-pane.mjs';
import { readEnvironment, environmentRegistry } from './environment.mjs';

const environment = { HERDR_ENV: '1', HERDR_PANE_ID: 'w1:p2' };
const args = ['--profile', 'work', '--url', 'http://localhost:3000'];

test('a retry keeps its identity while a different target, profile or source creates new intent', () => {
  const first = parseOpen(args, environment);
  assert.equal(first.bindingID, parseOpen(args, environment).bindingID);
  assert.equal(first.targetPane, 'w1:p2');
  assert.equal(first.placement, 'split');
  assert.equal(first.direction, 'right');
  for (const change of [
    ['--target-pane', 'w1:p3'], ['--placement', 'tab'], ['--direction', 'down'],
  ]) assert.notEqual(first.bindingID, parseOpen([...args, ...change], environment).bindingID);
  assert.notEqual(first.bindingID, parseOpen(['--profile', 'personal', '--url', 'http://localhost:3000'], environment).bindingID);
  assert.notEqual(first.bindingID, parseOpen(['--profile', 'work', '--session', 'existing'], environment).bindingID);
});

test('a caller key does not conceal changed intent', () => {
  const first = parseOpen([...args, '--key', 'qa'], environment);
  const second = parseOpen([...args, '--key', 'qa', '--direction', 'down'], environment);
  assert.equal(first.bindingID, 'qa');
  assert.equal(second.bindingID, 'qa');
  assert.notEqual(first.requestID, second.requestID);
});

test('ambiguous, unsafe and unscoped requests fail before opening anything', () => {
  assert.throws(() => parseOpen(args, {}), /target-pane/);
  for (const invalid of [
    ['--profile', '../work', '--url', 'about:blank'],
    ['--profile', 'live', '--url', 'about:blank'],
    ['--profile', 'external-test', '--url', 'about:blank'],
    ['--profile', 'work', '--url', 'file:///tmp/local'],
    ['--profile', 'work', '--url', 'javascript:alert(1)'],
    ['--profile', 'work'], [...args, '--session', 'existing'],
    [...args, '--profile', 'other'], [...args, '--typo', 'value'],
    [...args, '--key', 'x'.repeat(81)],
  ]) assert.throws(() => parseOpen(invalid, environment));
  assert.equal(parseOpen([...args, '--key', 'x'.repeat(80)], environment).bindingID.length, 80);
  assert.equal(parseOpen([...args, '--target-pane', 'w9:p8'], {}).targetPane, 'w9:p8');
});

test('host metadata preserves the exact selected target and profile without storage or URL', () => {
  const request = parseOpen(args, environment);
  const tokens = bindingTokens(request, { targetId: 'ABCD1234', session: 'existing' }, { port: 9300 });
  assert.equal(tokens.hide_browser_target, 'ABCD1234');
  assert.equal(tokens.hide_browser_profile, 'work');
  assert.equal(tokens.hide_browser_session, 'existing');
  assert.equal(tokens.hide_browser_cdp_port, '9300');
  assert.equal(tokens.hide_browser_request, request.requestID);
  assert.equal(tokens.hide_browser_owns_target, 'true');
  const borrowed = bindingTokens(parseOpen(['--profile', 'work', '--session', 'existing'], environment), { targetId: 'ABCD1234', session: 'existing' }, { port: 9300 });
  assert.equal(borrowed.hide_browser_owns_target, 'false');
  assert.ok(!JSON.stringify(tokens).includes('localhost:3000'));
});

test('environment errors report all problem keys without their values', () => {
  assert.throws(() => readEnvironment({ CHROMUX_HOME: 'secret-relative', HERDR_SOCKET_PATH: 'secret-socket', HIDE_BROWSER_REQUEST: 'secret-json' }), error => {
    assert.match(error.message, /CHROMUX_HOME/);
    assert.match(error.message, /HERDR_SOCKET_PATH/);
    assert.match(error.message, /HIDE_BROWSER_REQUEST/);
    assert.ok(!error.message.includes('secret-'));
    return true;
  });
  assert.throws(() => readEnvironment({ HIDE_BROWSER_REQUEST: 'null' }), /JSON object/);
  assert.throws(() => readEnvironment({ HERDR_ENV: 'yes' }), /HERDR_ENV/);
  assert.throws(() => readEnvironment({ HERDR_PANE_ID: 'not-a-pane' }), /HERDR_PANE_ID/);
});

test('environment example and raw reads cannot drift outside the registry', async () => {
  const example = await readFile(new URL('.env.example', import.meta.url), 'utf8');
  const keys = [...example.matchAll(/^#?\s*([A-Z][A-Z_]+)=/gm)].map(match => match[1]);
  assert.deepEqual(keys.sort(), Object.keys(environmentRegistry).sort());
  for (const rule of Object.values(environmentRegistry)) assert.ok(!(rule.required && 'fallback' in rule));
  for (const name of await readdir(new URL('.', import.meta.url))) {
    if (!name.endsWith('.mjs') || name === 'environment.mjs' || name.endsWith('.test.mjs')) continue;
    assert.doesNotMatch(await readFile(new URL(name, import.meta.url), 'utf8'), /process\s*\.\s*env/);
  }
});
