#!/usr/bin/env node
// Create a local, library-linked scratch. Never edit or copy the shared library.
import fs from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {spawn, execFileSync} from 'node:child_process';
import {CANVAS} from './pen-tokens.mjs';

const PEN_VERSION = '0.3.8'; // Re-verify import, rendering and reopen before changing.
const TIMEOUT_MS = 60_000;
const MAX_OUTPUT_BYTES = 1024 * 1024;
const usage = 'Usage: node scripts/design-scratch.mjs <task-slug>';

// The guard's stdin belongs only to this creator. EOF on owner crash/SIGKILL
// kills the isolated group even when this process cannot run its cleanup.
const guard = `
const {spawn} = require('node:child_process');
const stop = () => { try { process.kill(-process.pid, 'SIGKILL'); } catch { process.exit(1); } };
process.stdin.resume();
process.stdin.once('end', stop);
process.stdin.once('error', stop);
const cli = spawn('pen', process.argv.slice(1), {stdio: ['ignore', 'inherit', 'inherit']});
cli.once('error', error => { console.error('Cannot run pen: ' + error.message); process.exit(1); });
cli.once('exit', code => process.exit(code ?? 1));
`;

// One short-lived CLI process group at a time; no prompt means no model run.
function pen(args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['-e', guard, '--', ...args], {cwd, detached: true, stdio: ['pipe', 'pipe', 'pipe']});
    let stdout = '', stderr = '', bytes = 0, failure;
    const stop = () => {
      if (!child.pid) return;
      try { process.kill(-child.pid, 'SIGKILL'); }
      catch (error) { if (error.code !== 'ESRCH') failure ??= error; }
    };
    const abort = message => { failure ??= new Error(message); stop(); };
    const interrupt = () => abort('Interrupted; scratch creation cancelled.');
    process.once('SIGINT', interrupt);
    process.once('SIGTERM', interrupt);
    const timer = setTimeout(() => abort(`Pen exceeded ${TIMEOUT_MS / 1000}s; scratch creation cancelled.`), TIMEOUT_MS);
    for (const [stream, name] of [[child.stdout, 'stdout'], [child.stderr, 'stderr']]) {
      stream.on('data', chunk => {
        bytes += chunk.length;
        if (bytes > MAX_OUTPUT_BYTES) return abort('Pen output exceeded 1 MiB; scratch creation cancelled.');
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
      else if (code !== 0) reject(new Error(`Pen failed (${signal ?? code}).\n${stderr}${stdout}`));
      else resolve(stdout);
    });
  });
}

function ordinaryFile(file) {
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.size === 0) throw new Error(`Expected a non-empty ordinary file: ${file}`);
}

function localDirectory(root, parts) {
  let directory = root;
  for (const part of parts) {
    directory = path.join(directory, part);
    try { fs.mkdirSync(directory); }
    catch (error) { if (error.code !== 'EEXIST') throw error; }
    if (!fs.lstatSync(directory).isDirectory()) throw new Error(`Refusing non-directory or symlink: ${directory}`);
  }
  return directory;
}

let temporary;
try {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--help') {
    console.log(`${usage}\nCreates ignored agents/runs/<task-slug>/design/scratch.pen in the current checkout.\nRequires pen ${PEN_VERSION} and an existing Pen login. Never overwrites an existing scratch.`);
  } else {
    const [slug] = args;
    if (args.length !== 1 || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug) || slug.length > 80) throw new Error(usage + '\nUse a lowercase, hyphen-separated task name (at most 80 characters).');
    const root = fs.realpathSync(execFileSync('git', ['rev-parse', '--show-toplevel'], {encoding: 'utf8'}).trim());
    const library = path.join(root, CANVAS);
    ordinaryFile(library);
    if (fs.realpathSync(library) !== library) throw new Error(`Refusing a library linked outside its checkout path: ${library}`);
    const relative = `agents/runs/${slug}/design/scratch.pen`;
    // check-ignore refuses tracked files too. Do not create deliverables here.
    execFileSync('git', ['check-ignore', '--quiet', '--', relative], {cwd: root});
    const target = path.join(root, relative);
    if (fs.existsSync(target)) throw new Error(`Scratch already exists; not overwritten: ${target}`);
    const version = (await pen(['version'], root)).trim();
    if (version !== `pen ${PEN_VERSION}`) throw new Error(`Expected pen ${PEN_VERSION}, received ${JSON.stringify(version)}. Install with: npm install -g @pen.dev/cli@${PEN_VERSION}`);
    const directory = localDirectory(root, ['agents', 'runs', slug, 'design']);
    // Same directory preserves the import's relative path on publication.
    temporary = path.join(directory, `.scratch-${randomUUID()}.pen`);
    const output = await pen(['--repo', root, '--out', temporary, '--library', library], root);
    ordinaryFile(temporary);
    // Atomic publication fails if another creator (or a symlink) won the name.
    fs.linkSync(temporary, target);
    console.log(output.trim());
    console.log(`\nScratch: ${target}\nLibrary: ${library}\nNo product screens were added to the shared library.`);
    const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
    console.log(`\nEdit (one writer, close the desktop document first):\npen interactive --in ${quote(target)} --out ${quote(target)}`);
    console.log(`\nInside the CLI: list_libraries(), get_app_state(), read_skill().\nSave with save(), then exit() before human review.\nReview in Pen: open -a Pen ${quote(target)}`);
  }
} catch (error) {
  console.error(`Scratch creation failed: ${error.message}`);
  process.exitCode = 1;
} finally {
  if (temporary) {
    try { fs.unlinkSync(temporary); }
    catch (error) { if (error.code !== 'ENOENT') { console.error(`Could not remove temporary scratch: ${error.message}`); process.exitCode = 1; } }
  }
}
