#!/usr/bin/env node
// Driven frame callbacks, with per-pane sequence progress observed through the
// existing parsed-buffer probe. Neither CLI success nor WS traffic proves load.
import { execFile } from "node:child_process";
import { randomBytes } from "node:crypto";
import { promisify } from "node:util";
import { setTimeout as delay } from "node:timers/promises";
import { connectPage } from "./cdp.mjs";

const cdpPort = process.env.MEASURE_CDP_PORT;
const windowMs = Number(process.env.MEASURE_FRAME_WINDOW_MS ?? 120_000);
const paneId = process.env.MEASURE_PANE_ID;
const herdrBin = process.env.HERDR_BIN_PATH;
const drivenPanes = process.env.MEASURE_DRIVEN_PANES?.split(",") ?? [paneId];
if (!cdpPort || !paneId || !herdrBin) throw new Error("MEASURE_CDP_PORT, MEASURE_PANE_ID and HERDR_BIN_PATH are required");
// Ten minutes is the runner's longest supported window (memory-series mode).
if (!Number.isSafeInteger(windowMs) || windowMs < 1 || windowMs > 600_000) throw new Error("Invalid frame window: expected 1..600000 ms");
if (!drivenPanes.includes(paneId) || drivenPanes.length > 6 || drivenPanes.some((id) => !id) || new Set(drivenPanes).size !== drivenPanes.length) throw new Error("Invalid driven pane set");

const runId = randomBytes(8).toString("hex");
const panes = drivenPanes.map((pane_id, index) => ({ pane_id, marker: `f${runId}${index}:` }));
const interrupted = new AbortController();
const interrupt = () => interrupted.abort();
process.once("SIGINT", interrupt);
process.once("SIGTERM", interrupt);
const execute = promisify(execFile);
// At most six short CLI children at once, killed on timeout or interruption.
const herdr = (args, signal) => execute(herdrBin, args, { env: process.env, timeout: 10_000, killSignal: "SIGKILL", maxBuffer: 64 * 1024, signal });
const quote = (value) => `'${value.replaceAll("'", "'\\''")}'`;

// Runs in the measured page. Buffer reads happen at most twice a second.
// Observer retention is six panes * three phases * two progress records.
function collect({ windowMs, panes, owner }) {
  if (window.__hideFrames) throw new Error("frame collector already active");
  const state = window.__hideFrames = {
    owner, frames: [], done: false, error: null, window: null,
    workload_evidence: { method: "pane_sequence_progress_v1", sample_interval_ms: 500, panes: [] },
    cancel: () => cancelAnimationFrame(frame),
  };
  const readers = panes.map(({ pane_id, marker }) => {
    const evidence = { pane_id, marker, phases: Array.from({ length: 3 }, () => ({ first: null, last: null, observations: 0 })) };
    state.workload_evidence.panes.push(evidence);
    return { evidence, pattern: new RegExp(`^${marker}([0-9]+)$`, "gm"), sequence: -1 };
  });
  let frame = requestAnimationFrame((origin) => {
    let last = origin;
    let observedAt = -Infinity;
    state.window = { start: origin, end: null, duration_ms: null, requested_ms: windowMs };
    const onFrame = (t) => {
      try {
        if (state.frames.length >= 360_000) throw new Error("frame observation budget exceeded");
        state.frames.push({ t, dt: t - last });
        last = t;
        if (t - observedAt >= 500) {
          observedAt = t;
          const elapsed = t - origin;
          // Progress in each third is coverage, not a per-key latency limit.
          // An unobserved phase cannot earn a driven PASS.
          const phase = Math.min(2, Math.floor(elapsed / (windowMs / 3)));
          for (const reader of readers) {
            let sequence = -1;
            for (const match of window.__hideProbe.paneText(reader.evidence.pane_id).matchAll(reader.pattern)) {
              const value = Number(match[1]);
              if (Number.isSafeInteger(value)) sequence = Math.max(sequence, value);
            }
            if (sequence <= reader.sequence) continue;
            reader.sequence = sequence;
            const progress = reader.evidence.phases[phase];
            const observation = { at_ms: elapsed, sequence };
            progress.first ??= observation;
            progress.last = observation;
            progress.observations += 1;
          }
        }
        if (t - origin >= windowMs) {
          state.window.end = t;
          state.window.duration_ms = t - origin;
          state.done = true;
        } else frame = requestAnimationFrame(onFrame);
      } catch (error) {
        state.error = String(error);
      }
    };
    frame = requestAnimationFrame(onFrame);
  });
  return true;
}

