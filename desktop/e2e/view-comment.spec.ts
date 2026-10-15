// The operator's Comment mode on a page (PRD view-opener-feedback B9-B11,
// B16, B21, B23): an agent opens a page from its own pane, the operator
// turns Comment on in the real desktop window, picks an element in the page
// with trusted input the main process sends, writes a note in hided's
// overlay, and Send gives that agent one letter with the page, the element
// and a screenshot it can read. The operator's app, Herdr server and browser
// are never used.
import { expect, type ElectronApplication, type Page } from "@playwright/test";
import fs from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { agentPane, type HideAnswer } from "../../web/e2e/agent-pane";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { fitWindow, isolate, launch, test, type Isolated } from "./fixture";

test.describe.configure({ timeout: 180_000 });

const PAGE = `<!doctype html><meta charset="utf-8"><title>Sign in</title>
<h1 id="title" style="margin:40px">Welcome back</h1>
<button id="go" style="margin:40px;width:200px;height:48px" onclick="document.title='pressed'">Continue</button>`;

let herdr: HerdrFixture;
let run: Isolated;
let app: ElectronApplication | null = null;
let server: Server;
let origin: string;

/** Sends the page shown at `url` input the way a person's hardware would: the page reads it as trusted. */
async function input(url: string, events: Electron.MouseInputEvent[] | Electron.KeyboardInputEvent[]): Promise<void> {
  await app!.evaluate(({ BrowserWindow }, { url, events }) => {
    const contents = BrowserWindow.getAllWindows()[0]!.contentView.children
      .map((view) => (view as { webContents?: Electron.WebContents }).webContents)
      .find((page) => page && !page.isDestroyed() && page.getURL() === url);
    if (!contents) throw new Error(`no page shows ${url}`);
    for (const event of events) contents.sendInputEvent(event);
  }, { url, events });
}

/** Types `text` into the page's focused field a key at a time, as the keyboard does (`insertText` fires no `input`). */
async function type(url: string, text: string): Promise<void> {
  await app!.evaluate(({ BrowserWindow }, { url, text }) => {
    const contents = BrowserWindow.getAllWindows()[0]!.contentView.children
      .map((view) => (view as { webContents?: Electron.WebContents }).webContents)
      .find((page) => page && !page.isDestroyed() && page.getURL() === url);
    if (!contents) throw new Error(`no page shows ${url}`);
    for (const char of text) contents.sendInputEvent({ type: "char", keyCode: char });
  }, { url, text });
}

/** Whether the page shows the overlay's blue pin at (`x`, `y`): the pins live in a closed shadow root the page cannot read. */
async function pinAt(url: string, x: number, y: number): Promise<boolean> {
  return app!.evaluate(async ({ BrowserWindow }, { url, x, y }) => {
    const contents = BrowserWindow.getAllWindows()[0]!.contentView.children
      .map((view) => (view as { webContents?: Electron.WebContents }).webContents)
      .find((page) => page && !page.isDestroyed() && page.getURL() === url);
    if (!contents) throw new Error(`no page shows ${url}`);
    const [b, g, r] = (await contents.capturePage({ x, y, width: 1, height: 1 })).toBitmap();
    return b! > 200 && r! < 120 && g! > 100 && g! < 170;
  }, { url, x, y });
}

/** Evaluates `expression` in the page's own world. */
async function inPage<T>(url: string, expression: string): Promise<T> {
  return app!.evaluate(({ BrowserWindow }, { url, expression }) => {
    const contents = BrowserWindow.getAllWindows()[0]!.contentView.children
      .map((view) => (view as { webContents?: Electron.WebContents }).webContents)
      .find((page) => page && !page.isDestroyed() && page.getURL() === url);
    if (!contents) throw new Error(`no page shows ${url}`);
    return contents.executeJavaScript(expression) as Promise<T>;
  }, { url, expression });
}

