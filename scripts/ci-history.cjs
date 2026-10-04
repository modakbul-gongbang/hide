// Bounded Actions/artifact ledger, with collection errors raised to its job.
const fs = require('node:fs');
const path = require('node:path');
const {execFileSync} = require('node:child_process');
const ledger = require('./ci-ledger.cjs');
const HISTORY_DAYS = 30;
const MAX_HISTORY_RUNS = 30;

function readRecords(directory, records=[], deferredErrors) {
  const errors=deferredErrors || [];
  let files=0;
  function report(error,file) {
    if(errors.length>=256) throw Error('producer error inventory cap exceeded');
    errors.push({stage:'input',file:path.relative(directory,file),message:String(error.message).slice(0,16000)});
  }
  function visit(at, depth=0) {
    if (depth>3) throw new Error('ledger directory depth cap exceeded');
    for (const item of fs.readdirSync(at,{withFileTypes:true}).sort((a,b)=>a.name.localeCompare(b.name))) {
      const file=path.join(at,item.name);
      if (item.isDirectory()) visit(file,depth+1);
      else if (item.name.endsWith('.json')) {
        if(++files>1000) throw Error('producer file inventory cap exceeded');
        let value;
        try {
          if (fs.statSync(file).size>ledger.MAX_BYTES) throw new Error('ledger input byte cap exceeded');
          value=JSON.parse(fs.readFileSync(file,'utf8'));
          if (value.version!==1 || !Array.isArray(value.records) || !value.records.length) throw new Error(`unknown/empty ledger: ${item.name}`);
        } catch(error) { report(error,file); continue; }
        if(records.length+value.records.length>ledger.MAX_ROWS) throw new Error('ledger row cap exceeded');
        records.push(...value.records);
        if (value.collection==='partial-or-unknown' || value.collection?.status==='partial-or-unknown') report(new Error(`partial producer ledger: ${item.name}`),file);
      }
    }
  }
  visit(directory);
  if(errors.length && !deferredErrors) {
    const error=new Error(errors[0].message); error.issues=errors; throw error;
  }
  return records;
}

function confirmInventory(plan, rows) {
  const expected=plan.inventory;
  if (!expected || typeof expected!=='object') throw Error('missing planned suite inventory');
  for (const [lane, shards] of Object.entries(expected)) {
    if (!plan.lanes[lane]) continue;
    for (const shard of shards) {
      if (!rows.some(row=>row.lane===lane && row.shard===shard && !['skipped','unknown','interrupted'].includes(row.status)))
        throw new Error(`missing observed suite inventory: ${lane} ${shard}`);
    }
  }
}

