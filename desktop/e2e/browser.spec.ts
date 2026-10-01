// Browser displays end to end (issue 155): a page opened by `hide browser
// open` from an agent's pane, by the Explorer, and by the toolbar shows in a
// View area as a native view the desktop main process owns, follows its
// slot, survives a move between areas without loading again, freezes under
// a shell overlay, ends its renderer when closed, comes back after a
// relaunch, and is a quiet notice in a plain browser tab. Everything runs on
// a private Herdr server, hided and Electron profile (see `fixture.ts`).

import { chromium, expect, type ElectronApplication, type Page } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import type { AddressInfo } from "node:net";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { countSent, enterWorkspace } from "../../web/e2e/wire";
import { hostLog, isolate, launch, NEEDS_FOCUS, relaunch, screenshot, shellPage, test, type Isolated } from "./fixture";

test.describe.configure({ timeout: 240_000 });
test.use({ actionTimeout: 15_000 });

let herdr: HerdrFixture;
let run: Isolated;
let app: ElectronApplication | null = null;
let server: http.Server;
let origin: string;
let cliSequence = 0;
let extraWorkspace: string | null = null;
let nativeModifiers = new Set<string>();

const PAGES: Record<string, string> = {
  "/a.html": '<!doctype html><meta charset="utf-8"><title>Page A</title><body style="background:lavender"><h1>Page A</h1><input id="q" aria-label="query">',
  "/b.html": '<!doctype html><meta charset="utf-8"><title>Page B</title><body style="background:honeydew"><h1>Page B</h1><input id="q" aria-label="Page B input" autofocus>',
  "/c.html": '<!doctype html><meta charset="utf-8"><title>Page C</title><body style="background:mistyrose"><h1>Page C</h1>',
  "/korean.html": '<!doctype html><meta charset="utf-8"><title>한글 브라우저</title><body style="font:16px system-ui"><h1>작업 공간 검증</h1><p>다른 영역의 내용과 선택은 읽을 수 있어야 합니다.</p><input aria-label="한글 입력" value="한글 확인"><script>window.tabKeys=0;addEventListener("keydown",e=>{if(e.code==="Tab")window.tabKeys++})</script>',
  "/signin.html": '<!doctype html><meta charset="utf-8"><title>Sign in</title><body style="background:aliceblue"><h1>Sign in</h1>',
  // A sign-in popup reports to the page that opened it and closes itself.
  "/popup.html": '<!doctype html><meta charset="utf-8"><title>Popup</title><script>window.opener.postMessage("signed-in", "*"); window.close();</script>',
  "/stay.html": '<!doctype html><meta charset="utf-8"><title>Stay</title><h1>Stay</h1>',
};
/** Answers with a server-side redirect to another app's link, the way a sign-in hands back to a native app. */
const APP_REDIRECT = "/to-app";

