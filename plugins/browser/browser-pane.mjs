#!/usr/bin/env node
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { readFile, readdir, stat, mkdir, open, unlink } from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { readEnvironment } from './environment.mjs';

const execute = promisify(execFile);
const pluginRoot = path.dirname(fileURLToPath(import.meta.url));
const pluginID = 'hide.browser';
const tokenKeys = ['hide_content', 'hide_browser_binding', 'hide_browser_request', 'hide_browser_profile', 'hide_browser_target', 'hide_browser_session', 'hide_browser_cdp_port', 'hide_browser_owns_target', 'hide_browser_error'];

export function identifier(value, label) {
  if (typeof value !== 'string' || !/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,79}$/.test(value)) {
    throw new Error(`${label}: use 1-80 letters, digits, dots, underscores or hyphens, starting with a letter or digit`);
  }
  return value;
}

export function parseOpen(args, environment) {
  const options = {};
  const allowed = new Set(['profile', 'target-pane', 'url', 'session', 'target-id', 'placement', 'direction', 'key']);
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, '');
    if (!args[index]?.startsWith('--') || !allowed.has(key) || !args[index + 1] || args[index + 1].startsWith('--') || key in options) {
      throw new Error('Unknown, duplicate or incomplete option. Run browser-pane.mjs help.');
    }
    options[key] = args[index + 1];
  }
  identifier(options.profile, 'profile');
  if (options.profile === 'live' || options.profile.startsWith('external-')) throw new Error('Select an existing managed chromux profile, not live/external.');
  const targetPane = options['target-pane'] || (environment.HERDR_ENV === '1' && environment.HERDR_PANE_ID);
  if (!targetPane || !/^w[^:]+:p[^:]+$/.test(targetPane)) throw new Error('An explicit --target-pane is required outside a Herdr pane.');
  const sources = ['url', 'session', 'target-id'].filter(key => options[key]);
  if (sources.length !== 1) throw new Error('Choose exactly one of --url, --session or --target-id.');
  if (options.url) {
    const url = new URL(options.url);
    if (!['http:', 'https:'].includes(url.protocol) && options.url !== 'about:blank') throw new Error('Only http(s) URLs and about:blank can be opened.');
  }
  if (options.session) identifier(options.session, 'session');
  if (options['target-id']) identifier(options['target-id'], 'target-id');
  const placement = options.placement || 'split';
  const direction = options.direction || 'right';
  if (!['split', 'tab'].includes(placement) || !['right', 'down'].includes(direction)) throw new Error('Placement must be split/tab; direction must be right/down.');
  const identity = [environment.HERDR_SOCKET_PATH || 'default', targetPane, options.profile, sources[0], options[sources[0]], placement, direction];
  const requestID = createHash('sha256').update(JSON.stringify(identity)).digest('hex');
  const bindingID = options.key ? identifier(options.key, 'key') : requestID.slice(0, 24);
  return { profile: options.profile, targetPane, url: options.url, session: options.session, targetID: options['target-id'], placement, direction, bindingID, requestID };
}

async function command(program, args, responseFormat = 'json') {
  try {
    const { stdout } = await execute(program, args, { timeout: 30_000, maxBuffer: 8 * 1024 * 1024 });
    return responseFormat === 'json' ? JSON.parse(stdout) : undefined;
  } catch (error) {
    // Do not repeat args: they can contain a URL or the host's configuration.
    throw new Error(`${program} failed (${error.code || (error.killed ? 'timeout' : 'invalid response')}). Inspect its host pane for diagnostics.`, { cause: error });
  }
}

async function snapshot() {
  const response = await command('herdr', ['api', 'snapshot']);
  const result = response.result?.snapshot || response.result || response;
  if (!Array.isArray(result.panes)) throw new Error('Herdr snapshot did not contain panes.');
  return result;
}

async function profileState(environment, profile) {
  identifier(profile, 'profile');
  const directory = path.join(environment.CHROMUX_HOME, 'profiles', profile);
  if (!(await stat(directory)).isDirectory()) throw new Error('The selected chromux profile does not exist.');
  const state = JSON.parse(await readFile(path.join(directory, '.state'), 'utf8'));
  const port = Number(state.port);
  const daemonPort = Number(state.daemonEndpoint?.port || state.daemonPort);
  if (![port, daemonPort].every(value => Number.isInteger(value) && value > 0 && value < 65536)) {
    throw new Error('This profile needs a running TCP chromux daemon. Open a session in chromux first.');
  }
  return { port, daemonPort };
}