function suiteSummaries(rows, options) {
  const suites=new Map();
  for (const row of rows) {
    const key=`${row.os} / ${row.suite}`;
    const list=suites.get(key)||[]; list.push(row); suites.set(key,list);
  }
  return Object.fromEntries([...suites].map(([key,records])=>[key,ledger.merge(records,options).summary]));
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

async function collectInto(options, state) {
  const {github,context,directory,planPath}=options;
  state.stage='plan';
  const plan=planPath?JSON.parse(fs.readFileSync(planPath,'utf8')):options.plan;
  if (plan && (plan.version!==2 || !plan.lanes)) throw new Error('unknown CI plan');
  state.stage='input';
  const pending=[];
  state.pendingErrors=pending;
  const current=readRecords(directory,state.records,pending);
  state.stage='jobs-api';
  const inventory=await ledger.jobs(github,context);
  state.jobs=inventory;
  state.stage='inventory';
  if (!plan && !current.length) throw new Error('no observed suite collection');
  if (plan) try { confirmInventory(plan,current); }
  catch(error) { pending.push({stage:state.stage,message:String(error.message)}); }
  state.stage='source-and-job-identity';
  for (const row of current) {
    if (row.sha!==context.sha || String(row.run)!==String(context.runId) || row.runAttempt!==Number(process.env.GITHUB_RUN_ATTEMPT||1)) {
      pending.push({stage:state.stage,message:'ledger source identity mismatch'}); continue;
    }
    const matching=inventory.filter(job=>job.name===row.jobLabel || job.name.endsWith(' / '+row.jobLabel));
    if (matching.length!==1) { pending.push({stage:state.stage,message:`missing/ambiguous Actions job identity: ${row.jobLabel}`}); continue; }
    row.jobId=matching[0].id; row.jobUrl=matching[0].url;
  }
  if(pending.length) {
    const error=new Error(pending[0].message); error.issues=pending; throw error;
  }
  state.stage='history-api';
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
  state.stage='suite-summary';
  const suites=suiteSummaries(current);
  state.suites=suites;
  // A ci-history artifact contains only its run, never recursively merged history.
  const attempts=new Set();
  for (const artifact of artifacts.sort((a,b)=>Date.parse(b.created_at)-Date.parse(a.created_at))) {
    const key=`${artifact.workflow_run.id}/${artifact.name}`;
    if (attempts.has(key)) continue;
    if (attempts.size===MAX_HISTORY_RUNS) break;
    attempts.add(key);
    state.attempts=attempts.size;
    state.stage='history-archive';
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
    for (const [suite, value] of Object.entries(suiteSummaries(parsed.records,{allowConflicts:parsed.collection?.status==='partial-or-unknown'}))) {
      const total=suites[suite]||{firstAttempts:0,firstPass:0,excluded:0,unknown:0};
      for (const field of Object.keys(total)) total[field]+=value[field];
      suites[suite]=total;
    }
    if (Object.keys(suites).length>1000) throw new Error('history suite cap exceeded');
  }
  state.stage='complete';
}

module.exports=async function collect(options) {
  const {context,core,outputDirectory='.'}=options;
  const state={records:[],jobs:[],suites:{},attempts:0,stage:'start',errors:[]};
  let failure;
  function unknown(error) {
    const issues=error.issues || [...(state.pendingErrors || []),{stage:state.stage,message:String(error.message)}];
    if(issues.length>256) throw Error('collection error inventory cap exceeded');
    for(const issue of issues) {
      state.errors.push({...issue,message:issue.message.slice(0,16000)});
      state.records.push({...ledger.identity(),sha:context.sha,run:String(context.runId),
        suite:'CI collection',test:`${issue.stage} ${issue.file || state.errors.length}`,repeat:0,retry:0,status:'unknown',category:'collection',
        assertion:issue.message.slice(0,16000),signature:ledger.signature(issue.message)});
    }
  }
  function save() {
    const collection={status:state.errors.length?'partial-or-unknown':'complete',errors:state.errors};
    const current=ledger.merge(state.records,{allowConflicts:Boolean(failure)});
    // The current attempt is saved even when a remote API or required
    // inventory fails. Its unknown result remains visible to later windows.
    ledger.write(path.join(outputDirectory,'ci-history.json'),{...current,jobs:state.jobs,source:{sha:context.sha,run:String(context.runId),attempt:Number(process.env.GITHUB_RUN_ATTEMPT||1)},collection,windowDays:HISTORY_DAYS});
    const currentSuites=suiteSummaries(state.records,{allowConflicts:Boolean(failure)});
    const suites={...state.suites};
    // Prior summaries already contain current test rows; add only collection
    // errors after a partial history read, otherwise summarize all retained rows.
    if (!Object.keys(suites).length) Object.assign(suites,currentSuites);
    else for (const [key,value] of Object.entries(currentSuites).filter(([key])=>key.endsWith(' / CI collection'))) {
      const total=suites[key]||{firstAttempts:0,firstPass:0,excluded:0,unknown:0};
      for (const field of Object.keys(total)) total[field]+=value[field];
      suites[key]=total;
    }
    ledger.write(path.join(outputDirectory,'ci-history-window.json'),{version:1,suites,collection,windowDays:HISTORY_DAYS,attemptCap:MAX_HISTORY_RUNS,observedPreviousAttempts:state.attempts});
    return suites;
  }
  try { await collectInto(options,state); }
  catch (error) { failure=error; unknown(error); }
  let suites;
  try { suites=save(); }
  catch (error) {
    // A malformed duplicate or byte overflow cannot become an empty success.
    // Keep a bounded unknown receipt, naming why the partial rows could not be saved.
    failure ||= error;
    state.stage='attempt-save'; unknown(error);
    state.records=state.records.filter(row=>row.suite==='CI collection');
    state.suites={}; suites=save();
  }
  try { await core.summary.addRaw(summary(suites,state.jobs)).write(); }
  catch (error) {
    failure ||= error;
    state.stage='summary'; unknown(error); save();
  }
  if (failure) throw failure;
};
module.exports.confirmInventory=confirmInventory;
module.exports.summary=summary;
module.exports.suiteSummaries=suiteSummaries;
module.exports.readRecords=readRecords;
