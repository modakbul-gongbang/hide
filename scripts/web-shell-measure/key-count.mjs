#!/usr/bin/env node
// Lost and reordered keys (PRD core-host-node-terminal D-08): types
// MEASURE_KEY_COUNT distinct markers (10,000 by default) into the web
// shell's terminal through the shell's own key path, then reads the pane's
// content back from its isolated Herdr and counts the markers that never
// arrived and the ones that arrived out of order. It does not stop at the
// first loss: every count is reported.
//
// Each marker is one key event (`mNNNNN` and a space, a carriage return
// every 100th), sent as CDP `Input.insertText`, which reaches xterm as one
// input and the shell as one `key` frame, in order. The pane runs
// `stty -echo -icanon; cat`, so what Herdr holds is what the pane received.
// MEASURE_PANE_READ is the shell command that prints the pane's logical
// lines (default: `$HERDR_BIN_PATH pane read $MEASURE_PANE_ID --source
// recent-unwrapped --lines 4000` on the private socket); for a device pane
// it reads the device's isolated Herdr over SSH.
import { performance } from "node:perf_hooks";
import { spawnSync } from "node:child_process";
import { connectPage } from "./cdp.mjs";

const cdpPort = process.env.MEASURE_CDP_PORT;
const paneId = process.env.MEASURE_PANE_ID;
const screenPane = process.env.MEASURE_SCREEN_PANE_ID ?? paneId;
const count = Number(process.env.MEASURE_KEY_COUNT ?? 10_000);
const intervalMs = Number(process.env.MEASURE_KEY_INTERVAL_MS ?? 0);
const perLine = 100;
if (!cdpPort || !paneId) throw new Error("MEASURE_CDP_PORT and MEASURE_PANE_ID are required");
if (!(count > 0 && count <= 99_999)) throw new Error("MEASURE_KEY_COUNT must be 1..99999");
const readCommand =
  process.env.MEASURE_PANE_READ ??
  `"${process.env.HERDR_BIN_PATH}" pane read "${paneId}" --source recent-unwrapped --lines 4000`;

const marker = (index) => `m${String(index).padStart(5, "0")}`;
const readPane = () => {
  const read = spawnSync("/bin/sh", ["-c", readCommand], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: 60_000 });
  if (read.status !== 0) throw new Error(`pane read failed (${read.status}): ${read.stderr}`);
  return read.stdout;
};

const page = await connectPage(cdpPort);
const focused = await page.evaluate(`(() => {
  const view = document.querySelector(${JSON.stringify(`[data-pane-view="${screenPane}"]`)});
  const input = view?.querySelector("textarea");
  input?.focus();
  return document.activeElement === input && Boolean(input);
})()`);
if (!focused) throw new Error(`the terminal of ${screenPane} did not take keyboard focus`);
if (/m\d{5}/.test(readPane())) throw new Error("the pane already shows markers; reset it first");

const load_before = spawnSync("uptime", { encoding: "utf8" }).stdout.trim();
const started = performance.now();
for (let index = 0; index < count; index += 1) {
  const text = `${marker(index)}${(index + 1) % perLine === 0 || index + 1 === count ? "\r" : " "}`;
  await page.send("Input.insertText", { text });
  if (intervalMs > 0) await new Promise((resolve) => setTimeout(resolve, intervalMs));
}
const typed_ms = performance.now() - started;
page.close();

// Wait for the last marker to land, then read once more after a quiet spell.
const last = marker(count - 1);
const deadline = Date.now() + 60_000;
let content = readPane();
while (!content.includes(last) && Date.now() < deadline) {
  await new Promise((resolve) => setTimeout(resolve, 500));
  content = readPane();
}
await new Promise((resolve) => setTimeout(resolve, 2000));
content = readPane();

const seen = [...content.matchAll(/m(\d{5})/g)].map((match) => Number(match[1]));
const firstSeen = new Map();
let duplicated = 0;
for (const [position, index] of seen.entries()) {
  if (firstSeen.has(index)) duplicated += 1;
  else firstSeen.set(index, position);
}
const missing = [];
for (let index = 0; index < count; index += 1) if (!firstSeen.has(index)) missing.push(index);
// A marker is out of order when a later one was seen before it: count the
// adjacent inversions in what arrived.
let reordered = 0;
const order = seen.filter((index, position) => firstSeen.get(index) === position);
for (let position = 1; position < order.length; position += 1) if (order[position] < order[position - 1]) reordered += 1;
const unexpected = seen.filter((index) => index >= count).length;
console.log(JSON.stringify({
  method: "one CDP Input.insertText per marker into the measured pane's terminal; pane runs stty -echo -icanon; cat; content read back from the pane's own isolated Herdr; missing = markers never seen; reordered = adjacent inversions among first sightings",
  pane_id: paneId,
  screen_pane_id: screenPane,
  typed: count,
  interval_ms: intervalMs,
  typed_ms: Math.round(typed_ms),
  seen: firstSeen.size,
  missing: missing.length,
  missing_first: missing.slice(0, 20),
  reordered,
  duplicated,
  unexpected,
  load_before,
  load_after: spawnSync("uptime", { encoding: "utf8" }).stdout.trim(),
}, null, 2));
