// The one way a repository script runs the Pen CLI: a short-lived process
// group this script owns, a timeout, an output cap, and the pinned version.
// design-scratch.mjs and design-review.mjs both go through it.
import {spawn} from 'node:child_process';

export const PEN_VERSION = '0.3.8'; // Re-verify import, rendering and reopen before changing.
const MAX_OUTPUT_BYTES = 1024 * 1024;

// The guard's stdin belongs only to the calling script. EOF on owner crash or
// SIGKILL kills the isolated group even when the owner cannot run its cleanup.
// What the CLI itself reads (an interactive session's commands) arrives in
// PEN_CLI_INPUT and is written to the CLI's own stdin.
const guard = `
const {spawn} = require('node:child_process');
const stop = () => { try { process.kill(-process.pid, 'SIGKILL'); } catch { process.exit(1); } };
process.stdin.resume();
process.stdin.once('end', stop);
process.stdin.once('error', stop);
const input = process.env.PEN_CLI_INPUT;
const cli = spawn('pen', process.argv.slice(1), {stdio: [input ? 'pipe' : 'ignore', 'inherit', 'inherit']});
if (input) cli.stdin.end(input);
cli.once('error', error => { console.error('Cannot run pen: ' + error.message); process.exit(1); });
cli.once('exit', code => process.exit(code ?? 1));
`;

/** A Pen failure the operator can act on: `reason` says what, `next` what to run. */
export class PenError extends Error {
  constructor(reason, next, detail = '') {
    super(`${reason}${next ? `\n${next}` : ''}`);
    this.reason = reason;
    this.next = next;
    this.detail = detail;
  }
}

/** Runs one Pen CLI command in its own process group and resolves its stdout. */
export function pen(args, {cwd, input = '', timeoutMs = 60_000, action = 'Pen command'} = {}) {
  return new Promise((resolve, reject) => {
    const env = {...process.env};
    if (input) env.PEN_CLI_INPUT = input;
    else delete env.PEN_CLI_INPUT;
    const child = spawn(process.execPath, ['-e', guard, '--', ...args], {cwd, env, detached: true, stdio: ['pipe', 'pipe', 'pipe']});
    let stdout = '', stderr = '', bytes = 0, failure;
    const stop = () => {
      if (!child.pid) return;
      try { process.kill(-child.pid, 'SIGKILL'); }
      catch (error) { if (error.code !== 'ESRCH') failure ??= error; }
    };
    const abort = message => { failure ??= new Error(message); stop(); };
    const interrupt = () => abort(`Interrupted; ${action} cancelled.`);
    process.once('SIGINT', interrupt);
    process.once('SIGTERM', interrupt);
    const timer = setTimeout(() => abort(`Pen exceeded ${timeoutMs / 1000}s; ${action} cancelled.`), timeoutMs);
    for (const [stream, name] of [[child.stdout, 'stdout'], [child.stderr, 'stderr']]) {
      stream.on('data', chunk => {
        bytes += chunk.length;
        if (bytes > MAX_OUTPUT_BYTES) return abort(`Pen output exceeded 1 MiB; ${action} cancelled.`);
        if (name === 'stdout') stdout += chunk; else stderr += chunk;
      });
    }
    child.once('error', error => { failure = new Error(`Cannot run pen: ${error.message}`); });
    // Do not wait for close: a descendant may still hold the output pipes.
    child.once('exit', stop);
    child.once('close', (code, signal) => {
      stop();
      clearTimeout(timer);
      process.removeListener('SIGINT', interrupt);
      process.removeListener('SIGTERM', interrupt);
      if (failure) reject(failure);
      else if (code !== 0) {
        const output = plain(`${stderr}${stdout}`);
        if (/Authentication required|Not authenticated/i.test(output)) reject(new PenError('Pen is not logged in.', 'Run: pen login', output));
        else if (/Cannot run pen/.test(output)) reject(new PenError('Pen CLI is not installed.', installHint(), output));
        else reject(new Error(`Pen failed (${signal ?? code}).\n${output}`));
      } else resolve(stdout);
    });
  });
}

const installHint = () => `Install with: npm install -g @pen.dev/cli@${PEN_VERSION}`;

/** Pen's output without ANSI colors or the boxed update notice pen 0.3.8 prints on every command. */
export function plain(output) {
  // eslint-disable-next-line no-control-regex
  return output.replace(/\u001b\[[0-9;]*m/g, '').split('\n').filter(line => !/^\s*[│╭╰]/.test(line)).join('\n').trim();
}

/** The version Pen reports, read from its `pen X.Y.Z` line so an update notice around it does not matter. */
export function parseVersion(output) {
  const lines = plain(output).split('\n').map(line => line.trim()).filter(line => /^pen \S+$/.test(line));
  return lines.length ? lines.at(-1) : plain(output);
}

/** Refuses anything but the pinned Pen CLI. */
export async function requirePen(cwd) {
  const version = parseVersion(await pen(['version'], {cwd, action: 'the version check'}));
  if (version !== `pen ${PEN_VERSION}`) throw new PenError(`Expected pen ${PEN_VERSION}, received ${JSON.stringify(version)}.`, installHint());
  return version;
}
