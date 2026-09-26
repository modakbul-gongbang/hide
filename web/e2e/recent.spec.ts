// Recent navigation on an isolated pinned Herdr and hided
// (docs/UI_BEHAVIOR.md, Recent navigation): Recent Panels (⌥` in a browser)
// walks one order over Herdr tabs and View displays across checkouts and
// commits one event on releasing ⌥; Recent Projects (⌥Tab) brings the
// previous project back on the surface it was last used on.

import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
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
    ]) as { result: { tab: { tab_id: string }; root_pane: { pane_id: string } } };
    const betaTab = beta.result.tab.tab_id;
    // One agent per tab, so each row is named and marked by its agent: a
    // Codex in beta (the fixture's shim under that name) and a Claude alone
    // in a second fixture tab. The fixture's first tab holds two agents and
    // keeps its Herdr label and the neutral mark.
    fs.copyFileSync(path.join(herdr.root, "bin", "claude"), path.join(herdr.root, "bin", "codex"));
    const solo = herdr.run([
      "tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", "solo", "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]) as { result: { root_pane: { pane_id: string } } };
    for (const [name, kind, pane] of [["three", "codex", beta.result.root_pane.pane_id], ["four", "claude", solo.result.root_pane.pane_id]] as const) {
      await expect.poll(() => execFileSync(herdr.bin, ["pane", "read", pane, "--source", "recent", "--lines", "5"], { env: herdr.env, encoding: "utf8" }), { timeout: 20_000 }).toContain("fixture %");
      herdr.run(["agent", "start", name, "--kind", kind, "--pane", pane]);
    }
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
    // Meanwhile every row wears its marks: an agent's status mark then which
    // agent it is, the neutral mark for a tab of several, a file's own mark.
    const quiet = [sent.get("focus_checkout") ?? 0, sent.get("focus_tab") ?? 0];
    await page.keyboard.down("Alt");
    await page.keyboard.press("Backquote");
    await expect(page.locator("[data-cycle=panels]")).toBeVisible();
    const marks = (row: string) => page.locator(`[data-cycle=panels] [data-cycle-row="${row}"] [data-cycle-marks]`);
    await expect(marks(betaTab)).toHaveAttribute("data-cycle-marks", "codex");
    await expect(marks(betaTab).locator("[data-cycle-status]")).toHaveCount(1);
    await expect(marks(betaTab).locator('[data-agent-mark="codex"]')).toHaveCount(1);
    await expect(page.locator(`[data-cycle=panels] [data-cycle-row="${betaTab}"]`)).toHaveAttribute("aria-label", /codex agent/);
    await expect(page.locator('[data-cycle=panels] [data-cycle-marks="claude"] [data-agent-mark="claude"]')).toHaveCount(1);
    await expect(marks(herdr.tab)).toHaveAttribute("data-cycle-marks", "herdr");
    await expect(marks(herdr.tab).locator('[data-agent-mark="neutral"]')).toHaveCount(1);
    await expect(marks(herdr.tab).locator("[data-cycle-status]")).toHaveCount(0);
    await expect(marks(display!).locator('[data-view-mark="file"]')).toHaveCount(1);
    await screenshot(page, "recent-panels-marks");
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-cycle]")).toHaveCount(0);
    await page.keyboard.up("Alt");
    expect([sent.get("focus_checkout") ?? 0, sent.get("focus_tab") ?? 0]).toEqual(quiet);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
