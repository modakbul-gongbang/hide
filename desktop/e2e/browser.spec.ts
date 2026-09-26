// Browser displays end to end (issue 155): a page opened by `hide browser
// open` from an agent's pane, by the Explorer, and by the toolbar shows in a
// View area as a native view the desktop main process owns, follows its
// slot, survives a move between areas without loading again, freezes under
// a shell overlay, ends its renderer when closed, comes back after a
// relaunch, and is a quiet notice in a plain browser tab. Everything runs on
// a private Herdr server, hided and Electron profile (see `fixture.ts`).

import { chromium, expect, test, type ElectronApplication, type Page } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { hostLog, isolate, launch, screenshot, type Isolated } from "./fixture";

test.describe.configure({ timeout: 240_000 });
test.use({ actionTimeout: 15_000 });

let herdr: HerdrFixture;
let run: Isolated;
let app: ElectronApplication | null = null;
let server: http.Server;
let origin: string;

const PAGES: Record<string, string> = {
  "/a.html": '<!doctype html><meta charset="utf-8"><title>Page A</title><body style="background:lavender"><h1>Page A</h1><input id="q" aria-label="query">',
  "/b.html": '<!doctype html><meta charset="utf-8"><title>Page B</title><body style="background:honeydew"><h1>Page B</h1>',
  "/c.html": '<!doctype html><meta charset="utf-8"><title>Page C</title><body style="background:mistyrose"><h1>Page C</h1>',
};

