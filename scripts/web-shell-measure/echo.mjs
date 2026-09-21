#!/usr/bin/env node
// Echo latency on the product web shell, the S0 definition:
// t0 = `herdr pane send-text` CLI return, t1 = xterm write completion with
// the marker in the parsed buffer (window.__hideProbe). Prints JSON samples.
import { performance } from "node:perf_hooks";
import { spawnSync } from "node:child_process";
import { connectPage } from "./cdp.mjs";

const cdpPort = process.env.MEASURE_CDP_PORT ?? "9333";
const repeats = Number(process.env.MEASURE_ECHO_REPEATS ?? 50);
const paneId = process.env.MEASURE_PANE_ID;
const herdrBin = process.env.HERDR_BIN_PATH;
if (!paneId || !herdrBin) throw new Error("MEASURE_PANE_ID and HERDR_BIN_PATH are required");

const page = await connectPage(cdpPort);
const probePane = await page.evaluate("window.__hideProbe && window.__hideProbe.paneId()");
if (probePane !== paneId) throw new Error(`page shows pane ${probePane}, fixture pane is ${paneId}`);
const load = spawnSync("uptime", { encoding: "utf8" }).stdout.trim();
const samples = [];
const hops = [];
for (let i = 0; i < repeats; i += 1) {
  const marker = `s${String(i).padStart(4, "0")}`;
  await page.evaluate(`window.__hideProbe.arm(${JSON.stringify(marker)}); "armed"`);
  const t0 = performance.timeOrigin + performance.now();
  const sent = spawnSync(herdrBin, ["pane", "send-text", paneId, `${marker}\n`], {
    env: process.env, encoding: "utf8", timeout: 3000,
  });
  const cli_return_ms = performance.timeOrigin + performance.now();
  if (sent.status !== 0) throw new Error(`send-text failed: ${sent.stderr}`);
  const sample = await page.evaluate("window.__hideProbe.waitArmed()", true);
  hops.push({ sample: i, marker, t0_ms: t0, cli_return_ms, ...sample });
  samples.push(sample.write_ms - cli_return_ms);
  await new Promise((resolve) => setTimeout(resolve, 80));
}
page.close();
console.log(JSON.stringify({
  method: "t0 = send-text CLI return; t1 = xterm write completion with the marker in the parsed buffer; arrival_ms is the WS message event before JSON decoding; identical sNNNN + LF bytes; nearest-rank percentiles computed by summarize.py",
  load, pane_id: paneId, samples, hops,
}, null, 2));
