import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { emptyLedger, MAX_EVENTS } from "../../dist/hcoord/model.js";
import { execute } from "../../dist/hcoord/service.js";
import { inputReadiness, messageForDelivery } from "../../dist/hcoord/herdr.js";
import { loadLedger, saveLedger } from "../../dist/hcoord/store.js";
import { callDaemon } from "../../dist/hcoord/transport.js";

test("a denied local socket gives the caller a permission cause and next action", { skip: process.platform === "win32" }, async (t) => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "hcoord-denied-socket-"));
  const dir = path.join(home, ".hcoord");
  fs.mkdirSync(dir);
  const socketFile = path.join(dir, "api.sock");
  const server = net.createServer((socket) => socket.end());
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(socketFile, resolve); });
  t.after(async () => {
    fs.chmodSync(socketFile, 0o600);
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(home, { recursive: true, force: true });
  });
  fs.chmodSync(socketFile, 0o000);
  await assert.rejects(callDaemon("status", {}, home), (error) => error.code === "permission_denied" && /socket access was denied.*retry/.test(error.message));
});

test("an escalated human answer reaches the assigned parent and retains an unresolved relay", () => {
  const at = "2026-09-01T00:00:00.000Z";
  const state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name, instance) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance, name, pane: `${name}-pane`, runtime: "idle" });
  const parent = register("parent", "one");
  const child = register("child", "two");
  const request = run("request.send", { from: child.id, to: parent.id, body: "Choose a path", intent: "choice" });
  run("request.escalate", { id: request.id, actor: parent.id });
  const answered = run("request.reply", { id: request.id, body: "Proceed with A", respondent: "human", recordedBy: "human" });
  assert.equal(answered.intermediary, parent.id);
  assert.equal(answered.deliveries.at(-1).recipient, parent.id);
  assert.equal(run("inbox").find((item) => item.requestId === request.id).kind, "relay_problem");
  const reminderAt = "2026-09-01T00:15:01.000Z";
  run("tick", {}, reminderAt);
  assert.equal(state.requests[request.id].relayRemindedAt, reminderAt);
  assert.equal(state.requests[request.id].deliveries.at(-1).phase, "relay_problem");
  assert.equal(state.requests[request.id].deliveries.at(-1).recipient, parent.id);
  const escalationAt = "2026-09-01T00:30:01.000Z";
  run("tick", {}, escalationAt);
  assert.equal(state.requests[request.id].relayEscalatedAt, escalationAt);
  assert.equal(state.requests[request.id].deliveries.at(-1).phase, "relay_problem");
  assert.equal(state.requests[request.id].deliveries.filter((delivery) => delivery.recipient === "human").length, 2);
  run("tick", {}, "2026-09-01T00:31:01.000Z");
  assert.equal(state.requests[request.id].deliveries.filter((delivery) => delivery.recipient === "human").length, 2, "one reminder episode does not create repeated human notifications");
  run("request.relay", { id: request.id, actor: parent.id, body: "Proceed with A" });
  assert.equal(state.requests[request.id].deliveries.at(-1).recipient, child.id);
  assert.equal(run("inbox").some((item) => item.requestId === request.id && item.kind === "relay_problem"), false);
});

test("a restart coalesces overdue reminders and an expired empty event history rejects old cursors", () => {
  const at = "2026-09-01T00:00:00.000Z";
  const state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const sender = run("agent.register", { machine: "local", hostScope: "default", session: "s", instance: "i", name: "sender", pane: "p", runtime: "idle" });
  const request = run("request.send", { from: sender.id, to: "human", body: "Choose", intent: "one" });
  const lateTick = "2026-09-01T00:31:00.000Z";
  run("tick", {}, lateTick);
  assert.equal(state.requests[request.id].remindedAt, lateTick);
  assert.equal(state.requests[request.id].escalatedAt, lateTick);
  assert.equal(state.requests[request.id].deliveries.filter((delivery) => delivery.recipient === "human").length, 2, "initial notification plus one coalesced escalation");
  assert.equal(state.events.some((entry) => entry.type === "request.reminded"), false);
  run("request.cancel", { id: request.id, actor: sender.id }, "2026-09-01T00:32:00.000Z");
  run("tick", {}, "2026-10-03T00:00:00.000Z");
  assert.equal(state.events.length, 0);
  assert.ok(state.seq > 0);
  assert.throws(() => run("events", { cursor: 0 }), { code: "cursor_expired" });
  assert.equal(run("status").eventCursor, state.seq);
});

