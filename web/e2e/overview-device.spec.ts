// The device Overview's backlog and the pane head's record button (PRD
// overview-device-tasks B22-B24, B26-B32), on an isolated Herdr and hided.
import { expect, test } from "@playwright/test";
import { labelAgent, startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { openOverview } from "./overview-entry";
import { FIRST_GOAL } from "./session-work-fixture";
import { enterWorkspace } from "./wire";

test.describe.configure({ timeout: 180_000 });

test("@platform a folder's memo goes into its backlog from the input, opens in the panel, and 되돌리기 deletes it", async ({ page }) => {
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | undefined;
  try {
    daemon = await startHided(herdr, "overview-backlog");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await openOverview(page, "backlog");
    // B26: a folder keeps memos on this device; the input says so before Enter.
    const input = page.locator("[data-backlog-input]").first();
    await expect(input).toBeVisible({ timeout: 30_000 });
    await expect(page.locator("[data-backlog-destination]").first()).toHaveAttribute("data-backlog-destination", "local");
    await input.fill("폴더 메모");
    await input.press("Enter");
    // B29: the row is there at once and the input is empty for the next one.
    const row = page.locator("[data-backlog-row]", { hasText: "폴더 메모" });
    await expect(row).toBeVisible({ timeout: 30_000 });
    await expect(row).toContainText("L-1");
    await expect(input).toHaveValue("");
    await expect(page.locator('[data-main-tab="backlog"] [data-tab-count]')).toHaveText("1");
    // B31, B32: the row opens its panel, whose 되돌리기 deletes the memo.
    await row.getByRole("button").first().click();
    const item = page.locator("[data-overview-panel] [data-backlog-item]");
    await expect(item).toBeVisible();
    await item.locator("[data-item-undo]").click();
    await expect(row).toHaveCount(0, { timeout: 30_000 });
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform an agent pane's head opens its session record in the lane and leaves the keyboard in the terminal", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    daemon = await startHided(herdr, "pane-record");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const pane = herdr.panes[0]!;
    const button = page.locator(`[data-pane-view="${pane}"] [data-pane-record]`);
    // B24: a pane whose session is not known has no button; it becomes one once the session is known.
    await expect(button).toHaveCount(0);
    labelAgent(herdr, pane, { task: FIRST_GOAL, at: Date.now(), end: "done" });
    await expect(button).toBeVisible({ timeout: 45_000 });
    const terminal = page.locator(`[data-pane-view="${pane}"] .xterm-helper-textarea`);
    await terminal.focus();
    // B22: the lane opens on the record and the terminal keeps the keyboard.
    await button.click();
    await expect(page.locator('[data-session-record="lane"]')).toBeVisible({ timeout: 30_000 });
    await expect(terminal).toBeFocused();
    // B23: the same button again goes back to the Sessions list.
    await button.click();
    await expect(page.locator('[data-session-record="lane"]')).toHaveCount(0);
    await expect(terminal).toBeFocused();
  } finally { daemon?.stop(); herdr.stop(); }
});
