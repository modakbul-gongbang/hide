import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { createFakeRemote } from "../helpers/fake-remote.mjs";
import { hq } from "../helpers/hcoord-hq.mjs";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function pair(fake, coordinator) {
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const parent = coordinator.ok("agent", "register", "--machine", "local", "--session", "s-parent", "--instance", "i-parent", "--pane", "parent-pane", "--name", "parent");
  const child = coordinator.ok("agent", "register", "--machine", "local", "--session", "s-child", "--instance", "i-child", "--pane", "child-pane", "--name", "child", "--parent", parent.id);
  return { parent, child };
}

test("letters written while the daemon is down are applied once, in order, after it starts", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  await coordinator.start();
  const { parent, child } = pair(fake, coordinator);
  await coordinator.stop();

  const first = coordinator.json("request", "send", "--from", child.id, "--to", parent.id, "--body", "first", "--intent", "q-1");
  const repeated = coordinator.json("request", "send", "--from", child.id, "--to", parent.id, "--body", "first", "--intent", "q-1");
  const second = coordinator.json("request", "send", "--from", child.id, "--to", parent.id, "--body", "second", "--intent", "q-2");
  for (const result of [first, repeated, second]) assert.deepEqual([result.ok, result.delivery], [true, "pending"]);
  assert.equal(coordinator.outbox().length, 3);
  const text = spawnSync(process.execPath, [CLI, "request", "send", "--from", child.id, "--to", parent.id, "--body", "third", "--intent", "q-3"], { env: coordinator.env, encoding: "utf8" });
  assert.equal(text.status, 0);
  assert.match(text.stdout, /^pending: letter [0-9a-f-]{36} waits for the coordinator; the coordinator daemon is not running/);

  await coordinator.start();
  await coordinator.until(() => coordinator.outbox().length === 0, "the daemon sweeps waiting letters");
  const requests = Object.values(coordinator.ledger().requests).filter((item) => item.from === child.id);
  assert.deepEqual(requests.map((item) => item.body).sort(), ["first", "second", "third"], "a repeated intent records one request");
  const created = coordinator.ledger().events.filter((entry) => entry.type === "request.created").map((entry) => entry.correlationId);
  assert.deepEqual(created, ["q-1", "q-2", "q-3"], "letters apply in the order they were written");
  assert.equal(Object.values(coordinator.ledger().letters).filter((record) => record.operation === "request.send" && record.outcome === "applied").length, 4);
});

test("a letter recorded before a crash is removed on restart without a second effect", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  await coordinator.start();
  const { parent, child } = pair(fake, coordinator);
  const delivered = coordinator.json("request", "send", "--from", child.id, "--to", parent.id, "--body", "once", "--intent", "crash-1");
  assert.equal(delivered.delivery, "delivered");
  await coordinator.stop();
  // Crash window: the ledger save recorded the letter, the deletion never ran.
  const [record] = Object.values(coordinator.ledger().letters).filter((entry) => entry.operation === "request.send");
  const letter = { schema: "hcoord.letter.v1", id: record.id, operation: "request.send", args: { from: child.id, to: parent.id, body: "once", intent: "crash-1" }, createdAt: new Date(Date.now() - 60_000).toISOString(), writer: { host: "test", protocol: 1 } };
  fs.writeFileSync(path.join(coordinator.dir, "outbox", `${String(Date.now() - 60_000).padStart(15, "0")}-${record.id}.json`), JSON.stringify(letter));
  await coordinator.start();
  await coordinator.until(() => coordinator.outbox().length === 0, "the recorded letter is deleted");
  assert.equal(Object.values(coordinator.ledger().requests).filter((item) => item.intent === "crash-1").length, 1);
  assert.equal(coordinator.ledger().events.filter((entry) => entry.type === "request.created").length, 1, "no second effect or event");
});

test("an unknown letter version stays in the outbox and is shown with its reason", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  await coordinator.start();
  pair(fake, coordinator);
  const id = "00000000-0000-4000-8000-000000000001";
  const file = path.join(coordinator.dir, "outbox", `${String(Date.now() - 60_000).padStart(15, "0")}-${id}.json`);
  fs.writeFileSync(file, JSON.stringify({ schema: "hcoord.letter.v9", id, operation: "request.send", args: {}, createdAt: new Date().toISOString(), writer: { host: "future", protocol: 9 } }));
  await coordinator.until(() => coordinator.ok("inbox").some((item) => item.kind === "letter_rejected" && item.letter === id), "the refusal reaches the inbox");
  const item = coordinator.ok("inbox").find((entry) => entry.letter === id);
  assert.equal(item.code, "unsupported_letter");
  assert.match(item.reason, /hcoord\.letter\.v9 is unsupported/);
  await wait(2500);
  assert.equal(fs.existsSync(file), true, "an unsupported letter is kept for a newer coordinator");
  assert.equal(coordinator.ledger().events.filter((entry) => entry.type === "letter.rejected").length, 1, "the refusal is recorded once");
});

