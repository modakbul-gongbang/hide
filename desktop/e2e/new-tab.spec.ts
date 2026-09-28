import { expect, test, type ElectronApplication } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace, showExplorer } from "../../web/e2e/wire";
import { isolate, launch } from "./fixture";

test("Command T from the menu opens the new tab in the View area whose native page holds the keyboard", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "new-tab");
  let app: ElectronApplication | null = null;
  try {
    fs.writeFileSync(path.join(herdr.root, "fixture", "page.html"), '<!doctype html><meta charset="utf-8"><title>New tab page</title><h1>Browser keyboard owner</h1>');
    const launched = await launch(run.env, { switches: ["--disable-backgrounding-occluded-windows"] });
    app = launched.app;
    const page = launched.page;
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setSize(1600, 1000));
    await enterWorkspace(page);
    const agentTabs = page.locator('[data-agent-tab-bar] [role="tab"]');
    await expect(agentTabs).toHaveCount(1);
    await showExplorer(page);
    await page.locator('[data-explorer-row$="/page.html"]').click({ button: "right" });
    await page.locator('[data-explorer-menu] [data-menu-item="open-browser"]').click();
    await expect(page.locator("[data-browser-slot]")).toBeVisible();
    // The page lives in native child contents, outside the shell DOM; its
    // host reports the focus, and Command T reaches the shell as a menu click.
    await expect.poll(() => app!.evaluate(({ BrowserWindow }) => {
      const view = BrowserWindow.getAllWindows()[0]!.contentView.children.find((child) =>
        (child as { webContents?: Electron.WebContents }).webContents?.getTitle() === "New tab page");
      if (!view) return false;
      (view as unknown as { webContents: Electron.WebContents }).webContents.focus();
      return true;
    })).toBe(true);
    await app.evaluate(({ Menu }) => Menu.getApplicationMenu()!.getMenuItemById("new_tab")!.click());
    await expect(page.locator('[data-view-tab-bar] [role="tab"]')).toHaveCount(2);
    await expect(page.getByRole("textbox", { name: "Page address" })).toHaveValue("");
    await expect(agentTabs).toHaveCount(1);
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});
