import { expect, test, type Page } from "@playwright/test";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { chord, commandLabel } from "./chords";
import { chooseLane, enterWorkspace, screenshot } from "./wire";
import { TOOLS } from "../src/laneTools";

// A closed lane remembers the tool it last showed, and ⌥⌘1-4 press the rail's
// icons. Each test starts one stack and asserts one contract.

const screen = (page: Page) => page.locator("[data-workspace-screen]");
const closed = (page: Page) => expect(screen(page)).toHaveAttribute("data-lane-mode", "closed");
const shows = (page: Page, tool: string) => expect(screen(page)).toHaveAttribute("data-lane-shows", tool);

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto("about:blank");
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await enterWorkspace(page, "fixture");
}

test("⌘E reopens the tool the lane last showed, however it was closed, and Files for a lane that never showed one", async ({ page }) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1600, height: 900 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "lane-last-tool");
    await open(page, daemon);
    await closed(page);
    await page.keyboard.press(chord("toggle_lane"));
    await shows(page, "files");
    await page.keyboard.press(chord("toggle_lane"));
    await closed(page);

    // The rail icon hints show their chord after the name and scope.
    await page.locator('[data-lane-tool="memory"]').hover();
    await expect(page.locator('[data-slot="tooltip-content"]')).toContainText(commandLabel("toggle_lane_memory"));
    await screenshot(page, "lane-last-tool-rail-hint");
    await page.locator('[data-lane-tool="memory"]').click();
    await shows(page, "memory");
    await screenshot(page, "lane-last-tool-memory-hint");

    // Each way of closing the lane keeps Memory for the next ⌘E.
    await page.locator("[data-lane-close]").click();
    await closed(page);
    await page.keyboard.press(chord("toggle_lane"));
    await shows(page, "memory");
    await page.locator('[data-lane-tool="memory"]').click();
    await closed(page);
    await page.keyboard.press(chord("toggle_lane"));
    await shows(page, "memory");
    await page.keyboard.press(chord("toggle_lane"));
    await closed(page);
    await page.keyboard.press(chord("toggle_lane"));
    await shows(page, "memory");
    await screenshot(page, "lane-last-tool-reopened-memory");

    // The path menu's Show right lane returns to it too.
    await page.keyboard.press(chord("toggle_lane"));
    await closed(page);
    await chooseLane(page);
    await shows(page, "memory");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("⌥⌘1-4 open the lane on the rail's first to fourth tool and close it on the tool showing, and the last tool survives a restart", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1600, height: 900 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "lane-tool-chords");
    await open(page, daemon);
    for (const [index, tool] of TOOLS.entries()) {
      const id = `toggle_lane_${tool}` as const;
      await page.keyboard.press(chord(id));
      await shows(page, tool);
      await expect(page.locator(`[data-lane-tool="${tool}"]`)).toHaveAttribute("aria-pressed", "true");
      expect(commandLabel(id), `rail position ${index + 1}`).toMatch(String(index + 1));
    }
    // The loop ended on the last tool, so its chord closes the lane.
    await page.keyboard.press(chord(`toggle_lane_${TOOLS[TOOLS.length - 1]!}`));
    await closed(page);
    await page.keyboard.press(chord("toggle_lane_changes"));
    await shows(page, "changes");
    await page.keyboard.press(chord("toggle_lane_changes"));
    await closed(page);

    daemon = await daemon.restart();
    await open(page, daemon);
    await closed(page);
    await page.keyboard.press(chord("toggle_lane"));
    await shows(page, "changes");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
