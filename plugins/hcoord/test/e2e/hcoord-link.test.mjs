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