test("caller IDs cannot address inherited records or mutate policy prototypes", () => {
  const state = emptyLedger("2026-09-01T00:00:00.000Z");
  const run = (operation, args) => execute(state, operation, args, "2026-09-01T00:00:00.000Z").value;
  assert.throws(() => run("request.send", { from: "__proto__", to: "human", body: "spoof", intent: "probe" }), { code: "not_found" });
  assert.throws(() => run("request.cancel", { id: "__proto__", actor: "human" }), { code: "not_found" });
  assert.throws(() => run("agent.show", { id: "constructor" }), { code: "not_found" });
  assert.throws(() => run("config.set", { key: "constructor", value: 1000 }), { code: "invalid_argument" });
  assert.equal(Object.prototype.status, undefined);
  assert.equal(Object.keys(state.requests).length, 0);
  const parent = run("agent.register", { machine: "local", hostScope: "default", session: "s", instance: "i", name: "parent", pane: "p", runtime: "idle" });
  const intent = run("agent.spawn.reserve", { parent: parent.id, intent: "__proto__", machine: "local", session: "s", name: "child", kind: "codex", nativeArgs: [] });
  assert.equal(run("agent.spawn.reserve", { parent: parent.id, intent: "__proto__", machine: "local", session: "s", name: "child", kind: "codex", nativeArgs: [] }).key, intent.key);
  assert.equal(Object.getPrototypeOf(state.spawnIntents), Object.prototype);
  assert.equal(Object.hasOwn(state.spawnIntents, "__proto__"), true);
  assert.throws(() => run("agent.spawn.reserve", { parent: parent.id, intent: "bad-name", machine: "local", session: "s", name: "Bad Name", kind: "codex", nativeArgs: [] }), { code: "invalid_argument" });
  assert.throws(() => run("agent.spawn.reserve", { parent: parent.id, intent: "bad-kind", machine: "local", session: "s", name: "valid", kind: "other", nativeArgs: [] }), { code: "unsupported_runtime" });
  assert.equal(Object.keys(state.spawnIntents).length, 1, "invalid spawn leaves no reserved external effect");
  for (const free of [4, 5, 6]) {
    const intent = `no-capacity-${free}`;
    state.events.length = MAX_EVENTS - free;
    assert.throws(() => run("agent.spawn.reserve", { parent: parent.id, intent, machine: "local", session: "s", name: "valid", kind: "codex", nativeArgs: [] }), { code: "capacity" });
    assert.equal(Object.hasOwn(state.spawnIntents, intent), false, "a pane is never created without capacity to record its result");
  }
});

test("spawn reservation has room for the full first-turn progress and registration", () => {
  const at = "2026-09-01T00:00:00.000Z";
  const state = emptyLedger(at);
  const run = (operation, args) => execute(state, operation, args, at).value;
  const parent = run("agent.register", { machine: "local", hostScope: "default", session: "parent", instance: "parent-terminal", name: "parent", pane: "parent-pane", runtime: "idle" });
  state.events.length = MAX_EVENTS - 7;
  run("agent.spawn.reserve", { parent: parent.id, intent: "near-cap", machine: "local", session: "parent", name: "child", kind: "codex", nativeArgs: [] });
  run("agent.spawn.unknown", { intent: "near-cap", reason: "tab creation reserved; outcome pending", workspace: "workspace", cwd: "/fixture" });
  run("agent.spawn.pane", { intent: "near-cap", pane: "child-pane" });
  run("agent.spawn.initialization", { intent: "near-cap", phase: "reserved", instance: "child-terminal" });
  run("agent.spawn.identity", { intent: "near-cap", runtimeSession: "child-session", instance: "child-terminal" });
  const result = run("agent.spawn.complete", { intent: "near-cap", runtimeSession: "child-session", instance: "child-terminal", runtime: "idle" });
  assert.equal(result.intent.status, "complete");
  assert.equal(state.events.length, MAX_EVENTS - 1);
});

test("watch handover preserves one unchecked cycle and graph history", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const target = register("target"), first = register("first"), second = register("second");
  run("watch.start", { target: target.id, observer: first.id, actor: "human", intervalMs: 1000 });
  run("tick", {}, "2026-09-01T00:00:01.000Z");
  const cycle = state.watches[target.id].cycle;
  const original = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
  assert.ok(original);
  assert.throws(() => run("watch.start", { target: target.id, observer: first.id, actor: first.id }), { code: "conflict" });
  const handed = run("watch.assign", { target: target.id, observer: second.id, actor: first.id, expectedGeneration: "1" }, "2026-09-01T00:00:02.000Z");
  assert.equal(handed.cycle, cycle);
  assert.equal(original.to, second.id);
  assert.equal(original.deliveries[0].status, "superseded");
  assert.equal(original.deliveries[1].recipient, second.id);
  run("tick", {}, "2026-09-01T00:00:03.000Z");
  assert.equal(Object.values(state.requests).filter((item) => item.intent.startsWith("watch:")).length, 1);
  assert.deepEqual(run("graph").watch.filter((edge) => edge.target === target.id).map((edge) => edge.status), ["stopped", "active"]);
  run("watch.check", { target: target.id, cycle, actor: second.id });
  assert.equal(original.status, "answered");
  assert.equal(original.deliveries[1].status, "superseded");
});

test("only watch check closes a cycle, including an older replied cycle", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const target = register("target"), first = register("first"), second = register("second");
  run("watch.start", { target: target.id, observer: first.id, actor: "human", intervalMs: 1000 });
  run("tick", {}, "2026-09-01T00:00:01.000Z");
  const cycle = state.watches[target.id].cycle;
  const checkRequest = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
  assert.throws(() => run("request.reply", { id: checkRequest.id, body: "Observed", respondent: first.id, recordedBy: first.id }), { code: "conflict" });
  assert.throws(() => run("request.cancel", { id: checkRequest.id, actor: "human" }), { code: "conflict" });
  assert.equal(checkRequest.deliveries.length, 1, "reply cannot prompt the watched child");
  checkRequest.status = "answered";
  checkRequest.answeredAt = "2026-09-01T00:00:01.000Z";
  checkRequest.answer = "older stored reply";
  assert.equal(state.watches[target.id].cycle, cycle, "reply does not check the cycle");
  run("watch.assign", { target: target.id, observer: second.id, actor: first.id, expectedGeneration: "1" });
  assert.equal(state.watches[target.id].cycle, cycle);
  assert.equal(checkRequest.deliveries.at(-1).phase, "watch_check");
  assert.equal(checkRequest.deliveries.at(-1).recipient, second.id);
  checkRequest.deliveries.at(-1).status = "accepted";
  run("tick", { observedTargets: [] }, "2026-10-03T00:00:00.000Z");
  assert.ok(state.requests[checkRequest.id], "delivery acceptance cannot prune an unchecked watch cycle");
  assert.equal(state.watches[target.id].cycle, cycle);
  run("tick", {}, "2026-10-03T00:00:01.000Z");
  assert.equal(Object.values(state.requests).filter((item) => item.intent.startsWith("watch:")).length, 1);
  run("watch.check", { target: target.id, cycle, actor: second.id }, "2026-10-03T00:00:02.000Z");
  assert.equal(state.watches[target.id].cycle, null);
  assert.equal(checkRequest.deliveries.at(-1).status, "acknowledged");
});

