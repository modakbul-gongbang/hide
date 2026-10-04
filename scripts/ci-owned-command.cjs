// A preparation command owns the same native group/job as ordinary fixtures.
// Only this Node caller holds the supervisor's stdin pipe. Worker loss closes
// it even when a command shell has already exited or left descendants behind.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {spawn} = require('node:child_process');
const MAX_OWNERS = 256, MAX_FILES = 10000, MAX_RECEIPT = 16 * 1024;
const SHUTDOWN = 2000;
const MAX_FILE_BYTES = 512 * 1024 * 1024;
const active = new Set();
const ENV = {CI_FIXTURE_BASH: {scope: 'verification-only', requirement: 'required on Windows; optional on Unix',
  shape: 'absolute existing Bash executable', fallback: '/bin/bash on Unix only',
  note: 'The Actions verification shell; a missing Windows value refuses command launch instead of searching for WSL.'}};

function shell() {
  const selected = process.env.CI_FIXTURE_BASH
    || (process.platform === 'win32' ? undefined : '/bin/bash');
  if (!selected || !path.isAbsolute(selected) || !fs.statSync(selected).isFile()) {
    throw new Error('preparation requires the absolute verification Bash path; Windows uses the Actions Git Bash');
  }
  return fs.realpathSync(selected);
}

function readReceipt(file) {
  let stat;
  try { stat = fs.lstatSync(file); }
  catch (error) { if (error.code === 'ENOENT') return; throw error; }
  if (!stat.isFile() || stat.size > MAX_RECEIPT) throw new Error('preparation native receipt file/byte cap exceeded');
  const [version, phase, pid, birth, code, survivors, detail, ...extra] = fs.readFileSync(file, 'utf8').replace(/\n$/, '').split('\t');
  if (version !== 'v1' || !phase || !/^\d+$/.test(pid || '') || !/^\d+$/.test(birth || '')
    || !/^-?\d+$/.test(code || '') || !/^-?\d+$/.test(survivors || '')
    || detail === undefined || !/^(?:[a-f0-9]{2})*$/.test(detail) || extra.length) {
    throw new Error('invalid preparation native receipt');
  }
  return {phase, pid: Number(pid), birth, code: Number(code), survivors: Number(survivors), error: Buffer.from(detail, 'hex').toString('utf8')};
}

// Shared with preparation's manifest inventory. Stream hashing has a fixed
// allocation, including the native supervisor copied for each invocation.
function digest(file) {
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.size > MAX_FILE_BYTES) throw Error('invalid preparation file: ' + file);
  const hash = crypto.createHash('sha256'), buffer = Buffer.alloc(64 * 1024), fd = fs.openSync(file, 'r');
  try { let count; while ((count = fs.readSync(fd, buffer, 0, buffer.length, null))) hash.update(buffer.subarray(0, count)); }
  finally { fs.closeSync(fd); }
  return {size: stat.size, sha256: hash.digest('hex')};
}

