import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import test from "node:test";
import { createFakeRemote } from "../helpers/fake-remote.mjs";
import { hq } from "../helpers/hcoord-hq.mjs";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");
const require = createRequire(import.meta.url);
const { remoteCallAsync } = require("../../dist/hcoord/remote.js");

/** An agent's view of hcoord on a remote host: its own HOME and outbox. */
function remoteAgent(fake, host) {
  const env = fake.env(host);
  return {
    env,
    json(...args) { const result = spawnSync(process.execPath, [CLI, ...args, "--json"], { env, encoding: "utf8" }); return JSON.parse(result.stdout); },
    outbox: () => { const dir = path.join(env.HOME, ".hcoord", "outbox"); return fs.existsSync(dir) ? fs.readdirSync(dir).filter((name) => name.endsWith(".json")) : []; },
  };
}

async function setup(t) {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addMachine("mini", "mini-ssh");
  fake.installHcoord("mini");
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("mini", "w1:p1", { name: "worker", session: "s-worker", instance: "i-worker" });
  const coordinator = hq(t, fake);
  await coordinator.start();
  const parent = coordinator.ok("agent", "register", "--machine", "local", "--session", "s-parent", "--instance", "i-parent", "--pane", "parent-pane", "--name", "parent");
  return { fake, coordinator, parent };
}

test("an asynchronous remote call kills SSH when stderr crosses its cumulative cap", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addMachine("mini", "mini-ssh");
  fake.flag("mini", "stderr-flood");
  const originalPath = process.env.PATH;
  const originalRoot = process.env.FAKE_REMOTE_ROOT;
  process.env.PATH = fake.env().PATH;
  process.env.FAKE_REMOTE_ROOT = fake.root;
  t.after(() => {
    if (originalPath === undefined) delete process.env.PATH; else process.env.PATH = originalPath;
    if (originalRoot === undefined) delete process.env.FAKE_REMOTE_ROOT; else process.env.FAKE_REMOTE_ROOT = originalRoot;
  });

  const started = Date.now();
  const result = await remoteCallAsync("mini-ssh", ["take", "--limit", "1"]);

  assert.equal(result.status, null);
  assert.ok(Buffer.byteLength(result.stderr) <= 16 * 1024 * 1024);
  assert.ok(Date.now() - started < 5_000, "the overflowing SSH process is killed promptly");
});

test("a saved Herdr machine name registers a remote agent with the existing command and binds its HQ", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  const worker = coordinator.ok("agent", "register", "--machine", "mini", "--session", "s-worker", "--instance", "i-worker", "--pane", "w1:p1", "--name", "worker", "--parent", parent.id);
  assert.deepEqual([worker.machine, worker.hostScope, worker.pane], ["mini", "default", "w1:p1"]);
  assert.ok(fake.calls("mini").some((argv) => argv.join(" ") === "agent get w1:p1"), "the pane was confirmed on the remote Herdr server");
  assert.ok(fake.sshCommands("mini").some((command) => /^HCOORD_HOME="\$HOME\/\.hcoord" exec "\$HOME\/\.hcoord"\/bin\/hcoord remote 'hello' '--hq' '[^']+' --json$/.test(command)));
  assert.deepEqual(fake.pane("mini", "w1:p1").tokens, { parent_pane: "parent-pane", parent_machine: worker.lineage.parentMachine });
  assert.equal(JSON.parse(fs.readFileSync(path.join(fake.home("mini"), ".hcoord", "hq.json"), "utf8")).hq, os.hostname());
  assert.equal(fs.existsSync(path.join(fake.home("mini"), ".hcoord", "ledger.json")), false, "the remote keeps no conversation record");
});

test("remote registration refuses unknown machines, failed SSH authentication, missing remote hcoord, and version skew", async (t) => {
  const { fake, coordinator } = await setup(t);
  const register = (machine) => coordinator.json("agent", "register", "--machine", machine, "--session", "s-worker", "--instance", "i-worker", "--pane", "w1:p1", "--name", "worker");
  assert.equal(register("nowhere").error.code, "machine_unknown");
  assert.match(register("nowhere").error.message, /herdr machine add --label nowhere/);
  fake.flag("mini", "auth-denied");
  assert.equal(register("mini").error.code, "auth_failed");
  assert.match(register("mini").error.message, /hcoord stores no credentials/);
  fake.flag("mini", "auth-denied", false);
  fs.rmSync(path.join(fake.home("mini"), ".hcoord", "bin", "hcoord"));
  assert.equal(register("mini").error.code, "remote_not_installed");
  fs.writeFileSync(path.join(fake.home("mini"), ".hcoord", "bin", "hcoord"), `#!/bin/sh\necho '{"ok":true,"value":{"protocol":99}}'\n`, { mode: 0o755 });
  const skewed = register("mini");
  assert.equal(skewed.error.code, "version_mismatch");
  assert.match(skewed.error.message, /remote protocol 99; this HQ speaks 1/);
  fake.flag("mini", "down");
  assert.equal(register("mini").error.code, "machine_unreachable");
  assert.equal(coordinator.ok("agent", "list").items.filter((item) => item.machine === "mini" && item.registered).length, 0, "no refused registration leaves a participant");
});

