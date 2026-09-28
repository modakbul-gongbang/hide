import { expect, test, type ElectronApplication } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace, showExplorer } from "../../web/e2e/wire";
import { isolate, launch, NEEDS_FOCUS } from "./fixture";

test("Command W closes the keyboard's display or pane, never its tab", { tag: NEEDS_FOCUS }, async () => {
  const herdr = await startHerdr();
  const run = isolate(herdr, "close-policy");
  let app: ElectronApplication | null = null;
  try {
    fs.writeFileSync(path.join(herdr.root, "fixture", "notes.md"), "# Notes\n\n한글과 English\n");
    fs.writeFileSync(path.join(herdr.root, "fixture", "page.html"), '<!doctype html><meta charset="utf-8"><title>Close page</title><h1>Browser keyboard owner</h1><input aria-label="Page input">');
    const launched = await launch(run.env);
    app = launched.app;
    const page = launched.page;
    const identity = await app.evaluate(({ BrowserWindow }) => {
      const windows = BrowserWindow.getAllWindows();
      if (windows.length !== 1) throw new Error("expected exactly one candidate window");
      const window = windows[0]!;
      window.setSize(1600, 1000);
      window.blur();
      return { pid: process.pid, executable: process.execPath, window: window.getMediaSourceId() };
    });
    const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
    await enterWorkspace(page);
    if (evidence) fs.writeFileSync(path.join(evidence, "native-close-identity.json"), JSON.stringify({ ...identity, daemonPid: run.daemonPid(), state: run.env.HIDE_STATE_DIR, socket: herdr.socket }, null, 2));
    const capture = (name: string) => {
      if (evidence && process.platform === "darwin") {
        const id = identity.window.split(":")[1]!;
        execFileSync("/usr/sbin/screencapture", ["-x", "-o", "-l", id, path.join(evidence, `${name}.png`)]);
      }
    };
    await showExplorer(page);
    await page.locator('[data-explorer-row$="/notes.md"]').click();
    const editor = page.locator("[data-editor-body] .cm-content");
    await expect(editor).toBeVisible();
    await editor.click();
    capture("native-close-view-before");
    await page.keyboard.press("Meta+KeyW");
    await expect(editor).toHaveCount(0);
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    await expect(page.locator("[data-side-panel]")).toHaveAttribute("data-panel-content", "tools");
    capture("native-close-view-tools");
    // Browser pages live in native child contents, outside the shell DOM.
    await page.locator('[data-explorer-row$="/page.html"]').click({ button: "right" });
    await page.locator('[data-explorer-menu] [data-menu-item="open-browser"]').click();
    await expect(page.locator("[data-browser-slot]")).toBeVisible();
    // A page holds the keyboard only in the key window, so this test's window comes to the front.
    await expect.poll(() => app!.evaluate(({ app: electron, BrowserWindow }) => {
      const window = BrowserWindow.getAllWindows()[0]!;
      const view = window.contentView.children.find((child) =>
        (child as { webContents?: Electron.WebContents }).webContents?.getTitle() === "Close page");
      if (!view) return false;
      electron.focus({ steal: true });
      window.focus();
      const contents = (view as unknown as { webContents: Electron.WebContents }).webContents;
      contents.focus();
      return contents.isFocused();
    })).toBe(true);
    // The native application menu uses the same Command W command while a
    // child page owns focus; it must not close the agents behind that page.
    await app.evaluate(({ Menu }) => {
      const item = Menu.getApplicationMenu()!.getMenuItemById("close_tab")!;
      item.click();
    });
    await expect(page.locator("[data-browser-slot]")).toHaveCount(0);
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    await page.locator('[data-tool-tab="explorer"]').focus();
    await page.keyboard.press("Meta+KeyW");
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    await page.keyboard.press("Meta+KeyE");
    await expect(page.locator("[data-side-panel]")).toHaveCount(0);
    await page.locator(`[data-terminal-host="${herdr.panes[1]}"]`).click();
    await page.keyboard.press("Meta+KeyW");
    await expect(page.locator(`[data-pane-view="${herdr.panes[1]}"]`)).toHaveCount(0, { timeout: 15_000 });
    await expect(page.locator(`[data-pane-view="${herdr.panes[0]}"]`)).toBeVisible();
    capture("native-close-pane-survivor");
    await page.locator(`[data-terminal-host="${herdr.panes[0]}"]`).click();
    await page.keyboard.press("Meta+KeyW");
    await expect(page.locator(`[data-tab="${herdr.tab}"]`)).toHaveCount(0);
    await expect(page.locator("[data-pane-view]")).toHaveCount(0);
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});