test.beforeAll(async () => {
  herdr = await startHerdr({ agents: false });
  server = http.createServer((request, response) => {
    if (request.url === APP_REDIRECT) {
      response.writeHead(302, { location: "hide-e2e-app://redirected" });
      response.end();
      return;
    }
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
  let releaseFailure: string | null = null;
  if (nativeModifiers.size) {
    const released = releaseNativeModifiers();
    if (released.status !== 0) releaseFailure = String(released.stderr) || "native modifier release failed";
  }
  const info = test.info();
  if (info.status !== info.expectedStatus) console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
  await app?.close().catch(() => undefined);
  app = null;
  if (extraWorkspace) herdr.run(["workspace", "close", extraWorkspace]);
  extraWorkspace = null;
  run.cleanup();
  expect(releaseFailure).toBeNull();
});

type View = { url: string; title: string; visible: boolean; bounds: { x: number; y: number; width: number; height: number }; pid: number };

/** Every page view the window holds, as the main process sees it; a page's popup is a child window, not this one. */
function views(): Promise<View[]> {
  return app!.evaluate(({ BrowserWindow }) =>
    (BrowserWindow.getAllWindows().find((window) => window.getParentWindow() === null)?.contentView.children ?? []).flatMap((child) => {
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
      const child = BrowserWindow.getAllWindows().find((window) => window.getParentWindow() === null)!.contentView.children.find(
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

/** The window this run draws in: a CI runner's whole screen width, and a height its work area holds. */
const WINDOW = { width: 1024, height: 640 };

/** The window as macOS draws it, page views included, captured by window id without focusing it. */
async function windowShot(name: string): Promise<void> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!dir) return;
  const source = await app!.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getMediaSourceId());
  const shot = spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", source.split(":")[1]!, path.join(dir, `${name}.png`)], { encoding: "utf8" });
  if (shot.status !== 0) {
    const png = await app!.evaluate(async ({ BrowserWindow }) => (await BrowserWindow.getAllWindows()[0]!.capturePage()).toPNG().toString("base64"));
    fs.writeFileSync(path.join(dir, `${name}.png`), Buffer.from(png, "base64"));
  }
  expect(fs.existsSync(path.join(dir, `${name}.png`)), "native window capture is missing").toBe(true);
}

/** Capture the candidate's live native page when macOS denies window capture. */
async function nativePageShot(url: string, name: string): Promise<void> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!dir) return;
  const png = await app!.evaluate(async ({ BrowserWindow }, target) => {
    const child = BrowserWindow.getAllWindows()[0]!.contentView.children.find(
      (view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === target,
    ) as unknown as { webContents: Electron.WebContents } | undefined;
    if (!child) throw new Error(`native page missing: ${target}`);
    return (await child.webContents.capturePage()).toPNG().toString("base64");
  }, url);
  fs.writeFileSync(path.join(dir, `${name}.png`), Buffer.from(png, "base64"));
}

function quote(value: string): string { return `'${value.replaceAll("'", "'\\''")}'`; }

async function cliFromPane(args: string[], pane = herdr.panes[0]): Promise<Record<string, unknown>> {
  const sequence = ++cliSequence;
  const output = path.join(herdr.root, `desktop-cli-${sequence}.json`);
  const status = path.join(herdr.root, `desktop-cli-${sequence}.status`);
  const command = `HIDE_STATE_DIR=${quote(run.env.HIDE_STATE_DIR!)} ${[path.resolve("..", "target", "debug", "hide"), ...args].map(quote).join(" ")} > ${quote(output)}; printf '%s' "$?" > ${quote(status)}\n`;
  const sent = spawnSync(herdr.bin, ["pane", "send-text", pane, command], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
  expect(sent.status, sent.stderr).toBe(0);
  await expect.poll(() => fs.existsSync(status) ? fs.readFileSync(status, "utf8") : null, { timeout: 20_000 }).not.toBeNull();
  return { status: Number(fs.readFileSync(status, "utf8")), ...(JSON.parse(fs.readFileSync(output, "utf8").trim().split("\n").at(-1) || "{}") as Record<string, unknown>) };
}

function openFromCli(target: string, extra: string[] = []): Promise<Record<string, unknown>> {
  return cliFromPane(["browser", "open", target, ...extra]);
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
  ({ app } = await launch(run.env));
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
  const opened = await openFromCli(`${origin}/a.html`, ["--reveal", "--wait"]);
  expect(opened).toMatchObject({ status: 0, ok: true, result: { context: { device_id: "local", checkout_path: checkout } }, page: { state: "loaded" } });
  expect(await cliFromPane(["view", "status", (opened.result as { view_id: string }).view_id])).toMatchObject({ status: 0, ok: true, view: { page: { state: "loaded" } } });
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

  // A second page opens in the calling pane's Workspace.
  expect(await openFromCli(`${origin}/b.html`)).toMatchObject({ status: 0, ok: true });
  const b = await displayIdOf(page, "Page B");
  await inPage(`${origin}/b.html`, "window.__hideMarker = 'b'");
  const pageB = await viewOf(`${origin}/b.html`);
  await expect.poll(async () => (await viewOf(`${origin}/a.html`)).visible).toBe(false);

  // Two View areas side by side need the Workspace's width: the side panel
  // is expanded over it (issue 170). The open panel beside the agents leaves
  // the View area too narrow to split, and the core stores the expansion, so
  // the menu below offers Split right only once the expanded panel is drawn:
  // it reads the geometry drawn when it opens.
  await page.keyboard.press("Meta+KeyK");
  await page.keyboard.type("Expand side panel");
  await page.locator('[data-palette-row="command:panel:expanded"]').click();
  await expect(page.locator("[data-palette-input]")).toHaveCount(0);
  await expect(page.locator("[data-workspace-screen]")).toHaveAttribute("data-panel", "expanded");
  const toolsShown = page.locator('[data-tools-toggle="on"]');
  if (await toolsShown.count()) await toolsShown.click();
  await expect(page.locator("[data-workspace-tools]")).toHaveCount(0);

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
  await nativePageShot(`${origin}/a.html`, "browser-native-page-a");
  await nativePageShot(`${origin}/b.html`, "browser-native-page-b");

  // A narrower window moves the pages with their slots.
  await app.evaluate(({ BrowserWindow }) => {
    const window = BrowserWindow.getAllWindows()[0]!;
    const [width, height] = window.getSize();
    window.setSize(width! - 160, height! - 80);
  });
  await expectOnSlot(page, `${origin}/b.html`, b);
  await expectOnSlot(page, `${origin}/a.html`, a);
  await windowShot("browser-two-areas");

  // A divider drag marks the root: both pages give way to their stills, so
  // the guide draws over them, and come back live at release.
  const divider = (await page.locator("[data-view-divider]").boundingBox())!;
  const slotA = (await page.locator(`[data-browser-slot="${a}"]`).boundingBox())!;
  await page.mouse.move(divider.x + divider.width / 2, divider.y + divider.height / 2);
  await page.mouse.down();
  await page.mouse.move(slotA.x + slotA.width * 0.6, divider.y + divider.height / 2, { steps: 4 });
  await expect(page.locator("html")).toHaveAttribute("data-view-drag", "col-resize");
  await expect(page.locator("[data-view-resize-guide]")).toBeVisible();
  await expect.poll(async () => (await views()).filter((view) => view.visible).length).toBe(0);
  await expect(page.locator("[data-browser-still]")).toHaveCount(2);
  await windowShot("browser-divider-drag-frozen");
  await page.mouse.up();
  await expect(page.locator("html")).not.toHaveAttribute("data-view-drag", /.*/);
  await expect.poll(async () => (await views()).filter((view) => view.visible).length).toBe(2);
  await expect(page.locator("[data-browser-still]")).toHaveCount(0);
  await expectOnSlot(page, `${origin}/a.html`, a);
  await expectOnSlot(page, `${origin}/b.html`, b);
  expect((await viewOf(`${origin}/a.html`)).pid).toBe(pageA.pid);

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

  // Explicitly bind the separate global command: these pages occupy two
  // single-tab areas, so focused-area cycling deliberately does nothing.
  await page.locator("[data-open-settings]").click();
  await page.locator('[data-settings-tab="shortcuts"]').click();
  await page.locator('[data-shortcut-record="recent_panel"]').click();
  await page.keyboard.press("Control+KeyG");
  await page.locator('[data-shortcut-apply="recent_panel"]').click();
  await expect(page.locator('[data-shortcut-effective="recent_panel"]')).toHaveText("⌃G");
  await page.keyboard.press("Escape");

  // Global Recent Panels freezes native pages while its modifier is held.
  await page.keyboard.down("Control");
  await page.keyboard.press("KeyG");
  const cycle = page.locator("[data-cycle=panels]");
  await expect(cycle).toBeVisible();
  await expect.poll(async () => (await views()).filter((view) => view.visible).length).toBeLessThan(2);
  await expect(page.locator("[data-browser-still]").first()).toBeVisible();
  await windowShot("browser-cycle-frozen");
  await page.keyboard.press("Escape");
  await page.keyboard.up("Control");
  await expect(cycle).toHaveCount(0);
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

  // A page's new tab is another browser display of the Workspace, not a window.
  await inPage(`${origin}/a.html`, `void window.open(${JSON.stringify(`${origin}/c.html`)}, "_blank")`);
  await expect(tab(page, "Page C")).toBeVisible({ timeout: 20_000 });
  expect((await views()).filter((view) => view.url === `${origin}/c.html`)).toHaveLength(1);
  expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(1);

  // A page that cannot load says so in its area, with Reload.
  const failed = await openFromCli("http://127.0.0.1:1/", ["--wait"]);
  expect(failed).toMatchObject({ status: 2, ok: false, applied: "applied", reason: "page_failed", page: { state: "failed" } });
  expect(await cliFromPane(["view", "status", failed.view_id as string])).toMatchObject({ status: 0, ok: true, view: { page: { state: "failed" } } });
  await expect(page.locator('[data-area-empty="browser-failed"]')).toBeVisible({ timeout: 20_000 });
  await expect(page.locator("[data-browser-retry]")).toBeVisible();
  await windowShot("browser-load-failed");

  // The Explorer opens an HTML file of the checkout as a page.
  await page.locator('[data-tools-toggle="off"]').click();
  await page.locator('[data-tool-tab="explorer"]').click();
  await page.locator(`[data-explorer-row="${path.join(checkout, "리포트 1.html")}"]`).click({ button: "right" });
  await page.locator('[data-explorer-menu] [data-menu-item="open-browser"]').click();
  await expect(tab(page, "Local report")).toBeVisible({ timeout: 20_000 });
  expect((await views()).some((view) => view.url.startsWith("file://") && view.title === "Local report")).toBe(true);
  // A file outside every checkout is refused by the daemon and never shown.
  const outside = path.join(run.root, "outside.html");
  fs.writeFileSync(outside, "<title>Outside</title>");
  expect(await openFromCli(outside)).toMatchObject({ ok: false, reason: "path_outside_checkout" });

  // Closing B's display ends its renderer process. Without the Explorer both
  // areas show again, B's tab among them (behind Page C, which opened beside A).
  await page.locator('[data-tools-toggle="on"]').click();
  await expect(page.locator("[data-workspace-tools]")).toHaveCount(0);
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
  app = await relaunch(run.env);
  const again = await shellPage(app);
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
    // Page C, opened beside Page A, shows the same notice in its own area.
    await expect(plain.locator(`[data-browser-display="${a}"] [data-area-empty="browser-host"]`)).toContainText("Pages open in the hide desktop app.");
    await expect(plain.locator(`[data-browser-open-external="${a}"]`)).toBeVisible();
    await screenshot(plain, "browser-plain-tab");
  } finally {
    await browser.close();
  }
});

test("browser: waiting for a hidden page does not take the operator's keyboard target", async () => {
  ({ app } = await launch(run.env));
  const page = await app.firstWindow();
  await enterWorkspace(page, "fixture");
  const focused = await page.locator('[data-pane-view][data-focused="true"]').getAttribute("data-pane-view");
  expect(focused).toBeTruthy();
  const hidden = await openFromCli(`${origin}/b.html`, ["--wait"]);
  expect(hidden).toMatchObject({ status: 2, ok: false, applied: "applied", reason: "page_wait_timeout", page: { state: "pending" } });
  expect(await cliFromPane(["view", "status", hidden.view_id as string])).toMatchObject({ status: 0, ok: true, view: { page: { state: "pending" } } });
  await expect(page.locator('[data-pane-view][data-focused="true"]')).toHaveAttribute("data-pane-view", focused!);
  const revealed = await openFromCli(`${origin}/b.html`, ["--reveal", "--wait"]);
  expect(revealed).toMatchObject({ status: 0, ok: true, page: { state: "loaded" } });
});

test("browser: a login in one Workspace is available in another", async () => {
  const other = path.join(herdr.root, "second-checkout");
  fs.mkdirSync(other, { recursive: true });
  const created = herdr.run(["workspace", "create", "--cwd", other, "--label", "second", "--no-focus"]) as { result: { workspace: { workspace_id: string }; root_pane: { pane_id: string } } };
  extraWorkspace = created.result.workspace.workspace_id;
  const otherPane = created.result.root_pane.pane_id;
  await expect.poll(() => {
    const read = spawnSync(herdr.bin, ["pane", "read", otherPane, "--source", "visible", "--format", "text"], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
    return read.stdout.includes("fixture %");
  }).toBe(true);

  ({ app } = await launch(run.env));
  const page = await app.firstWindow();
  await enterWorkspace(page, "fixture");
  expect(await openFromCli(`${origin}/a.html`, ["--reveal", "--wait"])).toMatchObject({ status: 0, ok: true });
  await viewOf(`${origin}/a.html`);
  await inPage(`${origin}/a.html`, "document.cookie = 'shared=ready; path=/'");
  const firstScreen = await page.locator("[data-workspace-screen]").getAttribute("data-workspace-screen");

  const opened = await cliFromPane(["browser", "open", `${origin}/b.html`, "--reveal", "--wait"], otherPane);
  expect(opened).toMatchObject({ status: 0, ok: true, page: { state: "loaded" } });
  await expect.poll(() => page.locator("[data-workspace-screen]").getAttribute("data-workspace-screen")).not.toBe(firstScreen);
  expect(await inPage(`${origin}/b.html`, "document.cookie")).toContain("shared=ready");
});

test("browser: a sign-in popup keeps its opener, belongs to its page, and a link to another app asks first", async () => {
  ({ app } = await launch(run.env));
  const page = await app.firstWindow();
  // The window a CI runner's screen holds.
  await app.evaluate(({ BrowserWindow }, size) => BrowserWindow.getAllWindows()[0]!.setSize(size.width, size.height), WINDOW);
  await enterWorkspace(page, "fixture");
  const signin = `${origin}/signin.html`;
  const opened = await openFromCli(signin, ["--reveal", "--wait"]);
  expect(opened).toMatchObject({ status: 0, ok: true });
  await viewOf(signin);
  const windows = () => app!.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length);
  const events = (name: string, reason?: string) => hostLog(run.env).filter((line) => line.event === name && (reason === undefined || line.reason === reason));

  // A sized popup is a window that keeps its opener: the sign-in posts its
  // result back and closes itself, and no browser display is added for it.
  await inPage(signin, `window.addEventListener("message", (event) => { document.title = "got " + event.data; }); void window.open(${JSON.stringify(`${origin}/popup.html`)}, "signin", "width=420,height=520")`);
  await expect.poll(() => inPage<string>(signin, "document.title"), { timeout: 20_000 }).toBe("got signed-in");
  await expect.poll(windows).toBe(1);
  expect(events("browser.popup_opened")).toHaveLength(1);

  // Only a size reaches the window, held to the work area: no window feature
  // makes it frameless, always on top, unclosable or off screen. At most four
  // stay open, and the page closing takes its popups with it.
  await inPage(signin, `void window.open(${JSON.stringify(`${origin}/stay.html`)}, "stay0", "left=-4000,top=-4000,width=9000,height=9000,frame=false,alwaysOnTop=yes,closable=no,modal=yes")`);
  await expect.poll(windows).toBe(2);
  const hostile = await app.evaluate(({ BrowserWindow, screen }) => {
    const popup = BrowserWindow.getAllWindows().find((window) => window.getParentWindow() !== null)!;
    const area = screen.getDisplayMatching(popup.getBounds()).workArea;
    const bounds = popup.getBounds();
    const inside = bounds.x >= area.x && bounds.y >= area.y && bounds.x + bounds.width <= area.x + area.width && bounds.y + bounds.height <= area.y + area.height;
    return { inside, framed: popup.getContentBounds().height < bounds.height, onTop: popup.isAlwaysOnTop(), closable: popup.isClosable(), modal: popup.isModal() };
  });
  expect(hostile).toEqual({ inside: true, framed: true, onTop: false, closable: true, modal: false });
  for (let i = 1; i < 5; i++) await inPage(signin, `void window.open(${JSON.stringify(`${origin}/stay.html`)}, "stay${i}", "width=300,height=300")`);
  await expect.poll(windows).toBe(5);
  await expect.poll(() => events("browser.popup_refused", "cap").length).toBe(1);
  // Many round trips after the first popup posted back, no popup became a display.
  await expect(tab(page, "Popup")).toHaveCount(0);
  await expect(tab(page, "Stay")).toHaveCount(0);
  expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().filter((window) => window.getParentWindow() !== null).length)).toBe(4);
  expect(await cliFromPane(["view", "close", (opened.result as { view_id: string }).view_id])).toMatchObject({ status: 0, ok: true });
  await expect.poll(windows).toBe(1);

  // A link to another app asks first, naming the origin and the link, and
  // opens only on Open; macOS is stood in for so no app starts and no sheet
  // waits for a click.
  await app.evaluate(({ app: electronApp, dialog, shell }) => {
    const probe = { asked: [] as string[], opened: [] as string[], answer: 0 };
    (globalThis as { appLinkProbe?: typeof probe }).appLinkProbe = probe;
    electronApp.getApplicationNameForProtocol = (url: string) => (url.startsWith("hide-e2e-app:") ? "Fixture App" : "");
    dialog.showMessageBox = (async (_window: unknown, options: Electron.MessageBoxOptions) => {
      probe.asked.push(`${options.message} | ${options.detail} | default ${options.defaultId}`);
      return { response: probe.answer, checkboxChecked: false };
    }) as typeof dialog.showMessageBox;
    shell.openExternal = async (url: string) => { probe.opened.push(url); };
  });
  const probe = () => app!.evaluate(() => (globalThis as unknown as { appLinkProbe: { asked: string[]; opened: string[] } }).appLinkProbe);
  expect(await openFromCli(signin, ["--reveal", "--wait"])).toMatchObject({ status: 0, ok: true });
  await viewOf(signin);
  await inPage(signin, `location.href = "hide-e2e-app://open?room=1"`);
  await expect.poll(async () => (await probe()).opened).toEqual(["hide-e2e-app://open?room=1"]);
  expect((await probe()).asked).toEqual([`Open Fixture App? | ${origin} wants to open this link in Fixture App.\n\nhide-e2e-app://open?room=1 | default 1`]);

  // Cancel leaves the page where it was, and the page asks nothing more
  // until it navigates; a redirect and a frame ask the same way.
  await app.evaluate(() => { (globalThis as unknown as { appLinkProbe: { answer: number } }).appLinkProbe.answer = 1; });
  await inPage(signin, `location.href = ${JSON.stringify(`${origin}${APP_REDIRECT}`)}`);
  await expect.poll(async () => (await probe()).asked.length).toBe(2);
  await inPage(signin, `document.body.insertAdjacentHTML("beforeend", '<iframe src="hide-e2e-app://frame"></iframe>')`);
  await expect.poll(() => events("browser.app_link_refused", "declined").length).toBe(1);
  expect(await inPage<string>(signin, "location.href")).toBe(signin);
  await inPage(signin, `location.reload()`);
  await expect.poll(() => inPage<string>(signin, "document.readyState")).toBe("complete");
  await inPage(signin, `document.body.insertAdjacentHTML("beforeend", '<iframe src="hide-e2e-app://frame"></iframe>')`);
  await expect.poll(async () => (await probe()).asked.length).toBe(3);
  expect((await probe()).opened).toEqual(["hide-e2e-app://open?room=1"]);

  // A scheme no app claims never asks.
  await inPage(signin, `location.href = "hide-e2e-none://x"`);
  await expect.poll(() => events("browser.app_link_refused", "no_app").length).toBe(1);

  // A shift-click on a link is Chromium's new-window too, but with no window
  // features it is a tab: another browser display, not a popup, and it opens
  // beside the page that asked, which stays in view.
  await inPage(signin, `document.body.insertAdjacentHTML("afterbegin", '<a href="${origin}/c.html" style="position:fixed;left:0;top:0;width:160px;height:60px;display:block">Page C</a>')`);
  await app.evaluate(({ BrowserWindow }, target) => {
    const main = BrowserWindow.getAllWindows().find((window) => window.getParentWindow() === null)!;
    const child = main.contentView.children.find((view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === target) as unknown as { webContents: Electron.WebContents };
    for (const type of ["mouseDown", "mouseUp"] as const) child.webContents.sendInputEvent({ type, x: 20, y: 20, button: "left", clickCount: 1, modifiers: ["shift"] });
  }, signin);
  await expect(tab(page, "Page C")).toBeVisible({ timeout: 20_000 });
  // Two View areas side by side need the Workspace's width: in this run's
  // window the side panel is expanded over it, as the first test does.
  await page.keyboard.press("Meta+KeyK");
  await page.keyboard.type("Expand side panel");
  await page.locator('[data-palette-row="command:panel:expanded"]').click();
  await expect(page.locator("[data-palette-input]")).toHaveCount(0);
  await expect(page.locator("[data-view-area-id]")).toHaveCount(2);
  await expect.poll(async () => (await views()).filter((view) => view.visible).map((view) => view.url).sort()).toEqual([`${origin}/c.html`, signin].sort());
  expect(await windows()).toBe(1);

  // Behind another page in its own area, the sign-in page is hidden: a
  // hidden page opens no popup and asks nothing.
  await tab(page, "Sign in").click();
  expect(await openFromCli(`${origin}/b.html`, ["--wait"])).toMatchObject({ status: 0, ok: true });
  await expect.poll(async () => (await viewOf(signin)).visible).toBe(false);
  await inPage(signin, `void window.open(${JSON.stringify(`${origin}/stay.html`)}, "hidden", "width=300,height=300"); location.href = "hide-e2e-app://hidden"`);
  await expect.poll(() => events("browser.popup_refused", "hidden").length).toBe(1);
  await expect.poll(() => events("browser.app_link_refused", "hidden").length).toBe(1);
  expect(await windows()).toBe(1);
  expect((await probe()).asked).toHaveLength(3);
});

test("new-tab: empty page creates no native renderer and address loads in the same display", async () => {
  ({ app } = await launch(run.env));
  expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(1);
  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setSize(1024, 640));
  const page = await app.firstWindow();
  await enterWorkspace(page, "fixture");
  expect(await openFromCli(`${origin}/a.html`, ["--reveal", "--wait"])).toMatchObject({ ok: true });
  await viewOf(`${origin}/a.html`);
  const count = (await views()).length;
  await page.locator('[data-view-new-tab]').click();
  await expect(page.locator('[data-new-tab-page]')).toBeVisible();
  const address = page.getByRole("textbox", { name: "Page address" });
  await expect(address).toBeFocused();
  await expect(address).toHaveValue("");
  expect((await views()).length).toBe(count);
  const id = await page.locator('[data-browser-display]').getAttribute("data-browser-display");
  await expect.poll(async () => (await views()).filter((view) => view.visible).length).toBe(0);
  await screenshot(page, "new-tab-native-empty-shell");
  // Let macOS present the shell frame before capturing this background window.
  await page.waitForTimeout(500);
  await windowShot("new-tab-native-empty");
  await address.fill(`${origin}/b.html`);
  await address.press("Enter");
  await expect(tab(page, "Page B")).toHaveAttribute("data-display", id!);
  await viewOf(`${origin}/b.html`);
  await expectOnSlot(page, `${origin}/b.html`, id!);
  await windowShot("new-tab-native-navigated");
});

