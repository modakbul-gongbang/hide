// Runs the existing product-page drivers against the private SSH fixture.
// The fixture owns Chrome/Herdr/hided; this process owns only bounded drivers.
import fs from "node:fs";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { connectPage } from "./cdp.mjs";
import { observeFrames } from "./ws-count.mjs";

const config = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const { output, identity } = config;
const here = path.dirname(fileURLToPath(import.meta.url));
const children = new Set();
console.log("Connecting to the owned product page");
const page = await connectPage(config.cdp, false);
const counter = observeFrames(page);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const env = { ...process.env, MEASURE_CDP_PORT: String(config.cdp), MEASURE_PANE_ID: identity.pane };
const write = (name, value) => fs.writeFileSync(path.join(output, name), JSON.stringify(value, null, 2));
async function waitFor(expression, timeout = 30000) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    if (await page.evaluate(expression)) return;
    await sleep(100);
  }
  throw new Error(`private page did not become ready: ${expression}`);
}
function child(program, args, variables = {}) {
  if (children.size >= 3) throw new Error("driver child cap exceeded");
  const process = spawn(program, args, { env: { ...env, ...variables }, stdio: ["ignore", "pipe", "pipe"] });
  children.add(process);
  let stdout = "", stderr = "";
  process.stdout.on("data", data => { stdout += data; if (stdout.length > 16 * 1024 * 1024) process.kill(); });
  process.stderr.on("data", data => { stderr += data; if (stderr.length > 1024 * 1024) process.kill(); });
  const timer = setTimeout(() => process.kill("SIGKILL"), 180000);
  const result = new Promise((resolve, reject) => {
    process.once("error", reject);
    process.once("close", code => {
      clearTimeout(timer); children.delete(process);
      if (code !== 0) reject(new Error(`private driver exited ${code}: ${stderr}`));
      else resolve(stdout);
    });
  });
  // A concurrent resource probe can fail before the foreground driver ends.
  result.catch(() => {});
  return result;
}
const driver = async (name, script, variables = {}) => {
  const value = JSON.parse(await child(process.execPath, [path.join(here, script)], variables));
  write(`${name}.json`, value);
  return value;
};
async function phase(name, body) {
  console.log(`Starting ${name}`);
  const stop = path.join(output, `${name}.stop`);
  counter.reset();
  const resource = child("/usr/bin/python3", [path.join(here, "resources.py"), String(identity.hided_pid), String(identity.herdr_pid), String(identity.chrome_pid)], {
    MEASURE_RESOURCE_SECONDS: "170", MEASURE_RESOURCE_STOP_FILE: stop,
  });
  const started = Date.now();
  const load = spawnSync("uptime", { encoding: "utf8" }).stdout.trim();
  try { await body(); }
  finally { fs.writeFileSync(stop, "done"); }
  const resources = JSON.parse(await resource);
  write(`${name}-resources.json`, resources);
  write(`${name}-wire.json`, { started, elapsed_ms: Date.now() - started, load, ...counter.report() });
}
function herdr(args) {
  const result = spawnSync(process.env.HERDR_BIN_PATH, args, { env, encoding: "utf8", timeout: 15000 });
  if (result.status !== 0) throw new Error(`private Herdr refused: ${result.stderr}`);
  return result.stdout.trim() ? JSON.parse(result.stdout) : null;
}

