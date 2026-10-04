import { expect, test } from "@playwright/test";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";
import { chord, held } from "./chords";

test("shared Overview preserves work, returns the keyboard and has direct sidebar commands", async ({ page }) => {
  test.setTimeout(120_000);
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "overview-modal");
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const pane = page.locator('[data-pane-view][data-focused="true"]').first();
    const input = pane.locator(".xterm-helper-textarea");
    await input.focus();
    const paneId = await pane.getAttribute("data-pane-view");
    const geometry = await pane.boundingBox();
    const sidebar = page.locator("[data-sidebar-overview]");
    const home = page.locator("[data-home-destination]");
    const modal = page.getByRole("dialog", { name: "Overview", exact: true });
    await page.keyboard.press(chord("overview"));
    await expect(modal).toBeVisible();
    await expect(sidebar).toHaveAttribute("aria-current", "page");
    await expect(home).not.toHaveAttribute("aria-current");
    await expect(page.locator("[data-project-overview]")).toHaveCount(0);
    expect(await pane.boundingBox()).toEqual(geometry);
    expect(await modal.evaluate((node) => node.contains(document.activeElement))).toBe(true);
    await page.keyboard.press("Tab");
    expect(await modal.evaluate((node) => node.contains(document.activeElement))).toBe(true);
    await screenshot(page, "overview-modal-all-dark");
    await page.getByRole("tab", { name: "fixture", exact: true }).click();
    await expect(modal.locator("[data-overview-screen]")).toBeVisible();
    await screenshot(page, "overview-modal-project-dark");
    await page.keyboard.press("Escape");
    await expect(modal).toHaveCount(0);
    await expect(input).toBeFocused();
    expect(await pane.boundingBox()).toEqual(geometry);
    await page.keyboard.type("overview-terminal-preserved");
    await expect.poll(() => page.evaluate((id) => window.__hideProbe?.paneText(id!), paneId)).toContain("overview-terminal-preserved");
    await page.keyboard.press("Control+KeyU");

    // The palette hands Overview its original keyboard owner, not its input.
    await page.keyboard.press(chord("search"));
    await page.locator("[data-palette-input]").fill("fixture");
    await page.keyboard.press(chord("overview"));
    await expect(page.locator("[data-palette]")).toHaveCount(0);
    await expect(modal).toBeVisible();
    await expect(modal.locator("[data-overview-screen]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(input).toBeFocused();

    // Settings prevents a second modal. Its own Escape closes it.
    await page.locator("[data-open-settings]").click();
    await page.keyboard.press(chord("overview"));
    await expect(modal).toHaveCount(0);
    await page.keyboard.press("Escape");

    // Clearing a shortcut persists, removes its keycap and disables its chord.
    await page.locator("[data-open-settings]").click();
    await page.locator('[data-settings-tab="shortcuts"]').click();
    await page.locator('[data-shortcut-clear="overview"]').click();
    await expect(page.locator('[data-shortcut-effective="overview"]')).toHaveText("-");
    await page.keyboard.press("Escape");
    await expect(sidebar.locator("kbd")).toHaveCount(0);
    await page.keyboard.press(chord("overview"));
    await expect(modal).toHaveCount(0);
    await page.locator("[data-open-settings]").click();
    await page.locator('[data-settings-tab="shortcuts"]').click();
    await page.locator('[data-shortcut-reset="overview"]').click();
    await expect(page.locator('[data-shortcut-effective="overview"]')).not.toHaveText("-");
    await page.keyboard.press("Escape");
    await expect(sidebar.locator("kbd")).toHaveCount(1);

    // A modal is not a visit; cycling begins on the most recent agent pane.
    await input.focus();
    await page.keyboard.press(chord("overview"));
    const cycle = held("recent_area_tab");
    for (const key of cycle.modifiers) await page.keyboard.down(key);
    await page.keyboard.press(cycle.key);
    await expect(page.locator('[data-cycle="agents"] [aria-selected="true"]')).toHaveAttribute("data-cycle-row", paneId!);
    await page.keyboard.press("Escape");
    for (const key of cycle.modifiers) await page.keyboard.up(key);
    await expect(modal).toBeVisible();
    const before = sent.get("focus_pane") ?? 0;
    for (const key of cycle.modifiers) await page.keyboard.down(key);
    await page.keyboard.press(cycle.key);
    for (const key of cycle.modifiers) await page.keyboard.up(key);
    await expect(modal).toHaveCount(0);
    await expect(input).toBeFocused();
    expect((sent.get("focus_pane") ?? 0) - before).toBe(1);

    // Direct commands close Overview, show the requested list and converge.
    await page.keyboard.press(chord("overview"));
    await page.keyboard.press(chord("sidebar_agents"));
    await expect(modal).toHaveCount(0);
    await expect(page.locator('[data-sidebar-mode="agents"]')).toHaveAttribute("aria-selected", "true");
    expect(await page.locator('nav[data-sidebar="agents"]').evaluate((node) => node.contains(document.activeElement))).toBe(true);
    await page.keyboard.press(chord("sidebar_agents"));
    await page.keyboard.press(chord("sidebar_projects"));
    await expect(page.locator('[data-sidebar-mode="projects"]')).toHaveAttribute("aria-selected", "true");
    await home.click();
    await expect(page.locator("[data-overview-page]")).toBeVisible();
    await expect(modal).toHaveCount(0);
    await sidebar.click();
    await expect(page.locator("[data-main-screen]")).toBeVisible();
    await screenshot(page, "overview-page-dark");
    await page.keyboard.press(chord("overview"));
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await page.setViewportSize({ width: 720, height: 640 });
    await page.locator("[data-open-overview]").click();
    const bounds = (await modal.boundingBox())!;
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(720);
    expect(await modal.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
    await screenshot(page, "overview-modal-narrow-dark");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
