// Recent navigation on an isolated pinned Herdr and hided
// (docs/UI_BEHAVIOR.md, Recent navigation): Recent Panels (⌥` in a browser)
// walks one order over Herdr tabs and View displays across checkouts and
// commits one event on releasing ⌥; Recent Projects (⌥Tab) brings the
// previous project back on the surface it was last used on.

import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });
test.use({ actionTimeout: 15_000 });

test("Recent Panels crosses checkouts onto a display and a tab; Recent Projects restores the last surface", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    fs.writeFileSync(path.join(herdr.root, "fixture", "plan.txt"), "plan line\n");
    fs.mkdirSync(path.join(herdr.root, "beta"), { recursive: true });
    const beta = herdr.run([
      "workspace", "create", "--cwd", path.join(herdr.root, "beta"), "--label", "beta", "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]) as { result: { tab: { tab_id: string } } };
    const betaTab = beta.result.tab.tab_id;
    daemon = await startHided(herdr, "recent");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const root = path.join(fs.realpathSync(herdr.root), "fixture");
    const canvas = page.locator("[data-canvas]").first();
    const cycleRow = page.locator("[data-cycle] [aria-selected=true]");

    // plan.txt pinned in the fixture Workspace's View area, the keyboard in it.
    await page.locator(`[data-explorer-row="${path.join(root, "plan.txt")}"]`).dblclick();
    const editor = page.locator("[data-view-area-id] [data-editor-body] .cm-content").first();
    await expect(editor).toContainText("plan line");
    await editor.click();
    const display = await page.locator('[data-view-tab-bar] [role="tab"][data-display][aria-selected="true"]').first().getAttribute("data-display");
    expect(display).toBeTruthy();

    // Then beta's terminal, from the sidebar.
    await page.locator('[data-sidebar-mode="projects"]').click();
    await page.locator("[data-project]", { hasText: "beta" }).locator("[data-checkout]").first().click();
    await expect(canvas).toHaveAttribute("data-canvas", betaTab);

    // ⌥` once, held: the previous surface is the other checkout's display.
    const focusCheckouts = sent.get("focus_checkout") ?? 0;
    await page.keyboard.down("Alt");
    await page.keyboard.press("Backquote");
    await expect(cycleRow).toHaveAttribute("data-cycle-row", display!);
    await expect(cycleRow).toHaveAttribute("data-cycle-kind", "file");
    await expect(page.locator("[data-cycle=panels]")).toContainText("Recent Panels");
    await expect(page.locator("[data-cycle=panels] [role=option]").first()).toContainText("beta · Terminal");
    await screenshot(page, "recent-panels");
    expect(sent.get("focus_checkout") ?? 0).toBe(focusCheckouts);

    // Releasing ⌥ is one focus_checkout naming the display; the keyboard lands in it.
    await page.keyboard.up("Alt");
    await expect(page.locator("[data-cycle]")).toHaveCount(0);
    await expect.poll(() => sent.get("focus_checkout") ?? 0).toBe(focusCheckouts + 1);
    expect(last.get("focus_checkout")).toMatchObject({ display_id: display });
    await expect.poll(() => page.evaluate(() => document.activeElement?.closest("[data-view-area]") !== null)).toBe(true);
    await expect(page.locator('[data-view-tab-bar] [role="tab"][aria-selected="true"]').first()).toHaveAttribute("data-display", display!);

    // ⌥` again goes straight back to beta's tab: one focus_tab across checkouts.
    const focusTabs = sent.get("focus_tab") ?? 0;
    await page.keyboard.down("Alt");
    await page.keyboard.press("Backquote");
    await expect(cycleRow).toHaveAttribute("data-cycle-row", betaTab);
    await page.keyboard.up("Alt");
    await expect(canvas).toHaveAttribute("data-canvas", betaTab);
    await expect.poll(() => sent.get("focus_tab") ?? 0).toBe(focusTabs + 1);

    // ⌥Tab: Recent Projects puts fixture first, on the display it was left on.
    const beforeProjects = sent.get("focus_checkout") ?? 0;
    await page.keyboard.down("Alt");
    await page.keyboard.press("Tab");
    await expect(page.locator("[data-cycle=projects]")).toContainText("Recent Projects");
    await expect(cycleRow).toContainText("plan.txt");
    await screenshot(page, "recent-projects");
    await page.keyboard.up("Alt");
    await expect.poll(() => sent.get("focus_checkout") ?? 0).toBe(beforeProjects + 1);
    expect(last.get("focus_checkout")).toMatchObject({ display_id: display });
    await expect.poll(() => page.evaluate(() => document.activeElement?.closest("[data-view-area]") !== null)).toBe(true);

    // Escape while held keeps the original selection and commits nothing.
    const quiet = [sent.get("focus_checkout") ?? 0, sent.get("focus_tab") ?? 0];
    await page.keyboard.down("Alt");
    await page.keyboard.press("Backquote");
    await expect(page.locator("[data-cycle=panels]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-cycle]")).toHaveCount(0);
    await page.keyboard.up("Alt");
    expect([sent.get("focus_checkout") ?? 0, sent.get("focus_tab") ?? 0]).toEqual(quiet);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