test("on a remote machine writes wait in its outbox and queries are refused with the HQ location", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  const worker = coordinator.ok("agent", "register", "--machine", "mini", "--session", "s-worker", "--instance", "i-worker", "--pane", "w1:p1", "--name", "worker", "--parent", parent.id);
  const agent = remoteAgent(fake, "mini");
  const sent = agent.json("request", "send", "--from", worker.id, "--to", parent.id, "--body", "Need a decision", "--intent", "remote-q1");
  assert.deepEqual([sent.ok, sent.delivery, sent.value.hq], [true, "pending", os.hostname()]);
  assert.equal(agent.outbox().length, 1);
  for (const query of [["request", "show", "r_x"], ["inbox"], ["graph"], ["agent", "list"], ["status"], ["events"], ["daemon", "start"]]) {
    const refused = agent.json(...query);
    assert.equal(refused.error.code, "hq_only", query.join(" "));
    assert.equal(refused.error.detail.hq, os.hostname());
  }
});

test("the HQ moves only without open work, and a moved HQ stops its daemon", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const child = coordinator.ok("agent", "register", "--machine", "local", "--session", "s-child", "--instance", "i-child", "--pane", "child-pane", "--name", "child", "--parent", parent.id);
  const open = coordinator.ok("request", "send", "--from", child.id, "--to", parent.id, "--body", "open", "--intent", "open-1");
  const refused = coordinator.json("config", "set", "hq", "mini");
  assert.equal(refused.error.code, "hq_busy");
  assert.deepEqual(refused.error.detail.requests.map((item) => item.id), [open.id]);
  coordinator.ok("request", "cancel", open.id, "--actor", child.id);
  const moved = coordinator.ok("config", "set", "hq", "mini");
  assert.deepEqual([moved.hq, moved.previous], ["mini", "local"]);
  assert.equal(coordinator.json("status").error.code, "hq_only");
  assert.equal(fs.existsSync(path.join(coordinator.dir, "manual-stop")), true, "the former HQ daemon stopped and stays stopped");
  const back = coordinator.ok("config", "set", "hq", "local");
  assert.equal(back.hq, "local");
  assert.equal(fs.existsSync(path.join(coordinator.dir, "hq.json")), false);
});

const until = async (predicate, message, attempts = 200) => {
  for (let attempt = 0; attempt < attempts; attempt += 1) { const value = predicate(); if (value) return value; await new Promise((resolve) => setTimeout(resolve, 100)); }
  assert.fail(message);
};

test("a remote child's request reaches the local parent, and the human answer returns to the child pane", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  const worker = coordinator.ok("agent", "register", "--machine", "mini", "--session", "s-worker", "--instance", "i-worker", "--pane", "w1:p1", "--name", "worker", "--parent", parent.id);
  const child = remoteAgent(fake, "mini");
  const sent = child.json("request", "send", "--from", worker.id, "--to", parent.id, "--body", "Use library A or B?", "--intent", "rq-1");
  assert.equal(sent.delivery, "pending");
  const request = await until(() => fake.prompts("local").find((entry) => entry.target === "parent-pane" && entry.text.startsWith("HCOORD_REQUEST")), "the collected request wakes the parent");
  assert.match(request.text, /Use library A or B\?/);
  await until(() => child.outbox().length === 0, "the collected letter leaves the remote outbox");
  const requestId = /^HCOORD_REQUEST (r_\S+)/.exec(request.text)[1];
  coordinator.ok("request", "escalate", requestId, "--actor", parent.id);
  coordinator.ok("request", "reply", requestId, "--as", "human", "--body", "A");
  const answer = await until(() => fake.prompts("local").find((entry) => entry.text.startsWith("HCOORD_ANSWER")), "the human answer wakes the parent");
  assert.match(answer.text, new RegExp(`^HCOORD_ANSWER ${requestId}\nrelay: hcoord request relay ${requestId} --actor ${parent.id} --body <text>\nanswer: A$`));
  coordinator.ok("request", "relay", requestId, "--actor", parent.id, "--body", "Go with A");
  const relay = await until(() => fake.prompts("mini").find((entry) => entry.target === "w1:p1" && entry.text.startsWith("HCOORD_RELAY")), "the relay reaches the remote pane through --machine");
  assert.ok(fake.calls("mini").some((argv) => argv[0] === "agent" && argv[1] === "prompt" && argv[2] === "w1:p1"));
  // B8: the remote child can act on the notice alone.
  const delivery = /--delivery (d_\S+)/.exec(relay.text)[1];
  assert.match(relay.text, new RegExp(`^HCOORD_RELAY ${requestId}\nack: hcoord request ack ${requestId} --actor ${worker.id} --delivery d_\\S+\nGo with A$`));
  assert.equal(child.json("request", "show", requestId).error.code, "hq_only");
  assert.equal(child.json("request", "ack", requestId, "--actor", worker.id, "--delivery", delivery).delivery, "pending");
  await until(() => coordinator.ok("request", "show", requestId).deliveries.find((entry) => entry.id === delivery)?.status === "acknowledged", "the remote ack is collected");
});

