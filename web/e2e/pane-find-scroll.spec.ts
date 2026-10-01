// Cmd+F reaches the whole of what a pane shows, on an isolated pinned Herdr:
// a pane whose history Herdr holds is searched by the find bar, which scrolls
// a match above the screen into view, and a full-screen agent, whose history
// Herdr does not hold, is handed to the agent's own search.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });
test.use({ actionTimeout: 15_000 });

function scrollOffset(herdr: HerdrFixture, pane: string): number {
  const answer = herdr.run(["pane", "get", pane]) as { result: { pane: { scroll?: { offset_from_bottom: number } } } };
  return answer.result.pane.scroll?.offset_from_bottom ?? -1;
}

async function paneText(page: Page, pane: string): Promise<string> {
  return page.evaluate((id) => window.__hideProbe?.paneText(id) ?? "", pane);
}

// @platform: Keys reach an agent pane's PTY through the platform's Herdr.
test("Cmd+F scrolls the focused pane to a match above the screen", { tag: "@platform" }, async ({ page }) => {
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

test("Cmd+F on a full-screen agent opens the agent's own search instead of the find bar", async ({ page }) => {
  await page.setViewportSize({ width: 1680, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    // The first agent has printed nothing, so Herdr holds no history for it,
    // as for an agent drawing on the alternate screen.
    const [agent] = herdr.panes;
    daemon = await startHided(herdr, "agent-find");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    // Herdr focused this pane when the fixture made it; a click would send
    // the agent a mouse report, so the pane is not clicked.
    const view = page.locator(`[data-pane-view="${agent}"]`);
    await expect(view).toHaveAttribute("data-focused", "true", { timeout: 20_000 });
    expect(scrollOffset(herdr, agent)).toBe(0);
    const before = fs.readFileSync(herdr.inputLogs[0], "latin1").length;

    await page.keyboard.press("Meta+f");
    // The agent hears Claude Code's transcript key and its search key, and
    // nothing else; Hide's own bar stays closed and the keyboard stays on the pane.
    // JSON keeps the control byte readable in a failure.
    await expect.poll(() => JSON.stringify(fs.readFileSync(herdr.inputLogs[0], "latin1").slice(before)), { timeout: 10_000 }).toBe(JSON.stringify("\x0f/"));
    await expect(page.locator("[data-find-bar]")).toHaveCount(0);
    expect(last.get("pane_find_open")).toMatchObject({ pane_id: agent });
    expect(sent.get("pane_find") ?? 0).toBe(0);
    await expect
      .poll(() => page.evaluate(() => document.activeElement?.closest("[data-pane-view]")?.getAttribute("data-pane-view") ?? null))
      .toBe(agent);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
