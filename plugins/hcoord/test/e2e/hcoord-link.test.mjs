import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { createFakeRemote } from "../helpers/fake-remote.mjs";
import { hq } from "../helpers/hcoord-hq.mjs";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");
// What Hide compares with the session each pane reports (`wire::session_digest`).
const digest = (session) => createHash("sha256").update(session, "utf8").digest("hex");
const lineageTokens = { parent_pane: "parent-pane", child_session: digest("s-child"), parent_session: digest("s-parent") };
const link = (coordinator) => {
  const linked = spawnSync(process.execPath, [CLI, "agent", "link", "--parent-pane", "parent-pane", "--child-pane", "child-pane", "--json"], { env: coordinator.env, encoding: "utf8" });
  assert.equal(linked.status, 0, linked.stderr || linked.stdout);
  return JSON.parse(linked.stdout);
};
const writes = (fake) => fake.calls("local").filter((argv) => argv[0] === "pane" && argv[1] === "report-metadata").length;
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("Hide fork linking registers both executions and lets hcoord own lineage tokens", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const coordinator = hq(fake);
  await coordinator.start();

  const linked = spawnSync(process.execPath, [CLI, "agent", "link", "--parent-pane", "parent-pane", "--child-pane", "child-pane", "--json"], { env: coordinator.env, encoding: "utf8" });
  assert.equal(linked.status, 0, linked.stderr || linked.stdout);
  const result = JSON.parse(linked.stdout);
  assert.equal(result.ok, true);
  assert.equal(result.value.child.parent, result.value.parent.id);
  assert.deepEqual(fake.pane("local", "child-pane").tokens, lineageTokens);
  assert.ok(fake.calls("local").some((argv) => argv.join(" ").startsWith("pane report-metadata child-pane --source hcoord")));
});

test("--parent here refuses before creating a child without pane identity or a reported agent", async (t) => {
  const withoutPane = createFakeRemote(CLI);
  t.after(() => withoutPane.cleanup());
  const noPaneCoordinator = hq(withoutPane);
  await noPaneCoordinator.start();
  const noPane = noPaneCoordinator.json("agent", "spawn", "--parent", "here", "--name", "child", "--kind", "claude", "--intent", "here-no-pane");
  assert.equal(noPane.error.code, "parent_here_unavailable");
  assert.match(noPane.error.message, /requires HERDR_PANE_ID/);
  assert.equal(withoutPane.calls("local").some((argv) => argv[0] === "tab" && argv[1] === "create"), false);

  const withoutAgent = createFakeRemote(CLI);
  t.after(() => withoutAgent.cleanup());
  const noAgentCoordinator = hq(withoutAgent, "local", { HERDR_PANE_ID: "missing-pane" });
  await noAgentCoordinator.start();
  const noAgent = noAgentCoordinator.json("agent", "spawn", "--parent", "here", "--name", "child", "--kind", "claude", "--intent", "here-no-agent");
  assert.equal(noAgent.error.code, "parent_here_unavailable");
  assert.match(noAgent.error.message, /could not confirm an agent in missing-pane/);
  assert.equal(withoutAgent.calls("local").some((argv) => argv[0] === "tab" && argv[1] === "create"), false);
});

test("--parent here registers the current execution before spawning its child", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  const coordinator = hq(fake, "local", { HERDR_PANE_ID: "parent-pane" });
  await coordinator.start();

  const spawned = coordinator.ok("agent", "spawn", "--parent", "here", "--name", "child", "--kind", "claude", "--intent", "here-success");

  const parent = coordinator.ok("agent", "list").items.find((item) => item.pane === "parent-pane");
  assert.equal(spawned.participant.parent, parent.id);
  assert.equal(spawned.participant.pane, "child-pane");
  assert.deepEqual(fake.pane("local", "child-pane").tokens, { ...lineageTokens, child_session: digest("child-session") });
  assert.equal(fake.calls("local").filter((argv) => argv[0] === "tab" && argv[1] === "create").length, 1);
});

