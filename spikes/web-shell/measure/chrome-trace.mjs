#!/usr/bin/env node
import fs from "node:fs";

const cdpPort = process.env.S0_CDP_PORT ?? "9222";
const out = process.argv[2];
if (!out) {
  console.error("usage: chrome-trace.mjs <out.json>");
  process.exit(2);
}
const targets = await fetch(`http://127.0.0.1:${cdpPort}/json/list`).then((r) => r.json());
const page = targets.find(
  (target) => target.type === "page" && String(target.url).includes("5173"),
);
if (!page) {
  console.error("chrome-trace: no tab on :5173");
  process.exit(1);
}
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve);
  ws.addEventListener("error", reject);
});
let id = 1;
const pending = new Map();
const chunks = [];
ws.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
  if (message.method === "Tracing.dataCollected" && message.params?.value) {
    if (chunks.length + message.params.value.length > 2000000) {
      console.error("trace event budget exceeded (2000000)");
      ws.close();
      process.exit(1);
    }
    chunks.push(...message.params.value);
  }
  if (message.method === "Tracing.tracingComplete") {
    const waiter = pending.get("complete");
    if (waiter) waiter();
  }
  const waiter = pending.get(message.id);
  if (waiter) {
    pending.delete(message.id);
    if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
    else waiter.resolve(message.result);
  }
});
function send(method, params) {
  const current = id++;
  return new Promise((resolve, reject) => {
    pending.set(current, { resolve, reject });
    ws.send(JSON.stringify({ id: current, method, params }));
  });
}
await send("Tracing.start", {
  categories: "devtools.timeline,disabled-by-default-devtools.timeline,blink.user_timing,v8.execute",
  options: "record-as-much-as-possible",
});
const replay = await send("Runtime.evaluate", { expression: "window.__s0StartReplay()", awaitPromise: true, returnByValue: true });
if (replay.exceptionDetails) throw new Error(JSON.stringify(replay.exceptionDetails));
const done = new Promise((resolve) => pending.set("complete", resolve));
await send("Tracing.end", {});
await done;
fs.writeFileSync(out, JSON.stringify({ traceEvents: chunks }));
ws.close();
console.error(`chrome-trace: ${chunks.length} events -> ${out}`);
