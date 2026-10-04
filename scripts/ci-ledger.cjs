// Versioned, bounded test/Actions observations. Artifacts remain outside source.
const fs = require('node:fs');
const crypto = require('node:crypto');
const MAX_ROWS = 20000;
const MAX_BYTES = 16 * 1024 * 1024;
const hash = value => crypto.createHash('sha256').update(value).digest('hex');

function signature(message) {
  return hash(message.replace(/\x1b\[[0-9;]*m/g, '').replace(/(?:[A-Z]:)?\/(?:[^\s:]+\/)+[^\s:]*/g, '<path>').replace(/\bw\w+:[pt]\w+\b/g, '<id>'));
}
function category(error) {
  if (!error) return 'passed';
  if (/expect\(|Expected:|Received:|assertion failed|panicked at/.test(error)) return 'assertion';
  if (/electron\.launch|Executable doesn't exist|download|ENOENT|spawn.*failed/i.test(error)) return 'provisioning';
  return 'fixture-or-runtime';
}
function identity(env = process.env) {
  let event = {};
  if (env.GITHUB_EVENT_PATH) event = JSON.parse(fs.readFileSync(env.GITHUB_EVENT_PATH, 'utf8'));
  const sha = env.GITHUB_SHA || env.CI_HEAD_SHA || 'local';
  const repository = env.GITHUB_REPOSITORY;
  return { sha, head: event.pull_request?.head?.sha || sha, base: event.pull_request?.base?.sha || null,
    checkout: event.pull_request ? 'merge' : 'branch', url: repository && env.GITHUB_RUN_ID ? `https://github.com/${repository}/actions/runs/${env.GITHUB_RUN_ID}/attempts/${env.GITHUB_RUN_ATTEMPT || 1}` : null,
    os: env.RUNNER_OS || ({darwin:'macOS',linux:'Linux',win32:'Windows'}[process.platform]),
    runner: env.ImageOS || env.RUNNER_NAME || 'local', run: env.GITHUB_RUN_ID || 'local',
    runAttempt: Number(env.GITHUB_RUN_ATTEMPT || 1), job: env.GITHUB_JOB || 'local',
    toolchain: { node: process.version, rust: env.CI_RUST_VERSION || 'unknown', image: env.ImageVersion || 'unknown' },
    runtime: JSON.parse(fs.readFileSync(require('node:path').join(__dirname,'../contracts/herdr-bundle.json'),'utf8')).version,
    shard: env.CI_SHARD || '1/1', lane: env.CI_LANE || env.GITHUB_JOB || 'local', jobLabel: env.CI_JOB_LABEL || env.GITHUB_JOB || 'local',
    invocation: env.CI_INVOCATION || 'tests' };
}
function quarantine(row, registry) {
  const entry = registry.entries.find(e => e.title === row.test && e.file === row.suite && e.oses.includes(row.os));
  if (!entry) return null;
  if (row.status === 'passed') return {id:entry.id, classification:'passed'};
  const known = row.category === entry.signature.category && entry.signature.any_of.some(pattern => pattern.every(token => row.assertion.includes(token)));
  return {id:entry.id, classification:known?'known-signature':'outside-registered-signature'};
}
function attemptKey(row) {
  // GITHUB_JOB is the reusable workflow's local id, not an Actions execution.
  // The producer supplies its exact job label; collection resolves that label
  // to the actual job id. Lane/invocation retain independent consumer runs.
  return JSON.stringify([row.sha,row.os,row.run,row.runAttempt,row.jobId ?? row.jobLabel ?? row.job,
    row.lane,row.invocation || 'tests',row.shard,row.project,row.suite,row.test,row.repeat,row.retry]);
}
function merge(rows, {allowConflicts=false}={}) {
  if (rows.length > MAX_ROWS) throw new Error('ledger row cap exceeded');
  const seen = new Map();
  for (const row of rows) {
    const key = attemptKey(row);
    const observations = seen.get(key) || [];
    if (!observations.some(previous=>JSON.stringify(previous)===JSON.stringify(row))) observations.push(row);
    seen.set(key, observations);
  }
  const records = [...seen.values()].flat();
  const conflicts = [...seen].filter(([,observations])=>observations.length>1).map(([identity,observations])=>({identity,observations}));
  const unambiguous = [...seen.values()].filter(observations=>observations.length===1).flat();
  const first = unambiguous.filter(row => row.retry === 0 && row.runAttempt === 1 && !['skipped','interrupted','unknown'].includes(row.status));
  const result = { version: 1, records, summary: { firstAttempts: first.length, firstPass: first.filter(row => row.status === 'passed').length,
    excluded: records.length - first.length, unknown: unambiguous.filter(row => row.status === 'unknown').length + conflicts.length } };
  if (conflicts.length) {
    result.conflicts=conflicts;
    result.collection='partial-or-unknown';
    if (!allowConflicts) {
      const error=new Error('conflicting duplicate attempt');
      error.partial=result;
      throw error;
    }
  }
  return result;
}
function write(filename, value) {
  const bytes = JSON.stringify(value, null, 2) + '\n';
  if (Buffer.byteLength(bytes) > MAX_BYTES) throw new Error('ledger byte cap exceeded');
  fs.mkdirSync(require('node:path').dirname(filename), { recursive: true });
  // One namespace replacement leaves complete JSON even if the writer dies.
  const temporary = filename + '.' + process.pid + '.tmp';
  let primary, owned=false;
  try {
    const descriptor=fs.openSync(temporary,'wx');
    owned=true;
    try { fs.writeFileSync(descriptor,bytes); }
    finally { fs.closeSync(descriptor); }
    fs.renameSync(temporary, filename);
  } catch (error) { primary = error; throw error; }
  finally {
    try { if(owned) fs.unlinkSync(temporary); }
    catch (error) {
      if (error.code !== 'ENOENT') {
        if (primary) primary.cause = error;
        else throw error;
      }
    }
  }
}
async function jobs(github, context) {
  const parameters = { ...context.repo, run_id: context.runId, attempt_number: Number(process.env.GITHUB_RUN_ATTEMPT || 1), per_page: 100 };
  const all = [];
  for (let page = 1; page <= 20; page++) {
    const response = await github.rest.actions.listJobsForWorkflowRunAttempt({ ...parameters, page });
    if (!Array.isArray(response.data.jobs)) throw new Error('unknown Actions job response');
    all.push(...response.data.jobs);
    if (response.data.jobs.length < 100) return all.map(job => ({ id: job.id, name: job.name, conclusion: job.conclusion || 'unknown',
      runner: job.runner_name || 'unknown', createdAt: job.created_at || null, startedAt: job.started_at || null,
      completedAt: job.completed_at || null, url: job.html_url, steps: job.steps.map(step => ({ name: step.name, conclusion: step.conclusion || 'unknown' })) }));
  }
  throw new Error('Actions job inventory overflow');
}
module.exports = { signature, category, identity, attemptKey, merge, write, jobs, quarantine, MAX_ROWS, MAX_BYTES };
function rust(text, metadata) {
  if (Buffer.byteLength(text) > MAX_BYTES) throw new Error('Rust log cap exceeded');
  // Cargo's artifacts give package/target identity even for identically named
  // integration binaries. Retain the executable identity for older raw logs.
  const artifacts = new Map();
  const sections = [];
  let section;
  for (const line of text.split('\n')) {
    if (line.startsWith('{"reason":')) {
      const value = JSON.parse(line);
      if (value.reason === 'compiler-artifact' && value.executable) {
        artifacts.set(value.executable.replace(/\\/g, '/').split('/').at(-1), value);
      }
    }
    const running = line.match(/^\s*Running (.+) \((.+)\)\s*$/);
    const doc = line.match(/^\s*Doc-tests (\S+)\s*$/);
    if (running || doc) {
      const executable = running ? running[2].replace(/\\/g, '/') : null;
      const artifact = executable && artifacts.get(executable.split('/').at(-1));
      const target = artifact ? {name:artifact.target.name,kind:artifact.target.kind} : {name:running ? running[1] : doc[1],kind:[doc ? 'doctest' : 'unknown']};
      section = {suite:running ? `${running[1]} (${executable})` : `Doc-tests ${doc[1]}`,
        package:artifact?.package_id || 'unknown', target, executable, lines:[]};
      sections.push(section);
    } else if (section) section.lines.push(line);
  }
  const rows = [];
  for (const {lines, ...subject} of sections) {
    const failures = new Map();
    for (const match of lines.join('\n').matchAll(/thread '([^']+)'(?: \(\d+\))? panicked at ([\s\S]*?)(?=\nthread '|\nfailures:|\ntest result:|$)/g)) {
      if (failures.has(match[1])) throw new Error('ambiguous Rust panic identity within suite');
      failures.set(match[1], match[2]);
    }
    for (const line of lines) {
      const test = line.match(/^test (.+?) \.\.\. (ok|FAILED|ignored)(?:\s|$)/);
      if (!test) continue;
      const assertion = test[2] === 'FAILED' ? failures.get(test[1]) || 'libtest failure detail not captured' : '';
      rows.push({...metadata,...subject,test:test[1],repeat:Number(process.env.CI_REPEAT || 0),retry:0,worker:'libtest',status:{ok:'passed',FAILED:'failed',ignored:'skipped'}[test[2]],
        category:test[2]==='FAILED'?(failures.has(test[1])?'assertion':'unknown'):test[2]==='ignored'?'skipped':'passed', assertion:assertion.slice(0,16000),signature:assertion?signature(assertion):null});
      if (rows.length > MAX_ROWS) throw new Error('Rust result row cap exceeded');
    }
  }
  if (!rows.length) throw new Error('Rust result has no observed tests');
  return merge(rows);
}
module.exports.rust = rust;
if (require.main === module) {
  const [mode, input, output] = process.argv.slice(2);
  if (mode !== 'rust' || !input || !output) throw new Error('usage: ci-ledger.cjs rust LOG OUTPUT');
  write(output, rust(fs.readFileSync(input,'utf8'), identity()));
}