let page;
let result;
const started = [];
const errors = [];
try {
  page = await connectPage(cdpPort);
  interrupted.signal.throwIfAborted();
  if (await page.evaluate("Boolean(window.__hideFrames)")) throw new Error("frame collector already active");
  // No cat in this output-only workload: Ctrl-C ends foreground Python itself.
  // A unique final marker acknowledges that the writer left its loop.
  const launches = await Promise.allSettled(panes.map(async (pane) => {
    started.push(pane);
    const source = [
      "import time", `end=time.monotonic()+${Math.ceil(windowMs / 1000) + 30}`, "i=0", "try:",
      " while time.monotonic()<end:", `  print(f\"${pane.marker}{i}\",flush=True); i+=1; time.sleep(0.008)`,
      "except KeyboardInterrupt:", " pass", "finally:", ` print(\"${pane.marker}end\",flush=True)`,
    ].join("\n");
    await herdr(["pane", "run", pane.pane_id, `stty -echo -icanon; /usr/bin/python3 -c ${quote(source)}`], interrupted.signal);
  }));
  for (const launch of launches) if (launch.status === "rejected") throw launch.reason;
  const arrivalsBefore = await page.evaluate("window.__hideProbe.arrivals()");
  await page.evaluate(`(${collect.toString()})(${JSON.stringify({ windowMs, panes, owner: runId })})`);
  const deadline = Date.now() + windowMs + 10_000;
  while (Date.now() < deadline) {
    interrupted.signal.throwIfAborted();
    if (await page.evaluate("window.__hideFrames.done || Boolean(window.__hideFrames.error)")) break;
    await delay(500, undefined, { signal: interrupted.signal });
  }
  result = await page.evaluate(`(() => {
    const { frames, window: span, done, error, workload_evidence } = window.__hideFrames;
    return { frames, window: span, done, error, workload_evidence };
  })()`);
  result.ws_frames_during_run = await page.evaluate("window.__hideProbe.arrivals()") - arrivalsBefore;
  result.driver_launch_complete = true;
  result.driven_panes = drivenPanes;
  result.workload = "one line per 8 ms per shown pane; parsed sequence progress sampled every 500 ms";
} catch (error) {
  errors.push(error);
} finally {
  try {
    if (page) await page.evaluate(`if (window.__hideFrames?.owner === ${JSON.stringify(runId)}) { window.__hideFrames.cancel(); delete window.__hideFrames; }`);
  } catch (error) { errors.push(error); }
  // Stop every attempted pane after a partial launch. Cleanup is independently
  // bounded and is not cancelled by the signal that interrupted measurement.
  const stops = await Promise.allSettled(started.map((pane) => herdr(["pane", "send-keys", pane.pane_id, "ctrl+c"])));
  for (const stop of stops) if (stop.status === "rejected") errors.push(stop.reason);
  try {
    if (page && started.length) {
      const deadline = Date.now() + 10_000;
      let retired = false;
      while (Date.now() < deadline) {
        retired = await page.evaluate(`(${JSON.stringify(started)}).every(({pane_id, marker}) => window.__hideProbe.paneText(pane_id).split("\\n").includes(marker + "end"))`);
        if (retired) break;
        await delay(100);
      }
      if (!retired) throw new Error("frame writer retirement unconfirmed; enclosing private-server owner must stop its panes");
    }
  } catch (error) { errors.push(error); }
  page?.close();
  process.off("SIGINT", interrupt);
  process.off("SIGTERM", interrupt);
}
if (errors.length) throw new AggregateError(errors, "frame measurement or cleanup failed");
result.driver_cleanup_complete = true;
console.log(JSON.stringify(result));
