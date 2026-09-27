import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot, showExplorer } from "./wire";

test("close follows the keyboard and the panel never renders an empty surface", async ({ page }) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1920, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    fs.writeFileSync(path.join(herdr.root, "fixture", "notes.md"), "# Notes\n\n한글과 English\n");
    daemon = await startHided(herdr, "close-policy");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page);
    await showExplorer(page);
    const panes = page.locator("[data-pane-view]");
    const panel = page.locator("[data-side-panel]");
    const file = page.locator('[data-explorer-row$="/notes.md"]');
    await file.click();
    const editor = page.locator("[data-editor-body] .cm-content");
    await expect(editor).toBeVisible();
    await editor.click();
    await screenshot(page, "close-view-before");
    // Browser column: Alt W; Electron uses the same command with Command W.
    await page.keyboard.press("Alt+KeyW");
    await expect(editor).toHaveCount(0);
    await expect(panes).toHaveCount(2);
    await expect(panel).toHaveAttribute("data-panel-content", "tools");
    await screenshot(page, "close-view-tools");

    // Tools own the keyboard: no View or terminal close is allowed.
    await page.locator('[data-tool-tab="explorer"]').focus();
    await page.keyboard.press("Alt+KeyW");
    await expect(panes).toHaveCount(2);
    await expect(panel).toHaveAttribute("data-panel-content", "tools");
    await file.click();
    await expect(editor).toBeVisible();
    await page.keyboard.press("Meta+KeyE");
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    // Hiding and restoring the whole panel preserves the hidden column.
    await page.keyboard.press("Meta+Shift+KeyB");
    await expect(panel).toHaveCount(0);
    await page.locator(`[data-terminal-host="${herdr.panes[0]}"]`).click();
    await page.setViewportSize({ width: 700, height: 1000 });
    await page.keyboard.press("Meta+Shift+KeyB");
    await expect(editor).toBeVisible();
    await expect(page.locator("[data-agent-area]")).toHaveAttribute("inert", "");
    // A terminal made inert by a covering panel no longer owns the chord.
    await page.keyboard.press("Alt+KeyW");
    await expect(panes).toHaveCount(2);
    await expect(editor).toBeVisible();
    await page.keyboard.press("Meta+KeyE");
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    await page.keyboard.press("Meta+Shift+KeyB");
    await expect(panel).toHaveCount(0);
    await page.keyboard.press("Meta+Shift+KeyB");
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    await page.keyboard.press("Meta+KeyE");
    await page.setViewportSize({ width: 1920, height: 1000 });
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    await page.keyboard.press("Meta+KeyE");
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    await editor.click();
    await page.keyboard.press("Alt+KeyW");
    await expect(panel).toHaveCount(0);
    await expect(panes).toHaveCount(2);
    await expect(page.getByText("No file or diff is open in this Workspace.")).toHaveCount(0);

    // An unfocused toolbar control has no closable owner.
    await page.locator("[data-panel-toggle]").focus();
    await page.keyboard.press("Alt+KeyW");
    await expect(panes).toHaveCount(2);
    const [first, second] = herdr.panes;
    await page.locator(`[data-terminal-host="${second}"]`).click();
    await page.keyboard.press("Alt+KeyW");
    const confirm = page.getByRole("button", { name: "Stop work and close" });
    if (await confirm.isVisible()) await confirm.click();
    await expect(page.locator(`[data-pane-view="${second}"]`)).toHaveCount(0, { timeout: 15_000 });
    await expect(page.locator(`[data-pane-view="${first}"]`)).toBeVisible();
    await expect(panes).toHaveCount(1);
    await screenshot(page, "close-pane-survivor");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
