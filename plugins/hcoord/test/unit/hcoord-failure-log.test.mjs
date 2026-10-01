import assert from "node:assert/strict";
import test from "node:test";
import { createFailureLog } from "../../dist/hcoord/failure-log.js";

const WINDOW = 10 * 60_000;
function harness() {
  let clock = Date.parse("2026-10-01T00:00:00.000Z");
  const lines = [];
  const log = createFailureLog({ failed: "x.failed", recovered: "x.recovered" }, (line) => lines.push(JSON.parse(line)), WINDOW, () => clock);
  return { log, lines, advance: (ms) => { clock += ms; } };
}

test("a failure that repeats on one machine logs once per window with the count it folded", () => {
  const { log, lines, advance } = harness();
  for (let attempt = 0; attempt < 120; attempt += 1) { log.failed("mini", "auth_failed", { machine: "mini", code: "auth_failed" }); advance(30_000); }
  assert.equal(lines.length, 6, "an hour of 30 s retries logs six lines, not 120");
  assert.equal(lines[0].repeated, undefined, "the first failure logs at once");
  assert.equal(lines[1].repeated, 19, "a later line says how many repeats it stands for");
});

test("a different cause or machine logs immediately and recovery reports what was folded", () => {
  const { log, lines, advance } = harness();
  assert.equal(log.failed("mini", "auth_failed", { machine: "mini" }), true);
  assert.equal(log.failed("mini", "auth_failed", { machine: "mini" }), false);
  assert.equal(log.failed("mini", "machine_unreachable", { machine: "mini" }), true, "a changed cause is news");
  assert.equal(log.failed("studio", "machine_unreachable", { machine: "studio" }), true, "another machine has its own episode");
  advance(1000);
  assert.equal(log.failed("studio", "machine_unreachable", { machine: "studio" }), false);
  assert.equal(log.recovered("studio", { machine: "studio" }), true);
  assert.equal(lines.at(-1).event, "x.recovered");
  assert.equal(lines.at(-1).repeated, 1);
  assert.equal(log.recovered("mini", { machine: "mini" }), false, "nothing was folded, so recovery stays quiet");
  assert.equal(log.failed("mini", "machine_unreachable", { machine: "mini" }), false, "a success does not end the window, so a flapping machine stays one episode");
  advance(WINDOW);
  assert.equal(log.failed("mini", "machine_unreachable", { machine: "mini" }), true, "the next window logs again");
});
