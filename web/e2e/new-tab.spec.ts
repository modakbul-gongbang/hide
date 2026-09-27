import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot, showExplorer } from "./wire";

test("View new tab replaces itself with a file or changed-file diff", async ({ page }) => {
  test.setTimeout(120_000);
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const root = path.join(herdr.root, "fixture");
    const git = (...args: string[]) => execFileSync("git", ["-C", root, ...args], { env: herdr.env });
    fs.writeFileSync(path.join(root, "note.txt"), "original\n");
    git("init", "-q"); git("add", ".");
    git("-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-qm", "Initial files");
    daemon = await startHided(herdr, "new-tab");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await showExplorer(page);
    // The first view gives the panel a View strip.
    await page.locator('[data-explorer-row$="/note.txt"]').click();
    await expect(page.locator('[data-view-tab-bar] [role="tab"]')).toHaveCount(1);
    await page.locator('[data-view-new-tab]').click();
    const body = page.locator('[data-new-tab-page]');
    await expect(body.getByRole("heading", { name: "Open" })).toBeVisible();
    await expect(page.getByRole("textbox", { name: "Page address" })).toBeFocused();
    await expect(page.getByRole("textbox", { name: "Page address" })).toHaveValue("");
    await expect(body.getByRole("button", { name: "Diff", exact: true })).toHaveCount(0);
    const displayId = await page.locator('[data-browser-display]').getAttribute("data-browser-display");
    await screenshot(page, "new-tab-clean");
    await body.getByRole("button", { name: "File" }).click();
    await page.locator('[data-palette-input]').fill("note.txt");
    await page.locator('[data-palette-row$="/note.txt"]').click();
    await expect(body).toHaveCount(0);
    // Coalesce the existing view in this area into the tab the user chose.
    await expect(page.locator('[data-view-tab-bar] [role="tab"]')).toHaveCount(1);
    await expect(page.locator(`[role="tab"][data-display="${displayId}"]`)).toContainText("note.txt");
    await screenshot(page, "new-tab-file");

    fs.writeFileSync(path.join(root, "note.txt"), "changed\n");
    await page.locator('[data-view-new-tab]').click();
    await expect(body.getByRole("button", { name: "Diff", exact: true })).toBeVisible({ timeout: 15_000 });
    const diffId = await page.locator('[data-browser-display]').getAttribute("data-browser-display");
    await screenshot(page, "new-tab-changed");
    await body.getByRole("button", { name: "Diff", exact: true }).click();
    await expect(page.locator('[data-palette="Open diff"]')).toBeVisible();
    await page.locator('[data-palette-row$="/note.txt"]').click();
    await expect(body).toHaveCount(0);
    await expect(page.locator(`[role="tab"][data-display="${diffId}"]`)).toHaveAttribute("data-tab-kind", "diff");

    await page.locator('[data-view-new-tab]').click();
    const emptyId = await page.locator('[data-browser-display]').getAttribute("data-browser-display");
    await page.locator(`[role="tab"][data-display="${emptyId}"]`).click({ button: "right" });
    await page.getByRole("menuitem", { name: "New tab", exact: true }).click();
    await expect(page.locator('[data-view-tab-bar] [role="tab"]')).toHaveCount(4);
    const address = page.getByRole("textbox", { name: "Page address" });
    await address.fill("https://example.com"); await address.press("Enter");
    await expect(page.locator('[data-browser-address]')).toHaveText("example.com");
    // Cmd+P remains a file palette outside the new-tab page.
    await page.keyboard.press("Meta+p");
    await expect(page.locator('[data-palette="Open file"]')).toBeVisible();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
