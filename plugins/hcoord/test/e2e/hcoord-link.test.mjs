import assert from "node:assert/strict";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { createFakeRemote } from "../helpers/fake-remote.mjs";
import { hq } from "../helpers/hcoord-hq.mjs";

const CLI = path.resolve(import.meta.dirname, "../../dist/hcoord/cli.js");

test("Hide fork linking registers both executions and lets hcoord own lineage tokens", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const coordinator = hq(t, fake);
  await coordinator.start();

  const linked = spawnSync(process.execPath, [CLI, "agent", "link", "--parent-pane", "parent-pane", "--child-pane", "child-pane", "--json"], { env: coordinator.env, encoding: "utf8" });
  assert.equal(linked.status, 0, linked.stderr || linked.stdout);
  const result = JSON.parse(linked.stdout);
  assert.equal(result.ok, true);
  assert.equal(result.value.child.parent, result.value.parent.id);
  assert.deepEqual(fake.pane("local", "child-pane").tokens, { parent_pane: "parent-pane" });
  assert.ok(fake.calls("local").some((argv) => argv.join(" ").startsWith("pane report-metadata child-pane --source hcoord")));
});

test("--parent here refuses before creating a child without pane identity or a reported agent", async (t) => {
  const withoutPane = createFakeRemote(CLI);
  t.after(() => withoutPane.cleanup());
  const noPaneCoordinator = hq(t, withoutPane);
  await noPaneCoordinator.start();
  const noPane = noPaneCoordinator.json("agent", "spawn", "--parent", "here", "--name", "child", "--kind", "claude", "--intent", "here-no-pane");
  assert.equal(noPane.error.code, "parent_here_unavailable");
  assert.match(noPane.error.message, /requires HERDR_PANE_ID/);
  assert.equal(withoutPane.calls("local").some((argv) => argv[0] === "tab" && argv[1] === "create"), false);

  const withoutAgent = createFakeRemote(CLI);
  t.after(() => withoutAgent.cleanup());
  const noAgentCoordinator = hq(t, withoutAgent, "local", { HERDR_PANE_ID: "missing-pane" });
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
  const coordinator = hq(t, fake, "local", { HERDR_PANE_ID: "parent-pane" });
  await coordinator.start();

  const spawned = coordinator.ok("agent", "spawn", "--parent", "here", "--name", "child", "--kind", "claude", "--intent", "here-success");

  const parent = coordinator.ok("agent", "list").items.find((item) => item.pane === "parent-pane");
  assert.equal(spawned.participant.parent, parent.id);
  assert.equal(spawned.participant.pane, "child-pane");
  assert.deepEqual(fake.pane("local", "child-pane").tokens, { parent_pane: "parent-pane" });
  assert.equal(fake.calls("local").filter((argv) => argv[0] === "tab" && argv[1] === "create").length, 1);
});

test("lineage a Herdr restart dropped comes back while the daemon keeps running, even when Herdr returns after it", async (t) => {
  const fake = createFakeRemote(CLI);
  t.after(() => fake.cleanup());
  fake.addAgent("local", "parent-pane", { name: "parent", session: "s-parent", instance: "i-parent" });
  fake.addAgent("local", "child-pane", { name: "child", session: "s-child", instance: "i-child" });
  const coordinator = hq(t, fake);
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
  assert.deepEqual(fake.pane("local", "child-pane").tokens, { parent_pane: "parent-pane" });
  assert.equal(fake.pane("local", "parent-pane").tokens, undefined, "a root gets no lineage token");
  await coordinator.until(() => coordinator.ledger().events.some((entry) => entry.type === "lineage.reconciled"), "the reconciliation was not recorded");
  const reconciled = coordinator.ledger().events.filter((entry) => entry.type === "lineage.reconciled");
  assert.deepEqual(reconciled.map((entry) => entry.detail), [{ scanned: 1, filled: 1, failed: 0 }]);

  const writes = () => fake.calls("local").filter((argv) => argv[0] === "pane" && argv[1] === "report-metadata").length;
  const settled = writes();
  await new Promise((resolve) => setTimeout(resolve, 5500));
  assert.equal(writes(), settled, "a token that is already current is not written again");
});
