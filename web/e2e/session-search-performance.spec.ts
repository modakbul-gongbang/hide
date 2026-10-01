// Measures real isolated worker, wire fan-out and terminal echo while a full
// admitted history indexes. Resource values are evidence, not synthetic limits.
import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { enterWorkspace } from "./wire";
import { openSessions } from "./server-session-fixture";

test("2000-session search progress preserves history and terminal input", async ({ browser }) => {
  test.setTimeout(180_000);
  const herdr = await startHerdr();
  const daemon = await startHided(herdr, "search-performance");
  const context = await browser.newContext({viewport:{width:1440,height:900}});
  const terminal = await context.newPage();
  const sessions = await context.newPage();
  const records: {client:string; at:number; bytes:number; searchBytes:number; historyRows:number; type:string; revision:number; indexed:number; total:number; indexing:boolean}[] = [];
  const errors:string[] = [];
  let finalSearch:{indexing?:boolean; indexed?:number; total?:number; failure?:string|null} | undefined;
  for (const [client,page] of [["terminal",terminal],["sessions",sessions]] as const) {
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("websocket", (socket) => socket.on("framereceived", ({payload}) => {
      if(typeof payload !== "string") return;
      const frame=JSON.parse(payload);
      const data=frame.payload;
      if(!data) return;
      if(client === "sessions" && data.session_search) finalSearch=data.session_search;
      records.push({client,type:String(frame.type),at:Date.now(),bytes:Buffer.byteLength(payload),searchBytes:data.session_search?Buffer.byteLength(JSON.stringify(data.session_search)):0,historyRows:data.project_sessions?.rows?.length??0,revision:data.revision??0,indexed:data.session_search?.indexed??-1,total:data.session_search?.total??-1,indexing:!!data.session_search?.indexing});
    }));
  }
  const browserCdp=await browser.newBrowserCDPSession();
  const processes=async()=> (await browserCdp.send("SystemInfo.getProcessInfo")).processInfo as {id:number;type:string;cpuTime:number}[];
  const daemonStats=()=> {const values=execFileSync("/bin/ps",["-p",String(daemon.pid),"-o","time=,rss="],{encoding:"utf8"}).trim().split(/\s+/);const parts=values[0]!.split(":").map(Number);return {cpuSeconds:parts.reduce((sum,value)=>sum*60+value,0),rssKiB:Number(values[1])};};
  const measure=async()=>({at:Date.now(),daemon:daemonStats(),browser:await processes()});
  const echo=async(page:Page,prefix:string)=> {const values:number[]=[];for(let i=0;i<24;i++){const marker=`${prefix}${String(i).padStart(3,"0")}`;await page.evaluate((value)=>window.__hideProbe!.arm(value),marker);execFileSync(herdr.bin,["pane","send-text",herdr.panes[0],`${marker}\n`],{env:herdr.env,timeout:3000});const returned=Date.now();const sample=await page.evaluate(()=>window.__hideProbe!.waitArmed());values.push(sample.write_ms-returned);await page.waitForTimeout(80);}return values;};
  const directory=path.join(daemon.home,".codex/sessions/2026/10/01");
  try {
    await terminal.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(terminal,"fixture");
    await expect.poll(()=>terminal.evaluate(()=>window.__hideProbe?.paneId())).toBe(herdr.panes[0]);
    await sessions.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(sessions,"fixture");
    await terminal.waitForTimeout(5000);
    const idleStart=await measure();
    await terminal.waitForTimeout(5000);
    const idleEnd=await measure();
    const idleEcho=await echo(terminal,"baseline");
    fs.mkdirSync(directory,{recursive:true});
    const cwd=fs.realpathSync(path.join(herdr.root,"fixture"));
    const timestamp=new Date().toISOString();
    for(let i=0;i<2000;i++) fs.writeFileSync(path.join(directory,`rollout-perf-${i}.jsonl`),JSON.stringify({type:"session_meta",payload:{id:`perf-${i}`,cwd}})+"\n"+JSON.stringify({type:"response_item",timestamp,payload:{type:"message",role:"user",content:[{type:"input_text",text:`대화검색 ${i} ${"context ".repeat(64)}`} ]}})+"\n");
    const drivenStart=await measure();
    await openSessions(sessions);
    await expect.poll(()=>finalSearch?.total,{timeout:30_000}).toBe(2000);
    const drivenEcho=await echo(terminal,"driven");
    await expect.poll(()=>finalSearch?.indexed===2000 && finalSearch.indexing===false,{timeout:90_000}).toBe(true);
    const drivenEnd=await measure();
    const idleIndexedFrames = records.length;
    await terminal.waitForTimeout(5000);
    const indexedIdleEnd=await measure();
    expect(finalSearch?.failure).toBeNull();
    const progress=records.filter(row=>row.at>=drivenStart.at && row.searchBytes>0 && row.indexing && row.indexed>0);
    const summarize=(a:Awaited<ReturnType<typeof measure>>,b:Awaited<ReturnType<typeof measure>>)=>{const seconds=(b.at-a.at)/1000;const old=new Map(a.browser.map(p=>[p.id,p.cpuTime]));const cpu=b.browser.reduce((sum,p)=>sum+Math.max(0,p.cpuTime-(old.get(p.id)??p.cpuTime)),0);return {seconds,daemonCpuPercent:100*(b.daemon.cpuSeconds-a.daemon.cpuSeconds)/seconds,browserCpuPercent:100*cpu/seconds,daemonRssKiB:b.daemon.rssKiB};};
    const percentile=(values:number[],fraction:number)=>[...values].sort((a,b)=>a-b)[Math.ceil(values.length*fraction)-1];
    const evidence={workload:"2000 local Codex sessions, one Human message each, 2 Chromium clients; debug hided; 5-second idle has no inputs; 24 identical marker+LF inputs before and during indexing",method:"echo: send-text CLI return to xterm write callback; process cumulative CPU / wall time; RSS ps KiB; full received wire bytes",load:execFileSync("/usr/bin/uptime",{encoding:"utf8"}).trim(),idle:summarize(idleStart,idleEnd),driven:summarize(drivenStart,drivenEnd),indexedIdle:summarize(drivenEnd,indexedIdleEnd),indexedIdleSearchFrames:records.slice(idleIndexedFrames).filter(row=>row.searchBytes>0).length,echo:{idle:{samples:idleEcho,p50:percentile(idleEcho,.5),p95:percentile(idleEcho,.95)},driven:{samples:drivenEcho,p50:percentile(drivenEcho,.5),p95:percentile(drivenEcho,.95)}},wire:{progressFrames:progress.length,clients:Object.fromEntries(["terminal","sessions"].map(client=>[client,progress.filter(row=>row.client===client).length])),maxCoalescedProgressFrameBytes:Math.max(...progress.map(row=>row.bytes)),maxSteadyProgressFrameBytes:Math.max(...progress.filter(row=>row.historyRows===0).map(row=>row.bytes)),maxSearchSectionBytes:Math.max(...progress.map(row=>row.searchBytes)),historyFrames:records.filter(row=>row.historyRows===2000).map(row=>({client:row.client,bytes:row.bytes})),historyRowsInProgress:progress.filter(row=>row.historyRows>0).map(row=>({client:row.client,indexed:row.indexed,bytes:row.bytes}))},errors};
    const output=process.env.HIDE_E2E_SCREENSHOT_DIR;
    if(output) fs.writeFileSync(path.join(output,"search-performance.json"),JSON.stringify({...evidence,frames:records},null,2));
    console.log(JSON.stringify(evidence));
    expect(progress.length).toBeGreaterThan(2);
    // The first independently loaded history may coalesce with a progress
    // transition; after that, progress must preserve the received rows.
    for (const client of ["terminal", "sessions"]) {
      const history = records.filter(row=>row.client===client && row.historyRows===2000);
      expect(history.length).toBe(1);
      expect(progress.filter(row=>row.client===client && row.at>history[0]!.at).every(row=>row.historyRows===0)).toBe(true);
    }
    expect(errors).toEqual([]);
  } finally {await context.close();daemon.stop();herdr.stop();}
});
