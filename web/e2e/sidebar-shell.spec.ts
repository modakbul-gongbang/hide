// The sidebar's shell on an isolated pinned Herdr and hided (PRD sidebar-shell
// B1-B7, with the Home row moved into the Projects list by quick
// device-rail-badges B3): the Home row heads the Projects list under the
// Projects | Agents strip, opens the device's Overview and carries its fill only while that screen is in front; Projects
// is the first tab and the default; Search at the strip's end opens the ⌘K
// palette, and a browser tab offers no Add project before it; the
// Search field row, the bottom new-workspace button and the All projects row
// inside the list are gone. Captured in Dark and Light.

import { expect, test, type Page } from "@playwright/test";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { screenshot } from "./wire";
import { chord, commandLabel } from "./chords";
import { animationsFinished } from "./wait";

test.describe.configure({ timeout: 120_000 });

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press(chord("settings"));
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="general"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  // Controls fade their colors into the new theme; a capture waits them out.
  await animationsFinished(page);
}

test("the Home row, the Projects | Agents strip and its Search icon, with no Add project in a browser tab", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "sidebar-shell");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    const sidebar = page.locator("nav[data-sidebar]");
    // B3: Projects is the first tab and the one shown.
    await expect(sidebar).toHaveAttribute("data-sidebar", "projects", { timeout: 20_000 });
    await expect(page.locator("[data-sidebar-mode]")).toContainText(["Projects", "Agents"]);
    await expect(page.locator('[data-sidebar-mode="projects"]')).toHaveAttribute("aria-selected", "true");
    const list = page.locator("[data-project-list]");
    await expect(list.locator("[data-checkout]").first()).toBeVisible({ timeout: 20_000 });

    // B1, B2: the Home row opens the device's Overview, titled with the device, and is marked.
    // No Overview row is left above the list.
    await expect(page.locator("[data-sidebar-overview]")).toBeVisible();
    const overview = page.locator("[data-home-destination]");
    await expect(overview).toContainText("Home");
    await expect(overview.locator("[data-home-count]")).toHaveText(/^\d+ projects?$/);
    await overview.click();
    const main = page.locator("[data-main-screen]");
    await expect(main).toBeVisible();
    await expect(main.locator("h1")).toContainText("Home");
    await expect(main.locator("[data-main-device-name]")).toHaveText("This Mac");
    await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
    await expect(overview).not.toHaveAttribute("aria-current", "page");
    // The row is the Projects list's own, under the tab strip.
    expect(await overview.evaluate((row) => row.closest("[data-project-list]") !== null)).toBe(true);
    expect((await overview.boundingBox())!.y).toBeGreaterThan((await page.locator("[data-sidebar-strip]").boundingBox())!.y);

    // B7 and the removed controls: the list starts at a project, and no field or bottom button is left.
    await expect(list).not.toContainText("All projects");
    await expect(sidebar.locator("input")).toHaveCount(0);
    await expect(sidebar).not.toContainText("새 워크스페이스");

    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await screenshot(page, `sidebar-shell-overview-${theme}`);
    }

    // B2: a Workspace in front takes the fill away; Enter on the row brings the Home Overview back.
    await list.locator("[data-checkout]").first().click();
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await expect(overview).not.toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-go-main]")).toHaveText("Home");
    await overview.focus();
    await page.keyboard.press("Enter");
    await expect(main).toBeVisible();
    await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
    await expect(overview).not.toHaveAttribute("aria-current", "page");

    // B4: Search at the strip's end, hinted with its chord, opens the palette.
    const search = page.locator("[data-sidebar-search]");
    await expect(search).toHaveAccessibleName("Search");
    await search.hover();
    await expect(page.locator('[data-slot="tooltip-content"]')).toContainText(`Search${commandLabel("search")}`);
    await screenshot(page, "sidebar-shell-search-hint-light");
    await search.click();
    await expect(page.locator('[data-palette="Search"] [data-palette-input]')).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-palette="Search"]')).toHaveCount(0);

    // B5: a browser tab has no folder picker, so the strip offers no Add project
    // (the desktop app draws it before Search: desktop/e2e/add-project.spec.ts).
    const add = page.locator("[data-sidebar-new-workspace]");
    await expect(add).toHaveCount(0);
    // Neither the Overview nor the chord offers it: the chord opens nothing.
    await expect(page.locator("[data-main-add-project]")).toHaveCount(0);
    await page.keyboard.press("Alt+Shift+KeyN");
    await page.keyboard.press(chord("new_workspace", "electron"));
    await expect(page.locator("[data-add-project]")).toHaveCount(0);

    // B3, B5: the Agents tab swaps the list and leaves Search alone at the end; the Home row is Projects' and goes with it.
    await page.locator('[data-sidebar-mode="agents"]').click();
    await expect(sidebar).toHaveAttribute("data-sidebar", "agents");
    await expect(page.locator("[data-agent-list]")).toBeVisible();
    await expect(add).toHaveCount(0);
    await expect(search).toBeVisible();
    await expect(overview).toHaveCount(0);
    await page.mouse.move(640, 700);
    await screenshot(page, "sidebar-shell-agents-light");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
