#!/usr/bin/env node
// Shared echo definition: t0 = send-text CLI return, t1 = xterm write completion.
import { performance } from "node:perf_hooks";
import { spawnSync } from "node:child_process";

const cdpPort = process.env.S0_CDP_PORT ?? "9222";
const repeats = Number(process.env.S0_ECHO_REPEATS ?? 50);
const paneId = process.env.S0_PANE_ID;
const herdrBin = process.env.HERDR_BIN ?? "herdr";
if (!paneId) {
  console.error("echo-web: S0_PANE_ID is required");
  process.exit(2);
}

const targets = await fetch(`http://127.0.0.1:${cdpPort}/json/list`).then((r) => r.json());
const page = targets.find(
  (target) => target.type === "page" && String(target.url).includes("5173"),
);
if (!page) {
  console.error("echo-web: no Chrome tab on :5173", targets.map((t) => t.url));
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve);
  ws.addEventListener("error", reject);
});

let nextId = 1;
const pending = new Map();
ws.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
  const waiter = pending.get(message.id);
  if (waiter) {
    pending.delete(message.id);
    if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
    else waiter.resolve(message.result);
  }
});

function send(method, params) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

await send("Runtime.enable", {});
const samples = [];
const hops = [];
const load = spawnSync("uptime", { encoding: "utf8" }).stdout.trim();
for (let i = 0; i < repeats; i += 1) {
  const marker = `s${String(i).padStart(4, "0")}`;
  await send("Runtime.evaluate", {
    expression: `window.__s0Arm(${JSON.stringify(marker)}); "armed"`,
    returnByValue: true,
  });
  const t0 = performance.timeOrigin + performance.now();
  const sent = spawnSync(herdrBin, ["pane", "send-text", paneId, `${marker}\n`], {
    env: process.env, encoding: "utf8", timeout: 3000,
  });
  const cli_return_ms = performance.timeOrigin + performance.now();
  if (sent.status !== 0) throw new Error(`send-text failed: ${sent.stderr}`);
  const result = await send("Runtime.evaluate", {
    expression: "window.__s0WaitFor()", awaitPromise: true, returnByValue: true,
  });
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  const hop = { sample: i, t0_ms: t0, cli_return_ms, ...result.result.value };
  samples.push(hop.write_ms - cli_return_ms);
  hops.push(hop);
  await new Promise((resolve) => setTimeout(resolve, 80));
}

ws.close();
const out = { method: "send-text CLI return -> xterm write callback; t0_ms in hops retains pre-spawn start for overhead; WS message timestamp recorded before parsing; identical sNNNN + LF bytes", load, samples, hops };
console.log(JSON.stringify(out, null, 2));