test("watch ownership distinguishes human handover from observer confirmation", () => {
  const state = emptyLedger("2026-09-01T00:00:00.000Z");
  const run = (operation, args = {}, at = "2026-09-01T00:00:00.000Z") => execute(state, operation, args, at).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const target = register("target"), first = register("first"), second = register("second");
  assert.throws(() => run("watch.start", { target: target.id, observer: first.id, actor: first.id }), { code: "forbidden" });
  run("watch.start", { target: target.id, observer: first.id, actor: "human", intervalMs: 1000 });
  run("tick", {}, "2026-09-01T00:00:01.000Z");
  const cycle = state.watches[target.id].cycle;
  assert.throws(() => run("watch.check", { target: target.id, cycle, actor: "human" }), { code: "forbidden" });
  run("watch.stop", { target: target.id, actor: "human" });
  assert.throws(() => run("watch.start", { target: target.id, observer: second.id, actor: second.id }), { code: "forbidden" });
  run("watch.assign", { target: target.id, observer: second.id, actor: "human", expectedGeneration: "1" });
  assert.equal(state.watches[target.id].observer, second.id);
  assert.equal(state.watches[target.id].cycle, cycle, "restart carries the one unchecked cycle");
  const carried = Object.values(state.requests).filter((item) => item.intent.startsWith("watch:"));
  assert.equal(carried.length, 1);
  assert.equal(carried[0].to, second.id);
  assert.equal(carried[0].deliveries.at(-1).recipient, second.id);
});

test("restarting a stopped watch with the same observer reuses the original delivery", () => {
  for (const status of ["pending", "deferred", "unknown", "accepted", "acknowledged"]) {
    const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
    const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
    const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
    const target = register("target"), observer = register("observer");
    run("watch.start", { target: target.id, observer: observer.id, actor: "human", intervalMs: 1000 });
    run("tick", {}, "2026-09-01T00:00:01.000Z");
    const checkRequest = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
    checkRequest.deliveries[0].status = status;
    const originalCycle = state.watches[target.id].cycle;
    run("watch.stop", { target: target.id, actor: "human" });
    run("watch.start", { target: target.id, observer: observer.id, actor: "human" });
    assert.equal(state.watches[target.id].cycle, originalCycle, status);
    assert.equal(checkRequest.deliveries.length, 1, `${status} delivery must not be submitted twice`);
    assert.equal(checkRequest.deliveries[0].status, status);
  }
});

test("an ordinary request with a watch-like intent cannot close the actual cycle", () => {
  for (const legacy of [false, true]) {
    const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
    const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
    const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
    const target = register("target"), observer = register("observer");
    run("watch.start", { target: target.id, observer: observer.id, actor: "human", intervalMs: 1000 });
    run("tick", {}, "2026-09-01T00:00:01.000Z");
    const cycle = state.watches[target.id].cycle;
    const checkRequest = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
    if (legacy) delete state.watches[target.id].requestId;
    const decoy = run("request.send", { from: target.id, to: "human", body: "Ordinary question", intent: `watch:${target.id}:decoy:${cycle}` });
    assert.match(run("request.show", { id: decoy.id }).nextAction, /assigned recipient/);
    run("watch.stop", { target: target.id, actor: "human" });
    run("request.cancel", { id: decoy.id, actor: "human" });
    assert.equal(state.watches[target.id].cycle, cycle, `decoy cancellation must not close ${legacy ? "legacy" : "current"} watch`);
    run("watch.start", { target: target.id, observer: observer.id, actor: "human" });
    assert.equal(state.watches[target.id].requestId, checkRequest.id);
    run("watch.check", { target: target.id, cycle, actor: observer.id });
    assert.equal(checkRequest.status, "answered");
  }
});

test("a checked cycle resolves uncertain wake delivery without claiming acceptance", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const target = register("target"), observer = register("observer");
  run("watch.start", { target: target.id, observer: observer.id, actor: "human", intervalMs: 1000 });
  run("tick", {}, "2026-09-01T00:00:01.000Z");
  const cycle = state.watches[target.id].cycle;
  const checkRequest = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
  checkRequest.deliveries[0].status = "unknown";
  checkRequest.deliveries[0].reason = "guarded submission outcome unknown";
  run("watch.stop", { target: target.id, actor: "human" });
  run("watch.start", { target: target.id, observer: observer.id, actor: "human" });
  assert.equal(checkRequest.deliveries.length, 1);
  run("watch.check", { target: target.id, cycle, actor: observer.id }, "2026-09-01T00:00:02.000Z");
  assert.equal(checkRequest.deliveries[0].status, "unknown", "the transport outcome remains unconfirmed");
  assert.equal(checkRequest.deliveries[0].actionClosedAt, "2026-09-01T00:00:02.000Z");
  assert.match(checkRequest.deliveries[0].reason, /outcome unknown.*cycle checked/);
  assert.match(run("request.show", { id: checkRequest.id }).nextAction, /no further wake submission/);
  assert.equal(run("inbox").some((item) => item.requestId === checkRequest.id), false);
  run("tick", { observedTargets: [] }, "2026-10-03T00:00:00.000Z");
  assert.equal(state.requests[checkRequest.id], undefined, "a completed cycle eventually leaves retention");
});

