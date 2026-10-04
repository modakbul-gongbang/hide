const { test } = require('node:test');
const assert = require('node:assert/strict');
const { merge, category, signature, jobs, quarantine } = require('../ci-ledger.cjs');
const row = {sha:'head',os:'Linux',run:1,runAttempt:1,job:'web',shard:'1/6',suite:'suite',test:'test',repeat:0,retry:0,status:'failed'};
test('a later pass cannot replace the first failure, and repeated upload deduplicates', () => {
  const ledger = merge([row,row,{...row,retry:1,status:'passed'}]);
  assert.equal(ledger.records.length,2);
  assert.deepEqual(ledger.summary,{firstAttempts:1,firstPass:0,excluded:1,unknown:0});
  assert.throws(()=>merge([row,{...row,status:'passed'}]),/conflicting/);
  assert.equal(merge([row,{...row,runAttempt:2,status:'passed'}]).summary.firstPass,0);
});
test('provisioning, assertion, unknown and cancellation retain distinct outcomes', () => {
  assert.equal(category('electron.launch: timeout'),'provisioning');
  assert.equal(category('expect(locator).toHaveCount Expected: 3 Received: 2'),'assertion');
  assert.equal(merge([{...row,status:'interrupted'},{...row,test:'missing',status:'unknown'}]).summary.firstAttempts,0);
  assert.equal(merge([{...row,status:'unknown'}]).summary.unknown,1);
  assert.equal(signature('at /home/private/src/file.ts:3'),signature('at /Users/private/else/file.ts:3'));
});
test('an API failure is never an empty successful inventory', async () => {
  await assert.rejects(jobs({rest:{actions:{listJobsForWorkflowRunAttempt:async()=>{throw new Error('denied')}}}},{repo:{},runId:1}),/denied/);
});
test('registered identity and signature cannot hide a different failure', () => {
  const entry={id:'scenario',file:'web/e2e/spec.ts',title:'exact',oses:['Linux'],signature:{category:'assertion',any_of:[['Expected: 3','Received: 2']]}};
  const failure={suite:entry.file,test:entry.title,os:'Linux',status:'failed',category:'assertion',assertion:'Expected: 3 Received: 2'};
  assert.equal(quarantine(failure,{entries:[entry]}).classification,'known-signature');
  assert.equal(quarantine({...failure,assertion:'Expected: 3 Received: 1'},{entries:[entry]}).classification,'outside-registered-signature');
  assert.equal(quarantine({...failure,suite:'web/e2e/other.ts'},{entries:[entry]}),null);
  assert.equal(quarantine({...failure,test:'exact longer'},{entries:[entry]}),null);
});
test('a successful aggregate still fails when a shard has no observed suite', () => {
  const {confirmInventory,suiteSummaries}=require('../ci-history.cjs');
  const plan={lanes:{'web-e2e':true}};
  const rows=Array.from({length:6},(_,i)=>({...row,lane:'web-e2e',shard:`${i+1}/6`,suite:'real-suite'}));
  confirmInventory(plan,rows);
  assert.throws(()=>confirmInventory(plan,rows.slice(0,5)),/missing observed/);
  assert.throws(()=>confirmInventory(plan,rows.map(r=>r.shard==='6/6'?{...r,status:'skipped'}:r)),/missing observed/);
  assert.equal(suiteSummaries([row,{...row,runAttempt:2,status:'passed'}])['Linux / suite'].firstPass,0);
});
