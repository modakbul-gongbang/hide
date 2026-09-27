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