try {
  await page.send("Network.enable");
  await page.send("Emulation.setDeviceMetricsOverride", { width: 1280, height: 900, deviceScaleFactor: 1, mobile: false });
  console.log("Navigating to the product shell");
  await page.send("Page.navigate", { url: config.url });
  console.log("Waiting for the measured checkout");
  const id = JSON.stringify(config.checkout);
  const workspace = JSON.stringify(config.workspace);
  await waitFor(`document.querySelector('[data-project-toggle="' + ${workspace} + '"]') !== null`);
  await page.evaluate(`(() => {
    const toggle = document.querySelector('[data-project-toggle="' + ${workspace} + '"]');
    if (toggle.getAttribute('aria-expanded') === 'false') toggle.click();
  })()`);
  await waitFor(`[...document.querySelectorAll('[data-checkout]')].some(row => row.dataset.checkout === ${id})`);
  await page.evaluate(`[...document.querySelectorAll('[data-checkout]')].find(row => row.dataset.checkout === ${id}).click()`);
  await waitFor(`document.querySelector('[data-workspace-screen]') && window.__hideProbe?.paneId() === ${JSON.stringify(identity.pane)}`);
  await waitFor("document.querySelectorAll('[data-pane-view]').length === 5 && window.__hideProbe.attachedPanes().length >= 5");
  await sleep(3000);
  const seen = await page.evaluate("({ dpr:devicePixelRatio, inner:[innerWidth,innerHeight], panes:document.querySelectorAll('[data-pane-view]').length, live:window.__hideProbe.liveTerminals() })");
  if (seen.dpr !== 1 || seen.inner.join() !== "1280,900" || seen.panes !== 5) throw new Error("viewport/DPR/pane fixture drift");
  write("page.json", seen);
  write("initial-wire.json", counter.report());
  if (!counter.report().first_snapshot?.remote_sessions) throw new Error("initial page lacked a populated remote session");
  const screenshot = await page.send("Page.captureScreenshot", { format: "png" });
  fs.writeFileSync(path.join(output, "browser.png"), Buffer.from(screenshot.data, "base64"));
  await phase("idle", () => sleep(20000));
  await phase("warm-echo", () => driver("warm-echo", "key-echo.mjs", { MEASURE_ECHO_REPEATS: "50", MEASURE_ECHO_PREFIX: "w" }));
  await phase("warm-zoom", () => driver("warm-zoom", "topology.mjs", { MEASURE_TOPOLOGY_ZOOM_ONLY: "1", MEASURE_TOPOLOGY_ROUNDS: "20" }));
  await phase("status-churn", async () => {
    for (let n = 0; n < 8; n += 1) {
      herdr(["pane", "report-agent", identity.status_pane, "--source", "measure-scale", "--agent", "claude", "--state", n % 2 ? "idle" : "working"]);
      await sleep(2000);
    }
  });
  // One Python printer per split, no date/sleep subprocess on each line.
  for (const pane of identity.other_panes) {
    herdr(["pane", "run", pane, "python3 -u -c 'import time,collections; collections.deque(((print(\"fixture output %08d\" % i, flush=True),time.sleep(0.008)) for i in range(20000)),maxlen=0)'"]);
  }
  await sleep(3000);
  await phase("driven-zoom", () => driver("driven-zoom", "topology.mjs", { MEASURE_TOPOLOGY_ZOOM_ONLY: "1", MEASURE_TOPOLOGY_ROUNDS: "20" }));
  await phase("driven-echo", () => driver("driven-echo", "key-echo.mjs", { MEASURE_ECHO_REPEATS: "50", MEASURE_ECHO_PREFIX: "d" }));
  await phase("ordered-keys", () => driver("ordered-keys", "key-count.mjs", { MEASURE_KEY_COUNT: "1000" }));
  await page.send("Page.navigate", { url: "about:blank" });
  await sleep(2000);
  await phase("windowless", () => sleep(10000));
  counter.reset();
  await page.send("Page.navigate", { url: config.url });
  await waitFor(`window.__hideProbe?.paneId() === ${JSON.stringify(identity.pane)}`);
  await waitFor("document.querySelectorAll('[data-pane-view]').length === 5");
  await sleep(1000);
  write("reconnect-wire.json", counter.report());
  const reconnect = counter.report().first_snapshot;
  if (!reconnect?.remote_sessions) throw new Error("reconnect lacked a complete remote session");
  write("completed.json", { success: true, client_count: 1, method: "one actual product page socket; page keydown to DOM RAF / xterm grid and echoed terminal buffer; no native click-to-paint claim" });
} catch (error) {
  write("failed-page.json", await page.evaluate("({ title:document.title, text:document.body.innerText, panes:document.querySelectorAll('[data-pane-view]').length, attached:window.__hideProbe?.attachedPanes(), focused:window.__hideProbe?.paneId() })").catch(() => ({ unavailable: true })));
  throw error;
} finally {
  for (const process of children) process.kill("SIGKILL");
  page.close();
}
