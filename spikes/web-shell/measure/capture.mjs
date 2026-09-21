#!/usr/bin/env node
// Capture hided-spike WS deltas as JSONL. Redaction happens in redact.py.
const fs = await import("node:fs");
const durationMs = Number(process.env.S0_CAPTURE_MS ?? 120000);
const url = process.env.HIDED_WS_URL ?? "ws://127.0.0.1:9876/ws";
const out = process.argv[2];
if (!out) {
  console.error("usage: capture.mjs <out.jsonl>");
  process.exit(2);
}

const t0 = Date.now();
const stream = fs.createWriteStream(out);
const ws = new WebSocket(url);
let count = 0;

ws.addEventListener("open", () => {
  console.error(`capture: connected ${url} for ${durationMs}ms`);
});
ws.addEventListener("message", (event) => {
  const parsed = JSON.parse(String(event.data));
  const row = { t_ms: Date.now() - t0, type: parsed.type, payload: parsed.payload };
  stream.write(`${JSON.stringify(row)}\n`);
  count += 1;
});
ws.addEventListener("error", (error) => {
  console.error("capture: ws error", error);
});

await new Promise((resolve) => setTimeout(resolve, durationMs));
ws.close();
stream.end();
console.error(`capture: wrote ${count} messages to ${out}`);
