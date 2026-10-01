// A checkout whose only tab holds a delegated child, on an isolated pinned
// Herdr and hided (PRD hide-orchestrator B1, B4): the tab stays off the strip,
// but the checkout is not empty. Opening the checkout, or its agent row in the
// sidebar, draws the child's pane rather than the "No agent tab" state that a
// checkout with no tab shows.

import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, declareParent } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });
test.use({ actionTimeout: 15_000 });

test("a checkout whose only tab is delegated opens on the child's pane", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [parent] = herdr.panes;
    // A second checkout with one pane, its agent spawned by the fixture's
    // first agent: the way an Observer dispatches an Implementor into a
    // worktree of its own.
    fs.mkdirSync(path.join(herdr.root, "beta"), { recursive: true });
    const beta = herdr.run([
      "workspace", "create", "--cwd", path.join(herdr.root, "beta"), "--label", "beta", "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]) as { result: { tab: { tab_id: string }; root_pane: { pane_id: string } } };
    const child = beta.result.root_pane.pane_id;
    await expect.poll(() => execFileSync(herdr.bin, ["pane", "read", child, "--source", "recent", "--lines", "5"], { env: herdr.env, encoding: "utf8" }), { timeout: 20_000 }).toContain("fixture %");
    herdr.run(["agent", "start", "three", "--kind", "claude", "--pane", child]);
    declareParent(herdr, child, parent);

    daemon = await startHided(herdr, "delegated-checkout");
    const last = new Map<string, Record<string, unknown>>();
    countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");

    // The checkout row opens the Workspace on the child's pane: the strip
    // has no tab for it (B1), and the empty state is not shown.
    await page.locator('[data-sidebar-mode="projects"]').click();
    const checkout = page.locator("[data-project]", { hasText: "beta" }).locator("[data-checkout]").first();
    await checkout.click();
    await expect(page.locator("[data-canvas]").first()).toHaveAttribute("data-canvas", beta.result.tab.tab_id, { timeout: 20_000 });
    await expect(page.locator(`[data-pane-view="${child}"]`)).toBeVisible();
    await expect(page.locator("[data-empty-new-tab]")).toHaveCount(0);
    await expect(page.locator('[data-agent-tab-bar] [role="tab"]')).toHaveCount(0);
    await screenshot(page, "delegated-checkout-open");

    // beta's line two names the agent it was raised from, behind an arrow at
    // the small icon size: a class naming an undefined size token once drew
    // it at lucide's 24px.
    const betaRow = page.locator("[data-project]", { hasText: "beta" }).first();
    const raisedArrow = betaRow.locator("[data-checkout-parent] svg");
    await expect(raisedArrow).toBeVisible();
    expect((await raisedArrow.boundingBox())?.width).toBe(12);
    await betaRow.screenshot({ path: test.info().outputPath("delegated-checkout-raised-from.png") });

    // Back on the first checkout, the child's own sidebar row brings its
    // pane to the screen with the keyboard in it.
    await page.locator("[data-project]", { hasText: "fixture" }).locator("[data-checkout]").first().click();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toBeVisible({ timeout: 20_000 });
    const betaProject = page.locator("[data-project]").filter({ has: page.getByRole("button", { name: /^beta(?:,|$)/ }) });
    // Opening beta unfolded it; switching checkout keeps that expansion.
    await expect(betaProject.locator("[data-checkout-toggle]")).toHaveAttribute("aria-expanded", "true");
    await betaProject.locator(`[data-agent-open="${child}"]`).click();
    await expect.poll(() => last.get("focus_pane")?.pane_id).toBe(child);
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveAttribute("data-focused", "true", { timeout: 20_000 });
    await expect(page.locator("[data-empty-new-tab]")).toHaveCount(0);
    await screenshot(page, "delegated-checkout-row");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
