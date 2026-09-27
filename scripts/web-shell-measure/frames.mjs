#!/usr/bin/env node
// Frame budget on the product web shell over a driven window: a rAF loop
// injected into the page records every frame's dt while the fixture pane
// prints a line every 8 ms (the S0 driver). Prints {frames, window, done}.
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";
import { connectPage } from "./cdp.mjs";

const cdpPort = process.env.MEASURE_CDP_PORT;
const windowMs = Number(process.env.MEASURE_FRAME_WINDOW_MS ?? 120_000);
const paneId = process.env.MEASURE_PANE_ID;
const herdrBin = process.env.HERDR_BIN_PATH;
if (!cdpPort || !paneId || !herdrBin) throw new Error("MEASURE_CDP_PORT, MEASURE_PANE_ID and HERDR_BIN_PATH are required");

const page = await connectPage(cdpPort);
const arrivalsBefore = await page.evaluate("window.__hideProbe.arrivals()");
await page.evaluate(`
  window.__frames = [];
  window.__framesDone = false;
  window.__framesWindow = null;
  new Promise((resolve) => requestAnimationFrame(resolve)).then((origin) => {
    let last = origin;
    const onFrame = (t) => {
      window.__frames.push({ t, dt: t - last });
      last = t;
      if (t - origin >= ${windowMs}) {
        window.__framesWindow = { start: origin, end: t, duration_ms: t - origin };
        window.__framesDone = true;
      } else requestAnimationFrame(onFrame);
    };
    requestAnimationFrame(onFrame);
  });
  "started"
`);
const seconds = Math.ceil(windowMs / 1000) + 2;
// Every shown area is driven. cat echoes the measured pane's input while
// its sibling writer produces the same 8 ms output stream as other panes.
const drivenPanes = process.env.MEASURE_DRIVEN_PANES?.split(",") ?? [paneId];
if (!drivenPanes.includes(paneId) || drivenPanes.length > 6) throw new Error("Invalid driven pane set");
const command = `stty -echo -icanon; /usr/bin/python3 -c "import time; end=time.monotonic()+${seconds}; i=0\nwhile time.monotonic()<end:\n print(f'drive {i:05d}',flush=True); i+=1; time.sleep(0.008)" & cat`;
const drivers = drivenPanes.map((id) => spawn(herdrBin, ["pane", "run", id, command], { env: process.env, stdio: "ignore" }));
await Promise.all(drivers.map((driver) => new Promise((resolve, reject) => {
  driver.on("error", reject);
  driver.on("exit", (code) => code === 0 ? resolve() : reject(new Error(`driver exited ${code}`)));
})));
await new Promise((resolve) => setTimeout(resolve, 2000));
const { stdout } = await promisify(execFile)(process.execPath, [fileURLToPath(new URL("./echo.mjs", import.meta.url))], {
  env: { ...process.env, MEASURE_ECHO_REPEATS: "50" }, timeout: 30000, maxBuffer: 1024 * 1024,
});
const drivenEcho = JSON.parse(stdout);
const deadline = Date.now() + windowMs + 10_000;
while (Date.now() < deadline) {
  if (await page.evaluate("window.__framesDone === true")) break;
  await new Promise((resolve) => setTimeout(resolve, 500));
}
const result = await page.evaluate("({frames: window.__frames, window: window.__framesWindow, done: window.__framesDone})");
const arrivalsAfter = await page.evaluate("window.__hideProbe.arrivals()");
page.close();
console.log(JSON.stringify({ ...result, ws_frames_during_run: arrivalsAfter - arrivalsBefore, driver_exit: 0, driven_panes: drivenPanes, workload: "one line per 8 ms per shown pane; 50 echo samples in the measured pane", driven_echo: drivenEcho }));