test("while the HQ cannot reach a machine, remote writes still succeed, its agents read as unobservable, and letters apply in order after reconnection", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  const worker = coordinator.ok("agent", "register", "--machine", "mini", "--session", "s-worker", "--instance", "i-worker", "--pane", "w1:p1", "--name", "worker", "--parent", parent.id);
  const child = remoteAgent(fake, "mini");
  fake.flag("mini", "down");
  for (const [index, body] of ["one", "two", "three"].entries()) assert.equal(child.json("request", "send", "--from", worker.id, "--to", parent.id, "--body", body, "--intent", `order-${index}`, "--notify-only").delivery, "pending");
  await until(() => coordinator.ok("agent", "show", worker.id).connection === "unavailable", "the HQ marks the remote agent unobservable");
  assert.equal(child.outbox().length, 3);
  fake.flag("mini", "down", false);
  await until(() => child.outbox().length === 0, "letters are collected after reconnection", 600);
  const created = coordinator.ledger().events.filter((entry) => entry.type === "request.created" && entry.correlationId.startsWith("order-")).map((entry) => entry.correlationId);
  assert.deepEqual(created, ["order-0", "order-1", "order-2"]);
});

test("a remote hcoord with another protocol is refused at collection and shown to the human", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  coordinator.ok("agent", "register", "--machine", "mini", "--session", "s-worker", "--instance", "i-worker", "--pane", "w1:p1", "--name", "worker", "--parent", parent.id);
  fs.writeFileSync(path.join(fake.home("mini"), ".hcoord", "bin", "hcoord"), `#!/bin/sh\necho '{"ok":true,"value":{"protocol":2,"letters":[]}}'\n`, { mode: 0o755 });
  const problem = await until(() => coordinator.ok("inbox").find((item) => item.kind === "machine_problem"), "the refusal reaches the inbox");
  assert.deepEqual([problem.machine, problem.code], ["mini", "version_mismatch"]);
  assert.match(problem.reason, /remote protocol 2; this HQ speaks 1/);
});

test("a remote spawn creates a Herdr worktree on the target, starts the child there, and records where it runs", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  const repo = path.join(fake.home("mini"), "src", "product");
  const worktree = path.join(fake.home("mini"), "trees", "feature-x");
  const spawnArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "mini", "--session", "s-parent", "--name", "builder", "--kind", "claude", "--repo", repo, "--branch", "feature-x", "--path", worktree, "--intent", "remote-spawn-1"];
  const missing = coordinator.json(...spawnArgs);
  assert.deepEqual([missing.ok, missing.error.code], [false, "repo_missing"]);
  assert.match(missing.error.message, /clone the source repository there first/);
  assert.equal(fake.worktrees("mini").length, 0);
  fs.mkdirSync(path.join(repo, ".git"), { recursive: true });
  const spawned = coordinator.ok(...spawnArgs);
  assert.deepEqual([spawned.participant.machine, spawned.participant.pane, spawned.participant.parent], ["mini", "builder-pane", parent.id]);
  assert.deepEqual(spawned.participant.worktree, { repo, branch: "feature-x", path: worktree });
  assert.equal(spawned.watch.observer, parent.id, "the local parent watches its remote child");
  const start = fake.calls("mini").find((argv) => argv[0] === "agent" && argv[1] === "start");
  assert.deepEqual(start.slice(0, 7), ["agent", "start", "builder", "--kind", "claude", "--pane", "builder-pane"]);
  assert.equal(coordinator.ok(...spawnArgs).participant.id, spawned.participant.id, "the same intent returns the same child");
  assert.equal(fake.worktrees("mini").length, 1, "and never creates a second worktree");
  const listed = coordinator.ok("agent", "list").items.find((item) => item.id === spawned.participant.id);
  assert.deepEqual([listed.machine, listed.worktree.branch, listed.worktree.path], ["mini", "feature-x", worktree]);
  assert.equal(coordinator.ok("graph").participants.find((item) => item.id === spawned.participant.id).worktree.repo, repo);
});

