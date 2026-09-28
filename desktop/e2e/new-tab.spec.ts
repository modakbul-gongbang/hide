import { expect, test, type ElectronApplication, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace, showExplorer } from "../../web/e2e/wire";
import { isolate, launch } from "./fixture";

const TITLE = "New tab page";

/**
 * Enters the native page the way the operator does and waits until the shell
 * has heard of it. The page lives in native child contents, outside the shell
 * DOM, and its host reports the focus; the shell records it before any later
 * listener on that channel runs.
 */
async function enterPage(app: ElectronApplication, page: Page) {
  const heard = page.evaluate(() => new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("the shell never heard the page take the keyboard")), 10_000);
    const off = window.hideHost!.browser.onEvent((event) => {
      if (event.kind === "focus") { clearTimeout(timer); off(); resolve(); }
    });
  }));
  await expect.poll(() => app.evaluate(({ app: electron, BrowserWindow }, title) => {
    const window = BrowserWindow.getAllWindows()[0]!;
    const view = window.contentView.children.find((child) =>
      (child as { webContents?: Electron.WebContents }).webContents?.getTitle() === title);
    if (!view) return false;
    // A page holds the keyboard only in the key window; where the machine
    // grants it, the menu command below also takes the host's hand-back path.
    // A click in the shell DOM does not move the contents' focus, so the shell
    // takes it first and the page reports entering again.
    electron.focus({ steal: true });
    window.focus();
    window.webContents.focus();
    (view as unknown as { webContents: Electron.WebContents }).webContents.focus();
    return true;
  }, TITLE)).toBe(true);
  await heard;
}

/**
 * What the browser does when the host hands the keyboard back to the shell
 * to deliver a menu command: it announces focus again on the element that had
 * it before the page took the keyboard, here the terminal's input.
 */
async function handBack(page: Page) {
  const inPane = await page.evaluate(() => {
    const element = document.activeElement!;
    element.dispatchEvent(new FocusEvent("focus"));
    element.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    return element.closest("[data-pane-view]") !== null;
  });
  expect(inPane).toBe(true);
}

const menuClick = (app: ElectronApplication, id: string) =>
  app.evaluate(({ Menu }, command) => Menu.getApplicationMenu()!.getMenuItemById(command)!.click(), id);

test("Command T and Command W from a native page act on its View area, not the pane that had the keyboard before", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "new-tab");
  let app: ElectronApplication | null = null;
  try {
    fs.writeFileSync(path.join(herdr.root, "fixture", "page.html"), `<!doctype html><meta charset="utf-8"><title>${TITLE}</title><h1>Browser keyboard owner</h1>`);
    const launched = await launch(run.env, { switches: ["--disable-backgrounding-occluded-windows"] });
    app = launched.app;
    const page = launched.page;
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setSize(1600, 1000));
    await enterWorkspace(page);
    const agentTabs = page.locator('[data-agent-tab-bar] [role="tab"]');
    const viewTabs = page.locator('[data-view-tab-bar] [role="tab"]');
    const panes = page.locator("[data-pane-view]");
    const terminal = page.locator(`[data-terminal-host="${herdr.panes[0]}"]`);
    await expect(agentTabs).toHaveCount(1);
    await showExplorer(page);
    await page.locator('[data-explorer-row$="/page.html"]').click({ button: "right" });
    await page.locator('[data-explorer-menu] [data-menu-item="open-browser"]').click();
    await expect(page.locator("[data-browser-slot]")).toBeVisible();

    // Command T: the page's area gets the View strip's New tab.
    await terminal.click();
    await enterPage(app, page);
    await handBack(page);
    await menuClick(app, "new_tab");
    await expect(viewTabs).toHaveCount(2);
    await expect(page.getByRole("textbox", { name: "Page address" })).toBeFocused();
    await expect(page.getByRole("textbox", { name: "Page address" })).toHaveValue("");
    await expect(agentTabs).toHaveCount(1);

    // Command W: the page's display closes and every pane stays.
    await viewTabs.filter({ hasText: TITLE }).click();
    await expect(page.locator("[data-browser-slot]")).toBeVisible();
    await terminal.click();
    await enterPage(app, page);
    await handBack(page);
    await menuClick(app, "close_tab");
    await expect(viewTabs).toHaveCount(1);
    await expect(viewTabs.filter({ hasText: TITLE })).toHaveCount(0);
    await expect(panes).toHaveCount(2);

    // A click back into the terminal is the operator's move: Command T is an agent tab again.
    await terminal.click();
    await menuClick(app, "new_tab");
    await expect(agentTabs).toHaveCount(2);
    await expect(viewTabs).toHaveCount(1);
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});