test("a stopped older answered cycle remains available for explicit checking", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const target = register("target"), observer = register("observer");
  run("watch.start", { target: target.id, observer: observer.id, actor: "human", intervalMs: 1000 });
  run("tick", {}, "2026-09-01T00:00:01.000Z");
  const cycle = state.watches[target.id].cycle;
  const checkRequest = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
  checkRequest.status = "answered";
  checkRequest.answeredAt = "2026-09-01T00:00:01.000Z";
  checkRequest.answer = "older stored reply";
  checkRequest.respondent = "human";
  checkRequest.intermediary = observer.id;
  checkRequest.deliveries[0].status = "accepted";
  for (const status of ["pending", "deferred", "unknown", "accepted"]) checkRequest.deliveries.push({ ...checkRequest.deliveries[0], id: `legacy-answer-${status}`, recipient: target.id, phase: "answer", status, reason: status === "unknown" ? "old answer outcome unknown" : null });
  run("watch.stop", { target: target.id, actor: "human" }, "2026-09-01T00:00:02.000Z");
  run("tick", { observedTargets: [] }, "2026-10-03T00:00:00.000Z");
  assert.ok(state.requests[checkRequest.id]);
  assert.match(run("request.show", { id: checkRequest.id }).nextAction, /explicit check/);
  run("watch.assign", { target: target.id, observer: observer.id, actor: "human", expectedGeneration: "1" }, "2026-10-03T00:00:01.000Z");
  assert.equal(state.watches[target.id].cycle, cycle);
  run("watch.check", { target: target.id, cycle, actor: observer.id }, "2026-10-03T00:00:02.000Z");
  assert.equal(checkRequest.deliveries.find((delivery) => delivery.id === "legacy-answer-pending").status, "superseded");
  assert.equal(checkRequest.deliveries.find((delivery) => delivery.id === "legacy-answer-deferred").status, "superseded");
  assert.equal(checkRequest.deliveries.find((delivery) => delivery.id === "legacy-answer-unknown").status, "unknown");
  assert.equal(checkRequest.deliveries.find((delivery) => delivery.id === "legacy-answer-unknown").actionClosedAt, "2026-10-03T00:00:02.000Z");
  assert.equal(checkRequest.deliveries.find((delivery) => delivery.id === "legacy-answer-accepted").status, "accepted");
  assert.equal(checkRequest.deliveries.find((delivery) => delivery.id === "legacy-answer-accepted").actionClosedAt, "2026-10-03T00:00:02.000Z");
  assert.equal(run("inbox").some((item) => item.requestId === checkRequest.id), false);
  assert.throws(() => run("request.relay", { id: checkRequest.id, actor: observer.id, body: "Late relay" }), { code: "conflict" });
  run("tick", { observedTargets: [] }, "2026-10-03T00:00:03.000Z");
  assert.ok(state.requests[checkRequest.id], "actual watch check starts the resolution retention window");
  run("tick", { observedTargets: [] }, "2026-11-05T00:00:00.000Z");
  assert.equal(state.requests[checkRequest.id], undefined);
});

test("a checked older relayed cycle schedules no new delivery reminders", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const target = register("target"), observer = register("observer");
  run("watch.start", { target: target.id, observer: observer.id, actor: "human", intervalMs: 1000 });
  run("tick", {}, "2026-09-01T00:00:01.000Z");
  const cycle = state.watches[target.id].cycle;
  const checkRequest = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
  checkRequest.status = "answered";
  checkRequest.answeredAt = "2026-09-01T00:00:01.000Z";
  checkRequest.answer = "older stored reply";
  checkRequest.intermediary = observer.id;
  checkRequest.relayBody = "older relay";
  checkRequest.relayAt = "2026-09-01T00:00:02.000Z";
  checkRequest.deliveries.push({ ...checkRequest.deliveries[0], id: "legacy-relay", phase: "relay", recipient: target.id, status: "pending" });
  run("watch.stop", { target: target.id, actor: "human" });
  run("watch.start", { target: target.id, observer: observer.id, actor: "human" });
  run("watch.check", { target: target.id, cycle, actor: observer.id }, "2026-09-01T00:00:03.000Z");
  assert.equal(checkRequest.deliveries.find((delivery) => delivery.id === "legacy-relay").status, "superseded");
  run("tick", { observedTargets: [] }, "2026-09-01T01:00:00.000Z");
  assert.equal(checkRequest.deliveries.some((delivery) => delivery.phase === "delivery_problem"), false);
  assert.equal(run("inbox").some((item) => item.requestId === checkRequest.id), false);
});

test("a stopped watch can release its canceled request and relation after retention", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const target = register("target"), observer = register("observer");
  run("watch.start", { target: target.id, observer: observer.id, actor: "human", intervalMs: 1000 });
  run("tick", {}, "2026-09-01T00:00:01.000Z");
  const checkRequest = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
  run("watch.stop", { target: target.id, actor: "human" }, "2026-09-01T00:00:02.000Z");
  assert.throws(() => run("request.reply", { id: checkRequest.id, body: "Observed", respondent: observer.id, recordedBy: observer.id }), { code: "conflict" });
  assert.match(run("request.show", { id: checkRequest.id }).nextAction, /stopped/);
  run("request.cancel", { id: checkRequest.id, actor: "human" }, "2026-09-01T00:00:03.000Z");
  assert.equal(state.watches[target.id].cycle, null);
  assert.match(run("request.show", { id: checkRequest.id }).nextAction, /cancellation/);
  run("tick", { observedTargets: [] }, "2026-10-03T00:00:00.000Z");
  assert.equal(state.requests[checkRequest.id], undefined);
  assert.equal(state.watches[target.id], undefined);
  assert.equal(run("status").counts.requests, 0);
});

