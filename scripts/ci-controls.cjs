// Exact acceptance identities for diagnostic fixtures, using the ordinary ledger.
const fs = require('node:fs');
const path = require('node:path');
const ledger = require('./ci-ledger.cjs');
const contract = require('../contracts/ci-failure-controls.json');

function scenario(name, os) {
  const selected = contract.scenarios[name];
  if (contract.version !== 1 || !selected || !selected.oses.includes(os)) throw Error(`unknown control scenario/OS: ${name}/${os}`);
  return selected;
}
function testList(tests) {
  return tests.map(([file,title])=>{
    const packageRoot=file.split('/')[0]+'/e2e';
    if(!['web/e2e','desktop/e2e'].includes(packageRoot) || !file.startsWith(packageRoot+'/')) throw Error('unsupported control file identity');
    // These ordinary configs own rootDir = PACKAGE/e2e. Repository suite
    // identity stays in the ledger; Playwright compares a rootDir-relative
    // file plus each describe/test title token, rather than that suite string.
    const titles=Array.isArray(title)?title:[title];
    if(!titles.length || titles.some(token=>typeof token!=='string' || !token.trim() || token!==token.trim() || /[\r\n>›]/.test(token))) throw Error('unsupported control title path');
    return `${path.posix.relative(packageRoot,file)} > ${titles.join(' > ')}\n`;
  }).join('');
}
function selection(name, os) { return testList(scenario(name,os).tests); }
function results(name, os, value, source, original, batch) {
  const expected = scenario(name, os).tests;
  if (batch !== undefined && (!Number.isInteger(batch) || batch < 0 || batch > 5)) throw Error('unknown control batch');
  const count = batch === undefined ? contract.repetitions : 5;
  const offset = batch === undefined ? 0 : batch * count;
  if (value.version !== 1 || !Array.isArray(value.records)) throw Error('unknown repetition ledger');
  const rows = ledger.merge(value.records).records;
  const checks = [];
  for (const [suite, test] of expected) {
    const exact = row => row.suite === suite && row.test === test && JSON.stringify(row.titlePath || [row.test]) === JSON.stringify([test]);
    const observed = rows.filter(exact);
    const indices = observed.map(row => row.repeat).sort((a,b) => a-b);
    const wanted = Array.from({length:count}, (_,i) => i + offset);
    // Do not deduplicate identical observations here: one required fixture is
    // exactly one reporter result, not a retry or an extra upload masquerading as it.
    const raw = value.records.filter(exact);
    const failures = raw.filter(row => row.os !== os || row.sha !== source.sha || row.run !== source.run || row.runAttempt !== source.runAttempt || row.retry !== 0 || row.status !== 'passed');
    if (raw.length !== count || JSON.stringify(indices) !== JSON.stringify(wanted) || failures.length) {
      throw Error(`incomplete controls: ${os} / ${suite} / ${test}: expected exactly ${count} passed retry-zero identities at ${offset}, observed ${raw.length}`);
    }
    let baseline = 'not-compared';
    if (original) {
      if (original.version !== 1 || !Array.isArray(original.records)) throw Error('unknown original suite ledger');
      const baselineRows = original.records.filter(row => exact(row) && row.os === os && row.sha === source.sha);
      if (baselineRows.length !== 1 || baselineRows[0].repeat !== 0 || baselineRows[0].retry !== 0 || baselineRows[0].status !== 'passed') throw Error(`original suite result missing or failed: ${os} / ${suite} / ${test}`);
      baseline = {run:baselineRows[0].run, runAttempt:baselineRows[0].runAttempt, job:baselineRows[0].job, status:baselineRows[0].status};
    }
    checks.push({suite,test,os,repeats:indices,results:raw.map(row=>({repeat:row.repeat,status:row.status,job:row.job,worker:row.worker})),original:baseline});
  }
  if (rows.length !== expected.length * count) throw Error('unexpected diagnostic selection identity');
  return {version:1,source,scenario:name,checks,complete:batch === undefined,batch:batch ?? null,originalCompared:Boolean(original)};
}
function collect(directory, output, source, downloads) {
  const receipt={version:1,source:source || null,checks:[],complete:false,errors:[]};
  let stage='source';
  try {
    receipt.source=source || ledger.identity();
    stage='artifact-download';
    if(downloads !== undefined) {
      receipt.downloads=typeof downloads==='string'?JSON.parse(downloads):downloads;
      if(!receipt.downloads || Object.keys(receipt.downloads).sort().join(',')!=='desktop,web') throw Error('unknown artifact download inventory');
      for(const [consumer,outcome] of Object.entries(receipt.downloads)) if(outcome!=='success') {
        receipt.errors.push({stage,consumer,message:`artifact download ${outcome==='failure'||outcome==='cancelled'||outcome==='skipped'?outcome:'unknown'}`});
      }
    }
    stage='input';
    const records=require('./ci-history.cjs').readRecords(directory);
    if (records.some(row=>!Object.values(contract.scenarios).some(scenario=>scenario.oses.includes(row.os) && scenario.tests.some(([suite,test])=>suite===row.suite && test===row.test)))) throw Error('unexpected collected control identity');
    stage='exact-results';
    for(const [name,scenario] of Object.entries(contract.scenarios)) for(const os of scenario.oses) {
      const selected=records.filter(row=>row.os===os && scenario.tests.some(([suite,test])=>row.suite===suite && row.test===test));
      receipt.checks.push(results(name,os,{version:1,records:selected},receipt.source));
    }
    if(receipt.errors.length) throw Error('artifact download incomplete');
    receipt.complete=true;
    return receipt;
  } catch(error) {
    receipt.errors.push({stage,message:String(error.message).slice(0,16000)});
    throw error;
  } finally { ledger.write(output,receipt); }
}
module.exports = {selection,testList,results,collect};
if (require.main === module) {
  const [mode,name,os,input,output,original] = process.argv.slice(2);
  if (mode === 'collect' && name && os && !output) collect(name,os,undefined,input);
  else if (mode === 'select' && input && !output) fs.writeFileSync(input,selection(name,os));
  else if (mode === 'results' && input && output) {
    const current = ledger.identity();
    ledger.write(output,results(name,os,JSON.parse(fs.readFileSync(input)),{sha:current.sha,run:current.run,runAttempt:current.runAttempt},original && JSON.parse(fs.readFileSync(original)),process.env.CI_CONTROL_BATCH === undefined ? undefined : Number(process.env.CI_CONTROL_BATCH)));
  } else throw Error('usage: ci-controls.cjs collect DIRECTORY OUTPUT [DOWNLOAD_RESULTS_JSON] | select NAME OS OUTPUT | results NAME OS LEDGER OUTPUT [ORIGINAL]');
}
