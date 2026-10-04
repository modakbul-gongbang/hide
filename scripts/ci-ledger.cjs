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
    shard: env.CI_SHARD || '1/1', lane: env.CI_LANE || env.GITHUB_JOB || 'local', jobLabel: env.CI_JOB_LABEL || env.GITHUB_JOB || 'local' };
}
function quarantine(row, registry) {
  const entry = registry.entries.find(e => e.title === row.test && e.file === row.suite && e.oses.includes(row.os));
  if (!entry) return null;
  if (row.status === 'passed') return {id:entry.id, classification:'passed'};
  const known = row.category === entry.signature.category && entry.signature.any_of.some(pattern => pattern.every(token => row.assertion.includes(token)));
  return {id:entry.id, classification:known?'known-signature':'outside-registered-signature'};
}
function merge(rows) {
  if (rows.length > MAX_ROWS) throw new Error('ledger row cap exceeded');
  const seen = new Map();
  for (const row of rows) {
    const key = JSON.stringify([row.sha,row.os,row.run,row.runAttempt,row.job,row.shard,row.suite,row.test,row.repeat,row.retry]);
    const previous = seen.get(key);
    if (previous && JSON.stringify(previous) !== JSON.stringify(row)) throw new Error('conflicting duplicate attempt');
    seen.set(key, row);
  }
  const records = [...seen.values()];
  const first = records.filter(row => row.retry === 0 && row.runAttempt === 1 && !['skipped','interrupted','unknown'].includes(row.status));
  return { version: 1, records, summary: { firstAttempts: first.length, firstPass: first.filter(row => row.status === 'passed').length,
    excluded: records.length - first.length, unknown: records.filter(row => row.status === 'unknown').length } };
}
function write(filename, value) {
  const bytes = JSON.stringify(value, null, 2) + '\n';
  if (Buffer.byteLength(bytes) > MAX_BYTES) throw new Error('ledger byte cap exceeded');
  fs.mkdirSync(require('node:path').dirname(filename), { recursive: true });
  fs.writeFileSync(filename, bytes);
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
module.exports = { signature, category, identity, merge, write, jobs, quarantine, MAX_ROWS, MAX_BYTES };
function rust(text, metadata) {
  if (Buffer.byteLength(text) > MAX_BYTES) throw new Error('Rust log cap exceeded');
  const rows = [];
  const failures = new Map([...text.matchAll(/thread '([^']+)' panicked at ([\s\S]*?)(?=\nthread '|\nfailures:|\ntest result:|$)/g)].map(match=>[match[1],match[2]]));
  let suite = 'unknown';
  for (const line of text.split('\n')) {
    const running = line.match(/Running (.*?) \(/);
    if (running) suite = running[1];
    const test = line.match(/^test (.+?) \.\.\. (ok|FAILED|ignored)(?:\s|$)/);
    if (!test) continue;
    const assertion = failures.get(test[1]) || '';
    rows.push({...metadata,suite,test:test[1],repeat:0,retry:0,worker:'libtest',status:{ok:'passed',FAILED:'failed',ignored:'skipped'}[test[2]],
      category:test[2]==='FAILED'?'assertion':test[2]==='ignored'?'skipped':'passed', assertion:assertion.slice(0,16000),signature:assertion?signature(assertion):null});
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