test("a retained stopped watch can restart after its old canceled cycle expires", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const target = register("target"), observer = register("observer");
  run("watch.start", { target: target.id, observer: observer.id, actor: "human", intervalMs: 1000 });
  run("tick", {}, "2026-09-01T00:00:01.000Z");
  const checkRequest = Object.values(state.requests).find((item) => item.intent.startsWith("watch:"));
  const oldCycle = state.watches[target.id].cycle;
  run("watch.stop", { target: target.id, actor: "human" }, "2026-09-01T00:00:02.000Z");
  run("request.cancel", { id: checkRequest.id, actor: "human" }, "2026-09-01T00:00:03.000Z");
  run("request.send", { from: target.id, to: "human", body: "Keep participant referenced", intent: "ordinary-open" });
  state.watches[target.id].cycle = oldCycle; // Reproduce a stopped record saved before cancellation cleared the cycle.
  run("tick", { observedTargets: [] }, "2026-10-03T00:00:00.000Z");
  assert.equal(state.requests[checkRequest.id], undefined);
  assert.ok(state.watches[target.id], "the unrelated open request retains the stopped watch");
  const restarted = run("watch.assign", { target: target.id, observer: observer.id, actor: "human", expectedGeneration: "1" }, "2026-10-03T00:00:01.000Z");
  assert.equal(restarted.status, "active");
  assert.equal(restarted.cycle, null);
});

test("request intent checks the full routing policy and notify-only has no question", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}) => execute(state, operation, args, at).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const sender = register("sender"), first = register("first"), second = register("second");
  const args = { from: sender.id, to: "human", body: "A?", intent: "same", intermediary: first.id, context: "context", waiting: true };
  const original = run("request.send", args);
  assert.equal(run("request.send", args).id, original.id);
  for (const changed of [{ intermediary: second.id }, { context: "other" }, { waiting: false }, { notifyOnly: true }]) {
    assert.throws(() => run("request.send", { ...args, ...changed }), { code: "intent_conflict" });
  }
  const notice = run("request.send", { from: sender.id, to: "human", body: "Heads up", intent: "notice", notifyOnly: true });
  assert.equal(notice.requiresReply, false);
  assert.equal(run("inbox").some((item) => item.requestId === notice.id && item.kind === "question"), false);
  const escalated = run("request.send", { from: sender.id, to: first.id, body: "Decide", intent: "escalated" });
  run("request.escalate", { id: escalated.id, actor: first.id });
  run("request.reply", { id: escalated.id, body: "Yes", respondent: "human", recordedBy: "human" });
  assert.equal(escalated.intermediary, first.id);
  assert.equal(run("request.send", { from: sender.id, to: first.id, body: "Decide", intent: "escalated" }).id, escalated.id, "the original payload remains idempotent after relay ownership is assigned");
});

test("answer and relay deliveries have distinct receipts and unresolved answers survive retention", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const register = (name) => run("agent.register", { machine: "local", hostScope: "default", session: name, instance: name, name, pane: `${name}-pane`, runtime: "idle" });
  const child = register("child"), parent = register("parent");
  const direct = run("request.send", { from: child.id, to: parent.id, body: "Question", intent: "direct" });
  direct.deliveries[0].status = "accepted";
  run("request.ack", { id: direct.id, actor: parent.id });
  run("request.escalate", { id: direct.id, actor: parent.id });
  run("request.reply", { id: direct.id, body: "Human answer", respondent: "human", recordedBy: "human" });
  const answerDelivery = direct.deliveries.at(-1);
  assert.equal(answerDelivery.phase, "answer");
  assert.equal(answerDelivery.recipient, parent.id);
  assert.throws(() => run("request.ack", { id: direct.id, actor: parent.id }), { code: "conflict" }, "an earlier ACK cannot satisfy a pending answer");
  answerDelivery.status = "accepted";
  run("request.ack", { id: direct.id, actor: parent.id });
  assert.equal(answerDelivery.status, "acknowledged", "an earlier receipt does not hide the latest accepted delivery");
  run("request.relay", { id: direct.id, actor: parent.id, body: "Human answer" });
  const relayDelivery = direct.deliveries.at(-1);
  assert.equal(relayDelivery.phase, "relay");
  run("tick", {}, "2026-10-03T00:00:00.000Z");
  assert.ok(state.requests[direct.id], "a pending relay preserves the original answer beyond retention");
  assert.ok(direct.deliveryRemindedAt, "an undelivered child answer reminds the parent");
  assert.ok(direct.deliveryEscalatedAt, "an unresolved delivery reaches the human inbox deadline");
  assert.equal(direct.deliveries.filter((delivery) => delivery.phase === "delivery_problem").length, 2);
  relayDelivery.status = "accepted";
  run("request.ack", { id: direct.id, actor: child.id, delivery: relayDelivery.id });
  assert.equal(direct.deliveries.filter((delivery) => delivery.phase === "delivery_problem").every((delivery) => delivery.status === "superseded"), true);
  run("tick", {}, "2026-10-03T00:01:00.000Z");
  assert.equal(state.requests[direct.id], undefined);
  const childAnswer = run("request.send", { from: child.id, to: parent.id, body: "Second", intent: "direct-answer" }, "2026-10-03T00:02:00.000Z");
  run("request.reply", { id: childAnswer.id, body: "Parent answer", respondent: parent.id, recordedBy: parent.id }, "2026-10-03T00:02:01.000Z");
  assert.equal(childAnswer.deliveries[0].status, "superseded");
  assert.equal(childAnswer.deliveries[1].phase, "answer");
  assert.equal(childAnswer.deliveries[1].recipient, child.id);
});

