// Runs the production host and its scoped endpoint against a private daemon
// and Herdr server. The operator's app, browser and profile are never used.
import { chromium, expect, type Browser, type ElectronApplication } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { WebSocket } from "ws";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { fitWindow, HIDE_CLI, isolate, launch, test, type Isolated } from "./fixture";

test.describe.configure({ timeout: 180_000 });
let herdr: HerdrFixture;
let run: Isolated;
let app: ElectronApplication | null = null;
let browser: Browser | null = null;
let server: Server;
let origin: string;
let sequence = 0;
let fileCanary: string;
let fileUrl: string;

function quote(value: string): string { return `'${value.replaceAll("'", "'\\''")}'`; }
async function browserCommand(url: string, method: string, params: Record<string, unknown>): Promise<{ error?: unknown }> {
  const socket = new WebSocket(url);
  let timer: NodeJS.Timeout | undefined;
  try {
    return await new Promise((resolve, reject) => {
      timer = setTimeout(() => reject(new Error("Scoped browser command timed out")), 10_000);
      socket.once("error", () => reject(new Error("Scoped browser command could not connect")));
      socket.once("open", () => socket.send(JSON.stringify({ id: 1, method, params })));
      socket.on("message", (data) => {
        const reply = JSON.parse(data.toString()) as { id?: number; error?: unknown };
        if (reply.id === 1) resolve(reply);
      });
    });
  } finally { clearTimeout(timer); socket.terminate(); }
}
async function fromPane(args: string[], succeeds = true): Promise<Record<string, unknown>> {
  const stem = path.join(herdr.root, `cdp-command-${++sequence}`);
  const output = `${stem}.json`, status = `${stem}.status`;
  const command = `HIDE_STATE_DIR=${quote(run.env.HIDE_STATE_DIR!)} ${[HIDE_CLI, ...args].map(quote).join(" ")} > ${quote(output)}; printf '%s' "$?" > ${quote(status)}\n`;
  const sent = spawnSync(herdr.bin, ["pane", "send-text", herdr.panes[0]!, command], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  expect(sent.status).toBe(0);
  await expect.poll(() => fs.existsSync(status), { timeout: 30_000 }).toBe(true);
  expect(Number(fs.readFileSync(status, "utf8")) === 0, "isolated Workspace command result").toBe(succeeds);
  return JSON.parse(fs.readFileSync(output, "utf8").trim().split("\n").at(-1) ?? "null") as Record<string, unknown>;
}

test.beforeAll(async () => {
  herdr = await startHerdr({ agents: false });
  fileCanary = path.join(herdr.root, "outside-cdp-canary.html");
  fs.writeFileSync(fileCanary, '<title>Forbidden file canary</title><h1>FORBIDDEN_CDP_FILE_CONTENT</h1>');
  fileUrl = pathToFileURL(fileCanary).href;
  server = createServer((request, response) => {
    if (request.url === "/file-redirect") { response.writeHead(302, { location: fileUrl }); response.end(); return; }
    response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    response.end('<!doctype html><meta charset="utf-8"><title>Scoped browser fixture</title><h1>Scoped browser fixture</h1><input aria-label="Name"><button onclick="document.querySelector(\'output\').textContent=document.querySelector(\'input\').value">Apply</button><output aria-label="Result"></output>');
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});
test.afterAll(async () => {
  herdr?.stop();
  await new Promise<void>((resolve) => server?.close(() => resolve()));
});
test.beforeEach(() => { run = isolate(herdr, "browser-cdp"); });
test.afterEach(async () => {
  await browser?.close().catch(() => undefined);
  browser = null;
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
});

test("browser CDP: standard Playwright controls the scoped native page and disconnect preserves it", async () => {
  const launched = await launch(run.env);
  app = launched.app;
  await fitWindow(app, { width: 1024, height: 640 });
  await enterWorkspace(launched.page, "fixture");
  const opened = await fromPane(["browser", "open", `${origin}/fixture`, "--reveal", "--wait"]);
  const displayId = (opened.result as { view_id: string }).view_id;
  expect(typeof displayId).toBe("string");
  await expect.poll(() => fromPane(["browser", "connect", "--display", displayId]).then((answer) => answer.ok), { timeout: 20_000 }).toBe(true);
  const connected = await fromPane(["browser", "connect", "--display", displayId]);
  const result = connected.result as { cdp_http_url: string; browser_ws_url: string };
  expect(typeof result.cdp_http_url).toBe("string");
  expect((await fetch(`${new URL(result.cdp_http_url).origin}/json/version`)).status).toBe(404);
  const inventory = await (await fetch(`${result.cdp_http_url}/json/list`)).json() as { type: string; title: string; id: string }[];
  expect(inventory.length).toBe(1);
  expect(inventory[0]?.type).toBe("page");
  // The default client requests a global download directory. This gateway
  // deliberately supports the existing context with no global defaults.
  try { browser = await chromium.connectOverCDP(result.browser_ws_url, { noDefaults: true, timeout: 20_000 }); }
  catch { throw new Error("The scoped Playwright connection could not initialize"); }
  const context = browser.contexts()[0]!;
  expect(context.pages().length).toBe(1);
  const page = context.pages()[0]!;
  await page.getByRole("textbox", { name: "Name" }).fill("한글 scoped input");
  await page.getByRole("button", { name: "Apply" }).click();
  await expect(page.locator("output")).toHaveText("한글 scoped input");
  await page.evaluate((url) => {
    const frame = document.createElement("iframe");
    frame.id = "cross-site";
    frame.src = url;
    document.body.append(frame);
  }, `${origin.replace("127.0.0.1", "localhost")}/cross-site`);
  const frame = page.frameLocator("#cross-site");
  await frame.getByRole("textbox", { name: "Name" }).fill("descendant input");
  await frame.getByRole("button", { name: "Apply" }).click();
  await expect(frame.locator("output")).toHaveText("descendant input");
  const cdp = await context.newCDPSession(page);
  await expect(cdp.send("Browser.close")).rejects.toThrow();
  await expect(browser.newContext()).rejects.toThrow();
  await cdp.detach();
  const candidate = await app.evaluate(({ BrowserWindow }) => {
    const windows = BrowserWindow.getAllWindows().filter((window) => window.getParentWindow() === null);
    if (windows.length !== 1) throw new Error("The candidate must have one main window");
    return { pid: process.pid, windowId: windows[0]!.getMediaSourceId().split(":")[1]! };
  });
  expect(candidate.pid).toBe(app.process().pid);
  const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (evidence) {
    fs.mkdirSync(evidence, { recursive: true });
    const capture = spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", candidate.windowId, path.join(evidence, "browser-cdp-native.png")], { encoding: "utf8" });
    expect(capture.status, "candidate window capture failed").toBe(0);
  }
  await browser.close();
  browser = null;
  await expect.poll(() => app!.evaluate(({ BrowserWindow }, url) => {
    const views = BrowserWindow.getAllWindows()[0]!.contentView.children;
    return views.filter((view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url).length;
  }, `${origin}/fixture`)).toBe(1);
  expect((await browserCommand(result.browser_ws_url, "Browser.getVersion", {})).error).toBeUndefined();
});

test("browser CDP: same-context creation becomes a core browser display in the originating area", async () => {
  const launched = await launch(run.env);
  app = launched.app;
  await enterWorkspace(launched.page, "fixture");
  await fromPane(["browser", "open", `${origin}/first`, "--reveal", "--wait"]);
  const connected = await fromPane(["browser", "connect"]);
  const result = connected.result as { browser_ws_url: string };
  try { browser = await chromium.connectOverCDP(result.browser_ws_url, { noDefaults: true, timeout: 20_000 }); }
  catch { throw new Error("The scoped Playwright connection could not initialize"); }
  const context = browser.contexts()[0]!;
  const page = await context.newPage();
  await page.goto(`${origin}/created`);
  await expect(page.getByRole("heading")).toHaveText("Scoped browser fixture");
  const views = await fromPane(["view", "list"]);
  expect((views.result as { views: { kind: string }[] }).views.filter((view) => view.kind === "browser").length).toBe(2);
  await page.close();
  await expect.poll(() => fromPane(["view", "list"]).then((answer) => (answer.result as { views: { kind: string }[] }).views.filter((view) => view.kind === "browser").length)).toBe(1);
});

test("browser CDP: native files cannot be read through runtime, frames, redirects or creation", async () => {
  const launched = await launch(run.env);
  app = launched.app;
  await enterWorkspace(launched.page, "fixture");
  const opened = await fromPane(["browser", "open", `${origin}/protected`, "--reveal", "--wait"]);
  const displayId = (opened.result as { view_id: string }).view_id;
  const connected = await fromPane(["browser", "connect"]);
  const capability = connected.result as { browser_ws_url: string; cdp_http_url: string };
  try { browser = await chromium.connectOverCDP(capability.browser_ws_url, { noDefaults: true, timeout: 20_000 }); }
  catch { throw new Error("The scoped Playwright connection could not initialize"); }
  const context = browser.contexts()[0]!;
  const page = context.pages()[0]!;
  const cdp = await context.newCDPSession(page);
  await expect(cdp.send("Page.navigate", { url: fileUrl })).rejects.toThrow();
  expect((await browserCommand(capability.browser_ws_url, "Target.createTarget", { url: fileUrl })).error).toBeDefined();
  await expect(cdp.send("Network.loadNetworkResource", { url: fileUrl, options: { disableCache: true, includeCredentials: false } })).rejects.toThrow();
  // The address is assembled at runtime. The native policy receives the
  // resolved request/navigation; the gateway never parses JavaScript.
  const encoded = [...fileUrl].map((letter) => letter.charCodeAt(0));
  await cdp.send("Runtime.evaluate", { expression: `location.assign(String.fromCharCode(...${JSON.stringify(encoded)})); 'attempted'` });
  await expect(page.getByRole("heading")).toHaveText("Scoped browser fixture");
  const resource = await cdp.send("Runtime.evaluate", { expression: `fetch(String.fromCharCode(...${JSON.stringify(encoded)})).then(r => r.text()).then(() => 'read', () => 'blocked')`, awaitPromise: true, returnByValue: true });
  expect(resource.result.value).toBe("blocked");
  await cdp.send("Runtime.evaluate", { expression: `const frame=document.createElement('iframe'); frame.srcdoc=${JSON.stringify(`<iframe src="${fileUrl}"></iframe>`)}; document.body.append(frame); const image=document.createElement('img'); image.src=${JSON.stringify(fileUrl)}; document.body.append(image);` });
  const nativeState = () => app!.evaluate(({ BrowserWindow }, url) => {
    const contents = BrowserWindow.getAllWindows()[0]!.contentView.children.flatMap((view) => {
      const page = (view as { webContents?: Electron.WebContents }).webContents;
      return page && page.getURL() === url ? [page] : [];
    });
    if (contents.length !== 1) throw new Error("Expected exactly one protected candidate page");
    return contents[0]!.mainFrame.framesInSubtree.map((frame) => frame.url);
  }, `${origin}/protected`);
  await expect.poll(async () => (await nativeState()).length).toBeGreaterThanOrEqual(2);
  expect((await nativeState()).some((url) => url.startsWith("file:"))).toBe(false);
  expect(await page.locator("body").innerText()).not.toContain("FORBIDDEN_CDP_FILE_CONTENT");
  // A manual file display retains ordinary native behavior but is excluded
  // even when its source is within this checkout and its renderer is live.
  const manual = path.join(herdr.root, "fixture", "manual-cdp-file.html");
  fs.writeFileSync(manual, "<title>Manual local file</title><h1>Manual local file</h1>");
  const manualOpened = await fromPane(["browser", "open", pathToFileURL(manual).href, "--reveal", "--wait"]);
  const manualId = (manualOpened.result as { view_id: string }).view_id;
  expect((await fromPane(["browser", "connect", "--display", manualId], false)).ok).toBe(false);
  const inventory = await (await fetch(`${capability.cdp_http_url}/json/list`)).json() as { id: string; url: string }[];
  expect(inventory).toHaveLength(1);
  expect(inventory[0]!.url).toBe(`${origin}/protected`);
  // A script installed before disconnect must not gain file access after it.
  await cdp.send("Runtime.evaluate", { expression: `addEventListener('cdp-file-attempt', () => location.assign(${JSON.stringify(fileUrl)}));` });
  await cdp.detach();
  await browser.close();
  browser = null;
  await app.evaluate(async ({ BrowserWindow }, url) => {
    for (const view of BrowserWindow.getAllWindows()[0]!.contentView.children) {
      const contents = (view as { webContents?: Electron.WebContents }).webContents;
      if (contents?.getURL() === url) await contents.executeJavaScript("dispatchEvent(new Event('cdp-file-attempt'))");
    }
  }, `${origin}/protected`);
  expect((await nativeState()).some((url) => url.startsWith("file:"))).toBe(false);
  const again = await fromPane(["browser", "connect", "--display", displayId]);
  try { browser = await chromium.connectOverCDP((again.result as { browser_ws_url: string }).browser_ws_url, { noDefaults: true, timeout: 20_000 }); }
  catch { throw new Error("The scoped Playwright reconnection could not initialize"); }
  const protectedPage = browser.contexts()[0]!.pages()[0]!;
  await protectedPage.goto(`${origin}/file-redirect`).catch(() => undefined);
  const urls = await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.contentView.children.flatMap((view) => {
    const contents = (view as { webContents?: Electron.WebContents }).webContents;
    return contents ? contents.mainFrame.framesInSubtree.map((frame) => frame.url) : [];
  }));
  expect(urls).not.toContain(fileUrl);
});
