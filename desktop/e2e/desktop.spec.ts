// The desktop app end to end: a private Herdr server with two fake agent
// panes, a private hided the app starts through `hide connect`, and the
// real Electron build. Each test owns its state directory and so its daemon.

import { expect, test, type ElectronApplication, type Page } from "@playwright/test";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import "../../web/src/host";
import { countSent, enterWorkspace } from "../../web/e2e/wire";
import { DESKTOP_DIR, HIDE_CLI, hostLog, isolate, launch, screenshot, type Isolated } from "./fixture";

let herdr: HerdrFixture;
let run: Isolated;
let app: ElectronApplication | null = null;

test.beforeAll(async () => {
  herdr = await startHerdr();
});

test.afterAll(() => {
  herdr?.stop();
});

test.beforeEach(() => {
  run = isolate(herdr, test.info().title.split(":")[0]!);
});

test.afterEach(async () => {
  const info = test.info();
  // The host's log says which discovery step failed; it goes with the failure.
  if (info.status !== info.expectedStatus) console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
});

async function shellShown(page: Page): Promise<void> {
  await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 30_000 });
}

test("attach: the app starts hided, shows the shell, and runs the native chords and the menu", async () => {
  ({ app } = await launch(run.env));
  const page = await app.firstWindow();
  const sent = countSent(page);
  // B1: the host's own screen while it looks, then the shell hided serves.
  await shellShown(page);
  expect(new URL(page.url()).hostname).toBe("127.0.0.1");
  expect(run.daemonPid()).not.toBeNull();
  await enterWorkspace(page);
  await expect(page.locator("[data-pane-view]").first()).toBeVisible();
  await expect(page.locator("[data-pane-view] .xterm").first()).toBeVisible();
  await screenshot(page, "desktop-attached");

  // B11: no Node API in the page; the bridge is the host kind, the menu channel and the pane-chord report.
  expect(
    await page.evaluate(() => ({
      require: typeof (globalThis as { require?: unknown }).require,
      process: typeof (globalThis as { process?: unknown }).process,
      bridge: Object.keys(window.hideHost ?? {}).sort(),
      kind: window.hideHost?.kind,
    })),
  ).toEqual({ require: "undefined", process: "undefined", bridge: ["kind", "onCommand", "reportBindings"], kind: "electron" });

  // B9: ⌘T is one create_tab here; the browser's ⌥T is not a chord in the app.
  const tabs = await page.locator("[role=tab]").count();
  await page.locator("[data-pane-view] .xterm-helper-textarea").first().focus();
  await page.keyboard.press("Meta+KeyT");
  await expect(page.locator("[role=tab]")).toHaveCount(tabs + 1);
  await expect.poll(() => sent.get("create_tab")).toBe(1);
  await page.keyboard.press("Alt+KeyT");
  // A menu click runs the same action through the bridge.
  await app.evaluate(({ Menu }) => Menu.getApplicationMenu()?.getMenuItemById("new_tab")?.click());
  await expect(page.locator("[role=tab]")).toHaveCount(tabs + 2);
  await expect.poll(() => sent.get("create_tab")).toBe(2);

  // B9: the ⌘/ sheet lists this host's chords, with no "moved for Chrome" note.
  await page.keyboard.press("Meta+Slash");
  const sheet = page.locator("[data-shortcut-sheet]");
  await expect(sheet).toBeVisible();
  await expect(sheet.getByText("desktop app")).toBeVisible();
  await expect(sheet.locator('[data-shortcut="new_tab"] kbd')).toHaveText("⌘T");
  await expect(sheet.locator('[data-shortcut="close_tab"] kbd')).toHaveText("⌘W");
  await expect(sheet.getByText("moved for Chrome")).toHaveCount(0);
  await screenshot(page, "desktop-shortcut-sheet");
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
});

test("links: external links open in the default browser and the window stays on the daemon", async () => {
  ({ app } = await launch(run.env));
  const page = await app.firstWindow();
  await shellShown(page);
  const origin = new URL(page.url()).origin;
  await app.evaluate(({ shell }) => {
    const opened: string[] = [];
    (globalThis as { opened?: string[] }).opened = opened;
    shell.openExternal = async (url: string) => {
      opened.push(url);
    };
  });
  // B10: a new window never opens; its URL goes to the default browser.
  expect(await page.evaluate(() => window.open("https://example.com/window") === null)).toBe(true);
  // A navigation off the daemon origin is refused and handed over the same way.
  await page.evaluate(() => {
    location.href = "https://example.com/navigate";
  });
  await expect.poll(() => app!.evaluate(() => (globalThis as { opened?: string[] }).opened)).toEqual([
    "https://example.com/window",
    "https://example.com/navigate",
  ]);
  // Playwright keeps waiting on the refused navigation, so the page is read directly.
  expect(await page.evaluate(() => [location.origin, !!document.querySelector("[data-main-screen], [data-workspace-screen]")])).toEqual([origin, true]);
});

