// The Project Overview's header at the widths a window can take (issue 618):
// the project name has to stay on the path back, and when the line runs out of
// room the actions give way first, keeping their names. Light and Dark captures
// at 720, 900 and 1600 land in HIDE_E2E_SCREENSHOT_DIR.
import { expect, test, type Page } from "@playwright/test";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";
import { chord } from "./chords";
import { openCurrentProjectOverview } from "./overview-entry";
import { animationsFinished } from "./wait";

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press(chord("settings"));
  await page.locator('[data-settings-tab="appearance"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  await animationsFinished(page);
}

test("the Project Overview header keeps the project name at every width", async ({ page }) => {
  test.setTimeout(120_000);
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "overview-header");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await openCurrentProjectOverview(page, "fixture");
    const header = page.locator("[data-overview-screen] > header");
    const name = header.getByRole("heading", { level: 1, name: "fixture" });
    const actions = [header.getByRole("button", { name: "New agent" }), header.getByRole("button", { name: "New issue" })];
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      for (const width of [1600, 900, 720]) {
        await page.setViewportSize({ width, height: 800 });
        // The name keeps a width of its own, and nothing in the header runs past it.
        await expect.poll(async () => (await name.boundingBox())?.width ?? 0, { message: `the name at ${width}px` }).toBeGreaterThan(0);
        for (const action of actions) await expect(action).toBeVisible();
        // Under 512 px of header the actions are square icons without the keycap; above it they carry their word and `C`.
        const issue = await actions[1].boundingBox();
        expect(issue!.width === issue!.height, `New issue is an icon at ${width}px`).toBe(width === 720);
        await expect(header.locator("[data-overview-new-issue] kbd")).toBeVisible({ visible: width !== 720 });
        expect(await header.evaluate((node) => node.scrollWidth <= node.clientWidth), `header overflow at ${width}px`).toBe(true);
        await screenshot(page, `overview-header-${width}-${theme}`);
      }
    }
    // The name is an actual readable label, not a sliver: it shows the whole of a short name.
    await page.setViewportSize({ width: 720, height: 800 });
    expect(await name.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
    // The icon keeps its name and shortcut in a hint.
    await actions[1].hover();
    await expect(page.getByRole("tooltip")).toContainText("New issue");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