test("lineage a Herdr restart dropped comes back while the daemon keeps running, even when Herdr returns after it", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const coordinator = hq(fake);
  await coordinator.start();
  const linked = spawnSync(process.execPath, [CLI, "agent", "link", "--parent-pane", "parent-pane", "--child-pane", "child-pane", "--json"], { env: coordinator.env, encoding: "utf8" });
  assert.equal(linked.status, 0, linked.stderr || linked.stdout);
  await coordinator.stop();

  // A reboot: the daemon starts while Herdr is still down, and Herdr comes back with new terminals and no tokens.
  fake.restartServer("local");
  fake.flag("local", "server-down");
  const eventsBefore = coordinator.ledger().events.length;
  await coordinator.start();
  await new Promise((resolve) => setTimeout(resolve, 6000));
  const listsWhileDown = fake.calls("local").filter((argv) => argv[0] === "pane" && argv[1] === "list").length;
  assert.ok(listsWhileDown >= 2, `the daemon keeps looking while Herdr is down (${listsWhileDown} reads)`);
  assert.equal(coordinator.ledger().events.length, eventsBefore, "a server that does not answer adds nothing to the ledger");

  fake.flag("local", "server-down", false);
  await coordinator.until(() => fake.pane("local", "child-pane").tokens?.parent_pane === "parent-pane", "the child's parent_pane token was not written back");
  assert.deepEqual(fake.pane("local", "child-pane").tokens, lineageTokens);
  assert.equal(fake.pane("local", "parent-pane").tokens, undefined, "a root gets no lineage token");
  await coordinator.until(() => coordinator.ledger().events.some((entry) => entry.type === "lineage.reconciled"), "the reconciliation was not recorded");
  const reconciled = coordinator.ledger().events.filter((entry) => entry.type === "lineage.reconciled");
  assert.deepEqual(reconciled.map((entry) => entry.detail), [{ scanned: 1, filled: 1, failed: 0 }]);

  const settled = writes(fake);
  await pause(5500);
  assert.equal(writes(fake), settled, "a token that is already current is not written again");
});

test("a child whose pane now hosts another session loses its lineage tokens, the ledger says so, and nothing is written again", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const coordinator = hq(t, fake);
  await coordinator.start();
  link(coordinator);
  assert.deepEqual(fake.pane("local", "child-pane").tokens, lineageTokens);

  // The child ends and a different agent starts in the same pane, with its own terminal.
  fake.setAgent("local", "child-pane", { session: "s-other", instance: "i-other" });
  await coordinator.until(() => coordinator.ledger().events.some((entry) => entry.type === "lineage.ended"), "the ended relationship was not recorded");
  assert.deepEqual(fake.pane("local", "child-pane").tokens, {}, "the new agent inherits no parent");
  const ended = coordinator.ledger().events.filter((entry) => entry.type === "lineage.ended");
  assert.equal(ended.length, 1);
  assert.deepEqual(ended[0].detail, { parent: coordinator.ledger().participants[ended[0].subjectId].parent, pane: "child-pane", recordedSession: "s-child", observedSession: "s-other" });

  const settled = writes(fake);
  await pause(5500);
  assert.equal(writes(fake), settled, "an ended relationship is neither re-asserted nor cleared again");
  assert.equal(coordinator.ledger().events.filter((entry) => entry.type === "lineage.ended").length, 1);
});

test("a child that reports no session keeps its tokens, and a refused clear waits for the retry interval", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const coordinator = hq(t, fake);
  await coordinator.start();
  link(coordinator);

  // A pane Herdr reports without a session proves neither that the relationship holds nor that it ended.
  fake.setAgent("local", "child-pane", { session: null, instance: "i-restarted" });
  await pause(5500);
  assert.deepEqual(fake.pane("local", "child-pane").tokens, lineageTokens);
  assert.equal(coordinator.ledger().events.some((entry) => entry.type === "lineage.ended"), false);

  // Herdr refuses the clear once the pane shows another session; the daemon does not hammer it.
  fake.flag("local", "metadata-denied");
  const before = writes(fake);
  fake.setAgent("local", "child-pane", { session: "s-other", instance: "i-other" });
  await pause(5500);
  const refused = writes(fake);
  assert.equal(refused, before + 1, "the daemon tried to clear the tokens once");
  await pause(5500);
  assert.equal(writes(fake), refused, "a refused clear waits for the retry interval");
  assert.deepEqual(fake.pane("local", "child-pane").tokens, lineageTokens);
  assert.equal(coordinator.ledger().events.some((entry) => entry.type === "lineage.ended"), false);
});

test("tokens written before sessions were recorded converge to the full set within one interval", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const coordinator = hq(t, fake);
  await coordinator.start();
  link(coordinator);

  fake.setPaneTokens("local", "child-pane", { parent_pane: "parent-pane" });
  await coordinator.until(() => fake.pane("local", "child-pane").tokens?.child_session !== undefined, "the session tokens were not added to an old declaration");
  assert.deepEqual(fake.pane("local", "child-pane").tokens, lineageTokens);
});
