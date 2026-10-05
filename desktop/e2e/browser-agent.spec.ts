// `hide browser` page commands against a real candidate display: the CLI runs
// in a pane of a private Herdr, reaches hided's relay with the pane's
// Workspace credential, and drives the display through the desktop's scoped
// gateway. The operator's app, Herdr server and browser are never used.
import { expect, type Dialog, type ElectronApplication } from "@playwright/test";
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
let server: Server;
let origin: string;
let child: string;
let sequence = 0;

const PAGE = `<!doctype html><meta charset="utf-8"><title>Agent fixture</title>
<h1>Agent fixture</h1>
<input placeholder="Name"><button onclick="document.querySelector('output').textContent='applied: '+document.querySelector('input').value">Apply</button>
<output></output>
<button onclick="alert('Hello from the fixture')">Alert</button>
<script>console.log('agent fixture ready'); console.warn('agent fixture warning');</script>`;
const CHILD = `<!doctype html><meta charset="utf-8"><title>Child</title>
<input placeholder="Child name"><button onclick="document.querySelector('output').textContent='child: '+document.querySelector('input').value">Send</button>
<output></output><button onclick="confirm('From the frame')">Frame confirm</button>`;

// Another site's frame whose script never yields once the page has loaded, so
// its renderer answers nothing. It tells the server when it starts spinning.
const HUNG_CHILD = `<!doctype html><meta charset="utf-8"><title>Hung child</title>
<script>addEventListener("load", () => setTimeout(() => { navigator.sendBeacon("/hung-now"); for (;;) {} }, 200));</script>`;
const HUNG_PAGE = `<!doctype html><meta charset="utf-8"><title>Hung page</title>
<h1>Beside a hung frame</h1><button>Still readable</button>`;
let hung = false;