test("an immediate refusal is returned to the writer and not repeated in the inbox", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  await coordinator.start();
  const { child } = pair(fake, coordinator);
  const refused = coordinator.json("request", "send", "--from", child.id, "--to", "a_missing", "--body", "x", "--intent", "bad");
  assert.deepEqual([refused.ok, refused.delivery, refused.error.code], [false, "delivered", "not_found"]);
  assert.equal(coordinator.outbox().length, 0);
  assert.equal(coordinator.ok("inbox").some((item) => item.kind === "letter_rejected"), false);
});

/** A private HCOORD_HOME in a short path, removed with the test. */
function privateHome(t) {
  const root = fs.mkdtempSync("/tmp/hcs-");
  t.after(() => { fs.chmodSync(root, 0o700); fs.rmSync(root, { recursive: true, force: true }); });
  return root;
}
const send = (home) => spawnSync(process.execPath, [CLI, "request", "send", "--from", "a_x", "--to", "human", "--body", "hello", "--intent", "sandboxed", "--json"], { env: { ...process.env, HCOORD_HOME: home }, encoding: "utf8" });

test("a session that may not write the hcoord directory gets a permission error with the blocked path, not internal", { skip: process.platform === "win32" || process.getuid?.() === 0 }, (t) => {
  // What a workspace-write sandbox without ~/.hcoord looks like: the directory is readable and cannot take a new entry.
  const home = privateHome(t);
  fs.chmodSync(home, 0o500);
  const result = send(home);
  const parsed = JSON.parse(result.stdout);
  assert.equal(result.status, 1);
  assert.equal(parsed.ok, false);
  assert.equal(parsed.error.code, "permission_denied");
  assert.ok(parsed.error.message.includes(path.join(home, "outbox")), parsed.error.message);
  assert.match(parsed.error.message, /nothing was sent.*writable roots.*run the same command again/);
  assert.equal(parsed.error.detail.errno, "EACCES");
  assert.equal(result.stderr.includes("hcoord.command_failed"), false, "a classified refusal is not an unexpected failure");
  assert.deepEqual(fs.readdirSync(home), [], "a refused write leaves nothing behind");
});

test("an exception hcoord does not classify keeps its code and message on stderr", { skip: process.platform === "win32" }, (t) => {
  const home = privateHome(t);
  fs.writeFileSync(path.join(home, "outbox"), "");
  const result = send(home);
  const parsed = JSON.parse(result.stdout);
  assert.equal(result.status, 1);
  assert.equal(parsed.error.code, "internal");
  assert.match(parsed.error.message, /\(EEXIST\).*hcoord\.command_failed/);
  const event = JSON.parse(result.stderr.trim().split("\n").find((line) => line.includes("hcoord.command_failed")));
  assert.equal(event.code, "EEXIST");
  assert.match(event.message, /EEXIST/);
});

test("a watch whose target and observer have both left is stopped by the daemon and shows why", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  const coordinator = hq(t, fake);
  await coordinator.start();
  const { parent, child } = pair(fake, coordinator);
  coordinator.ok("watch", "start", child.id, "--observer", parent.id, "--actor", "human", "--interval", "1s");
  await coordinator.stop();
  fake.removeAgent("local", "parent-pane");
  fake.removeAgent("local", "child-pane");
  // Both panes have been gone for two hours: past the grace, so the next due check ends the watch.
  const ledger = coordinator.ledger();
  ledger.watches[child.id].orphanedSince = new Date(Date.now() - 2 * 3_600_000).toISOString();
  fs.writeFileSync(path.join(coordinator.dir, "ledger.json"), `${JSON.stringify(ledger)}\n`);
  await coordinator.start();
  await coordinator.until(() => coordinator.ledger().watches[child.id].status === "stopped", "the daemon ends the orphaned watch");
  const ended = coordinator.ledger().events.find((entry) => entry.type === "watch.orphaned");
  assert.equal(ended.subjectId, child.id);
  assert.equal(ended.detail.observer, parent.id);
  assert.equal(coordinator.ok("watch", "list").filter((item) => item.status === "active").length, 0);
});