test.beforeAll(async () => {
  herdr = await startHerdr({ agents: false });
  server = createServer((_request, response) => {
    response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    response.end(PAGE);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});
test.afterAll(async () => {
  herdr?.stop();
  await new Promise<void>((resolve) => server?.close(() => resolve()));
});
test.beforeEach(() => { run = isolate(herdr, "view-comment"); });
test.afterEach(async () => {
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
});

test("Comment on a page: the operator picks an element in the page and its agent gets the page, the element and a screenshot", async () => {
  const launched = await launch(run.env);
  app = launched.app;
  const shell: Page = launched.page;
  await fitWindow(app, { width: 1400, height: 900 });
  await enterWorkspace(shell, "fixture");

  const pane = herdr.panes[0]!;
  const agent = await agentPane(herdr, run.env.HIDE_STATE_DIR!, pane);
  const ok = (answer: HideAnswer) => {
    expect(answer.status, answer.out).toBe(0);
    return answer.out;
  };
  const url = `${origin}/login`;
  const opened = JSON.parse(ok(await agent(["browser", "open", url, "--reveal", "--wait"])).trim().split("\n").at(-1)!) as { result: { view_id: string } };
  const display = opened.result.view_id;

  // B1: the page's tab wears the agent's mark.
  const front = shell.locator('[data-view-tab-bar] [role="tab"][aria-selected="true"]');
  await expect(front.locator("[data-view-opener]")).toHaveAttribute("data-view-opener", pane);

  // B9: Comment draws hided's overlay in the page, and the page's own clicks stop.
  const comment = shell.locator('[data-browser-comment]');
  await comment.click();
  await expect(comment).toHaveAttribute("data-browser-comment", "on");
  // The operator's pointer leaves for the page; a tooltip left open over it would hold the page still and keep its keys.
  await shell.mouse.move(0, 0);
  await expect(shell.getByRole("tooltip")).toHaveCount(0);
  await expect.poll(() => inPage<boolean>(url, "document.querySelector('hide-comment-overlay') !== null")).toBe(true);
  // While the operator comments, the agent's page commands wait their turn (BROWSER_DISPLAYS.md, Comment mode).
  const busy = await agent(["browser", "snapshot", display]);
  expect(busy.status).not.toBe(0);
  expect(busy.out).toContain("display_busy");

  // B10: a press on an element opens the note with its selector; the page never sees the press.
  const box = await inPage<{ x: number; y: number; right: number; top: number }>(url, "(() => { const r = document.getElementById('go').getBoundingClientRect(); return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2), right: Math.round(r.right), top: Math.round(r.top) }; })()");
  await input(url, [
    { type: "mouseMove", x: box.x, y: box.y },
    { type: "mouseDown", x: box.x, y: box.y, button: "left", clickCount: 1 },
    { type: "mouseUp", x: box.x, y: box.y, button: "left", clickCount: 1 },
  ]);
  await type(url, "Make the button one size smaller.");
  await input(url, [{ type: "keyDown", keyCode: "Enter", modifiers: [process.platform === "darwin" ? "meta" : "control"] }]);
  expect(await inPage<string>(url, "document.title")).toBe("Sign in");

  // B11, B14: the comment counts on the tab and the line under the View.
  await expect(front.locator("[data-view-comments]")).toHaveText("1");
  const row = shell.locator("[data-view-feedback-row]");
  await expect(row).toHaveAttribute("data-view-feedback-state", "unsent");
  await expect(row.locator("[data-view-feedback-summary]")).toHaveText(/1 comment · 1 view/);

  // B12: the comment's pin, at the element's corner, opens that comment again; what is typed there edits it.
  await expect.poll(() => pinAt(url, box.right, box.top)).toBe(true);
  await input(url, [
    { type: "mouseMove", x: box.right, y: box.top },
    { type: "mouseDown", x: box.right, y: box.top, button: "left", clickCount: 1 },
    { type: "mouseUp", x: box.right, y: box.top, button: "left", clickCount: 1 },
  ]);
  await type(url, " Keep the label.");
  await input(url, [{ type: "keyDown", keyCode: "Enter", modifiers: [process.platform === "darwin" ? "meta" : "control"] }]);
  // The overlay reports the edit before Esc's `off`, and hided reads them in order, so the edit is the core's once Comment is off.

  // Esc ends Comment mode: the overlay goes and the page takes clicks again; the edit left one comment.
  await input(url, [{ type: "keyDown", keyCode: "Escape" }]);
  await expect(comment).toHaveAttribute("data-browser-comment", "off");
  await expect.poll(() => inPage<boolean>(url, "document.querySelector('hide-comment-overlay') === null")).toBe(true);
  await expect(row.locator("[data-view-feedback-summary]")).toHaveText(/1 comment · 1 view/);

  // B16, B21, B23: Send gives the agent the page, the View, the element and a screenshot on its machine.
  await row.locator('[data-view-feedback-send-main="default"]').click();
  await expect(row).toHaveAttribute("data-view-feedback-state", "sent");
  await expect(front.locator("[data-view-comments]")).toHaveCount(0);
  const inbox = ok(await agent(["inbox"]));
  expect(inbox).toContain(url);
  expect(inbox).toContain(`hide browser snapshot ${display}`);
  expect(inbox).toContain("#go");
  expect(inbox).toContain("Make the button one size smaller.");
  expect(inbox).toContain("Keep the label.");
  const shot = /Screenshot: (\S+\.png)/.exec(inbox)?.[1];
  expect(shot, inbox).toBeTruthy();
  const bytes = fs.readFileSync(shot!);
  expect(bytes.subarray(0, 8).toString("hex")).toBe("89504e470d0a1a0a");
  // D-16: the attachment is the owner's alone.
  if (process.platform !== "win32") expect(fs.statSync(shot!).mode & 0o077).toBe(0);
});