test.beforeAll(async () => {
  herdr = await startHerdr();
  server = http.createServer((request, response) => {
    const body = PAGES[request.url ?? ""];
    response.writeHead(body ? 200 : 404, { "content-type": "text/html; charset=utf-8" });
    response.end(body ?? "not found");
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

test.afterAll(async () => {
  herdr?.stop();
  await new Promise((resolve) => server?.close(resolve));
});

test.beforeEach(() => {
  run = isolate(herdr, test.info().title.split(":")[0]!);
});

test.afterEach(async () => {
  const info = test.info();
  if (info.status !== info.expectedStatus) console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
});

type View = { url: string; title: string; visible: boolean; bounds: { x: number; y: number; width: number; height: number }; pid: number };

/** Every page view the window holds, as the main process sees it. */
function views(): Promise<View[]> {
  return app!.evaluate(({ BrowserWindow }) =>
    (BrowserWindow.getAllWindows()[0]?.contentView.children ?? []).flatMap((child) => {
      const contents = (child as { webContents?: Electron.WebContents }).webContents;
      if (!contents) return [];
      return [{ url: contents.getURL(), title: contents.getTitle(), visible: child.getVisible(), bounds: child.getBounds(), pid: contents.getOSProcessId() }];
    }),
  );
}

async function viewOf(url: string): Promise<View> {
  let found: View | undefined;
  await expect.poll(async () => (found = (await views()).find((view) => view.url === url)) !== undefined, { timeout: 20_000 }).toBe(true);
  return found!;
}

/** Runs `script` in the page of the view showing `url`. */
function inPage<T>(url: string, script: string): Promise<T> {
  return app!.evaluate(
    ({ BrowserWindow }, { url, script }) => {
      const child = BrowserWindow.getAllWindows()[0]!.contentView.children.find(
        (view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url,
      ) as unknown as { webContents: Electron.WebContents } | undefined;
      if (!child) throw new Error(`no view shows ${url}`);
      return child.webContents.executeJavaScript(script) as Promise<T>;
    },
    { url, script },
  );
}

/** A browser display's tab, by the address or title its label carries. */
function tab(page: Page, text: string) {
  return page.locator(`[data-view-tab-bar] [role="tab"][data-display][aria-label*="${text}"]`);
}

/** Where a browser display's slot sits, in window points (the shell runs at zoom 1). */
async function slotBounds(page: Page, displayId: string): Promise<View["bounds"]> {
  const box = (await page.locator(`[data-browser-slot="${displayId}"]`).boundingBox())!;
  const x = Math.round(box.x);
  const y = Math.round(box.y);
  return { x, y, width: Math.round(box.x + box.width) - x, height: Math.round(box.y + box.height) - y };
}

/**
 * The test window sits behind whatever the operator is using, and Chromium
 * stops painting an occluded window, so a capture by window id would show a
 * stale frame. The window is never focused for a capture; it keeps painting.
 */
const PAINT_WHILE_OCCLUDED = ["--disable-backgrounding-occluded-windows"];

/** The window this run draws in: a CI runner's whole screen width, and a height its work area holds. */
const WINDOW = { width: 1024, height: 640 };

/** The window as macOS draws it, page views included, captured by window id without focusing it. */
async function windowShot(name: string): Promise<void> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!dir) return;
  const source = await app!.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getMediaSourceId());
  spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", source.split(":")[1]!, path.join(dir, `${name}.png`)]);
}

function openFromCli(target: string, extra: string[] = []): Record<string, unknown> {
  const answer = run.hide(["browser", "open", target, ...extra]);
  return { status: answer.status, ...(JSON.parse(answer.stdout.trim().split("\n").at(-1) || "{}") as Record<string, unknown>) };
}

/** Waits until the view showing `url` covers its display's slot as the shell lays it out now. */
async function expectOnSlot(page: Page, url: string, displayId: string): Promise<void> {
  await expect
    .poll(async () => {
      const placed = { view: (await viewOf(url)).bounds, slot: await slotBounds(page, displayId) };
      return JSON.stringify(placed.view) === JSON.stringify(placed.slot) ? "on its slot" : placed;
    })
    .toBe("on its slot");
}

async function displayIdOf(page: Page, text: string): Promise<string> {
  await expect(tab(page, text)).toBeVisible({ timeout: 20_000 });
  return (await tab(page, text).getAttribute("data-display"))!;
}

test("browser: a page opens from an agent's pane, follows its area, moves without loading again, freezes under the palette and ends with its display", async () => {
  const report = path.join(herdr.root, "fixture", "리포트 1.html");
  fs.writeFileSync(report, '<!doctype html><meta charset="utf-8"><title>Local report</title><h1>리포트</h1>');
  ({ app } = await launch(run.env, PAINT_WHILE_OCCLUDED));
  // The window is the size this run needs, whatever the machine's screen:
  // a CI runner's screen is 1024 points wide.
  const bounds = await app.evaluate(({ BrowserWindow, screen }, size) => {
    const area = screen.getPrimaryDisplay().workArea;
    const window = BrowserWindow.getAllWindows()[0]!;
    window.setBounds({ x: area.x, y: area.y, ...size });
    return window.getBounds();
  }, WINDOW);
  expect(bounds, "the screen cannot hold the window this run needs").toMatchObject(WINDOW);
  const page = await app.firstWindow();
  await enterWorkspace(page, "fixture");
  const checkout = path.join(fs.realpathSync(herdr.root), "fixture");

  // `hide browser open` from an agent's pane opens the page in that pane's
  // Workspace and answers with the core's receipt.
  const opened = openFromCli(`${origin}/a.html`, ["--pane", herdr.panes[0]]);
  expect(opened).toMatchObject({ status: 0, ok: true, device_id: "local", path: checkout });
  const a = await displayIdOf(page, "Page A");
  let pageA = await viewOf(`${origin}/a.html`);
  await expect.poll(async () => (await viewOf(`${origin}/a.html`)).visible).toBe(true);
  await expectOnSlot(page, `${origin}/a.html`, a);
  await inPage(`${origin}/a.html`, "window.__hideMarker = 'a'");

  // Hangul reaches the page's field as committed text.
  await inPage(`${origin}/a.html`, "document.getElementById('q').focus()");
  await app.evaluate(({ BrowserWindow }, url) => {
    const child = BrowserWindow.getAllWindows()[0]!.contentView.children.find((view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url) as unknown as { webContents: Electron.WebContents };
    child.webContents.insertText("한글 입력");
  }, `${origin}/a.html`);
  expect(await inPage(`${origin}/a.html`, "document.getElementById('q').value")).toBe("한글 입력");

  // A second page, without a pane, opens in the Workspace in front.
  expect(openFromCli(`${origin}/b.html`)).toMatchObject({ status: 0, ok: true });
  const b = await displayIdOf(page, "Page B");
  await inPage(`${origin}/b.html`, "window.__hideMarker = 'b'");
  const pageB = await viewOf(`${origin}/b.html`);
  await expect.poll(async () => (await viewOf(`${origin}/a.html`)).visible).toBe(false);

  // Two View areas side by side need the Workspace's width: the Views take
  // it all and the Explorer steps aside until it is used below.
  await page.locator('[data-layout-choice="views"]').click();
  await page.locator('[data-tool-toggle="explorer"][aria-pressed="true"]').click();
  await expect(page.locator('[data-tool-toggle="explorer"]')).toHaveAttribute("aria-pressed", "false");

  // Split right moves B into a new area: both pages show, neither loaded again.
  await tab(page, "Page B").click({ button: "right" });
  const splitRight = page.locator('[role="menu"] [data-menu-item="split_right"]');
  await expect(splitRight, (await splitRight.textContent()) ?? "").not.toHaveAttribute("aria-disabled", "true");
  await splitRight.click();
  await expect(page.locator("[data-view-area-id]")).toHaveCount(2);
  await tab(page, "Page A").click();
  await expect.poll(async () => (await views()).filter((view) => view.visible).length).toBe(2);
  expect(await inPage(`${origin}/a.html`, "window.__hideMarker")).toBe("a");
  expect(await inPage(`${origin}/b.html`, "window.__hideMarker")).toBe("b");
  expect((await viewOf(`${origin}/a.html`)).pid).toBe(pageA.pid);
  expect((await viewOf(`${origin}/b.html`)).pid).toBe(pageB.pid);
  await expectOnSlot(page, `${origin}/b.html`, b);

  // A narrower window moves the pages with their slots.
  await app.evaluate(({ BrowserWindow }) => {
    const window = BrowserWindow.getAllWindows()[0]!;
    const [width, height] = window.getSize();
    window.setSize(width! - 160, height! - 80);
  });
  await expectOnSlot(page, `${origin}/b.html`, b);
  await expectOnSlot(page, `${origin}/a.html`, a);
  await windowShot("browser-two-areas");

  // The palette cannot draw over a native view: a page it meets hides and
  // its still stands in its place until the palette closes.
  await page.keyboard.press("Meta+KeyK");
  const palette = page.locator("[data-palette-input]");
  await expect(palette).toBeVisible();
  await expect.poll(async () => (await views()).filter((view) => view.visible).length).toBeLessThan(2);
  await expect(page.locator("[data-browser-still]").first()).toBeVisible();
  await windowShot("browser-palette-frozen");
  await screenshot(page, "browser-palette-frozen-shell");
  await page.keyboard.press("Escape");
  await expect(palette).toHaveCount(0);
  await expect.poll(async () => (await views()).filter((view) => view.visible).length).toBe(2);
  await expect(page.locator("[data-browser-still]")).toHaveCount(0);

  // The toolbar: a typed address loads in that display; Back returns.
  const address = page.locator(`[data-browser-display="${a}"] [data-browser-address]`);
  await address.click();
  await expect(address).toHaveValue(`${origin}/a.html`);
  await address.fill(`${origin}/c.html`);
  await expect(address).toHaveValue(`${origin}/c.html`);
  await address.press("Enter");
  await expect(tab(page, "Page C")).toBeVisible();
  await viewOf(`${origin}/c.html`);
  await page.locator(`[data-browser-display="${a}"] [data-browser-command="back"]`).click();
  await expect(tab(page, "Page A")).toBeVisible();
  pageA = await viewOf(`${origin}/a.html`);

  // A page's new window is another browser display of the Workspace, not a window.
  await inPage(`${origin}/a.html`, `void window.open(${JSON.stringify(`${origin}/c.html`)}, "_blank")`);
  await expect(tab(page, "Page C")).toBeVisible({ timeout: 20_000 });
  expect((await views()).filter((view) => view.url === `${origin}/c.html`)).toHaveLength(1);
  expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(1);

  // A page that cannot load says so in its area, with Reload.
  expect(openFromCli("http://127.0.0.1:1/")).toMatchObject({ status: 0, ok: true });
  await expect(page.locator('[data-area-empty="browser-failed"]')).toBeVisible({ timeout: 20_000 });
  await expect(page.locator("[data-browser-retry]")).toBeVisible();
  await windowShot("browser-load-failed");

  // The Explorer opens an HTML file of the checkout as a page.
  await page.locator('[data-tool-toggle="explorer"]').click();
  await page.locator(`[data-explorer-row="${path.join(checkout, "리포트 1.html")}"]`).click({ button: "right" });
  await page.locator('[data-explorer-menu] [data-menu-item="open-browser"]').click();
  await expect(tab(page, "Local report")).toBeVisible({ timeout: 20_000 });
  expect((await views()).some((view) => view.url.startsWith("file://") && view.title === "Local report")).toBe(true);
  // A file outside every checkout is refused by the daemon and never shown.
  const outside = path.join(run.root, "outside.html");
  fs.writeFileSync(outside, "<title>Outside</title>");
  expect(openFromCli(outside)).toMatchObject({ ok: false, reason: "outside_checkout" });

  // Closing B's display ends its renderer process. Without the Explorer both
  // areas show again, B's among them.
  await page.locator('[data-tool-toggle="explorer"][aria-pressed="true"]').click();
  await expect(tab(page, "Page B")).toBeVisible();
  await tab(page, "Page B").click({ button: "right" });
  await page.locator('[role="menu"] [data-menu-item="close_view"]').click();
  await expect(tab(page, "Page B")).toHaveCount(0);
  await expect
    .poll(() => {
      try {
        process.kill(pageB.pid, 0);
        return "alive";
      } catch {
        return "gone";
      }
    })
    .toBe("gone");
  expect((await views()).some((view) => view.url === `${origin}/b.html`)).toBe(false);

  // A relaunch brings the pages back at the addresses they last showed.
  await app.close();
  ({ app } = await launch(run.env, PAINT_WHILE_OCCLUDED));
  const again = await app.firstWindow();
  await enterWorkspace(again, "fixture");
  await expect(tab(again, "Page A")).toBeVisible({ timeout: 20_000 });
  await tab(again, "Page A").click();
  await expect.poll(async () => (await views()).find((view) => view.url === `${origin}/a.html`)?.visible).toBe(true);
  await windowShot("browser-relaunched");

  // A plain browser tab on the same daemon shows a notice, not a page.
  const status = JSON.parse(run.hide(["status", "--json"]).stdout) as { url: string };
  const browser = await chromium.launch();
  try {
    const plain = await browser.newPage();
    await plain.goto(status.url);
    await enterWorkspace(plain, "fixture");
    await tab(plain, "Page A").click();
    await expect(plain.locator('[data-area-empty="browser-host"]')).toContainText("Pages open in the hide desktop app.");
    await expect(plain.locator("[data-browser-open-external]")).toBeVisible();
    await screenshot(plain, "browser-plain-tab");
  } finally {
    await browser.close();
  }
});