test("internal observation advances the saved freshness time", () => {
  const state = emptyLedger("2026-09-01T00:00:00.000Z");
  const participant = execute(state, "agent.register", { machine: "local", hostScope: "default", session: "s", instance: "i", name: "worker", pane: "p", runtime: "idle" }, "2026-09-01T00:00:00.000Z").value;
  execute(state, "agent.observe", { id: participant.id, runtime: "done", connection: "connected" }, "2026-09-01T00:01:00.000Z");
  assert.equal(state.updatedAt, state.participants[participant.id].observedAt);
});

test("oversized calls return an actionable capacity error before connecting", async () => {
  await assert.rejects(callDaemon("request.send", { context: "a".repeat(1024 * 1024) }, "/nonexistent/hcoord-home"), { code: "capacity" });
});

test("invalid ledger JSON does not expose stored request text", () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "hcoord-corrupt-"));
  try {
    fs.mkdirSync(path.join(home, ".hcoord"));
    fs.writeFileSync(path.join(home, ".hcoord", "ledger.json"), '{"request":"PRIVATE_ANSWER", invalid');
    assert.throws(() => loadLedger(home), (error) => error.code === "corrupt_ledger" && !String(error).includes("PRIVATE_ANSWER"));
  } finally { fs.rmSync(home, { recursive: true, force: true }); }
});

test("retention releases ended participants and completed spawn intents without breaking the parent", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}, time = at) => execute(state, operation, args, time).value;
  const parent = run("agent.register", { machine: "local", hostScope: "default", session: "parent", instance: "parent", name: "parent", pane: "parent-pane", runtime: "idle" });
  run("agent.spawn.reserve", { parent: parent.id, intent: "completed-child", machine: "local", session: "parent", name: "child", kind: "codex", noWatch: true, nativeArgs: [] });
  run("agent.spawn.pane", { intent: "completed-child", pane: "child-pane" });
  const child = run("agent.spawn.complete", { intent: "completed-child", runtimeSession: "child", instance: "child", runtime: "done" }).participant;
  run("tick", {}, "2026-10-03T00:00:00.000Z");
  assert.equal(state.participants[child.id], undefined);
  assert.equal(state.spawnIntents["completed-child"], undefined);
  assert.ok(state.participants[parent.id]);
});

test("a saved spawn intent rejects a different terminal or session before registration", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}) => execute(state, operation, args, at).value;
  const parent = run("agent.register", { machine: "local", hostScope: "default", session: "parent", instance: "parent-terminal", name: "parent", pane: "parent-pane", runtime: "idle" });
  run("agent.spawn.reserve", { parent: parent.id, intent: "spawn", machine: "local", session: "parent", name: "child", kind: "codex", nativeArgs: [] });
  run("agent.spawn.pane", { intent: "spawn", pane: "child-pane" });
  run("agent.spawn.initialization", { intent: "spawn", phase: "reserved", instance: "original-terminal" });
  assert.throws(() => run("agent.spawn.identity", { intent: "spawn", runtimeSession: "original-session", instance: "replacement-terminal" }), { code: "identity_conflict" });
  run("agent.spawn.identity", { intent: "spawn", runtimeSession: "original-session", instance: "original-terminal" });
  assert.throws(() => run("agent.spawn.identity", { intent: "spawn", runtimeSession: "replacement-session", instance: "original-terminal" }), { code: "identity_conflict" });
  assert.throws(() => run("agent.spawn.complete", { intent: "spawn", runtimeSession: "replacement-session", instance: "original-terminal", runtime: "idle" }), { code: "identity_conflict" });
});

test("completed spawn progress clears old pending guidance and same-intent retry repairs an older record", () => {
  const at = "2026-09-01T00:00:00.000Z", state = emptyLedger(at);
  const run = (operation, args = {}) => execute(state, operation, args, at).value;
  const parent = run("agent.register", { machine: "local", hostScope: "default", session: "parent", instance: "parent-terminal", name: "parent", pane: "parent-pane", runtime: "idle" });
  const input = { parent: parent.id, intent: "spawn", machine: "local", session: "parent", name: "child", kind: "codex", nativeArgs: [], noWatch: false };
  run("agent.spawn.reserve", input);
  run("agent.spawn.unknown", { intent: "spawn", reason: "tab creation reserved; outcome pending", workspace: "w1", cwd: "/fixture" });
  run("agent.spawn.pane", { intent: "spawn", pane: "child-pane" });
  run("agent.spawn.initialization", { intent: "spawn", phase: "reserved", instance: "child-terminal" });
  const complete = run("agent.spawn.complete", { intent: "spawn", runtimeSession: "child-session", instance: "child-terminal", runtime: "done" });
  assert.equal(complete.intent.reason, null);
  assert.equal(complete.intent.initialization, "complete");
  complete.intent.reason = "tab creation reserved; outcome pending";
  complete.intent.initialization = "reserved";
  const repaired = execute(state, "agent.spawn.reserve", input, at);
  assert.equal(repaired.changed, true);
  assert.equal(repaired.value.reason, null);
  assert.equal(repaired.value.initialization, "complete");
  assert.equal(repaired.value.participant, complete.participant.id);
});

