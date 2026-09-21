#!/usr/bin/env node
const cdpPort = process.env.S0_CDP_PORT ?? "9222";
const expression = process.argv[2];
if (!expression) {
  console.error("usage: chrome-eval.mjs <expression>");
  process.exit(2);
}
const targets = await fetch(`http://127.0.0.1:${cdpPort}/json/list`).then((r) => r.json());
const page = targets.find(
  (target) => target.type === "page" && String(target.url).includes("5173"),
);
if (!page) {
  console.error("chrome-eval: no tab on :5173");
  process.exit(1);
}
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve);
  ws.addEventListener("error", reject);
});
const result = await new Promise((resolve, reject) => {
  ws.addEventListener("message", (event) => {
    const message = JSON.parse(String(event.data));
    if (message.id === 1) {
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolve(message.result);
    }
  });
  ws.send(
    JSON.stringify({
      id: 1,
      method: "Runtime.evaluate",
      params: { expression, awaitPromise: true, returnByValue: true },
    }),
  );
});
ws.close();
if (result.exceptionDetails) {
  console.error(JSON.stringify(result.exceptionDetails));
  process.exit(1);
}
console.log(JSON.stringify(result.result?.value ?? null));
