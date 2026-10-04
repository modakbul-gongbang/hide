// Exact acceptance identities for diagnostic fixtures, using the ordinary ledger.
const fs = require('node:fs');
const ledger = require('./ci-ledger.cjs');
const contract = require('../contracts/ci-failure-controls.json');

function scenario(name, os) {
  const selected = contract.scenarios[name];
  if (contract.version !== 1 || !selected || !selected.oses.includes(os)) throw Error(`unknown control scenario/OS: ${name}/${os}`);
  return selected;
}
function selection(name, os) {
  return scenario(name, os).tests.map(([file, title]) => `${file} > ${title}\n`).join('');
}
function results(name, os, value, source, original, batch) {
  const expected = scenario(name, os).tests;
  if (batch !== undefined && (!Number.isInteger(batch) || batch < 0 || batch > 5)) throw Error('unknown control batch');
  const count = batch === undefined ? contract.repetitions : 5;
  const offset = batch === undefined ? 0 : batch * count;
  if (value.version !== 1 || !Array.isArray(value.records)) throw Error('unknown repetition ledger');
  const rows = ledger.merge(value.records).records;
  const checks = [];
  for (const [suite, test] of expected) {
    const observed = rows.filter(row => row.suite === suite && row.test === test);
    const indices = observed.map(row => row.repeat).sort((a,b) => a-b);
    const wanted = Array.from({length:count}, (_,i) => i + offset);
    // Do not deduplicate identical observations here: one required fixture is
    // exactly one reporter result, not a retry or an extra upload masquerading as it.
    const raw = value.records.filter(row => row.suite === suite && row.test === test);
    const failures = raw.filter(row => row.os !== os || row.sha !== source.sha || row.run !== source.run || row.runAttempt !== source.runAttempt || row.retry !== 0 || row.status !== 'passed');
    if (raw.length !== count || JSON.stringify(indices) !== JSON.stringify(wanted) || failures.length) {
      throw Error(`incomplete controls: ${os} / ${suite} / ${test}: expected exactly ${count} passed retry-zero identities at ${offset}, observed ${raw.length}`);
    }
    let baseline = 'not-compared';
    if (original) {
      if (original.version !== 1 || !Array.isArray(original.records)) throw Error('unknown original suite ledger');
      const baselineRows = original.records.filter(row => row.suite === suite && row.test === test && row.os === os && row.sha === source.sha);
      if (baselineRows.length !== 1 || baselineRows[0].repeat !== 0 || baselineRows[0].retry !== 0 || baselineRows[0].status !== 'passed') throw Error(`original suite result missing or failed: ${os} / ${suite} / ${test}`);
      baseline = {run:baselineRows[0].run, runAttempt:baselineRows[0].runAttempt, job:baselineRows[0].job, status:baselineRows[0].status};
    }
    checks.push({suite,test,os,repeats:indices,results:raw.map(row=>({repeat:row.repeat,status:row.status,job:row.job,worker:row.worker})),original:baseline});
  }
  if (rows.length !== expected.length * count) throw Error('unexpected diagnostic selection identity');
  return {version:1,source,scenario:name,checks,complete:batch === undefined,batch:batch ?? null,originalCompared:Boolean(original)};
}
module.exports = {selection,results};
if (require.main === module) {
  const [mode,name,os,input,output,original] = process.argv.slice(2);
  if (mode === 'select' && input && !output) fs.writeFileSync(input,selection(name,os));
  else if (mode === 'results' && input && output) {
    const current = ledger.identity();
    ledger.write(output,results(name,os,JSON.parse(fs.readFileSync(input)),{sha:current.sha,run:current.run,runAttempt:current.runAttempt},original && JSON.parse(fs.readFileSync(original)),process.env.CI_CONTROL_BATCH === undefined ? undefined : Number(process.env.CI_CONTROL_BATCH)));
  } else throw Error('usage: ci-controls.cjs select NAME OS OUTPUT | results NAME OS LEDGER OUTPUT [ORIGINAL]');
}
