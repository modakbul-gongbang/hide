#!/usr/bin/env node
// Shared echo definition: t0 = herdr pane send-text, t1 = xterm.js buffer.
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
const alphabet = "abcdefghijklmnopqrstuvwxyz0123456789";
for (let i = 0; i < repeats; i += 1) {
  const marker = alphabet[i % alphabet.length] + String(i % 10);
  await send("Runtime.evaluate", {
    expression: `window.__s0Echo = ""; window.__s0EchoAt = null; "ok"`,
    returnByValue: true,
  });
  const t0 = Date.now();
  const sent = spawnSync(herdrBin, ["pane", "send-text", paneId, `${marker}\n`], {
    env: process.env,
    encoding: "utf8",
  });
  if (sent.status !== 0) {
    console.error("echo-web: send-text failed", sent.stderr);
    process.exit(1);
  }
  const result = await send("Runtime.evaluate", {
    expression: `window.__s0WaitFor(${JSON.stringify(marker)}, 3000)`,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails) {
    console.error("echo-web: wait failed", result.exceptionDetails);
    process.exit(1);
  }
  const t1 = result.result?.value;
  samples.push(Number(t1) - t0);
}

ws.close();
const out = { method: "herdr pane send-text t0 -> xterm.js buffer t1 (Date.now)", samples };
console.log(JSON.stringify(out, null, 2));
