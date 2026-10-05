// The Agent area's ⌥` order outlives a restart (issue 301, docs/UI_BEHAVIOR.md,
// Recent navigation): the page reports each pane visit and the core saves the
// order in `core-state.json`, so after hided restarts and the page reloads the
// cycle still offers an agent pane visited only before the restart.

import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { fixtureExecutable } from "./platform-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });
test.use({ actionTimeout: 15_000 });

test("the recent agent pane order survives a daemon restart and a reload", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    fs.mkdirSync(path.join(herdr.root, "beta"), { recursive: true });
    const beta = herdr.run([
      "workspace", "create", "--cwd", path.join(herdr.root, "beta"), "--label", "beta", "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]) as { result: { tab: { tab_id: string }; root_pane: { pane_id: string } } };
    const betaPane = beta.result.root_pane.pane_id;
    fs.copyFileSync(path.join(herdr.root, "bin", fixtureExecutable("claude")), path.join(herdr.root, "bin", fixtureExecutable("codex")));
    await expect.poll(() => execFileSync(herdr.bin, ["pane", "read", betaPane, "--source", "recent", "--lines", "5"], { env: herdr.env, encoding: "utf8" }), { timeout: 20_000 }).toContain("fixture %");
    herdr.run(["agent", "start", "three", "--kind", "codex", "--pane", betaPane]);
    daemon = await startHided(herdr, "recent-agents-restart");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const canvas = page.locator("[data-canvas]").first();

    // The keyboard in beta's Codex, then back in the fixture's pane.
    await page.locator("[data-project]", { hasText: "beta" }).locator("[data-checkout]").first().click();
    await expect(canvas).toHaveAttribute("data-canvas", beta.result.tab.tab_id);
    await page.locator(`[data-pane-view="${betaPane}"]`).click();
    await page.locator("[data-project]", { hasText: "fixture" }).locator("[data-checkout]").first().click();
    await expect(canvas).not.toHaveAttribute("data-canvas", beta.result.tab.tab_id);
    const fixturePane = (await page.locator('[data-pane-view][data-focused="true"]').getAttribute("data-pane-view"))!;
    await page.locator(`[data-pane-view="${fixturePane}"]`).click({ position: { x: 30, y: 60 } });
    const stored = () => (JSON.parse(fs.readFileSync(path.join(daemon!.stateDir, "core-state.json"), "utf8")) as { recent_pane_ids?: string[] }).recent_pane_ids;
    await expect.poll(stored).toEqual([fixturePane, betaPane]);

    daemon = await daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");

    // From a sidebar control, which supplies no pane visit, the cycle starts
    // at the most recent agent pane and still reaches beta's, visited only
    // before the restart.
    const projectsMode = page.locator('[data-sidebar-mode="projects"]');
    await projectsMode.focus();
    await expect(projectsMode).toBeFocused();
    const agents = page.locator("[data-cycle=agents]");
    const cycleRow = page.locator("[data-cycle] [aria-selected=true]");
    await page.keyboard.down("Alt");
    await page.keyboard.press("Backquote");
    await expect(agents).toBeVisible();
    await expect(agents.locator("[role=option]")).toHaveCount(2);
    await expect(cycleRow).toHaveAttribute("data-cycle-row", fixturePane);
    await page.keyboard.press("Backquote");
    await expect(cycleRow).toHaveAttribute("data-cycle-row", betaPane);
    await expect(cycleRow).toHaveAttribute("aria-label", /codex agent/);
    await screenshot(page, "recent-agents-after-restart");
    await page.keyboard.up("Alt");
    await expect(canvas).toHaveAttribute("data-canvas", beta.result.tab.tab_id);
    await expect.poll(stored).toEqual([betaPane, fixturePane]);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
