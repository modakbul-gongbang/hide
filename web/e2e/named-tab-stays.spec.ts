// A tab the operator named outlives its agents (docs/UI_BEHAVIOR.md, Agent
// areas): closing its last agent pane leaves a shell in the tab, and closing
// that shell closes the tab.
import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { chord } from "./chords";
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

/**
 * Whether the core has said there is a close to reopen. Reopen closed tab
 * does nothing until then, and nothing on the page shows it, so the spec
 * reads the snapshot frames the page receives.
 */
function reopenOffered(page: Page): () => boolean {
  let offered = false;
  const find = (value: unknown): boolean =>
    typeof value === "object" && value !== null &&
    Object.entries(value).some(([key, inner]) => (key === "recent_closed" && (inner as { can_reopen?: unknown }).can_reopen === true) || find(inner));
  page.on("websocket", (ws) => ws.on("framereceived", (frame) => {
    try {
      offered ||= find(JSON.parse(String(frame.payload)));
    } catch {
      /* binary terminal frames are not snapshots */
    }
  }));
  return () => offered;
}

// Herdr keeps any label a reopen sends as the tab's name, so a reopen sends
// only the operator's: a tab never named comes back unnamed.
test("reopening an unnamed tab leaves it unnamed", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const snapshot = () => (herdr.run(["api", "snapshot"]) as Snapshot).result.snapshot;
    // A second tab keeps the workspace, which its last tab's close would end.
    const other = (herdr.run(["tab", "create", "--workspace", herdr.workspace, "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as { result: { tab: { tab_id: string } } }).result.tab.tab_id;
    daemon = await startHided(herdr, "named-tab-reopen");
    const offered = reopenOffered(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");

    const closing = page.locator(`[data-agent-tab-bar] [data-tab="${herdr.tab}"]`);
    await closing.hover();
    await closing.getByRole("button", { name: /^Close tab/ }).click();
    await expect.poll(() => snapshot().tabs.some((tab) => tab.tab_id === herdr.tab)).toBe(false);
    await expect.poll(offered).toBe(true);
    await page.locator(`[data-agent-tab-bar] [data-tab="${other}"]`).click();
    await page.keyboard.press(chord("reopen_closed_tab"));
    const reopenedTab = () => snapshot().tabs.find((tab) => snapshot().panes.filter((pane) => pane.tab_id === tab.tab_id).length === 2);
    await expect.poll(() => reopenedTab()?.tab_id).toBeTruthy();
    const reopened = reopenedTab()!;
    // Herdr numbers a tab it was given no name for.
    expect(reopened.label, "the reopened tab carries no name Hide made up").toMatch(/^\d+$/);

    // The sidebar names it as any unnamed tab, so its last agent's close
    // takes it (closing restored agents waits on a status check here).
    await expect(page.locator(`[data-sidebar-tab-line="${reopened.tab_id}"]`)).toContainText(`Tab ${reopened.label}`);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
