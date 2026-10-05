import { expect, type ElectronApplication, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { spawnAgent, startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { enterWorkspace, showExplorer } from "../../web/e2e/wire";
import { isolate, launch, NEEDS_FOCUS, test } from "./fixture";

type Session = { herdr: HerdrFixture; app: ElectronApplication; page: Page; capture: (name: string) => void };

/** A fresh Herdr, hided and app in the fixture workspace with the Explorer shown; always torn down. */
async function inWorkspace(label: string, body: (session: Session) => Promise<void>): Promise<void> {
  const herdr = await startHerdr();
  const run = isolate(herdr, label);
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
      return { pid: process.pid, executable: process.execPath, window: window.getMediaSourceId() };
    });
    const evidence = process.env.HIDE_E2E_SCREENSHOT_DIR;
    await enterWorkspace(page);
    // macOS keeps the window within the screen, which on a CI runner is
    // about 1024 wide; zoomed out, the body still reaches the wide step, so
    // File Views and Tools show side by side as these steps assume (PRD
    // three-column-panel D-07).
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.webContents.setZoomFactor(0.6));
    await expect(page.locator("[data-workspace-screen]")).toHaveAttribute("data-workspace-body", "wide");
    if (evidence) fs.writeFileSync(path.join(evidence, `${label}-identity.json`), JSON.stringify({ ...identity, daemonPid: run.daemonPid(), state: run.env.HIDE_STATE_DIR, socket: herdr.socket }, null, 2));
    const capture = (name: string) => {
      if (evidence && process.platform === "darwin") {
        const id = identity.window.split(":")[1]!;
        execFileSync("/usr/sbin/screencapture", ["-x", "-o", "-l", id, path.join(evidence, `${name}.png`)]);
      }
    };
    await showExplorer(page);
    await body({ herdr, app, page, capture });
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
}

test("Command W closes the keyboard's display or pane, never its tab", async () => {
  await inWorkspace("close-policy", async ({ herdr, page, capture }) => {
    await page.locator('[data-explorer-row$="/notes.md"]').click();
    const editor = page.locator("[data-editor-body] .cm-content");
    await expect(editor).toBeVisible();
    await editor.click();
    capture("native-close-view-before");
    await page.keyboard.press("Meta+KeyW");
    await expect(editor).toHaveCount(0);
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    // The last view closed turns File Views off and leaves Tools showing.
    await expect(page.locator('[data-column="views"]')).toHaveCount(0);
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    capture("native-close-view-tools");
    await page.locator('[data-tool-tab="explorer"]').focus();
    await page.keyboard.press("Meta+KeyW");
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    await page.keyboard.press("Meta+KeyE");
    await expect(page.locator('[data-column="tools"]')).toHaveCount(0);
    await page.locator(`[data-terminal-host="${herdr.panes[1]}"]`).click();
    await page.keyboard.press("Meta+KeyW");
    await expect(page.locator(`[data-pane-view="${herdr.panes[1]}"]`)).toHaveCount(0, { timeout: 15_000 });
    await expect(page.locator(`[data-pane-view="${herdr.panes[0]}"]`)).toBeVisible();
    capture("native-close-pane-survivor");
    await page.locator(`[data-terminal-host="${herdr.panes[0]}"]`).click();
    await page.keyboard.press("Meta+KeyW");
    await expect(page.locator(`[data-tab="${herdr.tab}"]`)).toHaveCount(0);
    await expect(page.locator("[data-pane-view]")).toHaveCount(0);
  });
});

test("Command W from the app menu closes a browser page that holds the keyboard, never the agents behind it", { tag: NEEDS_FOCUS }, async () => {
  await inWorkspace("close-policy-page", async ({ app, page }) => {
    // Browser pages live in native child contents, outside the shell DOM.
    await page.locator('[data-explorer-row$="/page.html"]').click({ button: "right" });
    await page.locator('[data-explorer-menu] [data-menu-item="open-browser"]').click();
    await expect(page.locator("[data-browser-slot]")).toBeVisible();
    // A page holds the keyboard only in the key window, so this test's window comes to the front.
    // eslint-disable-next-line hide-e2e/no-action-in-poll -- #ISSUE retried interaction: native focus lands after the page is shown
    await expect.poll(() => app.evaluate(({ app: electron, BrowserWindow }) => {
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
  });
});

test("Command W on an agent that spawned others asks once and closes the subtree on Enter", async () => {
  await inWorkspace("close-subtree", async ({ herdr, page, capture }) => {
    const target = herdr.panes[1];
    const child = await spawnAgent(herdr, "child", target);
    const grandchild = await spawnAgent(herdr, "grandchild", child);
    await page.keyboard.press("Meta+KeyE");
    await expect(page.locator('[data-column="tools"]')).toHaveCount(0);
    await page.locator('[data-sidebar-mode="agents"]').click();
    await page.locator(`[data-agent-tree-toggle="${target}"]`).click({ timeout: 30_000 });
    await expect(page.locator(`[data-agent-tree-toggle="${child}"]`)).toBeVisible({ timeout: 30_000 });
    await page.locator(`[data-terminal-host="${target}"]`).click();
    await page.keyboard.press("Meta+KeyW");
    const sheet = page.locator("[data-confirm-subtree]");
    await expect(sheet.getByRole("heading")).toHaveText("Close this agent and 2 children?");
    await expect(sheet.locator("[data-subtree-row]")).toHaveCount(3);
    await expect(sheet.locator("[data-subtree-close-all]")).toBeFocused();
    capture("native-close-subtree-sheet");
    await page.keyboard.press("Enter");
    for (const pane of [grandchild, child, target]) await expect(page.locator(`[data-pane="${pane}"]`)).toHaveCount(0, { timeout: 30_000 });
    await expect(page.locator(`[data-pane-view="${herdr.panes[0]}"]`)).toBeVisible();
    capture("native-close-subtree-closed");
  });
});
