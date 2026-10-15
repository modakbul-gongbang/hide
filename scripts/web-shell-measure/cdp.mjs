// One CDP session to the product page (the tab whose URL carries probe=1).
export async function connectPage(cdpPort, requireProbe = true) {
  const targets = await fetch(`http://127.0.0.1:${cdpPort}/json/list`, { signal: AbortSignal.timeout(5000) }).then((r) => r.json());
  const page = targets.find((t) => t.type === "page" && (!requireProbe || String(t.url).includes("probe=1")));
  if (!page) throw new Error(`no ${requireProbe ? "probe " : ""}page on CDP :${cdpPort} (${targets.length} targets)`);
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { ws.close(); reject(new Error("CDP open deadline")); }, 5000);
    ws.addEventListener("open", () => { clearTimeout(timer); resolve(); });
    ws.addEventListener("error", error => { clearTimeout(timer); reject(error); });
  });
  let nextId = 1;
  const pending = new Map();
  const listeners = new Map();
  ws.addEventListener("message", (event) => {
    const message = JSON.parse(String(event.data));
    if (message.method) {
      for (const listener of listeners.get(message.method) ?? []) listener(message.params);
      return;
    }
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    clearTimeout(waiter.timer);
    if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
    else waiter.resolve(message.result);
  });
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      if (pending.size >= 64) return reject(new Error("CDP outstanding command cap"));
      const id = nextId++;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`CDP command deadline: ${method}`));
      }, params.awaitPromise ? 30000 : 15000);
      pending.set(id, { resolve, reject, timer });
      ws.send(JSON.stringify({ id, method, params }));
    });
  const disconnected = () => {
    for (const waiter of pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(new Error("CDP disconnected"));
    }
    pending.clear();
  };
  ws.addEventListener("close", disconnected);
  ws.addEventListener("error", disconnected);
  const evaluate = async (expression, awaitPromise = false) => {
    const result = await send("Runtime.evaluate", { expression, awaitPromise, returnByValue: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  /** Calls `listener` with each event `method` sends until the page closes. */
  const on = (method, listener) => {
    listeners.set(method, [...(listeners.get(method) ?? []), listener]);
  };
  await send("Runtime.enable");
  return { send, evaluate, on, close: () => ws.close() };
}
