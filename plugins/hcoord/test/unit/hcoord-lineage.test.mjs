import assert from "node:assert/strict";
import test from "node:test";
import { hostsAnotherSession, lineageCurrent, sessionDigest } from "../../dist/hcoord/lineage.js";

const participant = (patch) => ({ id: "a_x", machine: "local", hostScope: "default", session: "s-child", instance: "i", name: "n", project: null, parent: null, pane: "w1:p2", runtime: "idle", connection: "connected", observedAt: "2026-10-01T00:00:00.000Z", ...patch });
const parent = participant({ id: "a_parent", session: "s-parent", pane: "w1:p1" });
const child = participant({ id: "a_child", parent: "a_parent" });
const written = { session: "s-child", instance: "i", parentPane: "w1:p1", parentMachine: null, childSession: sessionDigest("s-child"), parentSession: sessionDigest("s-parent") };

test("a session is written as the SHA-256 Hide computes for the same value", () => {
  // `printf s-child | shasum -a 256`; herdr-core/src/wire.rs asserts the same string.
  assert.equal(sessionDigest("s-child"), "e91e031561ec0bc9093101da407c3e78b99ebf139047f29f4ac6d5948e30cf0b");
  assert.notEqual(sessionDigest("s-child"), sessionDigest("s-other"));
});

test("tokens are current only when all of them match the relationship", () => {
  assert.equal(lineageCurrent(parent, child, written), true);
  assert.equal(lineageCurrent(parent, child, { ...written, parentSession: null }), false, "a declaration from before sessions were recorded is rewritten");
  assert.equal(lineageCurrent(parent, child, { ...written, childSession: sessionDigest("s-other") }), false);
  assert.equal(lineageCurrent(parent, child, { ...written, parentPane: "w1:p9" }), false);
});

test("only a session that is there and is not the recorded one ends a relationship", () => {
  assert.equal(hostsAnotherSession(child, { ...written, session: "s-other" }), true);
  assert.equal(hostsAnotherSession(child, { ...written, session: "s-child" }), false);
  assert.equal(hostsAnotherSession(child, { ...written, session: null }), false, "a pane that reports no session proves nothing either way");
});