function quote(value: string): string { return `'${value.replaceAll("'", "'\\''")}'`; }
/** Runs `hide ...` in the isolated pane and returns its exit status and output. */
async function hideCli(args: string[]): Promise<{ status: number; out: string }> {
  const stem = path.join(herdr.root, `browser-agent-${++sequence}`);
  const output = `${stem}.out`, status = `${stem}.status`;
  const command = `HIDE_STATE_DIR=${quote(run.env.HIDE_STATE_DIR!)} ${[HIDE_CLI, ...args].map(quote).join(" ")} > ${quote(output)}; printf '%s' "$?" > ${quote(status)}\n`;
  const sent = spawnSync(herdr.bin, ["pane", "send-text", herdr.panes[0]!, command], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  expect(sent.status).toBe(0);
  await expect.poll(() => fs.existsSync(status), { timeout: 60_000 }).toBe(true);
  return { status: Number(fs.readFileSync(status, "utf8")), out: fs.readFileSync(output, "utf8") };
}
const browserCli = (args: string[]) => hideCli(["browser", ...args]);
/** Which View each area shows, from `hide view list`. */
async function shownViews(): Promise<unknown> {
  const answer = JSON.parse((await hideCli(["view", "list"])).out.trim().split("\n").at(-1)!) as { result: { views: { view_id: string; selected: boolean; active_area: boolean }[] } };
  expect(answer.result.views.length).toBeGreaterThan(0);
  return answer.result.views.map(({ view_id, selected, active_area }) => ({ view_id, selected, active_area }));
}
async function json(args: string[]): Promise<Record<string, unknown>> {
  const { status, out } = await browserCli(args);
  let answer: Record<string, unknown>;
  try { answer = JSON.parse(out.trim().split("\n").at(-1) ?? "null") as Record<string, unknown>; }
  catch { throw new Error(`hide browser ${args[0]} printed no JSON: ${out}`); }
  expect(status === 0, `hide browser ${args[0]} answered ${out.trim()}`).toBe(answer.ok === true);
  return answer;
}
async function snapshot(display: string, ...options: string[]): Promise<string> {
  const { status, out } = await browserCli(["snapshot", display, ...options]);
  expect(status, out).toBe(0);
  return out;
}
/** The ref a snapshot line gives an element, `@N` or `@<tag>:N`. */
function ref(text: string, line: RegExp): string {
  const found = text.split("\n").find((row) => line.test(row));
  expect(found, `no line matches ${line} in\n${text}`).toBeDefined();
  return found!.trim().split(" ")[0]!;
}
async function openDisplay(url: string): Promise<string> {
  const stem = path.join(herdr.root, `browser-open-${++sequence}`);
  const command = `HIDE_STATE_DIR=${quote(run.env.HIDE_STATE_DIR!)} ${[HIDE_CLI, "browser", "open", url, "--reveal", "--wait"].map(quote).join(" ")} > ${quote(`${stem}.json`)}; printf '%s' "$?" > ${quote(`${stem}.status`)}\n`;
  expect(spawnSync(herdr.bin, ["pane", "send-text", herdr.panes[0]!, command], { env: herdr.env, encoding: "utf8", timeout: 10_000 }).status).toBe(0);
  await expect.poll(() => fs.existsSync(`${stem}.status`), { timeout: 30_000 }).toBe(true);
  const answer = JSON.parse(fs.readFileSync(`${stem}.json`, "utf8").trim().split("\n").at(-1)!) as { result: { view_id: string } };
  return answer.result.view_id;
}
/** Reads the display's own document through the main process, outside CDP. */
async function inDisplay<T>(url: string, expression: string): Promise<T> {
  return app!.evaluate(async ({ BrowserWindow }, { url, expression }) => {
    const pages = BrowserWindow.getAllWindows()[0]!.contentView.children.flatMap((view) => {
      const contents = (view as { webContents?: Electron.WebContents }).webContents;
      return contents && !contents.isDestroyed() && contents.getURL() === url ? [contents] : [];
    });
    if (pages.length !== 1) throw new Error("Expected exactly one fixture display");
    return pages[0]!.executeJavaScript(expression) as Promise<T>;
  }, { url, expression });
}
async function start(): Promise<void> {
  const launched = await launch(run.env);
  app = launched.app;
  await fitWindow(app, { width: 1200, height: 800 });
  await enterWorkspace(launched.page, "fixture");
}

test.beforeAll(async () => {
  herdr = await startHerdr({ agents: false });
  server = createServer((request, response) => {
    if (request.url === "/hung-now") { hung = true; response.writeHead(204).end(); return; }
    response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    if (request.url === "/hang") { response.end(HUNG_CHILD); return; }
    if (request.url === "/hung") { response.end(HUNG_PAGE.replace("</h1>", `</h1><iframe src="${origin.replace("127.0.0.1", "localhost")}/hang" width="300" height="100"></iframe>`)); return; }
    response.end(request.url?.startsWith("/child") ? CHILD : PAGE.replace("</h1>", `</h1><iframe id="child" src="${child}" width="420" height="120"></iframe>`));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  // Another site, so the frame runs in its own renderer process.
  child = `${origin.replace("127.0.0.1", "localhost")}/child`;
});
test.afterAll(async () => {
  herdr?.stop();
  await new Promise<void>((resolve) => server?.close(() => resolve()));
});
test.beforeEach(() => { run = isolate(herdr, "browser-agent"); });
test.afterEach(async () => {
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
});

test("hide browser: an agent reads and drives a page and its cross-site frame while the operator sees each action", async () => {
  await start();
  const display = await openDisplay(`${origin}/agent`);
  const views = await shownViews();
  const first = await snapshot(display);
  expect(first).toMatch(/^# Agent fixture\n# http:\/\/127\.0\.0\.1:\d+\/agent\n\n/);
  const name = ref(first, /textbox "Name"/);
  const apply = ref(first, /button "Apply"/);
  expect(first).toMatch(/# OOPIF [a-z2-9]{4} origin=http:\/\/localhost:\d+/);
  const childName = ref(first, /textbox "Child name"/);
  const send = ref(first, /button "Send"/);
  expect(childName).toMatch(/^@[a-z2-9]{4}:\d+$/);

  // Records the overlay host as the page sees it, whenever it is drawn.
  await inDisplay(`${origin}/agent`, `new MutationObserver(() => {
    const host = document.querySelector('hide-agent-overlay');
    if (host) window.overlaySeen = { parent: host.parentElement === document.documentElement, hidden: host.getAttribute('aria-hidden'), events: getComputedStyle(host).pointerEvents };
  }).observe(document.documentElement, { childList: true }); true`);
  const filled = await json(["fill", display, name, "한글 에이전트"]);
  expect(filled.changed).toContain("한글 에이전트");
  expect(filled.next).toBe(`hide browser snapshot ${display} --diff`);
  const clicked = await json(["click", display, apply]);
  expect(clicked.changed).toContain("applied: 한글 에이전트");
  // The operator saw the cursor: drawn over the page, hidden from assistive
  // technology, letting clicks through, and gone after the last action.
  expect(await inDisplay(`${origin}/agent`, "window.overlaySeen")).toEqual({ parent: true, hidden: "true", events: "none" });
  await expect.poll(() => inDisplay<boolean>(`${origin}/agent`, "document.querySelector('hide-agent-overlay') === null"), { timeout: 10_000 }).toBe(true);

  await json(["fill", display, childName, "inside the frame"]);
  const childClick = await json(["click", display, send]);
  expect(childClick.changed).toContain("child: inside the frame");
  await json(["wait", display, "--text", "child: inside the frame"]);

  const diff = await snapshot(display, "--diff");
  expect(diff).toMatch(/\+.*child: inside the frame/);
  const quiet = await snapshot(display, "--diff");
  expect(quiet).toContain("# diff: no changes since previous snapshot");
  const grep = await snapshot(display, "--grep", "Apply");
  expect(grep).toContain('button "Apply"');
  expect(grep).not.toContain('textbox "Name"');
  // A ref from a frame that navigated names nothing in its new document.
  const tag = send.slice(1).split(":")[0]!;
  await json(["eval", display, "document.getElementById('child').src += '?2'"]);
  await expect.poll(async () => /# OOPIF ([a-z2-9]{4})/.exec(await snapshot(display))?.[1], { timeout: 10_000 }).not.toBe(tag);
  expect(await json(["click", display, send])).toMatchObject({ ok: false, reason: "ref_stale" });

  const console_ = await browserCli(["console", display]);
  expect(console_.out).toContain("[log] agent fixture ready");
  expect(console_.out).toContain("[warning] agent fixture warning");
  const network = await browserCli(["network", display]);
  expect(network.out).toContain("headers and bodies are not available");
  expect(await json(["eval", display, "document.title"])).toMatchObject({ ok: true, value: "Agent fixture" });
  const shot = path.join(herdr.root, "agent.png");
  const captured = await json(["screenshot", display, shot]);
  expect(fs.readFileSync(shot).subarray(1, 4).toString()).toBe("PNG");
  expect((captured.image as { width: number }).width).toBeGreaterThan(0);
  // The display stays, still the View in front, and nothing holds its
  // debugger once a command ends.
  expect(await inDisplay<string>(`${origin}/agent`, "document.title")).toBe("Agent fixture");
  expect(await shownViews()).toEqual(views);
});

test("hide browser: a dialog is reported, never answered, and holds the display until the operator answers it", async () => {
  await start();
  const display = await openDisplay(`${origin}/agent`);
  // Playwright dismisses a dialog nobody listens for; listening leaves it to
  // the test, which answers it the way the operator would.
  await expect.poll(() => app!.context().pages().some((page) => page.url() === `${origin}/agent`)).toBe(true);
  const page = app!.context().pages().find((page) => page.url() === `${origin}/agent`)!;
  const shown: Dialog[] = [];
  page.on("dialog", (dialog) => { shown.push(dialog); });
  const alert = ref(await snapshot(display, "--interactive"), /button "Alert"/);
  const clicked = await json(["click", display, alert]);
  expect(clicked).toMatchObject({ ok: true, dialog: { type: "alert", message: "Hello from the fixture" } });
  expect(shown.map((dialog) => dialog.message())).toEqual(["Hello from the fixture"]);
  const held = await json(["snapshot", display]);
  expect(held).toMatchObject({ ok: false, reason: "dialog_open", display });
  expect(held.next_action).toContain("operator must answer the dialog");
  await shown[0]!.accept();
  const confirm = ref(await snapshot(display), /button "Frame confirm"/);
  // A cross-origin frame's dialog holds only that frame, which a later command
  // cannot tell from a script that never yields: it is noted, not a failure.
  expect(await json(["click", display, confirm])).toMatchObject({ ok: true, dialog: { type: "confirm", message: "From the frame" } });
  expect(await snapshot(display)).toMatch(/# OOPIF unresponsive origin=http:\/\/localhost:\d+/);
  await expect.poll(() => shown.length).toBe(2);
  await shown[1]!.dismiss();
  expect(await snapshot(display)).toContain('button "Frame confirm"');
});

test("hide browser: a cross-origin frame that never answers is noted and the rest of the page is read", async () => {
  hung = false;
  await start();
  const display = await openDisplay(`${origin}/hung`);
  await expect.poll(() => hung, { timeout: 15_000 }).toBe(true);
  const text = await snapshot(display);
  expect(text).toContain('button "Still readable"');
  expect(text).toMatch(/# OOPIF unresponsive origin=http:\/\/localhost:\d+ - no answer in time/);
  expect(text).not.toMatch(/# OOPIF [a-z2-9]{4} /);
});

test("hide browser: a busy, hidden, file or missing display and a stale ref are refused with what to do next", async () => {
  await start();
  const display = await openDisplay(`${origin}/agent`);
  // Another debugger on the display: the gateway admits one client.
  const connected = JSON.parse((await browserCli(["connect", "--display", display])).out.trim()) as { result: { browser_ws_url: string } };
  const holder = new WebSocket(connected.result.browser_ws_url);
  await new Promise((resolve, reject) => { holder.once("open", resolve); holder.once("error", reject); });
  try {
    const targets = await new Promise<{ result: { targetInfos: { targetId: string; type: string }[] } }>((resolve) => {
      holder.once("message", (data) => resolve(JSON.parse(data.toString())));
      holder.send(JSON.stringify({ id: 1, method: "Target.getTargets" }));
    });
    const page = targets.result.targetInfos.find((target) => target.type === "page")!;
    await new Promise((resolve) => {
      holder.once("message", resolve);
      holder.send(JSON.stringify({ id: 2, method: "Target.attachToTarget", params: { targetId: page.targetId, flatten: true } }));
    });
    expect(await json(["snapshot", display])).toMatchObject({ ok: false, reason: "display_busy" });
  } finally { holder.terminate(); }
  await expect.poll(async () => (await browserCli(["snapshot", display])).status, { timeout: 15_000 }).toBe(0);

  expect(await json(["click", display, "@999"])).toMatchObject({ ok: false, reason: "ref_stale" });
  // A View behind another tab of its area reads, but never takes input unseen.
  const apply = ref(await snapshot(display), /button "Apply"/);
  await openDisplay(`${origin}/front`);
  const hidden = await json(["click", display, apply]);
  expect(hidden).toMatchObject({ ok: false, reason: "display_hidden" });
  expect(hidden.next_action).toContain(`hide view select ${display} --reveal`);
  expect((await json(["screenshot", display, path.join(herdr.root, "hidden.png")])).reason).toBe("display_hidden");
  expect(await snapshot(display)).toContain('button "Apply"');
  const file = path.join(herdr.root, "fixture", "agent-file.html");
  fs.writeFileSync(file, "<title>Local file</title><h1>Local file</h1>");
  const fileDisplay = await openDisplay(pathToFileURL(file).href);
  expect(await json(["snapshot", fileDisplay])).toMatchObject({ ok: false, reason: "display_unsupported" });
  expect(await json(["snapshot", "browser-missing"])).toMatchObject({ ok: false, reason: "display_missing" });
});