test("D-17: an idle or done recipient without a readiness flag takes input; working or a false flag holds it", () => {
  assert.equal(inputReadiness("idle", null).ready, true, "a hand-started idle Observer");
  assert.equal(inputReadiness("done", null).ready, true);
  assert.equal(inputReadiness("working", null).ready, false);
  assert.equal(inputReadiness("unknown", null).ready, false);
  assert.equal(inputReadiness("idle", false).ready, false, "a false flag still holds");
  assert.equal(inputReadiness("idle", true).ready, true);
  assert.equal(inputReadiness("working", true).ready, false);
});

test("D-18: registering the same pane and session keeps one participant and records its current terminal and name; the session in another pane under that name conflicts", () => {
  const at = "2026-09-26T00:00:00.000Z", state = emptyLedger(at);
  const register = (fields) => execute(state, "agent.register", { machine: "local", hostScope: "default", session: "s", instance: "t1", name: "observer", pane: "w1:p1", runtime: "idle", ...fields }, at).value;
  const first = register();
  // Registered under the older terminal rule: a second record for the same execution.
  state.participants["a_legacy"] = { ...first, id: "a_legacy", instance: "t2", name: "observer-2" };
  assert.equal(register({ instance: "t3", name: "observer-2" }).id, "a_legacy", "the record already carrying the requested name is kept");
  const renamed = register({ instance: "t4", name: "observer-3" });
  assert.equal(renamed.id, "a_legacy", "otherwise the latest record");
  assert.deepEqual([renamed.instance, renamed.name], ["t4", "observer-3"]);
  assert.equal(state.events.filter((entry) => entry.type === "agent.binding_refreshed").length, 2);
  assert.throws(() => register({ pane: "w1:p9", name: "observer" }), { code: "identity_conflict", message: /another pane/ });
});

/** An observer watching a target, a second observer, and the ledger they share. */
function watchedLedger(at, runtime = "working") {
  const state = emptyLedger(at);
  const register = (name, parent = null) => execute(state, "agent.register", { machine: "local", hostScope: "default", session: `s-${name}`, instance: `t-${name}`, name, pane: `${name}-pane`, parent, runtime }, at).value.id;
  const observer = register("observer");
  const target = register("target", observer);
  const next = register("next");
  execute(state, "watch.start", { target, observer, actor: observer, intervalMs: 60_000, brief: "read the digest\n-> done: end it" }, at);
  return { state, observer, target, next };
}

test("D-19: watch assign moves the open question and the unrelayed answer to the new observer with the watch", () => {
  const at = "2026-09-26T00:00:00.000Z";
  const { state, observer, target, next } = watchedLedger(at);
  const open = execute(state, "request.send", { from: target, to: observer, body: "which?", intent: "q-open", waiting: true }, at).value;
  const answered = execute(state, "request.send", { from: target, to: observer, body: "which one?", intent: "q-answered", waiting: true }, at).value;
  execute(state, "request.escalate", { id: answered.id, actor: observer }, at);
  execute(state, "request.reply", { id: answered.id, body: "A", respondent: "human", recordedBy: observer }, at);
  const assigned = execute(state, "watch.assign", { target, observer: next, actor: "human", expectedGeneration: "1" }, at).value;
  assert.equal(assigned.brief, "read the digest\n-> done: end it", "the brief stays with the watch");
  assert.equal(state.requests[open.id].to, next);
  assert.ok(state.requests[open.id].deliveries.some((delivery) => delivery.recipient === next && delivery.status === "pending"));
  assert.equal(state.requests[open.id].deliveries.some((delivery) => delivery.recipient === observer && delivery.status === "pending"), false);
  assert.equal(state.requests[answered.id].intermediary, next, "the new observer relays the recorded answer");
  assert.throws(() => execute(state, "request.relay", { id: answered.id, actor: observer, body: "A" }, at), /authority/);
  execute(state, "request.relay", { id: answered.id, actor: next, body: "A" }, at);
  assert.equal(state.events.at(-2).detail.movedRequests, 2);
});

test("D-20: agent end stops the target's watch, drops its open cycle, cancels its questions, and a second end changes nothing", () => {
  const at = "2026-09-26T00:00:00.000Z";
  const { state, observer, target } = watchedLedger(at);
  execute(state, "tick", { runRetention: false }, "2026-09-26T00:01:00.000Z");
  const cycle = state.watches[target].requestId;
  assert.notEqual(cycle, null);
  const question = execute(state, "request.send", { from: target, to: observer, body: "which?", intent: "q", waiting: true }, at).value;
  assert.throws(() => execute(state, "agent.end", { id: target, actor: "a_stranger" }, at), { code: "forbidden" });
  const ended = execute(state, "agent.end", { id: target, actor: observer }, at);
  assert.equal(ended.changed, true);
  assert.equal(ended.value.watchStopped, true);
  assert.deepEqual(ended.value.canceled.sort(), [cycle, question.id].sort());
  assert.equal(state.watches[target].status, "stopped");
  assert.equal(state.watches[target].cycle, null);
  assert.equal(execute(state, "inbox", {}, at).value.length, 0, "nothing of the ended participant reaches a person");
  const before = state.seq;
  const again = execute(state, "agent.end", { id: target, actor: "human" }, at);
  assert.equal(again.changed, false);
  assert.equal(state.seq, before);
  execute(state, "tick", { runRetention: false }, "2026-09-26T02:00:00.000Z");
  assert.equal(Object.values(state.requests).filter((item) => item.status === "open").length, 0, "no cycle, reminder or escalation after end");
});

