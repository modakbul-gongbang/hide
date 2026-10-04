const { test } = require('node:test');
const assert = require('node:assert/strict');
const { merge, category, signature, jobs, quarantine } = require('../ci-ledger.cjs');
const row = {sha:'head',os:'Linux',run:1,runAttempt:1,job:'web',shard:'1/6',suite:'suite',test:'test',repeat:0,retry:0,status:'failed'};
test('atomic bounded publication keeps the previous ledger on byte overflow', () => {
  const fs=require('node:fs'),path=require('node:path'),{write,MAX_BYTES}=require('../ci-ledger.cjs');
  const root=path.resolve('agents/runs/ci-test-refactor/reporter-controls'); fs.mkdirSync(root,{recursive:true});
  const file=path.join(root,'atomic-control.json');
  write(file,{version:1,records:[row]});
  const before=fs.readFileSync(file,'utf8');
  assert.throws(()=>write(file,{value:'x'.repeat(MAX_BYTES)}),/byte cap/);
  assert.equal(fs.readFileSync(file,'utf8'),before);
  assert.equal(fs.existsSync(file+'.'+process.pid+'.tmp'),false);
  assert.equal(merge([row,{...row,project:'another project'}]).records.length,2);
});
test('thirty repetitions mean every exact focus identity, with the original suite independently present', () => {
  const controls=require('../ci-controls.cjs');
  const contract=require('../../contracts/ci-failure-controls.json');
  const source={sha:'head',run:1,runAttempt:1};
  const records=contract.scenarios.focus.tests.flatMap(([suite,test])=>Array.from({length:30},(_,repeat)=>({...row,os:'Windows',suite,test,repeat,status:'passed'})));
  const value={version:1,records};
  const original={version:1,records:records.filter(r=>r.repeat===0).map(r=>({...r,run:2}))};
  for(let batch=0;batch<6;batch++) {
    const subset={version:1,records:records.filter(r=>r.repeat>=batch*5 && r.repeat<(batch+1)*5)};
    assert.equal(controls.results('focus','Windows',subset,source,undefined,batch).complete,false);
    assert.throws(()=>controls.results('focus','Windows',subset,source),/incomplete controls/);
  }
  assert.equal(controls.results('focus','Windows',value,source,original).checks.length,2);
  for (const changed of [records.slice(0,30),records.slice(1),[...records,records[0]],records.map(r=>r.repeat===29?{...r,repeat:28}:r),
    records.map(r=>({...r,os:'macOS'})),records.map(r=>({...r,retry:1})),records.map(r=>({...r,sha:'other'})),records.map(r=>({...r,status:'skipped'}))]) {
    assert.throws(()=>controls.results('focus','Windows',{version:1,records:changed},source,original),/incomplete controls/);
  }
  assert.throws(()=>controls.results('focus','Windows',value,source,{version:1,records:original.records.slice(0,1)}),/original suite/);
});
test('reused preparation refuses source, build, toolchain or byte differences', () => {
  const {verify}=require('../ci-preparation.cjs');
  const identity={sha:'tested',profile:'dev',features:'default',os:'linux',arch:'x64',rust:'rustc version',node:'v22.12.0'};
  const files={'target/debug/hide':{size:3,sha256:'abc'}};
  const value={version:1,identity,files};
  verify(value,identity,files);
  for(const key of Object.keys(identity)) assert.throws(()=>verify({...value,identity:{...identity,[key]:'different'}},identity,files),/mismatch/);
  assert.throws(()=>verify({...value,files:{}},identity,files),/inventory/);
  assert.throws(()=>verify(value,identity,{'target/debug/hide':{size:3,sha256:'changed'}}),/digest/);
});
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
test('Rust package and executable identities retain equal test names and their own panic', () => {
  const {rust}=require('../ci-ledger.cjs');
  const first={reason:'compiler-artifact',package_id:'path+file:///checkout/hide-kit#0.1.0',target:{name:'hide_kit',kind:['lib']},executable:'/checkout/target/debug/deps/hide_kit-111'};
  const second={...first,package_id:'path+file:///checkout/hide-platform#0.1.0',target:{name:'hide_platform',kind:['lib']},executable:'/checkout/target/debug/deps/hide_platform-222'};
  const text=[JSON.stringify(first),JSON.stringify(second),
    'Running unittests src/lib.rs (target/debug/deps/hide_kit-111)',
    'test equal_name ... FAILED', "thread 'equal_name' panicked at first.rs:1:\nassertion failed: kit contract", 'test result: FAILED.',
    'Running unittests src/lib.rs (target/debug/deps/hide_platform-222)',
    'test equal_name ... FAILED', "thread 'equal_name' (108862837) panicked at second.rs:2:\nassertion failed: platform contract", 'test result: FAILED.'].join('\n');
  const result=rust(text,row);
  assert.equal(result.records.length,2);
  assert.equal(result.summary.firstAttempts,2);
  assert.equal(new Set(result.records.map(r=>r.suite)).size,2);
  assert.equal(new Set(result.records.map(r=>r.package)).size,2);
  assert.deepEqual(result.records.map(r=>r.target.name),['hide_kit','hide_platform']);
  assert.match(result.records[0].assertion,/kit contract/);
  assert.doesNotMatch(result.records[0].assertion,/platform contract/);
  assert.match(result.records[1].assertion,/platform contract/);
  const legacy=rust(text.split('\n').filter(line=>!line.startsWith('{')).join('\n'),row);
  assert.equal(legacy.records.length,2);
  assert.match(legacy.records[0].suite,/hide_kit-111/);
});
test('collection API and inventory errors persist a failing partial attempt for the next window', async () => {
  const fs=require('node:fs'), path=require('node:path');
  const collect=require('../ci-history.cjs');
  const root=path.resolve('agents/runs/ci-test-refactor/history-controls');
  fs.mkdirSync(root,{recursive:true});
  const directory=fs.mkdtempSync(path.join(root,'attempt-'));
  const input=path.join(directory,'input'); fs.mkdirSync(input);
  const actual={...row,sha:'tested',run:'7',lane:'web-e2e',jobLabel:'actual web',status:'failed'};
  fs.writeFileSync(path.join(input,'row.json'),JSON.stringify({version:1,records:[actual]}));
  const job={id:42,name:'actual web',steps:[],conclusion:'failure'};
  for(const kind of ['api','inventory','history-api']) {
    const output=path.join(directory,kind); fs.mkdirSync(output);
    const github={rest:{actions:{
      listJobsForWorkflowRunAttempt:async()=>{if(kind==='api') throw Error('jobs permission denied'); return {data:{jobs:[job]}};},
      listArtifactsForRepo:async()=>{throw Error('history permission denied');}
    }}};
    const options={github,context:{repo:{},runId:7,sha:'tested'},core:{summary:{addRaw:()=>({write:async()=>{}})}},directory:input,outputDirectory:output,
      plan:kind==='inventory'?{version:1,lanes:{'web-e2e':true}}:undefined};
    await assert.rejects(collect(options),kind==='inventory'?/missing observed/:/permission denied/);
    const attempt=JSON.parse(fs.readFileSync(path.join(output,'ci-history.json')));
    const window=JSON.parse(fs.readFileSync(path.join(output,'ci-history-window.json')));
    assert.equal(attempt.collection.status,'partial-or-unknown');
    assert.equal(attempt.source.sha,'tested');
    assert.equal(attempt.records.filter(r=>r.status==='failed').length,1);
    assert.equal(attempt.summary.unknown,1);
    assert.equal(window.collection.status,'partial-or-unknown');
    assert.ok(Object.values(collect.suiteSummaries(attempt.records)).some(s=>s.unknown===1));
  }
});
test('the following successful collector includes an earlier unknown artifact in its real archive window', async () => {
  const fs=require('node:fs'),path=require('node:path'),{execFileSync}=require('node:child_process');
  const collect=require('../ci-history.cjs');
  const root=path.resolve('agents/runs/ci-test-refactor/history-controls');
  const directory=fs.mkdtempSync(path.join(root,'next-')); const input=path.join(directory,'input'); fs.mkdirSync(input);
  const source=path.join(directory,'ci-history.json');
  fs.writeFileSync(path.join(input,'row.json'),JSON.stringify({version:1,records:[{...row,sha:'previous',run:'7',jobLabel:'actual web',status:'failed'}]}));
  await assert.rejects(collect({github:{rest:{actions:{listJobsForWorkflowRunAttempt:async()=>{throw Error('jobs permission denied');}}}},
    context:{repo:{},runId:7,sha:'previous'},core:{summary:{addRaw:()=>({write:async()=>{}})}},directory:input,outputDirectory:directory}),/permission denied/);
  fs.writeFileSync(path.join(input,'row.json'),JSON.stringify({version:1,records:[{...row,sha:'next',run:'8',jobLabel:'actual web',status:'passed'}]}));
  const zip=path.join(directory,'previous.zip');
  execFileSync('python3',['-c',"import sys,zipfile; z=zipfile.ZipFile(sys.argv[2],'w'); z.write(sys.argv[1],'ci-history.json'); z.close()",source,zip]);
  const github={rest:{actions:{
    listJobsForWorkflowRunAttempt:async()=>({data:{jobs:[{id:43,name:'actual web',steps:[],conclusion:'success'}]}}),
    listArtifactsForRepo:async()=>({data:{artifacts:[{id:100,name:'ci-history-attempt-1',created_at:new Date().toISOString(),size_in_bytes:fs.statSync(zip).size,expired:false,workflow_run:{id:7}}]}}),
    downloadArtifact:async()=>({data:fs.readFileSync(zip)})
  }}};
  await collect({github,context:{repo:{},runId:8,sha:'next'},core:{summary:{addRaw:()=>({write:async()=>{}})}},directory:input,outputDirectory:directory});
  const window=JSON.parse(fs.readFileSync(path.join(directory,'ci-history-window.json')));
  assert.equal(window.observedPreviousAttempts,1);
  assert.equal(Object.values(window.suites).reduce((count,s)=>count+s.unknown,0),1);
  assert.equal(JSON.parse(fs.readFileSync(path.join(directory,'ci-history.json'))).collection.status,'complete');
});
