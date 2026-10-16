// A tab the operator named outlives its agents (docs/UI_BEHAVIOR.md, Agent
// areas): closing its last agent pane leaves a shell in the tab, and closing
// that shell closes the tab.
import { expect, test } from "@playwright/test";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

type Snapshot = { result: { snapshot: { tabs: { tab_id: string; label: string }[]; panes: { pane_id: string; tab_id: string; cwd?: string | null }[] } } };

test("closing a named tab's last agent keeps the tab, its name and its sidebar line until its shell closes", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [first, second] = herdr.panes;
    const snapshot = () => (herdr.run(["api", "snapshot"]) as Snapshot).result.snapshot;
    const panesOfTab = () => snapshot().panes.filter((pane) => pane.tab_id === herdr.tab);
    herdr.run(["tab", "rename", herdr.tab, "Planning"]);
    daemon = await startHided(herdr, "named-tab-stays");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const line = page.locator(`[data-sidebar-tab-line="${herdr.tab}"]`);
    await expect(line).toContainText("Planning");

    // One agent of two leaves as any pane does.
    await page.locator(`[data-pane-view="${second}"]`).getByRole("button", { name: /^Close/ }).click();
    await expect.poll(() => panesOfTab().map((pane) => pane.pane_id)).toEqual([first]);

    // The last one leaves a shell at the checkout in the same tab.
    await page.locator(`[data-pane-view="${first}"]`).getByRole("button", { name: /^Close/ }).click();
    await expect.poll(() => panesOfTab().map((pane) => pane.pane_id).filter((id) => id !== first)).toHaveLength(1);
    await expect.poll(() => panesOfTab().some((pane) => pane.pane_id === first)).toBe(false);
    const [shell] = panesOfTab();
    expect(shell!.cwd).toBe(path.join(herdr.root, "fixture"));
    expect(snapshot().tabs.find((tab) => tab.tab_id === herdr.tab)?.label).toBe("Planning");
    await expect(page.locator(`[data-agent-tab-bar] [data-tab="${herdr.tab}"]`)).toContainText("Planning");
    await expect(page.locator(`[data-pane-view="${shell!.pane_id}"]`)).toBeVisible();
    await expect(page.locator(`[data-pane-view="${shell!.pane_id}"]`)).toHaveAttribute("data-focused", "true");
    await expect(line).toContainText("Planning");
    await screenshot(page, "named-tab-kept-with-shell");

    // Choosing the line opens the tab; closing the shell closes the tab and its name.
    await line.locator("[data-sidebar-tab]").click();
    await expect(page.locator("[data-canvas]").first()).toHaveAttribute("data-canvas", herdr.tab);
    await page.locator(`[data-pane-view="${shell!.pane_id}"]`).getByRole("button", { name: /^Close/ }).click();
    await expect.poll(() => snapshot().tabs.some((tab) => tab.tab_id === herdr.tab)).toBe(false);
    await expect(line).toHaveCount(0);
    await expect(page.locator(`[data-tab="${herdr.tab}"]`)).toHaveCount(0);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
