import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import test from "node:test";
import { MAX_BODY_BYTES, MAX_EVENTS, MAX_LEDGER_BYTES, MAX_MESSAGE_BYTES, MAX_REQUESTS } from "../../dist/hcoord/model.js";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");
const ADAPTER = path.resolve(import.meta.dirname, "../../examples/channel-adapter.mjs");
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("coordinator keeps a request identity, answer, cancellation, and watch cycle through restart", async (t) => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "hcoord-e2e-"));
  const bin = path.join(home, "bin");
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, "herdr"), `#!/usr/bin/env node
const fs=require('node:fs'), path=require('node:path');
const target=process.argv[4];
fs.appendFileSync(path.join(process.env.HOME,'herdr-calls.log'),process.argv.slice(2).join(' ')+'\\n');
if(process.argv[2]==='agent' && process.argv[3]==='get') {
  const row=target==='parent-pane'?{name:'parent',session:'one',instance:'a'}:target==='child-pane'?{name:'child',session:'two',instance:'b'}:target?.endsWith('-pane') && fs.existsSync(path.join(process.env.HOME,target+'.started'))?{name:target.slice(0,-5),session:target+'-session',instance:target+'-instance'}:null;
  if(!row){process.stderr.write(JSON.stringify({error:{code:'agent_not_found'}}));process.exitCode=1;}
  else {const statusFile=path.join(process.env.HOME,target+'.status');const status=fs.existsSync(statusFile)?fs.readFileSync(statusFile,'utf8').trim():'idle';const instanceFile=path.join(process.env.HOME,target+'.instance');const instance=fs.existsSync(instanceFile)?fs.readFileSync(instanceFile,'utf8').trim():row.instance;const initializing=target.endsWith('-pane')&&!['parent-pane','child-pane'].includes(target)&&!fs.existsSync(path.join(process.env.HOME,row.name+'.initialized'));const nameLost=process.env.HCOORD_FAKE_NAME_LOST==='1'&&!['parent-pane','child-pane'].includes(target)&&!fs.existsSync(path.join(process.env.HOME,target+'.renamed'));process.stdout.write(JSON.stringify({result:{type:'agent_info',agent:{pane_id:target,name:process.env.HCOORD_FAKE_OBSERVER_REPLACED==='1'&&target==='parent-pane'?'replacement':nameLost?null:row.name,agent:process.env.HCOORD_FAKE_WRONG_KIND==='1'&&target==='kind-check-pane'?'claude':'codex',agent_session:initializing?undefined:{value:process.env.HCOORD_FAKE_OBSERVER_REPLACED==='1'&&target==='parent-pane'?'replacement-session':row.session},terminal_id:instance,agent_status:status,interactive_ready:process.env.HCOORD_FAKE_NOT_READY!=='1'}}}));}
} else if(process.argv[2]==='pane' && process.argv[3]==='get') {
  if(target?.endsWith('-pane') && !['parent-pane','child-pane'].includes(target) && !fs.existsSync(path.join(process.env.HOME,target.slice(0,-5)+'.tab'))){process.stderr.write('pane missing');process.exitCode=1;}
  else {const cwdFile=path.join(process.env.HOME,target==='parent-pane'?'parent.cwd':target.slice(0,-5)+'.cwd');const cwd=fs.existsSync(cwdFile)?fs.readFileSync(cwdFile,'utf8'):process.env.HOME;process.stdout.write(JSON.stringify({result:{type:'pane_info',pane:{pane_id:process.env.HCOORD_FAKE_PANE_ID_MISSING==='1'&&target==='partial-pane'?undefined:target,workspace_id:'test-workspace',cwd}}}));}
} else if(process.argv[2]==='tab' && process.argv[3]==='create') {
  const label=process.argv[process.argv.indexOf('--label')+1], marker=path.join(process.env.HOME,label+'.tab');
  if(fs.existsSync(marker)){process.stderr.write('duplicate tab');process.exitCode=9;}
  else {fs.writeFileSync(marker,'1');fs.writeFileSync(path.join(process.env.HOME,label+'.cwd'),process.argv[process.argv.indexOf('--cwd')+1]);process.stdout.write(JSON.stringify({result:{root_pane:process.env.HCOORD_FAKE_TAB_INVALID==='1'?{}:{pane_id:label+'-pane'}}}));}
} else if(process.argv[2]==='agent' && process.argv[3]==='start') {
  const name=process.argv[4], pane=process.argv[process.argv.indexOf('--pane')+1];
  if(process.env.HCOORD_FAKE_START_FAIL==='1'){process.stderr.write('start outcome unknown');process.exitCode=8;}
  else if(process.env.HCOORD_FAKE_START_UNCONFIRMED){fs.writeFileSync(path.join(process.env.HOME,pane+'.started'),'1');process.stderr.write(JSON.stringify({error:{code:process.env.HCOORD_FAKE_START_UNCONFIRMED,message:'agent start was not confirmed'}}));process.exitCode=1;}
  else {fs.writeFileSync(path.join(process.env.HOME,pane+'.started'),'1');if(name==='optioned')fs.writeFileSync(path.join(process.env.HOME,'optioned.start-args.json'),JSON.stringify(process.argv.slice(2)));process.stdout.write(JSON.stringify({result:{agent:{name,pane_id:pane}}}));}
} else if(process.argv[2]==='agent' && process.argv[3]==='rename') {
  if(process.env.HCOORD_FAKE_RENAME_FAIL==='1'){process.stderr.write(JSON.stringify({error:{code:'agent_rename_failed'}}));process.exit(1);}
  fs.writeFileSync(path.join(process.env.HOME,process.argv[4]+'.renamed'),JSON.stringify(process.argv.slice(2)));
  process.stdout.write(JSON.stringify({result:{type:'agent_renamed'}}));
} else if(process.argv[2]==='agent' && process.argv[3]==='read') {
  process.stdout.write(process.env.HCOORD_FAKE_FIRST_TURN_SCREEN||'› Ask Codex to do anything');
} else if(process.argv[2]==='agent' && process.argv[3]==='prompt') {
  if(process.argv[4]==='--help') process.stdout.write(process.env.HCOORD_FAKE_PROMPT_API==='0'?'Usage: herdr agent prompt --unsupported':'Usage: herdr agent prompt <TARGET> <TEXT>');
  else if(!process.argv.includes('--expected-input-guard')) {
    if(process.argv[4].endsWith('-pane') && fs.existsSync(path.join(process.env.HOME,process.argv[4]+'.started'))) fs.writeFileSync(path.join(process.env.HOME,process.argv[4].slice(0,-5)+'.initialized'),'1');
    fs.appendFileSync(path.join(process.env.HOME,'official-prompts.jsonl'),JSON.stringify({target:process.argv[4],text:process.argv[5]})+'\\n');
    if(process.env.HCOORD_FAKE_PROMPT_UNKNOWN_ONCE==='1' && process.argv[4].startsWith('uncertain-')) {
      const marker=path.join(process.env.HOME,process.argv[4]+'.unknown');
      if(!fs.existsSync(marker)) {fs.writeFileSync(marker,'1');if(process.env.HCOORD_FAKE_REPLACE_TERMINAL_ON_UNKNOWN==='1')fs.writeFileSync(path.join(process.env.HOME,process.argv[4]+'.instance'),'replacement-instance');process.stderr.write('submitted but reply lost');process.exit(8);}
    }
    process.stdout.write(JSON.stringify({result:{outcome:'submitted'}}));
  } else {fs.writeFileSync(path.join(process.env.HOME,'UNSUPPORTED_GUARD'),'1');process.stderr.write('unexpected guarded prompt');process.exitCode=9;}
} else if(process.argv[2]==='notification' && process.argv[3]==='show') {
  fs.writeFileSync(path.join(process.env.HOME,'notification-args.json'),JSON.stringify(process.argv.slice(2)));
  process.stdout.write(JSON.stringify({result:{outcome:'shown'}}));
} else {process.stderr.write('unexpected Herdr operation');process.exitCode=9;}
`, { mode: 0o755 });
  const env = { ...process.env, HOME: home, PATH: `${bin}${path.delimiter}${process.env.PATH}` };
  delete env.HERDR_SOCKET_PATH;
  delete env.HERDR_BIN_PATH;
  delete env.HCOORD_FAKE_PROMPT_API;
  delete env.HCOORD_FAKE_OBSERVER_REPLACED;
  let daemon = null;
  let daemonErrors = "";
  const start = async () => {
    daemon = spawn(process.execPath, [CLI, "daemon", "run"], { env, stdio: ["ignore", "ignore", "pipe"] });
    let error = "";
    daemon.stderr.on("data", (chunk) => { error += chunk; daemonErrors += chunk; });
    for (let attempt = 0; attempt < 500; attempt += 1) {
      const socket = path.join(home, ".hcoord", "api.sock");
      if (fs.existsSync(socket) && (fs.statSync(socket).mode & 0o777) === 0o600) return;
      if (daemon.exitCode !== null) throw new Error(`daemon exited: ${error}`);
      await wait(20);
    }
    throw new Error(`daemon did not create socket: ${error}`);
  };
  const stop = async () => {
    if (!daemon) return;
    const old = daemon;
    if (old.exitCode === null) {
      old.kill("SIGTERM");
      await new Promise((resolve) => old.once("exit", resolve));
    }
    daemon = null;
  };
  t.after(async () => { await stop(); fs.rmSync(home, { recursive: true, force: true }); });
  const command = (...args) => spawnSync(process.execPath, [CLI, ...args, "--json"], { env, encoding: "utf8" });
  const ok = (...args) => {
    const result = command(...args);
    assert.equal(result.status, 0, `${args.join(" ")}: ${result.stderr} ${result.stdout}`);
    const parsed = JSON.parse(result.stdout);
    assert.equal(parsed.ok, true);
    return parsed.value;
  };
  await start();
  assert.equal(fs.statSync(path.join(home, ".hcoord", "api.sock")).mode & 0o777, 0o600);
  const competingDaemon = spawnSync(process.execPath, [CLI, "daemon", "run", "--json"], { env, encoding: "utf8" });
  assert.equal(competingDaemon.status, 1);
  assert.equal(JSON.parse(competingDaemon.stdout).error.code, "already_running");
  assert.equal(ok("daemon", "status").stale, undefined, "a competing start preserves the original daemon socket");
  const malformed = await new Promise((resolve, reject) => {
    const socket = net.createConnection(path.join(home, ".hcoord", "api.sock"));
    let received = "";
    socket.on("connect", () => socket.write('{"body": SECRET_ABC}\n'));
    socket.on("data", (chunk) => { received += chunk; });
    socket.on("end", () => resolve(JSON.parse(received.trim())));
    socket.on("error", reject);
  });
  assert.equal(malformed.error.code, "protocol");
  assert.equal(daemonErrors.includes("SECRET_ABC"), false, "a parser failure does not copy request text to logs");
  assert.equal(ok("daemon", "status").platform.macos.localSocket, "verified_isolated");
  const parent = ok("agent", "register", "--machine", "local", "--session", "one", "--instance", "a", "--name", "parent", "--pane", "parent-pane");
  // D-19, B3: --check answers what registration would decide and saves nothing.
  const checkChild = ["agent", "register", "--check", "--machine", "local", "--session", "two", "--instance", "b", "--name", "child", "--parent", parent.id, "--pane", "child-pane"];
  const checked = ok(...checkChild);
  assert.deepEqual([checked.ready, checked.saved, checked.participant, checked.pane], [true, false, null, "child-pane"]);
  assert.equal(ok("agent", "register", "--check", "--machine", "local", "--session", "one", "--instance", "a", "--name", "parent", "--pane", "parent-pane").participant, parent.id, "a registered execution is named");
  assert.equal(JSON.parse(command("agent", "register", "--check", "--machine", "local", "--session", "other", "--instance", "b", "--name", "child", "--pane", "child-pane").stdout).error.code, "identity_conflict", "another session in the pane is refused");
  assert.equal(ok("status").counts.agents, 1, "a check registers nobody");
  const child = ok("agent", "register", "--machine", "local", "--session", "two", "--instance", "b", "--name", "child", "--parent", parent.id, "--pane", "child-pane");
  assert.equal(JSON.parse(command("agent", "register", "--machine", "remote", "--session", "one", "--instance", "a", "--name", "remote", "--pane", "parent-pane").stdout).error.code, "unsupported_runtime", "a Herdr without --machine forwarding cannot host remote participants");
  assert.equal(ok("agent", "register", "--machine", "local", "--session", "one", "--instance", "a", "--name", "renamed", "--pane", "parent-pane").id, parent.id, "the same pane and session under another name is the same participant (D-18)");
  assert.equal(ok("agent", "register", "--machine", "local", "--session", "one", "--instance", "a", "--name", "parent", "--pane", "parent-pane").name, "parent");
  // B24: hcoord has no client-specific commands.
  const retired = command("sasu", "list");
  assert.equal(retired.status, 2);
  assert.equal(JSON.parse(retired.stdout).error.code, "invalid_argument");
  assert.doesNotMatch(JSON.parse(retired.stdout).error.message, /sasu/);
  const checkParent = ["agent", "register", "--check", "--machine", "local", "--session", "one", "--instance", "a", "--name", "parent", "--pane", "parent-pane"];
  await stop();
  env.HCOORD_FAKE_OBSERVER_REPLACED = "1";
  fs.writeFileSync(path.join(home, ".hcoord", "api.sock.lock"), "99999999\n");
  fs.mkdirSync(path.join(home, ".hcoord", "api.sock.lock.recovery"));
  fs.writeFileSync(path.join(home, ".hcoord", "api.sock.lock.recovery", "owner"), "99999999\n");
  await start();
  assert.equal(ok("daemon", "status").stale, undefined, "abandoned lock recovery does not block restart");
  assert.equal(JSON.parse(command(...checkParent).stdout).error.code, "identity_conflict", "another session in the parent's pane is refused");
  await stop();
  delete env.HCOORD_FAKE_OBSERVER_REPLACED;
  env.HCOORD_FAKE_PROMPT_API = "0";
  await start();
  assert.equal(JSON.parse(command(...checkParent).stdout).error.code, "unsupported_runtime", "no official delivery, no ready answer");
  await stop();
  delete env.HCOORD_FAKE_PROMPT_API;
  await start();
  assert.equal(ok(...checkParent).participant, parent.id);
  const watched = ok("watch", "start", child.id, "--observer", parent.id, "--actor", parent.id, "--brief", "read the log");
  assert.equal(watched.brief, "read the log");
  assert.equal(JSON.parse(command("watch", "start", child.id, "--observer", parent.id, "--actor", parent.id).stdout).error.code, "conflict", "one active watch per target");
  assert.ok(ok("status").usage.ledgerBytes > 0);
  ok("config", "set", "--key", "remindMs", "--value", "1s");
  ok("config", "set", "--key", "escalateMs", "--value", "2m");
  const relayReminder = ok("request", "send", "--from", child.id, "--to", "human", "--intermediary", parent.id, "--body", "Confirm relay", "--intent", "relay-reminder");
  ok("request", "reply", relayReminder.id, "--body", "Yes", "--as", "human");
  let reminded;
  for (let attempt = 0; attempt < 300; attempt += 1) {
    reminded = ok("request", "show", relayReminder.id);
    if (reminded.deliveries.some((delivery) => delivery.phase === "relay_problem" && delivery.recipient === parent.id && delivery.status === "accepted")) break;
    await wait(100);
  }
  assert.equal(reminded.deliveries.some((delivery) => delivery.phase === "relay_problem" && delivery.recipient === parent.id && delivery.status === "accepted"), true, "parent receives the 15-minute policy reminder on a shortened test clock");
  ok("config", "set", "--key", "escalateMs", "--value", "1s");
  for (let attempt = 0; attempt < 300; attempt += 1) {
    reminded = ok("request", "show", relayReminder.id);
    if (reminded.deliveries.some((delivery) => delivery.phase === "relay_problem" && delivery.recipient === "human" && delivery.status === "accepted")) break;
    await wait(100);
  }
  assert.equal(reminded.deliveries.some((delivery) => delivery.phase === "relay_problem" && delivery.recipient === "human" && delivery.status === "accepted"), true, "human receives the relay escalation on a shortened test clock");
  ok("request", "relay", relayReminder.id, "--actor", parent.id, "--body", "Yes");
  ok("config", "set", "--key", "remindMs", "--value", "15m");
  ok("config", "set", "--key", "escalateMs", "--value", "30m");
  await stop();
  await start();
  const spawned = ok("agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "worker", "--intent", "spawn-1");
  assert.equal(spawned.participant.parent, parent.id);
  assert.equal(spawned.watch.observer, parent.id);
  assert.equal(ok("agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "worker", "--intent", "spawn-1").participant.id, spawned.participant.id);
  assert.equal(fs.readFileSync(path.join(home, "worker.tab"), "utf8"), "1");
  const optionedResult = spawnSync(process.execPath, [CLI, "agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "optioned", "--intent", "spawn-optioned", "--json", "--", "-m", "gpt-6-sol", "-c", "model_reasoning_effort=xhigh"], { env, encoding: "utf8" });
  assert.equal(optionedResult.status, 0, optionedResult.stdout);
  assert.equal(JSON.parse(optionedResult.stdout).value.participant.name, "optioned");
  const startArgs = JSON.parse(fs.readFileSync(path.join(home, "optioned.start-args.json"), "utf8"));
  assert.deepEqual(startArgs.slice(-4), ["-m", "gpt-6-sol", "-c", "model_reasoning_effort=xhigh"]);
  assert.equal(fs.existsSync(path.join(home, "optioned.initialized")), true, "first turn follows Herdr readiness");
  const taskResult = spawnSync(process.execPath, [CLI, "agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "tasked", "--intent", "spawn-tasked", "--json", "--", "-m", "gpt-6-sol", "Return blue"], { env, encoding: "utf8" });
  assert.equal(taskResult.status, 0, taskResult.stdout);
  assert.equal(fs.readFileSync(path.join(home, "official-prompts.jsonl"), "utf8").split("\n").some((line) => line.includes('"target":"tasked-pane","text":"Return blue"')), true, "explicit user task is submitted after readiness without replacement by the initialization text");
  env.HCOORD_FAKE_PROMPT_UNKNOWN_ONCE = "1";
  await stop();
  await start();
  const uncertainArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "uncertain-child", "--intent", "uncertain-child"];
  assert.equal(JSON.parse(command(...uncertainArgs).stdout).error.code, "spawn_uncertain");
  const recovered = ok(...uncertainArgs);
  assert.equal(recovered.participant.name, "uncertain-child", "same intent binds the observed first execution");
  assert.equal(fs.readFileSync(path.join(home, "official-prompts.jsonl"), "utf8").split("\n").filter((line) => line.includes('"target":"uncertain-child-pane"')).length, 1, "unknown prompt outcome is never blindly resubmitted");
  env.HCOORD_FAKE_REPLACE_TERMINAL_ON_UNKNOWN = "1";
  await stop();
  await start();
  const replacedArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "uncertain-replaced", "--intent", "uncertain-replaced"];
  assert.equal(JSON.parse(command(...replacedArgs).stdout).error.code, "spawn_uncertain");
  assert.equal(JSON.parse(command(...replacedArgs).stdout).error.code, "identity_conflict", "a replacement with the same pane, name and kind cannot inherit the reserved first turn");
  assert.equal(fs.readFileSync(path.join(home, "official-prompts.jsonl"), "utf8").split("\n").filter((line) => line.includes('"target":"uncertain-replaced-pane"')).length, 1);
  delete env.HCOORD_FAKE_PROMPT_UNKNOWN_ONCE;
  delete env.HCOORD_FAKE_REPLACE_TERMINAL_ON_UNKNOWN;
  env.HCOORD_FAKE_FIRST_TURN_SCREEN = "Updating Codex via pnpm add -g @openai/codex";
  await stop();
  await start();
  const unsafeArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "update-screen", "--intent", "update-screen"];
  assert.equal(JSON.parse(command(...unsafeArgs).stdout).error.code, "spawn_uncertain", "a reported idle agent on an update screen is not ready for a first prompt");
  assert.equal(fs.readFileSync(path.join(home, "official-prompts.jsonl"), "utf8").includes('"target":"update-screen-pane"'), false);
  delete env.HCOORD_FAKE_FIRST_TURN_SCREEN;
  await stop();
  await start();
  assert.equal(ok(...unsafeArgs).participant.name, "update-screen", "the same saved intent resumes when its real composer appears");
  await stop();
  await start();
  assert.equal(JSON.parse(command("agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "Bad Name", "--intent", "invalid-spawn").stdout).error.code, "invalid_argument");
  assert.equal(fs.existsSync(path.join(home, "Bad Name.tab")), false, "invalid spawn does not create a pane");
  // #237: native arguments are refused before the parent registration, the letter, and any pane.
  const refusedSpawn = (name, ...native) => spawnSync(process.execPath, [CLI, "agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", name, "--intent", name, "--json", "--", ...native], { env, encoding: "utf8" });
  const claudeUnderCodex = refusedSpawn("wrong-kind", "claude", "--dangerously-skip-permissions");
  assert.equal(claudeUnderCodex.status, 2);
  assert.equal(JSON.parse(claudeUnderCodex.stdout).error.code, "invalid_argument");
  assert.match(JSON.parse(claudeUnderCodex.stdout).error.message, /pass --kind claude/, "Claude arguments under the default kind name the missing --kind");
  assert.equal(JSON.parse(refusedSpawn("misplaced-task", "Return blue", "-m", "gpt-6-sol").stdout).error.message, "Codex task must be the final native argument");
  const worktreeRefused = spawnSync(process.execPath, [CLI, "agent", "spawn", "--parent", parent.id, "--session", "one", "--name", "wt-refused", "--intent", "wt-refused", "--repo", "/home/me/repo", "--branch", "wt-refused", "--path", "/home/me/wt-refused", "--json", "--", "claude", "--dangerously-skip-permissions"], { env, encoding: "utf8" });
  assert.equal(JSON.parse(worktreeRefused.stdout).error.code, "invalid_argument");
  assert.equal(fs.readFileSync(path.join(home, "herdr-calls.log"), "utf8").includes("worktree create"), false, "a refused worktree spawn never asks Herdr for a worktree");
  for (const name of ["wrong-kind", "misplaced-task"]) assert.equal(fs.existsSync(path.join(home, `${name}.tab`)), false, `${name} creates no pane`);
  assert.equal(Object.keys(JSON.parse(fs.readFileSync(path.join(home, ".hcoord", "ledger.json"), "utf8")).spawnIntents).some((key) => ["wrong-kind", "misplaced-task"].includes(key)), false, "a refused spawn reserves no intent");
  const hereRefused = spawnSync(process.execPath, [CLI, "agent", "spawn", "--parent", "here", "--name", "here-refused", "--intent", "here-refused", "--json", "--", "claude", "--dangerously-skip-permissions"], { env: { ...env, HERDR_PANE_ID: "absent-pane" }, encoding: "utf8" });
  assert.equal(JSON.parse(hereRefused.stdout).error.code, "invalid_argument", "--parent here validates before it inspects or registers the current pane");
  const spawnHelp = spawnSync(process.execPath, [CLI, "agent", "spawn", "--help"], { env, encoding: "utf8" });
  assert.equal(spawnHelp.status, 0);
  assert.match(spawnHelp.stdout, /--kind\s+codex, claude/);
  assert.match(spawnHelp.stdout, /claude: -- <claude flags> "<prompt>"/);
  const unobserved = ok("agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "solo", "--intent", "spawn-2", "--no-watch");
  assert.equal(unobserved.watch, null);
  assert.equal(ok("graph").creation.find((edge) => edge.child === unobserved.participant.id).parent, parent.id);
  await stop();
  env.HCOORD_FAKE_TAB_INVALID = "1";
  await start();
  const malformedTabArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "malformed-tab", "--intent", "malformed-tab"];
  const malformedTab = JSON.parse(command(...malformedTabArgs).stdout);
  assert.equal(malformedTab.error.code, "spawn_uncertain");
  assert.equal(malformedTab.error.detail.intent, "malformed-tab");
  assert.equal(malformedTab.error.detail.unfinishedStep, "record_pane");
  assert.equal(JSON.parse(command(...malformedTabArgs).stdout).error.code, "spawn_uncertain", "malformed create result cannot create a second tab");
  assert.equal(fs.readFileSync(path.join(home, "malformed-tab.tab"), "utf8"), "1");
  await stop();
  delete env.HCOORD_FAKE_TAB_INVALID;
  env.HCOORD_FAKE_WRONG_KIND = "1";
  await start();
  const kindArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "kind-check", "--intent", "kind-check"];
  assert.equal(JSON.parse(command(...kindArgs).stdout).error.code, "identity_conflict", "an observed agent kind must match the original intent");
  await stop();
  delete env.HCOORD_FAKE_WRONG_KIND;
  await start();
  assert.equal(ok(...kindArgs).intent.pane, "kind-check-pane", "a corrected observation binds the original pane");
  assert.equal(fs.readFileSync(path.join(home, "kind-check.tab"), "utf8"), "1");
  await stop();
  // #236: Herdr reports the agent hcoord just started without its name.
  env.HCOORD_FAKE_NAME_LOST = "1";
  env.HCOORD_FAKE_PROMPT_UNKNOWN_ONCE = "1";
  await start();
  const nameless = ok("agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "nameless", "--intent", "nameless");
  assert.equal(nameless.participant.name, "nameless", "one spawn names its own started agent and registers it");
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(home, "nameless-pane.renamed"), "utf8")), ["agent", "rename", "nameless-pane", "nameless"]);
  // A prompt passed as a native argument keeps the agent working past readiness: the start is unconfirmed and the agent is unnamed.
  for (const code of ["timeout", "agent_not_ready"]) {
    env.HCOORD_FAKE_START_UNCONFIRMED = code;
    await stop();
    await start();
    const name = `unconfirmed-${code.replaceAll("_", "-")}`;
    assert.equal(ok("agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", name, "--intent", name).participant.name, name, `a start ending in ${code} still names and registers the agent it started`);
    assert.equal(fs.existsSync(path.join(home, `${name}-pane.renamed`)), true);
  }
  delete env.HCOORD_FAKE_START_UNCONFIRMED;
  await stop();
  await start();
  const lostArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "uncertain-lost", "--intent", "uncertain-lost"];
  assert.equal(JSON.parse(command(...lostArgs).stdout).error.code, "spawn_uncertain");
  fs.rmSync(path.join(home, "uncertain-lost-pane.renamed"));
  assert.equal(ok(...lostArgs).participant.name, "uncertain-lost", "a retry names the recorded execution again when Herdr drops its name after the rename");
  assert.equal(fs.existsSync(path.join(home, "uncertain-lost-pane.renamed")), true);
  env.HCOORD_FAKE_REPLACE_TERMINAL_ON_UNKNOWN = "1";
  await stop();
  await start();
  const swappedArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "uncertain-swapped", "--intent", "uncertain-swapped"];
  assert.equal(JSON.parse(command(...swappedArgs).stdout).error.code, "spawn_uncertain");
  fs.rmSync(path.join(home, "uncertain-swapped-pane.renamed"));
  const swapped = JSON.parse(command(...swappedArgs).stdout).error;
  assert.equal(swapped.code, "identity_conflict", "an unnamed agent in another terminal is not the execution hcoord started");
  assert.match(swapped.message, /another terminal or session/);
  assert.equal(fs.existsSync(path.join(home, "uncertain-swapped-pane.renamed")), false, "a replacement is never renamed");
  delete env.HCOORD_FAKE_REPLACE_TERMINAL_ON_UNKNOWN;
  delete env.HCOORD_FAKE_PROMPT_UNKNOWN_ONCE;
  env.HCOORD_FAKE_RENAME_FAIL = "1";
  await stop();
  await start();
  const renameArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "rename-refused", "--intent", "rename-refused"];
  const renameRefused = JSON.parse(command(...renameArgs).stdout).error;
  assert.deepEqual([renameRefused.code, renameRefused.detail.unfinishedStep], ["spawn_uncertain", "name_agent"], "a rename Herdr refuses leaves the intent to retry");
  delete env.HCOORD_FAKE_RENAME_FAIL;
  await stop();
  await start();
  assert.equal(ok(...renameArgs).participant.name, "rename-refused", "the retry names and binds the same execution");
  await stop();
  delete env.HCOORD_FAKE_NAME_LOST;
  env.HCOORD_FAKE_START_FAIL = "1";
  await start();
  const partialArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "partial", "--intent", "partial-start"];
  const partial = JSON.parse(command(...partialArgs).stdout);
  assert.equal(partial.error.code, "spawn_uncertain");
  assert.equal(partial.error.detail.pane, "partial-pane");
  assert.equal(partial.error.detail.unfinishedStep, "agent_start");
  assert.equal(JSON.parse(command(...partialArgs).stdout).error.code, "spawn_uncertain", "retry does not repeat an uncertain start");
  await stop();
  delete env.HCOORD_FAKE_START_FAIL;
  const capacityLedgerFile = path.join(home, ".hcoord", "ledger.json");
  const beforeCapacityProbe = fs.readFileSync(capacityLedgerFile, "utf8");
  const nearCap = JSON.parse(beforeCapacityProbe);
  const observedAt = new Date().toISOString();
  // Startup can record lineage repair plus participant observations before calls resume.
  while (nearCap.events.length < MAX_EVENTS - 5) nearCap.events.push({ seq: ++nearCap.seq, at: observedAt, type: "fixture.capacity", subjectId: "fixture", correlationId: null, detail: {} });
  fs.writeFileSync(capacityLedgerFile, `${JSON.stringify(nearCap)}\n`);
  await start();
  const resumedAtCap = JSON.parse(command(...partialArgs, "--resume-start").stdout);
  assert.equal(resumedAtCap.error.code, "capacity", "a saved pane is not started without room to record first-turn and registration progress");
  assert.equal(fs.existsSync(path.join(home, "partial-pane.started")), false, "capacity refusal has no agent start effect");
  await stop();
  fs.writeFileSync(capacityLedgerFile, beforeCapacityProbe);
  const nearByteCap = JSON.parse(beforeCapacityProbe);
  const requestTemplate = Object.values(nearByteCap.requests)[0];
  assert.ok(requestTemplate, "the fixture has a real request shape before filling the ledger");
  const createdAt = new Date().toISOString();
  const filler = "x".repeat(MAX_BODY_BYTES);
  const row = (index) => {
    const id = `r_byte_capacity_${index}`;
    return { ...requestTemplate, id, intent: `byte-capacity-${index}`, body: filler, context: null, status: "open", waiting: true, createdAt, answeredAt: null, answer: null, respondent: null, recordedBy: null, canceledAt: null, lateAnswers: [], relayBody: null, relayAt: null, escalatedAt: null, remindedAt: null, relayRemindedAt: null, relayEscalatedAt: null, deliveryRemindedAt: null, deliveryEscalatedAt: null, deliveries: [] };
  };
  const baseBytes = Buffer.byteLength(JSON.stringify(nearByteCap)) + 1;
  const entryBytes = Buffer.byteLength(JSON.stringify({ r_byte_capacity_0: row(0) })) - 2;
  const fillCount = Math.ceil((MAX_LEDGER_BYTES - 3 * MAX_MESSAGE_BYTES - baseBytes) / entryBytes);
  assert.ok(fillCount < MAX_REQUESTS - Object.keys(nearByteCap.requests).length);
  for (let index = 0; index < fillCount; index += 1) nearByteCap.requests[`r_byte_capacity_${index}`] = row(index);
  const nearByteCapSize = Buffer.byteLength(JSON.stringify(nearByteCap)) + 1;
  assert.ok(nearByteCapSize < MAX_LEDGER_BYTES && nearByteCapSize > MAX_LEDGER_BYTES - 4 * MAX_MESSAGE_BYTES, "plausible unresolved requests leave less than four progress payloads of ledger space");
  const bytePromptIntent = { ...nearByteCap.spawnIntents["partial-start"], key: "byte-prompt", name: "byte-prompt", pane: "byte-prompt-pane", status: "unknown", reason: "first turn pending", initialization: "pending", observedInstance: null, observedSession: null };
  nearByteCap.spawnIntents[bytePromptIntent.key] = bytePromptIntent;
  fs.writeFileSync(capacityLedgerFile, `${JSON.stringify(nearByteCap)}\n`);
  fs.writeFileSync(path.join(home, "byte-prompt.tab"), "1");
  fs.writeFileSync(path.join(home, "byte-prompt-pane.started"), "1");
  await start();
  const freshAtByteCap = JSON.parse(command("agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "byte-fresh", "--intent", "byte-fresh").stdout);
  assert.equal(freshAtByteCap.error.code, "capacity");
  assert.equal(freshAtByteCap.error.detail.unfinishedStep, "create_pane");
  assert.equal(fs.existsSync(path.join(home, "byte-fresh.tab")), false, "ledger-byte refusal precedes tab creation");
  const startAtByteCap = JSON.parse(command(...partialArgs, "--resume-start").stdout);
  assert.equal(startAtByteCap.error.code, "capacity");
  assert.equal(startAtByteCap.error.detail.unfinishedStep, "agent_start");
  assert.equal(fs.existsSync(path.join(home, "partial-pane.started")), false, "ledger-byte refusal precedes agent start");
  const promptAtByteCap = JSON.parse(command("agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "byte-prompt", "--intent", "byte-prompt").stdout);
  assert.equal(promptAtByteCap.error.code, "capacity");
  assert.equal(promptAtByteCap.error.detail.unfinishedStep, "initialize_agent");
  assert.equal(fs.existsSync(path.join(home, "byte-prompt.initialized")), false, "ledger-byte refusal precedes the first prompt");
  await stop();
  fs.writeFileSync(capacityLedgerFile, beforeCapacityProbe);
  fs.writeFileSync(path.join(home, "parent.cwd"), path.join(home, "moved-parent"));
  env.HCOORD_FAKE_PANE_ID_MISSING = "1";
  await start();
  assert.equal(JSON.parse(command(...partialArgs, "--resume-start").stdout).error.code, "identity_conflict", "pane identity must be explicit before a resumed start");
  await stop();
  delete env.HCOORD_FAKE_PANE_ID_MISSING;
  await start();
  const resumed = ok(...partialArgs, "--resume-start");
  assert.equal(resumed.intent.pane, "partial-pane");
  assert.equal(resumed.intent.placement.cwd, home, "resume uses the saved placement after the parent changes cwd");
  assert.equal(fs.readFileSync(path.join(home, "partial.tab"), "utf8"), "1", "resume reuses the original pane");
  await stop();
  const ledgerFile = path.join(home, ".hcoord", "ledger.json");
  const interrupted = JSON.parse(fs.readFileSync(ledgerFile, "utf8"));
  interrupted.spawnIntents["reconcile-1"] = { ...resumed.intent, key: "reconcile-1", name: "reconciled", status: "unknown", pane: null, participant: null, reason: "pane recording interrupted", initialization: "pending", observedInstance: null, observedSession: null };
  const legacyIntent = { ...resumed.intent, key: "legacy-1", name: "legacy", status: "unknown", pane: null, participant: null, reason: "older pane recording interrupted" };
  delete legacyIntent.placement;
  interrupted.spawnIntents["legacy-1"] = legacyIntent;
  fs.writeFileSync(ledgerFile, `${JSON.stringify(interrupted)}\n`);
  fs.writeFileSync(path.join(home, "reconciled.tab"), "1");
  fs.writeFileSync(path.join(home, "legacy.tab"), "1");
  await start();
  const reconcileArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "reconciled", "--intent", "reconcile-1"];
  assert.equal(JSON.parse(command(...reconcileArgs).stdout).error.detail.unfinishedStep, "record_pane");
  const reconciled = ok(...reconcileArgs, "--reconcile-pane", "reconciled-pane", "--resume-start");
  assert.equal(reconciled.intent.pane, "reconciled-pane");
  assert.equal(fs.readFileSync(path.join(home, "reconciled.tab"), "utf8"), "1", "reconciliation does not create a second tab");
  const legacyArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "local", "--session", "one", "--name", "legacy", "--intent", "legacy-1"];
  assert.equal(JSON.parse(command(...legacyArgs, "--reconcile-pane", "legacy-pane", "--resume-start").stdout).error.code, "identity_conflict", "an old intent without placement cannot bypass the current parent placement check");
  assert.equal(fs.existsSync(path.join(home, "legacy-pane.started")), false);
  const sent = ok("request", "send", "--from", child.id, "--to", "human", "--intermediary", parent.id, "--body", "Which option?", "--intent", "choice-1");
  assert.equal(ok("request", "send", "--from", child.id, "--to", "human", "--intermediary", parent.id, "--body", "Which option?", "--intent", "choice-1").id, sent.id);
  assert.equal(command("request", "send", "--from", child.id, "--to", "human", "--body", "Other?", "--intent", "choice-1").status, 1);
  for (let attempt = 0; attempt < 100 && !fs.existsSync(path.join(home, "notification-args.json")); attempt += 1) await wait(100);
  assert.equal(fs.existsSync(path.join(home, "notification-args.json")), true);
  assert.equal(fs.readFileSync(path.join(home, "notification-args.json"), "utf8").includes("Which option?"), false);
  const answered = ok("request", "reply", sent.id, "--body", "A, not B", "--as", "human", "--recorded-by", parent.id);
  assert.equal(answered.answer, "A, not B");
  assert.equal(answered.recordedBy, parent.id);
  assert.equal(ok("inbox").find((item) => item.requestId === sent.id).kind, "relay_problem");
  ok("request", "relay", sent.id, "--body", "A", "--actor", parent.id);
  assert.equal(ok("inbox").some((item) => item.requestId === sent.id && item.kind === "relay_problem"), false);
  const canceled = ok("request", "send", "--from", child.id, "--to", "human", "--body", "Cancel this?", "--intent", "choice-2");
  ok("request", "cancel", canceled.id, "--actor", child.id);
  const late = ok("request", "reply", canceled.id, "--body", "Late A", "--as", "human");
  assert.equal(late.status, "canceled");
  assert.equal(late.lateAnswers.length, 1);
  assert.equal(late.lateAnswers[0].recordedBy, "human");
  const adapterRequest = ok("request", "send", "--from", child.id, "--to", "human", "--body", "Adapter question", "--intent", "adapter-1");
  const adapter = (...args) => spawnSync(process.execPath, [ADAPTER, ...args], { env, encoding: "utf8" });
  const notice = adapter("notify-only", adapterRequest.id);
  assert.equal(notice.status, 0, notice.stderr);
  assert.match(JSON.parse(notice.stdout).replyPath, /hcoord request reply/);
  assert.equal(notice.stdout.includes("Adapter question"), false);
  const callback = adapter("reply", adapterRequest.id, "Approved A");
  assert.equal(callback.status, 0, callback.stderr);
  assert.equal(ok("request", "show", adapterRequest.id).answer, "Approved A");
  assert.equal(adapter("reply", adapterRequest.id, "Changed answer").status, 1);
  assert.equal(ok("request", "show", adapterRequest.id).answer, "Approved A");
  ok("watch", "stop", child.id, "--actor", parent.id);
  assert.equal(JSON.parse(command("watch", "start", child.id, "--observer", parent.id, "--interval", "1s").stdout).error.code, "forbidden", "a stopped watch has no active owner to restart it");
  ok("watch", "start", child.id, "--observer", parent.id, "--actor", "human", "--interval", "1s");
  assert.equal(ok("graph").watch.filter((item) => item.target === child.id).length, 2);
  fs.writeFileSync(path.join(home, "child-pane.status"), "done");
  fs.writeFileSync(path.join(home, "parent-pane.status"), "working");
  let watch;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    watch = ok("watch", "list").find((item) => item.target === child.id);
    if (watch?.cycle) break;
    await wait(100);
  }
  assert.ok(watch.cycle);
  assert.match(watch.observation, /done\/connected/, "the cycle includes a fresh exact Herdr observation");
  const firstEvent = (type) => {
    for (let cursor = 0; ;) {
      const page = ok("events", "--cursor", String(cursor));
      const found = page.events.find((item) => item.type === type);
      if (found || !page.hasMore) return found;
      cursor = page.cursor;
    }
  };
  const cycleRequestId = firstEvent("watch.cycle").detail.requestId;
  let cycleRequest;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    cycleRequest = ok("request", "show", cycleRequestId);
    if (cycleRequest.deliveries[0].status === "deferred") break;
    await wait(100);
  }
  assert.equal(cycleRequest.deliveries[0].status, "deferred");
  assert.match(cycleRequest.deliveries[0].reason, /recipient is working/);
  ok("watch", "stop", child.id, "--actor", parent.id);
  fs.writeFileSync(path.join(home, "parent-pane.status"), "idle");
  await wait(1200);
  cycleRequest = ok("request", "show", cycleRequestId);
  assert.equal(cycleRequest.deliveries[0].status, "deferred", "a stopped watch holds its unsent check even when the old observer is idle");
  assert.match(cycleRequest.nextAction, /watch is stopped/);
  assert.equal(fs.readFileSync(path.join(home, "official-prompts.jsonl"), "utf8").includes(watch.cycle), false, "a stopped watch sends no misleading request prompt");
  ok("config", "set", "--key", "escalateMs", "--value", "1s");
  for (let attempt = 0; attempt < 100; attempt += 1) {
    cycleRequest = ok("request", "show", cycleRequestId);
    if (cycleRequest.deliveries.some((delivery) => delivery.recipient === "human" && delivery.status === "accepted")) break;
    await wait(100);
  }
  assert.equal(cycleRequest.deliveries[0].status, "deferred", "human escalation does not wake the former observer");
  assert.equal(cycleRequest.deliveries.some((delivery) => delivery.recipient === "human" && delivery.status === "accepted"), true, "a stopped watch still notifies the human when its request escalates");
  assert.match(fs.readFileSync(path.join(home, "notification-args.json"), "utf8"), new RegExp(cycleRequestId));
  ok("config", "set", "--key", "escalateMs", "--value", "30m");
  const resumedWatch = ok("watch", "start", child.id, "--observer", parent.id, "--actor", "human", "--interval", "1s");
  assert.equal(resumedWatch.cycle, watch.cycle);
  assert.equal(resumedWatch.requestId, cycleRequestId);
  for (let attempt = 0; attempt < 100; attempt += 1) {
    cycleRequest = ok("request", "show", cycleRequestId);
    if (cycleRequest.deliveries[0].status === "accepted") break;
    await wait(100);
  }
  assert.equal(cycleRequest.deliveries[0].status, "accepted");
  const watchPrompt = fs.readFileSync(path.join(home, "official-prompts.jsonl"), "utf8").trim().split("\n").map((line) => JSON.parse(line)).find((entry) => entry.text.startsWith("HCOORD_WATCH_CHECK") && entry.text.includes(watch.cycle));
  assert.equal(watchPrompt.text, `HCOORD_WATCH_CHECK child (${child.id}) cycle ${watch.cycle}\nclose: hcoord watch check ${child.id} --cycle ${watch.cycle} --actor ${parent.id}\nread the log`);
  assert.equal(fs.existsSync(path.join(home, "UNSUPPORTED_GUARD")), false);
  const decoy = ok("request", "send", "--from", child.id, "--to", parent.id, "--body", "Which option should I choose?", "--intent", `watch:${child.id}:999:${watch.cycle}`);
  let decoyPrompt;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    decoyPrompt = fs.readFileSync(path.join(home, "official-prompts.jsonl"), "utf8").trim().split("\n").map((line) => JSON.parse(line)).find((entry) => entry.text.includes(decoy.id));
    if (decoyPrompt) break;
    await wait(100);
  }
  assert.ok(decoyPrompt, "ordinary watch-shaped request reaches the agent");
  assert.match(decoyPrompt.text, /^HCOORD_REQUEST/);
  assert.match(decoyPrompt.text, /Which option should I choose\?/);
  assert.doesNotMatch(decoyPrompt.text, /HCOORD_WATCH_CHECK|hcoord watch check/);
  ok("request", "cancel", decoy.id, "--actor", child.id);
  ok("watch", "check", child.id, "--cycle", watch.cycle, "--actor", parent.id);
  assert.equal(ok("watch", "list").find((item) => item.target === child.id).cycle, null);
  assert.equal(command("watch", "assign", child.id, "--observer", spawned.participant.id, "--actor", "human").status, 1);
  assert.equal(command("watch", "assign", child.id, "--observer", spawned.participant.id, "--actor", "human", "--expected-generation", "0").status, 1);
  assert.equal(ok("watch", "assign", child.id, "--observer", spawned.participant.id, "--actor", "human", "--expected-generation", "3").generation, 4);
  await stop();
  const stale = ok("request", "show", sent.id);
  assert.equal(stale.stale, true);
  assert.equal(stale.data.answer, "A, not B");
  const queued = command("request", "send", "--from", child.id, "--to", "human", "--body", "No service", "--intent", "choice-3");
  assert.equal(queued.status, 0, "a write while the daemon is down waits in the outbox instead of failing");
  const queuedLetter = JSON.parse(queued.stdout);
  assert.equal(queuedLetter.delivery, "pending");
  assert.equal(fs.readdirSync(path.join(home, ".hcoord", "outbox")).filter((name) => name.endsWith(`${queuedLetter.value.letter}.json`)).length, 1);
  await start();
  for (let attempt = 0; attempt < 100 && fs.readdirSync(path.join(home, ".hcoord", "outbox")).some((name) => name.includes(queuedLetter.value.letter)); attempt += 1) await wait(100);
  assert.equal(fs.readdirSync(path.join(home, ".hcoord", "outbox")).some((name) => name.includes(queuedLetter.value.letter)), false, "the restarted daemon collects the waiting letter and removes it");
  assert.equal(JSON.parse(fs.readFileSync(path.join(home, ".hcoord", "ledger.json"), "utf8")).letters[queuedLetter.value.letter].outcome, "applied");
  const resent = JSON.parse(command("request", "send", "--from", child.id, "--to", "human", "--body", "No service", "--intent", "choice-3").stdout);
  assert.equal(resent.delivery, "delivered");
  assert.equal(resent.value.body, "No service", "the same intent resolves to the request the waiting letter created");
  assert.equal(ok("request", "show", sent.id).answer, "A, not B");
  assert.equal(ok("graph").creation.find((edge) => edge.child === child.id).parent, parent.id);
  const guarded = ok("request", "send", "--from", child.id, "--to", parent.id, "--body", "Official question", "--intent", "official-1");
  let accepted;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    accepted = ok("request", "show", guarded.id);
    if (accepted.deliveries[0].status === "accepted") break;
    await wait(100);
  }
  assert.equal(accepted.deliveries[0].status, "accepted");
  assert.equal(accepted.status, "open", "Herdr acceptance is not a request answer");
  const acknowledged = ok("request", "ack", guarded.id, "--actor", parent.id);
  assert.equal(acknowledged.deliveries[0].status, "acknowledged");
  assert.equal(ok("request", "ack", guarded.id, "--actor", parent.id).deliveries[0].status, "acknowledged");
  const promptCount = () => fs.readFileSync(path.join(home, "official-prompts.jsonl"), "utf8").trim().split("\n").map((line) => JSON.parse(line)).filter((entry) => entry.text.includes(guarded.id)).length;
  assert.equal(promptCount(), 1);
  await stop();
  await start();
  await wait(1100);
  assert.equal(promptCount(), 1, "restart does not repeat an accepted external submission");
  const queuedAfterStop = net.createConnection(path.join(home, ".hcoord", "api.sock"));
  await new Promise((resolve, reject) => { queuedAfterStop.once("connect", resolve); queuedAfterStop.once("error", reject); });
  assert.equal(ok("daemon", "stop").stopped, true);
  const refused = await new Promise((resolve, reject) => {
    let received = "";
    queuedAfterStop.on("data", (chunk) => { received += chunk; });
    queuedAfterStop.once("end", () => resolve(JSON.parse(received.trim())));
    queuedAfterStop.once("error", reject);
    queuedAfterStop.write(`${JSON.stringify({ version: 1, operation: "config.set", args: { key: "watchMs", value: 1000 } })}\n`);
  });
  assert.equal(refused.error.code, "manual_stop", "a connection accepted before stop cannot mutate afterward");
  for (let attempt = 0; attempt < 250 && daemon.exitCode === null; attempt += 1) await wait(20);
  assert.notEqual(daemon.exitCode, null);
  const manuallyStopped = command("daemon", "run");
  assert.equal(manuallyStopped.status, 0, "a manual stop exits successfully so KeepAlive does not restart it");
  assert.equal(JSON.parse(manuallyStopped.stdout).value.manualStop, true);
  fs.writeFileSync(path.join(home, ".hcoord", "ledger.json"), '{"answer":"PRIVATE_ANSWER", invalid');
  const corrupt = command("status");
  assert.equal(JSON.parse(corrupt.stdout).error.code, "corrupt_ledger");
  assert.equal(`${corrupt.stdout}${corrupt.stderr}`.includes("PRIVATE_ANSWER"), false);
});