type Zoomed = { factor: number; focused: boolean; pid: number };

/** The zoom factor of the page showing `url`, whether it holds the keyboard, and its renderer. */
function zoomOf(url: string): Promise<Zoomed> {
  return app!.evaluate(({ BrowserWindow }, url) => {
    const child = BrowserWindow.getAllWindows()[0]!.contentView.children.find((view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url) as unknown as { webContents: Electron.WebContents };
    return { factor: Math.round(child.webContents.getZoomFactor() * 100) / 100, focused: child.webContents.isFocused(), pid: child.webContents.getOSProcessId() };
  }, url);
}

/** An app-menu click, which reaches the host the way an accelerator does. */
function menuClick(id: string): Promise<void> {
  return app!.evaluate(({ Menu }, id) => Menu.getApplicationMenu()!.getMenuItemById(id)!.click(), id);
}

/** macOS virtual key codes, and each modifier's flag bit, for the keys these cases press. */
const MODIFIER_KEYS: Record<string, [code: number, flag: number]> = { control: [59, 0x40000], option: [58, 0x80000], shift: [56, 0x20000] };
const KEY_CODES: Record<string, number> = { tab: 48, escape: 53, "1": 18, "2": 19, "3": 20 };

/** Posts key events at the HID tap; with a pid, each one is refused unless that process is frontmost. */
function postEvents(pid: number | null, events: [code: number, down: boolean, modifier: boolean, flags: number][]): ReturnType<typeof spawnSync> {
  const script = `ObjC.import("CoreGraphics"); ObjC.import("AppKit");
for (const [code, down, modifier, flags] of ${JSON.stringify(events)}) {
  if (${pid !== null} && $.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier !== ${pid ?? 0}) throw new Error("candidate lost foreground");
  const event = $.CGEventCreateKeyboardEvent(null, code, down);
  if (modifier) $.CGEventSetType(event, 12);
  $.CGEventSetFlags(event, flags);
  $.CGEventPost(0, event);
  delay(0.025);
}`;
  return spawnSync("/usr/bin/osascript", ["-l", "JavaScript", "-e", script], { encoding: "utf8", timeout: 10_000 });
}

/**
 * Real macOS input to the candidate `pid`, which must be the foreground
 * process. A modifier is its own flags-changed event carrying its key code,
 * as a physical key sends it; System Events' `key down control` sends none,
 * so a page never sees Control go down or come up and a release proves nothing.
 */
function nativeKeys(pid: number, keys: string[]): void {
  expect(Number.isInteger(pid) && pid > 0, `candidate pid ${pid}`).toBe(true);
  const held = new Set(nativeModifiers);
  const flagsOf = () => [...held].reduce((sum, name) => sum | MODIFIER_KEYS[name]![1], 0);
  const events: [code: number, down: boolean, modifier: boolean, flags: number][] = [];
  for (const key of keys) {
    const [name, direction] = key.split(" ");
    const modifier = MODIFIER_KEYS[name!];
    if (modifier) {
      if (direction === "down") held.add(name!);
      else held.delete(name!);
      events.push([modifier[0], direction === "down", true, flagsOf()]);
      continue;
    }
    const code = KEY_CODES[key];
    if (code === undefined) throw new Error(`no key code for ${key}`);
    events.push([code, true, false, flagsOf()], [code, false, false, flagsOf()]);
  }
  // A modifier counts as down once its event may have been posted, so the
  // test's cleanup releases it even when the script stopped partway.
  for (const key of keys) if (key.endsWith(" down")) nativeModifiers.add(key.split(" ")[0]!);
  const result = postEvents(pid, events);
  if (result.status === 0) nativeModifiers = held;
  expect(result.status, String(result.stderr)).toBe(0);
}

/** Lets go of every modifier a case left down, wherever the keyboard now is: an up event and nothing else. */
function releaseNativeModifiers(): ReturnType<typeof spawnSync> {
  const events = [...nativeModifiers].map((name) => [MODIFIER_KEYS[name]![0], false, true, 0] as [number, boolean, boolean, number]);
  nativeModifiers = new Set();
  return postEvents(null, events);
}

test("area cycle native: page input previews one exact area, releases once and cancels", { tag: NEEDS_FOCUS }, async () => {
  ({ app } = await launch(run.env));
  const page = await app.firstWindow();
  const sent = countSent(page);
  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setSize(1280, 800));
  await enterWorkspace(page, "fixture");
  const outside = `${origin}/a.html`;
  const previous = `${origin}/b.html`;
  const current = `${origin}/korean.html`;
  for (const url of [outside, previous]) expect(await openFromCli(url, ["--reveal", "--wait"])).toMatchObject({ ok: true });
  await page.keyboard.press("Meta+KeyK");
  await page.keyboard.type("Expand side panel");
  await page.locator('[data-palette-row="command:panel:expanded"]').click();
  await expect(page.locator("[data-workspace-screen]")).toHaveAttribute("data-panel", "expanded");
  if (await page.locator('[data-tools-toggle="on"]').count()) await page.locator('[data-tools-toggle="on"]').click();
  await tab(page, "Page B").click({ button: "right" });
  await page.locator('[role=menu] [data-menu-item=split_right]').click();
  await expect(page.locator("[data-view-area-id]")).toHaveCount(2);
  expect(await openFromCli(current, ["--reveal", "--wait"])).toMatchObject({ ok: true });
  const originalId = await displayIdOf(page, "한글 브라우저");
  const previousId = await displayIdOf(page, "Page B");
  const outsideId = await displayIdOf(page, "Page A");
  const pid = app.process().pid!;
  const focus = async (url: string, displayId: string) => {
    await app!.evaluate(({ app: electron, BrowserWindow }, url) => {
      const window = BrowserWindow.getAllWindows()[0]!;
      electron.focus({ steal: true }); window.focus();
      const child = window.contentView.children.find(view => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url) as unknown as { webContents: Electron.WebContents };
      // A page already holding native focus announces nothing when focused
      // again, so the shell takes it first and the page enters as an operator's click would.
      window.webContents.focus(); child.webContents.focus();
    }, url);
    // macOS activates the window asynchronously, and an activation still in
    // flight can hand the keyboard back to the shell a moment after the page
    // took it; keys posted then reach no page. Accept the page only once it
    // has kept the keyboard in the key window for a while, refocusing it otherwise.
    const holds = () => app!.evaluate(({ BrowserWindow }, url) => {
      const window = BrowserWindow.getAllWindows()[0]!;
      const child = window.contentView.children.find(view => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url) as unknown as { webContents: Electron.WebContents };
      return window.isFocused() && child.webContents.isFocused();
    }, url);
    // Native focus is not yet a hold the shell admits. The host starts one
    // only from a page it shows, and the shell only when its keyboard owner
    // is that page's area and the core's snapshot selects the page there
    // (`web/src/keyboard.ts`). A tab click or a closed sheet just before
    // reaches both through a round trip, and the window's own activation can
    // hand the owner to the shell element it focuses, so keys posted on
    // native focus alone drew nothing.
    const admitted = async () => (await viewOf(url)).visible && (await page.evaluate((id) =>
      document.querySelector("[data-keyboard-area=true] [data-view-tab-bar] [aria-selected=true]")?.getAttribute("data-display") === id, displayId));
    await expect.poll(async () => {
      await app!.evaluate(({ app: electron, BrowserWindow }, url) => {
        const window = BrowserWindow.getAllWindows()[0]!;
        electron.focus({ steal: true }); window.focus();
        const child = window.contentView.children.find(view => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url) as unknown as { webContents: Electron.WebContents };
        window.webContents.focus(); child.webContents.focus();
      }, url);
      if (!(await holds())) return false;
      await new Promise((resolve) => setTimeout(resolve, 250));
      return (await holds()) && (await admitted());
    }).toBe(true);
  };
  const capture = async (name: string) => {
    const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (!dir) return;
    const source = await app!.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getMediaSourceId());
    const result = spawnSync("/usr/sbin/screencapture", ["-x", "-o", "-l", source.split(":")[1]!, path.join(dir, `${name}.png`)], { encoding: "utf8" });
    expect(result.status, "exact native window capture failed: " + result.stderr).toBe(0);
    fs.writeFileSync(path.join(dir, "area-native-identity.json"), JSON.stringify({ pid, window: source, daemonPid: run.daemonPid(), socket: herdr.socket, state: run.env.HIDE_STATE_DIR, userData: run.env.HIDE_DESKTOP_USER_DATA_DIR, pages: await views() }, null, 2));
  };
  await focus(current, originalId);
  await expect(page.locator('[data-keyboard-area=true]')).toHaveCount(1);
  const selections = () => page.locator('[data-view-tab-bar] [aria-selected=true]').evaluateAll(tabs => tabs.map(tab => tab.getAttribute("data-display")));
  const before = await selections();
  const commits = sent.get("view_layout") ?? 0;
  nativeKeys(pid, ["control down", "tab"]);
  await expect(page.locator('[data-cycle=area] [aria-selected=true]')).toHaveAttribute("data-cycle-row", previousId);
  await expect(page.locator(`[data-cycle-row="${outsideId}"]`)).toHaveCount(0);
  expect(await selections()).toEqual(before);
  expect(sent.get("view_layout") ?? 0).toBe(commits);
  await capture("area-native-preview");
  nativeKeys(pid, ["control up"]);
  await expect(page.locator("[data-cycle]")).toHaveCount(0);
  await expect(tab(page, "Page B")).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => sent.get("view_layout") ?? 0).toBe(commits + 1);
  await page.waitForTimeout(300);
  expect(sent.get("view_layout") ?? 0).toBe(commits + 1);
  expect(await inPage(current, "window.tabKeys")).toBe(0);
  await expect.poll(async () => (await zoomOf(previous)).focused).toBe(true);
  // A key code passes through the operator's input source, so the typing
  // uses digits: a letter is ㅋ under Korean 2-set and leaves a composition
  // open that turns the next chord into IME input.
  nativeKeys(pid, ["1"]);
  expect(await inPage(previous, "document.getElementById('q').value")).toBe("1");
  expect(await inPage(current, "document.querySelector('input').value")).toBe("한글 확인");

  // Escape from the actual native page preserves both the selection and owner.
  await tab(page, "한글 브라우저").click();
  await inPage(current, "document.querySelector('input').focus()");
  await focus(current, originalId);
  const canceled = sent.get("view_layout") ?? 0;
  nativeKeys(pid, ["control down", "tab"]);
  await expect(page.locator("[data-cycle=area]")).toBeVisible();
  nativeKeys(pid, ["escape", "control up"]);
  await expect(page.locator("[data-cycle]")).toHaveCount(0);
  await expect(tab(page, "한글 브라우저")).toHaveAttribute("aria-selected", "true");
  expect(sent.get("view_layout") ?? 0).toBe(canceled);
  await expect.poll(async () => (await zoomOf(current)).focused).toBe(true);
  await expect(page.locator('[data-keyboard-area=true]')).toHaveCount(1);
  nativeKeys(pid, ["2"]);
  // The caret sits wherever the script's focus put it, so only the landing is checked.
  expect(await inPage(current, "document.querySelector('input').value")).toContain("2");
  await capture("area-native-readable");

  // A native-window blur cancels a fresh hold; a later release cannot commit it.
  nativeKeys(pid, ["control down", "tab"]);
  await expect(page.locator("[data-cycle=area]")).toBeVisible();
  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.blur());
  await expect(page.locator("[data-cycle]")).toHaveCount(0);
  // The window coming back gives the keyboard to the page that started the hold.
  await app.evaluate(({ app: electron, BrowserWindow }) => { electron.focus({ steal: true }); BrowserWindow.getAllWindows()[0]!.focus(); });
  await expect.poll(async () => (await zoomOf(current)).focused).toBe(true);
  nativeKeys(pid, ["control up"]);
  await expect(tab(page, "한글 브라우저")).toHaveAttribute("aria-selected", "true");
  expect(sent.get("view_layout") ?? 0).toBe(canceled);
  expect(originalId).not.toBe(previousId);

  // A rebound two-modifier chord keeps the release contract: letting go of
  // Control while Option is still down commits once, and the next typing
  // reaches the chosen page rather than the page that started the hold.
  await page.locator("[data-open-settings]").click();
  await page.locator('[data-settings-tab="shortcuts"]').click();
  await page.locator('[data-shortcut-record="recent_area_tab"]').click();
  await page.keyboard.press("Control+Alt+Tab");
  // A native page's hold starts from the host's copy of the chords, which
  // the shell reports after the core stores the binding; the host takes it
  // and then rebuilds the menu in one call, so a new menu means it has it.
  await app.evaluate(({ Menu }) => { (globalThis as { menuBeforeRebind?: unknown }).menuBeforeRebind = Menu.getApplicationMenu(); });
  await page.locator('[data-shortcut-apply="recent_area_tab"]').click();
  await expect(page.locator('[data-shortcut-effective="recent_area_tab"]')).toHaveText("⌃⌥⇥");
  await expect.poll(() => app!.evaluate(({ Menu }) => Menu.getApplicationMenu() !== (globalThis as { menuBeforeRebind?: unknown }).menuBeforeRebind)).toBe(true);
  await page.keyboard.press("Escape");
  await inPage(previous, "document.getElementById('q').value = ''");
  await inPage(current, "document.querySelector('input').focus()");
  await focus(current, originalId);
  const rebound = sent.get("view_layout") ?? 0;
  const originText = await inPage(current, "document.querySelector('input').value");
  nativeKeys(pid, ["option down", "control down", "tab"]);
  await expect(page.locator('[data-cycle=area] [aria-selected=true]')).toHaveAttribute("data-cycle-row", previousId);
  expect(sent.get("view_layout") ?? 0).toBe(rebound);
  await capture("area-native-rebound-preview");
  nativeKeys(pid, ["control up"]);
  await expect(page.locator("[data-cycle]")).toHaveCount(0);
  await expect(tab(page, "Page B")).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => sent.get("view_layout") ?? 0).toBe(rebound + 1);
  nativeKeys(pid, ["option up"]);
  await expect.poll(async () => (await zoomOf(previous)).focused).toBe(true);
  nativeKeys(pid, ["3"]);
  expect(await inPage(previous, "document.getElementById('q').value")).toBe("3");
  expect(await inPage(current, "document.querySelector('input').value")).toBe(originText);
  expect(sent.get("view_layout") ?? 0).toBe(rebound + 1);
  await capture("area-native-rebound-released");
});

