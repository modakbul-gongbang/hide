// Cmd+F moves the pane to the match it counts: a match above the screen is
// scrolled into view, on an isolated pinned Herdr.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });
test.use({ actionTimeout: 15_000 });

function scrollOffset(herdr: HerdrFixture, pane: string): number {
  const answer = herdr.run(["pane", "get", pane]) as { result: { pane: { scroll?: { offset_from_bottom: number } } } };
  return answer.result.pane.scroll?.offset_from_bottom ?? -1;
}

async function paneText(page: Page, pane: string): Promise<string> {
  return page.evaluate((id) => window.__hideProbe?.paneText(id) ?? "", pane);
}

test("Cmd+F scrolls the focused pane to a match above the screen", async ({ page }) => {
  await page.setViewportSize({ width: 1680, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [, focused] = herdr.panes;
    const rows = Array.from({ length: 160 }, (_, index) => `row-${String(index + 1).padStart(3, "0")}`).join("\n");
    execFileSync(herdr.bin, ["pane", "send-text", focused, `${rows}\n`], { env: herdr.env, timeout: 30_000 });
    daemon = await startHided(herdr, "find-scroll");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");

    const view = page.locator(`[data-pane-view="${focused}"]`);
    await expect.poll(() => paneText(page, focused), { timeout: 15_000 }).toContain("row-160");
    await view.locator("[data-terminal-host]").click();
    await expect(view).toHaveAttribute("data-focused", "true");
    expect(scrollOffset(herdr, focused)).toBe(0);

    await page.keyboard.press("Meta+f");
    const bar = page.locator("[data-find-bar]");
    await bar.locator("input").fill("row-005");
    await page.keyboard.press("Enter");
    await expect(bar).toContainText("1/1");
    await expect.poll(() => scrollOffset(herdr, focused), { timeout: 10_000 }).toBeGreaterThan(0);
    await expect.poll(() => paneText(page, focused), { timeout: 10_000 }).toContain("row-005");
    await screenshot(page, "pane-find-scrolled");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
