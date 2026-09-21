#!/usr/bin/env node
const cdpPort = process.env.S0_CDP_PORT ?? "9222";
const url = process.argv[2];
if (!url) {
  console.error("usage: chrome-navigate.mjs <url>");
  process.exit(2);
}
const targets = await fetch(`http://127.0.0.1:${cdpPort}/json/list`).then((r) => r.json());
const page = targets.find(
  (target) => target.type === "page" && String(target.url).includes("5173"),
) ?? targets.find((target) => target.type === "page");
if (!page) {
  console.error("chrome-navigate: no page target");
  process.exit(1);
}
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve);
  ws.addEventListener("error", reject);
});
await new Promise((resolve, reject) => {
  ws.addEventListener("message", (event) => {
    const message = JSON.parse(String(event.data));
    if (message.id === 1) {
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve(message.result);
    }
  });
  ws.send(JSON.stringify({ id: 1, method: "Page.navigate", params: { url } }));
});
ws.close();