test("D-20: a resting target gets one check for the change, no cycle, reminder or escalation while it rests, and patrol resumes when it works", () => {
  const at = "2026-09-26T00:00:00.000Z";
  const { state, observer, target } = watchedLedger(at);
  const tick = (minutes) => execute(state, "tick", { runRetention: false }, new Date(Date.parse(at) + minutes * 60_000).toISOString());
  const cycles = () => Object.values(state.requests).filter((item) => item.intent.startsWith("watch:"));
  tick(1);
  execute(state, "watch.check", { target, cycle: state.watches[target].cycle, actor: observer }, at);
  assert.equal(cycles().length, 1, "a working target is patrolled");
  state.participants[target].runtime = "idle";
  tick(3);
  assert.equal(cycles().length, 2, "the change to idle is announced once");
  assert.notEqual(state.watches[target].quietSince, null);
  const transition = cycles().at(-1);
  for (let minute = 4; minute <= 120; minute += 1) tick(minute);
  assert.equal(cycles().length, 2, "no further cycle while the target rests");
  assert.equal(transition.remindedAt, null, "the open check is not reminded while the target rests");
  assert.equal(transition.escalatedAt, null);
  assert.equal(execute(state, "inbox", {}, at).value.length, 0);
  execute(state, "watch.check", { target, cycle: state.watches[target].cycle, actor: observer }, at);
  state.participants[target].runtime = "done";
  tick(130);
  assert.equal(cycles().length, 2, "idle to done is still resting");
  state.participants[target].runtime = "working";
  tick(140);
  assert.equal(state.watches[target].quietSince, null);
  assert.equal(cycles().length, 3, "patrol resumes once the target works again");
  assert.deepEqual(state.events.filter((entry) => entry.type === "watch.quiet" || entry.type === "watch.resumed").map((entry) => entry.type), ["watch.quiet", "watch.resumed"]);
});

test("D-21: every hcoord message is an identifier line, the command that closes it, and the carried content", () => {
  const at = "2026-09-26T00:00:00.000Z";
  const { state, observer, target } = watchedLedger(at);
  execute(state, "tick", { runRetention: false }, "2026-09-26T00:01:00.000Z");
  const watch = state.watches[target];
  const check = state.requests[watch.requestId];
  const peer = state.participants[target];
  assert.equal(messageForDelivery(check, check.deliveries[0], watch, peer), [
    `HCOORD_WATCH_CHECK target (${target}) cycle ${watch.cycle}`,
    `close: hcoord watch check ${target} --cycle ${watch.cycle} --actor ${observer}`,
    "read the digest",
    "-> done: end it",
  ].join("\n"));
  const asked = execute(state, "request.send", { from: target, to: observer, body: "which?", intent: "q", waiting: true }, at).value;
  const request = messageForDelivery(asked, asked.deliveries[0], null, peer).split("\n");
  assert.deepEqual(request, [`HCOORD_REQUEST ${asked.id} from target (${target})`, `reply: hcoord request reply ${asked.id} --as ${observer} --body <answer> | escalate: hcoord request escalate ${asked.id} --actor ${observer}`, "which?"]);
  const told = execute(state, "request.send", { from: target, to: observer, body: "plan ready", intent: "n", notifyOnly: true }, at).value;
  assert.deepEqual(messageForDelivery(told, told.deliveries[0], null, peer).split("\n"), [`HCOORD_NOTICE ${told.id} from target (${target})`, "no reply needed", "plan ready"]);
  execute(state, "request.reply", { id: asked.id, body: "B", respondent: observer, recordedBy: observer }, at);
  const answer = state.requests[asked.id].deliveries.at(-1);
  assert.deepEqual(messageForDelivery(state.requests[asked.id], answer, null).split("\n"), [`HCOORD_ANSWER ${asked.id}`, `ack: hcoord request ack ${asked.id} --actor ${target} --delivery ${answer.id}`, "answer: B"]);
});

// Older ledgers kept a table of Sasu runs; the loader keeps only fields it knows, so any such table is dropped.
test("B24: a ledger field this version does not know, such as an old table of one client's runs, loads away and the next save drops it", (t) => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "hcoord-old-ledger-"));
  t.after(() => fs.rmSync(home, { recursive: true, force: true }));
  const previous = process.env.HCOORD_HOME;
  process.env.HCOORD_HOME = path.join(home, "data");
  t.after(() => { if (previous === undefined) delete process.env.HCOORD_HOME; else process.env.HCOORD_HOME = previous; });
  const old = { ...emptyLedger("2026-09-26T00:00:00.000Z"), clientRuns: { "run-1": { observer: "a_1", implementor: "a_2" } } };
  fs.mkdirSync(path.join(home, "data"), { recursive: true });
  fs.writeFileSync(path.join(home, "data", "ledger.json"), JSON.stringify(old));
  const loaded = loadLedger();
  assert.equal(Object.hasOwn(loaded, "clientRuns"), false);
  saveLedger(loaded);
  assert.equal(Object.hasOwn(JSON.parse(fs.readFileSync(path.join(home, "data", "ledger.json"), "utf8")), "clientRuns"), false);
  assert.deepEqual(Object.keys(execute(loaded, "status", {}, "2026-09-26T00:00:00.000Z").value.counts).sort(), ["agents", "events", "requests", "watches"]);
});
