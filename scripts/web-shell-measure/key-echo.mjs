#!/usr/bin/env node
// Screen key echo (PRD core-host-node-terminal D-07): a key typed into the
// web shell's terminal, through the shell's own key path, until the pane's
// echo of it is drawn. `echo.mjs` puts bytes into the pane with `herdr pane
// send-text` and so times only the output path; this one types.
//
// Each sample is one key: the marker `aNNNN` is typed first and waited for,
// then the last key `z` is typed and timed. t0 = the page's capture-phase
// keydown for `z`, t1 = the xterm write completion that first shows `aNNNNz`
// in the parsed buffer (window.__hideProbe), both on the page's clock; the
// CDP dispatch hop before the keydown, the same for every build, is left
// out. This process's clock is a separate one whose offset from the page's
// differs per run, so its t0 is kept only as a cross-check beside the
// offset a bracketed round trip measures. The pane runs
// `stty -echo -icanon; cat`, so the drawn character is the pane's echo, not
// the terminal's local one. Prints JSON samples; summarize.py reads them.
import { performance } from "node:perf_hooks";
import { spawnSync } from "node:child_process";
import { connectPage } from "./cdp.mjs";

const cdpPort = process.env.MEASURE_CDP_PORT;
const repeats = Number(process.env.MEASURE_ECHO_REPEATS ?? 50);
const paneId = process.env.MEASURE_PANE_ID;
if (!cdpPort || !paneId) throw new Error("MEASURE_CDP_PORT and MEASURE_PANE_ID are required");

const page = await connectPage(cdpPort);
const probePane = await page.evaluate("window.__hideProbe && window.__hideProbe.paneId()");
if (probePane !== paneId) throw new Error(`page shows pane ${probePane}, fixture pane is ${paneId}`);
// Keys go wherever the page's keyboard is: the measured pane's terminal.
const focused = await page.evaluate(`(() => {
  const view = document.querySelector(${JSON.stringify(`[data-pane-view="${paneId}"]`)});
  const input = view?.querySelector("textarea");
  input?.focus();
  return document.activeElement === input && Boolean(input);
})()`);
if (!focused) throw new Error(`the terminal of ${paneId} did not take keyboard focus`);
// The timed key's keydown on the page's clock, before the shell's own
// handlers see it (capture phase on window).
await page.evaluate(`(() => {
  window.__hideKeyEcho = { keydown_ms: null };
  window.addEventListener("keydown", () => {
    window.__hideKeyEcho.keydown_ms = performance.timeOrigin + performance.now();
  }, true);
  return true;
})()`);

const now = () => performance.timeOrigin + performance.now();
const key = (text) =>
  page.send("Input.dispatchKeyEvent", { type: "keyDown", text, unmodifiedText: text, key: text });
const enter = () =>
  page.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, text: "\r" });

// Every /ws frame the page receives while keys are typed, by type: a key
// that made the core publish shows up as a delta frame (B5).
const frames = {};
page.on("Network.webSocketFrameReceived", ({ response }) => {
  const type = /"type":"([a-z_]+)"/.exec(response?.payloadData ?? "")?.[1] ?? "binary";
  frames[type] = (frames[type] ?? 0) + 1;
});
await page.send("Network.enable");

const load = spawnSync("uptime", { encoding: "utf8" }).stdout.trim();
const samples = [];
const node_t0_samples = [];
const hops = [];
for (let i = 0; i < repeats; i += 1) {
  const prefix = `a${String(i).padStart(4, "0")}`;
  await page.evaluate(`window.__hideProbe.arm(${JSON.stringify(prefix)}); "armed"`);
  for (const character of prefix) await key(character);
  await page.evaluate("window.__hideProbe.waitArmed()", true);
  const marker = `${prefix}z`;
  await page.evaluate(`window.__hideProbe.arm(${JSON.stringify(marker)}); window.__hideKeyEcho.keydown_ms = null; "armed"`);
  // This process's clock minus the page's, read between two of its own.
  const before = now();
  const page_ms = await page.evaluate("performance.timeOrigin + performance.now()");
  const after = now();
  const clock_offset_ms = (before + after) / 2 - page_ms;
  const t0 = now();
  await key("z");
  const sent_ms = now();
  const sample = await page.evaluate(
    "window.__hideProbe.waitArmed().then((s) => ({ ...s, keydown_ms: window.__hideKeyEcho.keydown_ms }))",
    true,
  );
  if (sample.keydown_ms === null) throw new Error(`no keydown reached the page for ${marker}`);
  const value = sample.write_ms - sample.keydown_ms;
  if (value < 0) throw new Error(`${marker} was drawn ${-value} ms before its keydown on one clock`);
  hops.push({ sample: i, marker, t0_ms: t0, sent_ms, clock_offset_ms, clock_round_trip_ms: after - before, ...sample });
  samples.push(value);
  node_t0_samples.push(sample.write_ms - t0);
  await enter();
  await new Promise((resolve) => setTimeout(resolve, 80));
}
page.close();
console.log(JSON.stringify({
  method: "t0 = the page's capture-phase keydown for the marker's last key; t1 = xterm write completion with the marker in the parsed buffer; both on the page's clock, the CDP dispatch hop excluded; node_t0_samples = t1 minus this process's clock before the CDP keyDown, a cross-check across two clocks (clock_offset_ms per hop = this process minus the page, by a bracketed round trip); the pane runs stty -echo -icanon; cat; one timed key per sample, its four preceding keys typed and drawn first; nearest-rank percentiles computed by summarize.py",
  load, pane_id: paneId, keys_typed: repeats * 7, ws_frames_by_type: frames, samples, node_t0_samples, hops,
}, null, 2));
