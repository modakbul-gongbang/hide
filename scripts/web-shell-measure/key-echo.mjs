#!/usr/bin/env node
// Screen key echo (PRD core-host-node-terminal D-07): a key typed into the
// web shell's terminal, through the shell's own key path, until the pane's
// echo of it is drawn. `echo.mjs` puts bytes into the pane with `herdr pane
// send-text` and so times only the output path; this one types.
//
// Each sample is one key: the marker `aNNNN` is typed first and waited for,
// then the last key `z` is typed and timed. t0 = just before the CDP key
// event for `z` is sent, t1 = the xterm write completion that first shows
// `aNNNNz` in the parsed buffer (window.__hideProbe). The pane runs
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
const hops = [];
for (let i = 0; i < repeats; i += 1) {
  const prefix = `a${String(i).padStart(4, "0")}`;
  await page.evaluate(`window.__hideProbe.arm(${JSON.stringify(prefix)}); "armed"`);
  for (const character of prefix) await key(character);
  await page.evaluate("window.__hideProbe.waitArmed()", true);
  const marker = `${prefix}z`;
  await page.evaluate(`window.__hideProbe.arm(${JSON.stringify(marker)}); "armed"`);
  const t0 = now();
  await key("z");
  const sent_ms = now();
  const sample = await page.evaluate("window.__hideProbe.waitArmed()", true);
  hops.push({ sample: i, marker, t0_ms: t0, sent_ms, ...sample });
  samples.push(sample.write_ms - t0);
  await enter();
  await new Promise((resolve) => setTimeout(resolve, 80));
}
page.close();
console.log(JSON.stringify({
  method: "t0 = before the CDP keyDown for the marker's last key; t1 = xterm write completion with the marker in the parsed buffer; the pane runs stty -echo -icanon; cat; one timed key per sample, its four preceding keys typed and drawn first; nearest-rank percentiles computed by summarize.py",
  load, pane_id: paneId, keys_typed: repeats * 7, ws_frames_by_type: frames, samples, hops,
}, null, 2));
