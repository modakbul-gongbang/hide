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

// The parsed screen can contain explicit space cells through the right edge,
// even when xterm trims empty cells. Ignore only that row padding, not prefixes
// or embedded text: echoed commands and stale runs cannot be output evidence.
function observePane({ pane_id, marker }, diagnostic = false) {
  const text = window.__hideProbe.paneText(pane_id);
  let sequence = -1;
  let retired = false;
  for (const match of text.matchAll(new RegExp(`^${marker}([0-9]+|end) *$`, "gm"))) {
    if (match[1] === "end") retired = true;
    else {
      const value = Number(match[1]);
      if (Number.isSafeInteger(value)) sequence = Math.max(sequence, value);
    }
  }
  return { pane_id, marker, sequence, retired, ...(diagnostic ? { tail: text.slice(-4096) } : {}) };
}

// Runs in the measured page. Buffer reads happen at most twice a second.
// Observer retention is six panes * three phases * two progress records.
function collect({ windowMs, panes, owner }, observePane) {
  if (window.__hideFrames) throw new Error("frame collector already active");
  const state = window.__hideFrames = {
    owner, frames: [], done: false, error: null, window: null,
    workload_evidence: { method: "pane_sequence_progress_v1", sample_interval_ms: 500, panes: [] },
    cancel: () => cancelAnimationFrame(frame),
  };
  const readers = panes.map(({ pane_id, marker }) => {
    const evidence = { pane_id, marker, phases: Array.from({ length: 3 }, () => ({ first: null, last: null, observations: 0 })) };
    state.workload_evidence.panes.push(evidence);
    return { evidence, sequence: -1 };
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
            const { sequence } = observePane(reader.evidence);
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
const result = {
  frames: [], window: null, done: false, error: null, workload_evidence: null,
  driver_launch_complete: false, driver_cleanup_complete: false,
  driven_panes: drivenPanes, writer_launches: [], pane_diagnostics: {},
  workload: "one line per 8 ms per shown pane; parsed sequence progress sampled every 500 ms",
};
const started = [];
const errors = [];
const paneObservations = () => page.evaluate(`(${JSON.stringify(started)}).map(pane => (${observePane.toString()})(pane, true))`);
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
  result.writer_launches = launches.map((launch, index) => ({
    pane_id: panes[index].pane_id, accepted: launch.status === "fulfilled",
    ...(launch.status === "rejected" ? { error: String(launch.reason).slice(0, 2048) } : {}),
  }));
  for (const launch of launches) if (launch.status === "rejected") errors.push(launch.reason);
  if (errors.length) throw new Error("frame writer launch incomplete");
  result.driver_launch_complete = true;
  const arrivalsBefore = await page.evaluate("window.__hideProbe.arrivals()");
  await page.evaluate(`(${collect.toString()})(${JSON.stringify({ windowMs, panes, owner: runId })}, ${observePane.toString()})`);
  const deadline = Date.now() + windowMs + 10_000;
  while (Date.now() < deadline) {
    interrupted.signal.throwIfAborted();
    if (await page.evaluate("window.__hideFrames.done || Boolean(window.__hideFrames.error)")) break;
    await delay(500, undefined, { signal: interrupted.signal });
  }
  result.ws_frames_during_run = await page.evaluate("window.__hideProbe.arrivals()") - arrivalsBefore;
} catch (error) {
  errors.push(error);
} finally {
  const cleanupErrorsBefore = errors.length;
  try {
    // Capture before deleting, including interruption and collector failures.
    if (page) {
      const observed = await page.evaluate(`(() => {
        const state = window.__hideFrames;
        if (state?.owner !== ${JSON.stringify(runId)}) return null;
        state.cancel();
        const { frames, window: span, done, error, workload_evidence } = state;
        delete window.__hideFrames;
        return { frames, window: span, done, error, workload_evidence };
      })()`);
      if (observed) Object.assign(result, observed);
      if (started.length) result.pane_diagnostics.before_stop = await paneObservations();
    }
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
        result.pane_diagnostics.after_stop = await paneObservations();
        retired = result.pane_diagnostics.after_stop.every(pane => pane.retired);
        if (retired) break;
        await delay(100);
      }
      if (!retired) throw new Error("frame writer retirement unconfirmed; enclosing private-server owner must stop its panes");
    }
  } catch (error) { errors.push(error); }
  result.driver_cleanup_complete = errors.length === cleanupErrorsBefore;
  if (!result.done || result.error) errors.push(new Error(result.error || "frame observation window incomplete"));
  if (errors.length && started.length) {
    // One bounded native tail per attempted pane, after the measured window.
    // This separates writer output from a missing/delayed browser observation.
    result.pane_diagnostics.native_after_stop = await Promise.all(started.map(async ({ pane_id }) => {
      try {
        const { stdout } = await herdr(["pane", "read", pane_id, "--source", "recent-unwrapped", "--lines", "20", "--format", "text"]);
        return { pane_id, tail: stdout.slice(-4096) };
      } catch (error) { return { pane_id, error: String(error).slice(0, 2048) }; }
    }));
  }
  page?.close();
  process.off("SIGINT", interrupt);
  process.off("SIGTERM", interrupt);
}
if (errors.length) result.error = errors.map(error => String(error).slice(0, 2048)).join("; ");
// stdout stays a measurement document even on failure; the nonzero exit and
// error/cleanup fields prevent retained observations from becoming a PASS.
console.log(JSON.stringify(result));
if (errors.length) {
  console.error(result.error);
  process.exitCode = 1;
}