test("an uncertain worktree creation is never repeated and is reconciled to the pane a person names", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  const repo = path.join(fake.home("mini"), "src", "product");
  fs.mkdirSync(path.join(repo, ".git"), { recursive: true });
  const spawnArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "mini", "--session", "s-parent", "--name", "lost", "--kind", "claude", "--repo", repo, "--branch", "lost-reply", "--path", path.join(fake.home("mini"), "trees", "lost"), "--intent", "lost-1"];
  fake.flag("mini", "worktree-lost-reply");
  const uncertain = coordinator.json(...spawnArgs);
  assert.equal(uncertain.error.code, "spawn_uncertain");
  assert.match(uncertain.error.message, /herdr --machine mini worktree list/);
  await new Promise((resolve) => setTimeout(resolve, 31_000)); // the lost reply's connection backoff
  assert.equal(coordinator.json(...spawnArgs).error.code, "spawn_uncertain", "a retry without a named pane creates nothing");
  assert.equal(fake.worktrees("mini").length, 1);
  const reconciled = coordinator.ok(...spawnArgs.slice(0, -2), "--intent", "lost-1", "--reconcile-pane", "lost-pane", "--resume-start");
  assert.equal(reconciled.participant.pane, "lost-pane");
  assert.equal(reconciled.participant.worktree.branch, "lost-reply");
  assert.equal(fake.worktrees("mini").length, 1);
});

test("spawn without --machine stays beside its parent, and another machine needs a repository and branch", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  const local = coordinator.ok("agent", "spawn", "--parent", parent.id, "--session", "s-parent", "--name", "helper", "--kind", "claude", "--intent", "local-1");
  assert.equal(local.participant.machine, "local");
  assert.ok(fake.calls("local").some((argv) => argv[0] === "tab" && argv[1] === "create" && argv.includes("helper")));
  assert.equal(fake.calls("mini").some((argv) => argv[0] === "worktree"), false);
  const refused = coordinator.json("agent", "spawn", "--parent", parent.id, "--machine", "mini", "--session", "s-parent", "--name", "nowhere", "--kind", "claude", "--intent", "remote-2");
  assert.equal(refused.error.code, "invalid_argument");
  assert.match(refused.error.message, /--repo <source repository on mini> and --branch/);
});

test("a spawned agent waiting on its own trust prompt is reported for a person, and the same intent resumes after it is answered", async (t) => {
  const { fake, coordinator, parent } = await setup(t);
  const repo = path.join(fake.home("mini"), "src", "product");
  fs.mkdirSync(path.join(repo, ".git"), { recursive: true });
  const spawnArgs = ["agent", "spawn", "--parent", parent.id, "--machine", "mini", "--session", "s-parent", "--name", "trusting", "--kind", "claude", "--repo", repo, "--branch", "trust", "--path", path.join(fake.home("mini"), "trees", "trust"), "--intent", "trust-1"];
  fake.flag("mini", "start-blocked");
  const blocked = coordinator.json(...spawnArgs);
  assert.equal(blocked.error.code, "spawn_blocked");
  assert.match(blocked.error.message, /pane trusting-pane on mini is waiting on its own prompt .*herdr --machine mini agent read trusting-pane/);
  assert.equal(coordinator.json(...spawnArgs).error.code, "spawn_blocked", "a retry before the person answers repeats the reason, not a false first-turn warning");
  fake.flag("mini", "start-blocked", false);
  fake.setAgent("mini", "trusting-pane", { session: "trusting-session", status: "idle", ready: true });
  const resumed = coordinator.ok(...spawnArgs);
  assert.equal(resumed.participant.pane, "trusting-pane");
  assert.equal(fake.worktrees("mini").length, 1);
  assert.equal(fake.calls("mini").filter((argv) => argv[0] === "agent" && argv[1] === "start").length, 1, "the agent is started once");
});