test("lifetime: a second launch focuses the first, closing the window keeps the app, and quitting leaves hided for the next launch", async () => {
  ({ app } = await launch(run.env));
  let page = await app.firstWindow();
  await shellShown(page);
  const pid = run.daemonPid();
  expect(pid).not.toBeNull();

  // B6: a second launch on the same profile exits and the first comes forward.
  const electronBinary = fs.readFileSync(path.join(DESKTOP_DIR, "node_modules", "electron", "path.txt"), "utf8");
  const second = spawn(path.join(DESKTOP_DIR, "node_modules", "electron", "dist", electronBinary), [DESKTOP_DIR], { env: run.env, stdio: "ignore" });
  const code = await new Promise<number | null>((resolve) => second.once("exit", resolve));
  expect(code).toBe(0);
  await expect.poll(() => hostLog(run.env).some((line) => line.event === "host.reopen" && line.trigger === "second-instance")).toBe(true);
  expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(1);

  // B7: closing the last window keeps the app; a Dock click (activate) brings a window back.
  await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.close());
  await expect.poll(() => app!.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().length)).toBe(0);
  const reopened = app.waitForEvent("window");
  await app.evaluate(({ app: electronApp }) => electronApp.emit("activate"));
  page = await reopened;
  await shellShown(page);
  expect(run.daemonPid()).toBe(pid);

  // B8: the size and position come back on the next launch.
  // Fitted inside this machine's primary work area (a CI runner's display is
  // smaller than a workstation's), and the expectation is what macOS actually
  // placed, since the window manager may still adjust a requested rect.
  const bounds = await app.evaluate(({ BrowserWindow, screen }) => {
    const area = screen.getPrimaryDisplay().workArea;
    const window = BrowserWindow.getAllWindows()[0]!;
    window.setBounds({
      x: area.x + 20,
      y: area.y + 20,
      width: Math.min(1000, area.width - 40),
      height: Math.min(700, area.height - 40),
    });
    return window.getNormalBounds();
  });

  // B5: quitting the app leaves the daemon; the next launch attaches to it.
  await app.close();
  app = null;
  expect(run.daemonPid()).toBe(pid);
  ({ app } = await launch(run.env));
  page = await app.firstWindow();
  await shellShown(page);
  expect(run.daemonPid()).toBe(pid);
  expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getBounds())).toEqual(bounds);
  // First launch, the reopened window (bounds written when the first closed), then the relaunch.
  expect(hostLog(run.env).filter((line) => line.event === "window.bounds").map((line) => line.source)).toEqual(["default", "stored", "stored"]);
});

test("failure: a missing CLI shows its reason and Retry attaches once it exists", async () => {
  const cli = path.join(run.root, "bin", "hide");
  ({ app } = await launch({ ...run.env, HIDE_CLI_PATH: cli }));
  const page = await app.firstWindow();
  // B2, B3: one screen, the reason category, and Retry; the tried path is logged.
  await expect(page.locator("#reason")).toHaveText("The hide command was not found.", { timeout: 20_000 });
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
  await screenshot(page, "desktop-cli-missing");
  expect(hostLog(run.env).find((line) => line.event === "cli.missing")?.tried).toBe(cli);
  fs.mkdirSync(path.dirname(cli), { recursive: true });
  fs.symlinkSync(HIDE_CLI, cli);
  await page.getByRole("button", { name: "Retry" }).click();
  await shellShown(page);
});

test("reattach: a daemon that dies shows the shell's disconnected state, and the app follows the next one", async () => {
  ({ app } = await launch(run.env));
  const page = await app.firstWindow();
  await shellShown(page);
  const first = new URL(page.url()).origin;
  // B4: the shell's own disconnected state; the host reloads nothing.
  run.hide(["stop"]);
  await expect(page.locator("[data-connection]")).toBeVisible({ timeout: 20_000 });
  expect(new URL(page.url()).origin).toBe(first);
  await screenshot(page, "desktop-daemon-lost");
  // Another client starts hided again, on a new port with a new token.
  expect(run.hide(["connect"]).status).toBe(0);
  await expect.poll(() => new URL(page.url()).origin, { timeout: 20_000 }).not.toBe(first);
  await shellShown(page);
  await expect(page.locator("[data-connection]")).toHaveCount(0);
});
