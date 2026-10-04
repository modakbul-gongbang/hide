import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { messageForDelivery } from "../../dist/hcoord/herdr.js";

// The header Hide reads the sender from (hide-session/src/envelope.rs); both
// sides test against the same examples (PRD overview-request-view D-19).
const contract = JSON.parse(readFileSync(new URL("../../../../contracts/hcoord-envelope.json", import.meta.url), "utf8"));

const request = (patch) => ({ id: "r_1a2b", intent: "i", from: "p_9f", to: "p_2", intermediary: null, body: "Please look", context: null, status: "open", requiresReply: true, waiting: false, createdAt: "2026-10-03T00:00:00.000Z", answeredAt: null, answer: "done", respondent: "p_2", recordedBy: null, canceledAt: null, lateAnswers: [], relayBody: "relayed", relayAt: null, escalatedAt: null, remindedAt: null, deliveries: [], ...patch });
const delivery = (phase) => ({ id: "d_1", requestId: "r_1a2b", recipient: "p_2", status: "reserved", reason: null, phase, reservedAt: "2026-10-03T00:00:00.000Z", attemptedAt: null, acceptedAt: null, acknowledgedAt: null, runtimeCode: null });
const peer = (name, id) => ({ id, machine: "local", hostScope: "default", session: "s", instance: "i", name, project: null, parent: null, pane: null, runtime: "idle", connection: "connected", observedAt: "2026-10-03T00:00:00.000Z" });

const written = {
  "HCOORD_REQUEST r_1a2b from observer (p_9f)": () => messageForDelivery(request({}), delivery("request"), null, peer("observer", "p_9f")),
  "HCOORD_REQUEST r_1a2b from p_9f": () => messageForDelivery(request({}), delivery("request"), null),
  "HCOORD_NOTICE r_1a2b from ci-lead (p_77)": () => messageForDelivery(request({ from: "p_77", requiresReply: false }), delivery("request"), null, peer("ci-lead", "p_77")),
  "HCOORD_WATCH_CHECK implementor (p_42) cycle 3": () => messageForDelivery(request({ from: "p_42" }), delivery("watch_check"), { cycle: 3, brief: null }, peer("implementor", "p_42")),
  "HCOORD_ANSWER r_1a2b": () => messageForDelivery(request({}), delivery("answer"), null),
  "HCOORD_RELAY r_1a2b": () => messageForDelivery(request({}), delivery("relay"), null),
  "HCOORD_RELAY_PROBLEM r_1a2b": () => messageForDelivery(request({}), delivery("relay_problem"), null),
  "HCOORD_DELIVERY_PROBLEM r_1a2b child (p_3) has not acknowledged the relay": () => messageForDelivery(request({ from: "p_3" }), delivery("delivery_problem"), null, peer("child", "p_3")),
};

test("every message hcoord delivers starts with a header the contract lists", () => {
  for (const example of contract.examples) {
    const write = written[example.first_line];
    assert.ok(write, `no writer case for ${example.first_line}`);
    assert.equal(write().split("\n")[0], example.first_line);
    assert.ok(example.first_line.startsWith(`${example.kind} `) || example.first_line === example.kind);
  }
  assert.equal(Object.keys(written).length, contract.examples.length);
});