async function daemon(state, route, body) {
  const response = await fetch(`http://127.0.0.1:${state.daemonPort}${route}`, {
    method: body ? 'POST' : 'GET', redirect: 'error', signal: AbortSignal.timeout(5_000),
    headers: body ? { 'content-type': 'application/json' } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  const value = await response.json();
  if (!response.ok || value.error) throw new Error(`chromux ${route.split('/')[1]} failed (${response.status}): ${typeof value.error === 'string' ? value.error : 'profile paused or target unavailable'}`);
  return value;
}

async function requireHealthy(state) {
  const health = await daemon(state, '/health');
  if (health.mode !== 'default' || health.paused) throw new Error('Browser panes require an unpaused default-mode chromux profile.');
}

export function bindingTokens(request, target, state) {
  return {
    hide_content: 'browser-v1', hide_browser_binding: request.bindingID, hide_browser_request: request.requestID,
    hide_browser_profile: request.profile, hide_browser_target: identifier(target.targetId, 'target ID'),
    hide_browser_session: identifier(target.session, 'session'), hide_browser_cdp_port: String(state.port),
    hide_browser_owns_target: String(Boolean(request.url)),
  };
}

async function report(paneID, tokens, sequence) {
  const args = ['pane', 'report-metadata', paneID, '--source', 'hide-browser', '--seq', String(sequence), '--ttl-ms', '30000'];
  for (const key of tokenKeys) {
    if (tokens[key] !== undefined) args.push('--token', `${key}=${tokens[key]}`);
    else args.push('--clear-token', key);
  }
  // Herdr's display-metadata command acknowledges success with exit status,
  // unlike snapshot and plugin commands, which print JSON.
  await command('herdr', args, 'status');
}

function receipt(pane, reused) {
  const tokens = pane.tokens || {};
  if (tokens.hide_browser_error) throw new Error(`Browser pane ${pane.pane_id}: ${tokens.hide_browser_error}`);
  if (!tokens.hide_browser_target || !tokens.hide_browser_cdp_port) return null;
  return {
    ok: true, pane_id: pane.pane_id, tab_id: pane.tab_id, reused,
    profile: tokens.hide_browser_profile, session: tokens.hide_browser_session,
    target_id: tokens.hide_browser_target,
    cdp_http_url: `http://127.0.0.1:${tokens.hide_browser_cdp_port}`,
    binding_id: tokens.hide_browser_binding,
  };
}

async function withBindingLock(identity, work) {
  const directory = path.join(os.tmpdir(), `hide-browser-${os.userInfo().uid}`);
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const lockPath = path.join(directory, `${createHash('sha256').update(identity).digest('hex')}.lock`);
  let lock;
  for (let attempt = 0; attempt < 80; attempt++) {
    try { lock = await open(lockPath, 'wx', 0o600); break; }
    catch (error) {
      if (error.code !== 'EEXIST') throw error;
      // A crashed owner is recoverable. A live/reused PID is never evicted.
      const owner = Number(await readFile(lockPath, 'utf8').catch(() => '0'));
      if (owner > 0) {
        try { process.kill(owner, 0); }
        catch (failure) { if (failure.code === 'ESRCH') await unlink(lockPath).catch(e => { if (e.code !== 'ENOENT') throw e; }); }
      }
      await delay(250);
    }
  }
  if (!lock) throw new Error('Another browser request holds this binding. Retry after it finishes.');
  await lock.writeFile(String(process.pid));
  try { return await work(); }
  finally { await lock.close(); await unlink(lockPath); }
}

async function openPane(request, environment) {
  // Validate existence before Herdr or chromux can create anything.
  const profile = path.join(environment.CHROMUX_HOME, 'profiles', request.profile);
  if (!(await stat(profile)).isDirectory()) throw new Error('Choose an existing chromux profile.');
  return withBindingLock(`${environment.HERDR_SOCKET_PATH || 'default'}:${request.bindingID}`, async () => {
    const before = await snapshot();
    const target = before.panes.find(pane => pane.pane_id === request.targetPane);
    if (!target) throw new Error('The target pane no longer exists. Nothing was opened.');
    const existing = before.panes.find(pane => pane.tokens?.hide_browser_binding === request.bindingID);
    if (existing) {
      if (existing.tokens?.hide_browser_request !== request.requestID) throw new Error('This key already belongs to a different browser request. Choose a new key.');
      const found = receipt(existing, true);
      if (found) return found;
      throw new Error(`Browser pane ${existing.pane_id} is still starting or unavailable.`);
    }
    const listed = await command('herdr', ['plugin', 'list', '--plugin', pluginID, '--json']);
    const installed = listed.result?.plugins || listed.plugins || [];
    if (installed.length && path.resolve(installed[0].plugin_root) !== pluginRoot) {
      throw new Error('hide.browser is linked to another installation. Unlink it explicitly before using this build.');
    }
    if (!installed.length) await command('herdr', ['plugin', 'link', pluginRoot, '--enabled']);
    await command('herdr', ['plugin', 'pane', 'open', '--plugin', pluginID, '--entrypoint', 'browser',
      '--placement', request.placement, '--target-pane', request.targetPane, '--direction', request.direction,
      '--cwd', target.cwd || process.cwd(), '--no-focus', '--env', `HIDE_BROWSER_REQUEST=${JSON.stringify(request)}`]);
    for (let attempt = 0; attempt < 120; attempt++) {
      const current = await snapshot();
      const pane = current.panes.find(pane => pane.tokens?.hide_browser_binding === request.bindingID);
      if (pane) { const ready = receipt(pane, false); if (ready) return ready; }
      await delay(250);
    }
    throw new Error('Browser pane did not become ready. Inspect the newly opened host pane; no other pane was closed.');
  });
}

async function host(environment) {
  if (!environment.HERDR_PLUGIN_STATE_DIR || !environment.HIDE_BROWSER_REQUEST) {
    throw new Error('Host mode requires the Herdr plugin state directory and browser request.');
  }
  const request = JSON.parse(environment.HIDE_BROWSER_REQUEST);
  identifier(request.bindingID, 'binding ID');
  const logPath = path.join(environment.HERDR_PLUGIN_STATE_DIR, `${request.bindingID}.log`);
  const log = await open(logPath, 'a', 0o600);
  try {
    // Herdr kills the pane's process group. A detached lease observes pane
    // retirement and can finish owned-tab cleanup after that group is gone.
    const lease = spawn(process.execPath, [fileURLToPath(import.meta.url), 'lease'], {
      detached: true, stdio: ['ignore', log.fd, log.fd],
    });
    process.stdout.write(`Browser host diagnostics: ${logPath}\n`);
    await new Promise((resolve, reject) => {
      lease.once('error', reject);
      lease.once('exit', (code, signal) => code === 0 ? resolve() : reject(new Error(`Browser lease stopped (${signal || code}). Inspect ${logPath}`)));
    });
  } finally { await log.close(); }
}

async function maintainLease(environment) {
  if (!environment.HIDE_BROWSER_REQUEST || !environment.HERDR_PANE_ID) throw new Error('Host mode must be launched by the Hide browser plugin.');
  const request = JSON.parse(environment.HIDE_BROWSER_REQUEST);
  // Validate the injected request with exactly the same contract as the CLI.
  const argumentsToValidate = ['--profile', request.profile, '--target-pane', request.targetPane,
    '--placement', request.placement, '--direction', request.direction, '--key', request.bindingID];
  for (const [field, flag] of [['url', '--url'], ['session', '--session'], ['targetID', '--target-id']]) {
    if (request[field]) argumentsToValidate.push(flag, request[field]);
  }
  const validated = parseOpen(argumentsToValidate, environment);
  if (validated.requestID !== request.requestID) throw new Error('HIDE_BROWSER_REQUEST: request identity mismatch');
  const paneID = environment.HERDR_PANE_ID;
  const abort = new AbortController();
  for (const signal of ['SIGHUP', 'SIGINT', 'SIGTERM']) process.once(signal, () => abort.abort());
  let sequence = 0;
  let ownedSession;
  let state;
  let target;
  let tokens = { hide_content: 'browser-v1', hide_browser_binding: request.bindingID, hide_browser_request: request.requestID, hide_browser_profile: request.profile };
  const paneStillExists = async () => {
    const current = await snapshot();
    return current.panes.some(pane => pane.pane_id === paneID || pane.tokens?.hide_browser_binding === request.bindingID);
  };
  try {
    if (!await paneStillExists()) return;
    await report(paneID, tokens, ++sequence);
    abort.signal.throwIfAborted();
    if (request.url) {
      // A unique host-owned session makes cleanup precise; borrowed sessions
      // below are never closed by this host.
      // A binding key is caller controlled and is not a safe session owner ID.
      // Never reuse a possibly pre-existing named chromux session for cleanup.
      ownedSession = `hide-${randomUUID()}`;
      await command('chromux', ['--profile', request.profile, 'open', ownedSession, request.url, '--background']);
      state = await profileState(environment, request.profile);
      await requireHealthy(state);
      target = await daemon(state, `/show/${encodeURIComponent(ownedSession)}`);
    } else {
      state = await profileState(environment, request.profile);
      await requireHealthy(state);
      if (request.session) target = await daemon(state, `/show/${encodeURIComponent(identifier(request.session, 'session'))}`);
      else {
        identifier(request.targetID, 'target ID');
        const sessions = await daemon(state, '/list');
        for (const session of Object.keys(sessions)) {
          const candidate = await daemon(state, `/show/${encodeURIComponent(session)}`);
          if (candidate.targetId === request.targetID) { target = candidate; break; }
        }
        if (!target) {
          abort.signal.throwIfAborted();
          ownedSession = `hide-${randomUUID()}`;
          await daemon(state, '/open', { session: ownedSession, attachTargetId: request.targetID, background: true });
          target = await daemon(state, `/show/${encodeURIComponent(ownedSession)}`);
        }
      }
    }
    tokens = bindingTokens(request, target, state);
    process.stdout.write(`Hide browser: ${request.profile}\n${request.url ? 'Closing this pane closes the browser tab created for it.' : 'Closing this pane detaches; the existing browser tab stays open.'}\n`);
    while (!abort.signal.aborted) {
      try {
        if (!await paneStillExists()) break;
        await daemon(state, '/wait', { session: target.session, ms: 1 });
        const current = await daemon(state, `/show/${encodeURIComponent(target.session)}`);
        if (current.targetId !== target.targetId) throw new Error('The chromux session now points to another target.');
        await report(paneID, tokens, ++sequence);
      } catch (error) {
        await report(paneID, { ...tokens, hide_browser_error: error.message.slice(0, 80) }, ++sequence);
        process.stderr.write(`browser.lease_failed: ${error.message}\n`);
      }
      await delay(10_000, undefined, { signal: abort.signal }).catch(error => { if (error.name !== 'AbortError') throw error; });
    }
  } catch (error) {
    if (!abort.signal.aborted) {
      process.stderr.write(`browser.host_failed: ${error.message}\n`);
      // Keep an observable error pane alive until the operator closes it.
      while (!abort.signal.aborted) {
        if (!await paneStillExists()) break;
        await report(paneID, { ...tokens, hide_browser_error: error.message.slice(0, 80) }, ++sequence);
        await delay(10_000, undefined, { signal: abort.signal }).catch(failure => { if (failure.name !== 'AbortError') throw failure; });
      }
    }
  } finally {
    if (ownedSession) {
      try { await command('chromux', ['--profile', request.profile, 'close', ownedSession]); }
      catch (error) { process.stderr.write(`browser.cleanup_failed: ${error.message}\n`); process.exitCode = 1; }
    }
  }
}

async function main(args) {
  const environment = readEnvironment();
  switch (args[0]) {
    case 'host': await host(environment); break;
    case 'lease': await maintainLease(environment); break;
    case 'open': console.log(JSON.stringify(await openPane(parseOpen(args.slice(1), environment), environment), null, 2)); break;
    case 'profiles': {
      const entries = await readdir(path.join(environment.CHROMUX_HOME, 'profiles'), { withFileTypes: true });
      console.log(JSON.stringify({ profiles: entries.filter(entry => entry.isDirectory() && /^[a-zA-Z0-9][a-zA-Z0-9._-]{0,79}$/.test(entry.name) && entry.name !== 'live' && !entry.name.startsWith('external-')).map(entry => entry.name) }));
      break;
    }
    case 'help': case undefined:
      console.log('node browser-pane.mjs open --profile NAME [--target-pane ID] (--url URL | --session NAME | --target-id ID) [--placement split|tab] [--direction right|down] [--key ID]\nnode browser-pane.mjs profiles\nRepeated identical requests reuse the pane without navigating or focusing it.');
      break;
    default: throw new Error('Unknown command. Run browser-pane.mjs help.');
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(JSON.stringify({ ok: false, error: error.message })); process.exitCode = 1; });
}