/** A two-finger pinch at the middle of the page showing `url`, then the page's visual zoom. */
function pinch(url: string): Promise<number> {
  return app!.evaluate(async ({ BrowserWindow }, url) => {
    const child = BrowserWindow.getAllWindows()[0]!.contentView.children.find((view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url) as unknown as { webContents: Electron.WebContents };
    const contents = child.webContents;
    contents.debugger.attach();
    try {
      await contents.debugger.sendCommand("Input.synthesizePinchGesture", { x: 100, y: 100, scaleFactor: 2, gestureSourceType: "touch" });
      return (await contents.executeJavaScript("window.visualViewport.scale")) as number;
    } finally {
      contents.debugger.detach();
    }
  }, url);
}

test("zoom: the text-size commands zoom a focused page in Chrome's steps and a pinch zooms it", { tag: NEEDS_FOCUS }, async () => {
  ({ app } = await launch(run.env));
  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setSize(1024, 640));
  const page = await app.firstWindow();
  const sent = countSent(page);
  await enterWorkspace(page, "fixture");
  const pageA = `${origin}/a.html`;
  expect(await openFromCli(pageA, ["--reveal", "--wait"])).toMatchObject({ ok: true });
  await expect.poll(async () => (await viewOf(pageA)).visible).toBe(true);
  const textScales = () => (sent.get("pane_text_scale") ?? 0) + (sent.get("editor_text_scale") ?? 0);

  // With the page holding the keyboard, each command zooms the page one of
  // Chrome's steps, the page keeps the keyboard, and no text size moves. A
  // page holds the keyboard only in the key window, so this test's window
  // comes to the front.
  await expect
    .poll(() =>
      app!.evaluate(({ app: electron, BrowserWindow }, url) => {
        const window = BrowserWindow.getAllWindows()[0]!;
        electron.focus({ steal: true });
        window.focus();
        const child = window.contentView.children.find((view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url) as unknown as { webContents: Electron.WebContents };
        child.webContents.focus();
        return child.webContents.isFocused();
      }, pageA),
    )
    .toBe(true);
  const steps: [string, number][] = [["text_larger", 1.1], ["text_larger", 1.25], ["text_smaller", 1.1], ["text_smaller", 1], ["text_smaller", 0.9], ["text_reset", 1]];
  for (const [command, factor] of steps) {
    await menuClick(command);
    expect(await zoomOf(pageA), command).toMatchObject({ factor, focused: true });
  }

  // ⌘+ (⌘⇧=) zooms in too, as in Chrome.
  await app.evaluate(({ BrowserWindow }, url) => {
    const child = BrowserWindow.getAllWindows()[0]!.contentView.children.find((view) => (view as { webContents?: Electron.WebContents }).webContents?.getURL() === url) as unknown as { webContents: Electron.WebContents };
    child.webContents.sendInputEvent({ type: "keyDown", keyCode: "=", modifiers: ["meta", "shift"] });
  }, pageA);
  await expect.poll(async () => (await zoomOf(pageA)).factor).toBe(1.1);
  await page.waitForTimeout(300);
  expect(textScales(), "a text size changed while the page held the keyboard").toBe(0);

  // With the shell holding the keyboard, the same command sizes the terminal's text and leaves the page alone.
  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.webContents.focus());
  await page.locator("[data-pane-view] .xterm-helper-textarea").first().focus();
  await expect.poll(async () => (await zoomOf(pageA)).focused).toBe(false);
  await menuClick("text_larger");
  await expect.poll(textScales).toBe(1);
  expect((await zoomOf(pageA)).factor).toBe(1.1);
  await menuClick("text_reset");
  await expect.poll(textScales).toBe(2);

  // A pinch zooms the page, also after it navigates into another renderer.
  expect(await pinch(pageA)).toBeGreaterThan(1.2);
  const before = (await zoomOf(pageA)).pid;
  const moved = `${origin.replace("127.0.0.1", "localhost")}/b.html`;
  await inPage(pageA, `location.href = ${JSON.stringify(moved)}`);
  await viewOf(moved);
  expect((await zoomOf(moved)).pid, "navigation did not swap the renderer").not.toBe(before);
  await expect.poll(() => inPage<number>(moved, "window.visualViewport.scale")).toBe(1);
  expect(await pinch(moved)).toBeGreaterThan(1.2);
  await windowShot("browser-zoom-pinched");
});