async function run(root, executable, args, {timeout = 5000, maxBuffer = 64 * 1024, subject = path.basename(executable), env, onOwner, onStderr} = {}) {
  if (!Number.isSafeInteger(timeout) || timeout <= 0 || !Number.isSafeInteger(maxBuffer) || maxBuffer <= 0) throw new Error('invalid preparation command bound');
  if (active.size >= MAX_OWNERS) throw new Error('preparation native owner cap exceeded');
  const helper = path.join(root, 'target/debug/examples/fixture-owner' + (process.platform === 'win32' ? '.exe' : ''));
  if (!fs.statSync(helper).isFile()) throw new Error('build this worktree\'s fixture-owner before preparation commands');
  const directory = path.join(root, 'agents/runs/ci-preparation/owners');
  fs.mkdirSync(directory, {recursive: true});
  if (fs.readdirSync(directory).length >= MAX_FILES - 4) throw new Error('preparation owner receipt file cap exceeded');
  const name = crypto.randomBytes(16).toString('hex');
  const owner = {receipt: path.join(directory, name + '.receipt'), home: path.join(directory, name + '.home'), executable, args, subject};
  const image = path.join(directory, name + '.supervisor' + (process.platform === 'win32' ? '.exe' : ''));
  let homeIdentity, imageIdentity;
  let child;
  try {
    fs.mkdirSync(owner.home, {mode: 0o700});
    homeIdentity = fs.lstatSync(owner.home);
    // Cargo may replace its own example output during build. A running
    // Windows image cannot be replaced, so this invocation executes its
    // private immutable copy outside the home that the supervisor releases.
    const source = digest(helper);
    fs.copyFileSync(helper, image, fs.constants.COPYFILE_EXCL);
    imageIdentity = fs.lstatSync(image);
    if (JSON.stringify(digest(image)) !== JSON.stringify(source) || JSON.stringify(digest(helper)) !== JSON.stringify(source)) {
      throw new Error('preparation supervisor changed during immutable launch copy');
    }
    owner.supervisorImage = {path: image, source: helper, digest: source};
    active.add(owner);
    child = spawn(image, [owner.receipt, owner.home, executable, ...args], {
      cwd: root, env, detached: process.platform !== 'win32', stdio: ['pipe', 'pipe', 'pipe']
    });
  } catch (error) {
    active.delete(owner);
    for (const release of [() => {if (imageIdentity) fs.unlinkSync(image);}, () => {if (homeIdentity) fs.rmdirSync(owner.home);}]) {
      try { release(); } catch (secondary) {
        (error.secondary ||= []).push({message: secondary.message, stack: secondary.stack, code: secondary.code});
        error.owner = owner;
      }
    }
    throw error;
  }
  owner.supervisorPid = child.pid;
  const output = [], diagnostics = [];
  let bytes = 0, primary, close, stopping = false;
  let deadline, cleanupDeadline;
  const note = error => {
    if (!primary) primary = error;
    else if (error !== primary) {
      const secondary = primary.secondary ||= [];
      if (secondary.length < 4) secondary.push({message: error.message, stack: error.stack, code: error.code});
      else if (!primary.secondaryCap) primary.secondaryCap = 'preparation secondary error cap exceeded';
    }
  };
  const stop = error => {
    note(error);
    if (stopping) return;
    stopping = true;
    clearTimeout(deadline);
    // EOF targets the original owned namespace. No late PID lookup or shell
    // process kill is permitted to substitute for native exit confirmation.
    child.stdin.destroy();
    cleanupDeadline = setTimeout(() => finish(false), SHUTDOWN);
  };
  let finish;
  return new Promise((resolve, reject) => {
    let finished = false;
    finish = supervisorExited => {
      if (finished) return;
      finished = true;
      clearTimeout(deadline); clearTimeout(cleanupDeadline);
      let observed;
      try {
        observed = readReceipt(owner.receipt);
        if (!supervisorExited || !observed || observed.survivors !== 0) throw new Error('preparation owned exit unconfirmed; preserve command home');
        if (observed.error) throw new Error(observed.error);
        if (observed.phase !== 'exited' && observed.phase !== 'owner-lost-exited') throw new Error('preparation native command failed: ' + observed.phase);
        if (!primary && (observed.code !== 0 || close?.code !== 0)) {
          const detail = Buffer.concat(diagnostics).toString('utf8').trim();
          const error = new Error(`preparation command ${subject} exited ${observed.code}: ${detail.slice(0, 8192)}`);
          error.code = 'COMMAND_FAILED';
          throw error;
        }
      } catch (error) { note(error); }
      if (supervisorExited && observed?.survivors === 0) {
        active.delete(owner);
        try {
          if (fs.existsSync(owner.home)) {
            const current = fs.lstatSync(owner.home);
            if (current.isSymbolicLink() || current.dev !== homeIdentity.dev || current.ino !== homeIdentity.ino) throw new Error('preparation home identity changed; preserve entry');
            fs.rmSync(owner.home, {recursive: true});
          }
        }
        catch (error) { note(error); }
        try {
          const current = fs.lstatSync(image);
          if (!current.isFile() || current.dev !== imageIdentity.dev || current.ino !== imageIdentity.ino) throw new Error('preparation supervisor image identity changed; preserve entry');
          fs.unlinkSync(image);
        } catch (error) { note(error); }
      }
      // A stalled supervisor remains the original owner. EOF stays closed,
      // its retained receipt/home report unknown exit, and it cannot keep this
      // caller alive indefinitely through inherited output pipes.
      child.stdin.destroy(); child.stdout.destroy(); child.stderr.destroy(); child.unref();
      owner.observed = observed || null;
      owner.supervisorExited = supervisorExited;
      if (primary) {
        primary.owner = owner;
        primary.stdout = Buffer.concat(output).toString('utf8');
        primary.stderr = Buffer.concat(diagnostics).toString('utf8');
        reject(primary);
      }
      else resolve({stdout: Buffer.concat(output).toString('utf8'), stderr: Buffer.concat(diagnostics).toString('utf8'), owner});
    };
    const capture = (chunks, chunk) => {
      if (stopping) return;
      bytes += chunk.length;
      if (bytes > maxBuffer) {
        const error = new Error('preparation command output byte cap exceeded'); error.code = 'OUTPUT_CAP';
        stop(error); return;
      }
      chunks.push(chunk);
      if (chunks === diagnostics) {
        try { onStderr?.(chunk); } catch (error) { stop(error); }
      }
    };
    child.stdout.on('data', chunk => capture(output, chunk));
    child.stderr.on('data', chunk => capture(diagnostics, chunk));
    child.stdin.on('error', error => { if (error.code !== 'EPIPE') stop(error); });
    child.once('error', error => { note(error); });
    child.once('close', (code, signal) => { close = {code, signal}; finish(true); });
    deadline = setTimeout(() => {
      const error = new Error(`preparation command ${subject} timed out after ${timeout}ms`);
      error.code = 'ETIMEDOUT'; stop(error);
    }, timeout);
    try { onOwner?.({...owner}); } catch (error) { stop(error); }
  });
}

module.exports = {run, shell, readReceipt, digest, ENV};
