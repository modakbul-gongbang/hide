// Bounded Actions/artifact ledger, with collection errors raised to its job.
const fs = require('node:fs');
const path = require('node:path');
const {execFileSync} = require('node:child_process');
const ledger = require('./ci-ledger.cjs');
const HISTORY_DAYS = 30;
const MAX_HISTORY_RUNS = 30;

function readRecords(directory) {
  const records=[];
  function visit(at, depth=0) {
    if (depth>3) throw new Error('ledger directory depth cap exceeded');
    for (const item of fs.readdirSync(at,{withFileTypes:true})) {
      const file=path.join(at,item.name);
      if (item.isDirectory()) visit(file,depth+1);
      else if (item.name.endsWith('.json')) {
        if (fs.statSync(file).size>ledger.MAX_BYTES) throw new Error('ledger input byte cap exceeded');
        const value=JSON.parse(fs.readFileSync(file,'utf8'));
        if (value.version!==1 || !Array.isArray(value.records) || !value.records.length) throw new Error(`unknown/empty ledger: ${item.name}`);
        records.push(...value.records);
        if (records.length>ledger.MAX_ROWS) throw new Error('ledger row cap exceeded');
      }
    }
  }
  visit(directory);
  return records;
}

function confirmInventory(plan, rows) {
  const expected={rust:['1/1'], 'web-e2e':['1/6','2/6','3/6','4/6','5/6','6/6'], 'web-e2e-platform':['1/1'], 'windows-e2e':['1/1'], 'desktop-e2e':['1/1']};
  for (const [lane, shards] of Object.entries(expected)) {
    if (!plan.lanes[lane]) continue;
    for (const shard of shards) {
      if (!rows.some(row=>row.lane===lane && row.shard===shard && !['skipped','unknown','interrupted'].includes(row.status)))
        throw new Error(`missing observed suite inventory: ${lane} ${shard}`);
    }
  }
}

function suiteSummaries(rows) {
  const suites=new Map();
  for (const row of rows) {
    const key=`${row.os} / ${row.suite}`;
    const list=suites.get(key)||[]; list.push(row); suites.set(key,list);
  }
  return Object.fromEntries([...suites].map(([key,records])=>[key,ledger.merge(records).summary]));
}

function summary(suites, inventory) {
  const lines=['First attempts keep the first workflow attempt and retry zero. Later attempts remain in each attempt artifact. History is bounded to 30 days and 30 observed attempts.','',
    '| OS / actual suite | First pass / observed | Later, skipped or cancelled | Unknown |','| --- | ---: | ---: | ---: |'];
  for (const [suite, s] of Object.entries(suites).sort()) {
    lines.push(`| ${suite.replace(/\|/g,'\\|')} | ${s.firstPass}/${s.firstAttempts} | ${s.excluded} | ${s.unknown} |`);
  }
  lines.push('', '| Actions job | Outcome | Queue seconds | Execution seconds |', '| --- | --- | ---: | ---: |');
  for (const job of inventory) {
    const seconds=(start,end)=>start&&end?Math.max(0,(Date.parse(end)-Date.parse(start))/1000):'unknown';
    lines.push(`| [${job.name.replace(/\|/g,'\\|')}](${job.url}) | ${job.conclusion} | ${seconds(job.createdAt,job.startedAt)} | ${seconds(job.startedAt,job.completedAt)} |`);
  }
  return lines.join('\n')+'\n';
}

module.exports=async function collect({github,context,core,directory,plan}) {
  const inventory=await ledger.jobs(github,context);
  const current=readRecords(directory);
  if (!plan && !current.length) throw new Error('no observed suite collection');
  if (plan) confirmInventory(plan,current);
  for (const row of current) {
    if (row.sha!==context.sha || String(row.run)!==String(context.runId) || row.runAttempt!==Number(process.env.GITHUB_RUN_ATTEMPT||1)) throw new Error('ledger source identity mismatch');
    const matching=inventory.filter(job=>job.name===row.jobLabel || job.name.endsWith(' / '+row.jobLabel));
    if (matching.length!==1) throw new Error(`missing/ambiguous Actions job identity: ${row.jobLabel}`);
    row.jobId=matching[0].id; row.jobUrl=matching[0].url;
  }
  const cutoff=Date.now()-HISTORY_DAYS*86400000;
  const artifacts=[];
  for (let page=1;page<=10;page++) {
    const response=await github.rest.actions.listArtifactsForRepo({...context.repo,per_page:100,page});
    if (!Array.isArray(response.data.artifacts)) throw new Error('unknown history API response');
    const batch=response.data.artifacts;
    artifacts.push(...batch.filter(a=>/^ci-history-attempt-\d+$/.test(a.name)&&!a.expired&&Date.parse(a.created_at)>=cutoff&&!(String(a.workflow_run?.id)===String(context.runId)&&a.name===`ci-history-attempt-${process.env.GITHUB_RUN_ATTEMPT||1}`)));
    if (batch.length<100 || batch.every(a=>Date.parse(a.created_at)<cutoff)) break;
    if (page===10) throw new Error('history artifact inventory overflow');
  }
  const suites=suiteSummaries(current);
  // A ci-history artifact contains only its run, never recursively merged history.
  const attempts=new Set();
  for (const artifact of artifacts.sort((a,b)=>Date.parse(b.created_at)-Date.parse(a.created_at))) {
    const key=`${artifact.workflow_run.id}/${artifact.name}`;
    if (attempts.has(key)) continue;
    if (attempts.size===MAX_HISTORY_RUNS) break;
    attempts.add(key);
    if (artifact.size_in_bytes>ledger.MAX_BYTES) throw new Error('history archive byte cap exceeded');
    const response=await github.rest.actions.downloadArtifact({...context.repo,artifact_id:artifact.id,archive_format:'zip'});
    const zip=path.join(directory,`history-${artifact.id}.zip`);
    const bytes=Buffer.from(response.data);
    if (bytes.length>ledger.MAX_BYTES) throw new Error('history download byte cap exceeded');
    fs.writeFileSync(zip,bytes);
    const value=execFileSync('python3',['scripts/ci-history-archive.py',zip],{encoding:'utf8',maxBuffer:ledger.MAX_BYTES,timeout:30000});
    const parsed=JSON.parse(value);
    if (parsed.version!==1 || !Array.isArray(parsed.records)) throw new Error('unknown history artifact schema');
    if (parsed.records.length>ledger.MAX_ROWS) throw new Error('history attempt row cap exceeded');
    for (const [suite, value] of Object.entries(suiteSummaries(parsed.records))) {
      const total=suites[suite]||{firstAttempts:0,firstPass:0,excluded:0,unknown:0};
      for (const field of Object.keys(total)) total[field]+=value[field];
      suites[suite]=total;
    }
    if (Object.keys(suites).length>1000) throw new Error('history suite cap exceeded');
  }
  ledger.write('ci-history.json',{...ledger.merge(current),jobs:inventory,windowDays:HISTORY_DAYS});
  ledger.write('ci-history-window.json',{version:1,suites,windowDays:HISTORY_DAYS,attemptCap:MAX_HISTORY_RUNS,observedPreviousAttempts:attempts.size});
  await core.summary.addRaw(summary(suites,inventory)).write();
};
module.exports.confirmInventory=confirmInventory;
module.exports.summary=summary;
module.exports.suiteSummaries=suiteSummaries;
