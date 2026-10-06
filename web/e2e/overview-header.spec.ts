// The Project Overview's title row at the widths a window can take (issue 618):
// a long project name keeps a readable share of the row at every width, and
// New agent and New issue give way to their icons only when the whole name no
// longer fits beside their words. Light and Dark captures at 720, 900 and 1600
// land in HIDE_E2E_SCREENSHOT_DIR.
import { expect, test, type Locator, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { chooseTheme, enterWorkspace, screenshot } from "./wire";
import { openCurrentProjectOverview } from "./overview-entry";
import { quietFor } from "./wait";

const LABEL = "a-project-whose-name-is-long-enough-to-fill-the-header-row";
/** The least the name may keep: what a tab title keeps (`--size-tab-title-min`). */
const NAME_MIN = 104;

async function width(node: Locator): Promise<number> {
  return (await node.boundingBox())?.width ?? 0;
}

/** The state under test: whether New issue is its icon (square) or carries its word and keycap. */
async function actionsAreIcons(page: Page, issue: Locator): Promise<boolean> {
  const box = await issue.boundingBox();
  return box !== null && box.width === box.height;
}

test("the Project Overview title row keeps a long project name readable at every width", async ({ page }) => {
  test.setTimeout(120_000);
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const project = path.join(herdr.root, LABEL);
    fs.mkdirSync(project);
    herdr.run(["workspace", "create", "--cwd", project, "--label", LABEL, "--no-focus"]);
    daemon = await startHided(herdr, "overview-header");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await page.setViewportSize({ width: 1600, height: 800 });
    await enterWorkspace(page, LABEL);
    await openCurrentProjectOverview(page, LABEL);
    const header = page.locator("[data-overview-screen] > header");
    const name = header.getByRole("heading", { level: 1, name: LABEL });
    const issue = header.getByRole("button", { name: "New issue" });
    const agent = header.getByRole("button", { name: "New agent" });
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      for (const viewport of [1600, 900, 720]) {
        await page.setViewportSize({ width: viewport, height: 800 });
        // At 1600 the whole name and both words fit; at 900 and 720 the name needs the room, so the actions are icons.
        await expect.poll(() => actionsAreIcons(page, issue), { message: `New issue as an icon at ${viewport}px` }).toBe(viewport !== 1600);
        await expect(issue.locator("kbd")).toBeVisible({ visible: viewport === 1600 });
        await expect(agent).toBeVisible();
        expect(await width(name), `the name's width at ${viewport}px`).toBeGreaterThanOrEqual(NAME_MIN);
        expect(await name.evaluate((node) => node.scrollWidth > node.clientWidth), `the name is cut with an ellipsis only at ${viewport}px`).toBe(viewport !== 1600);
        expect(await page.locator("[data-overview-screen]").evaluate((node) => node.scrollWidth <= node.clientWidth), `nothing runs past the page at ${viewport}px`).toBe(true);
        await screenshot(page, `overview-header-${viewport}-${theme}`);
      }
    }
    // An icon keeps its name and shortcut in a hint; where the words show (1600, last) nothing opens.
    await issue.hover();
    await expect(page.getByRole("tooltip")).toContainText("New issue");
    await expect(page.getByRole("tooltip").locator("kbd")).toHaveText("C");
    await page.mouse.move(600, 600);
    await expect(page.getByRole("tooltip")).toHaveCount(0);
    // A disabled control takes no pointer, so the hint hangs on the span around it.
    // The DOM is forced here: no project this fixture can make has New agent disabled.
    await agent.evaluate((node: HTMLButtonElement) => {
      node.disabled = true;
    });
    await agent.locator("xpath=..").hover();
    await expect(page.getByRole("tooltip")).toContainText("New agent");
    await page.setViewportSize({ width: 1600, height: 800 });
    await expect.poll(() => actionsAreIcons(page, issue)).toBe(false);
    await page.mouse.move(600, 600);
    await issue.hover();
    await quietFor(page, 800, "a hint opens after 500 ms; none does while the word is showing");
    await expect(page.getByRole("tooltip")).toHaveCount(0);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
