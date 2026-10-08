// Synthetic native-format writers on actual pinned Herdr and hided.
// Installed CLI measurements remain a separate acceptance boundary.
import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { PI_ID, PI_TITLE, preparePiWriter, reportPiWriter } from "./session-reader-fixture";
import { screenshot } from "./wire";

test.describe.configure({ timeout: 150_000 });

test("Pi native title and durable sleep wake the exact conversation in a fresh pane", async ({ page }) => {
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const sourcePane = herdr.panes[0];
    const session = preparePiWriter(herdr);
    expect(fs.existsSync(session)).toBe(false);
    herdr.run(["agent", "start", "pi-reader", "--kind", "pi", "--pane", sourcePane]);
    await expect.poll(() => fs.existsSync(session)).toBe(true);
    reportPiWriter(herdr, sourcePane, session);
    daemon = await startHided(herdr, "pi-reader", herdr.env.HOME);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await page.locator('[data-sidebar-mode="agents"]').click();
    const row = page.locator(`[data-agent-list] [data-pane="${sourcePane}"]`);
    await expect(row).toContainText(PI_TITLE, { timeout: 30_000 });
    await screenshot(page, "pi-native-title");
    await row.click();
    await page.locator(`[data-pane-menu="${sourcePane}"]`).click();
    await expect(page.locator('[data-menu-item="sleep_agent"]')).toBeEnabled({ timeout: 20_000 });
    await page.locator('[data-menu-item="sleep_agent"]').click();
    const sleeping = page.locator('[data-sleeping-session]');
    await expect(sleeping).toContainText(PI_TITLE, { timeout: 30_000 });
    await expect(row).toHaveCount(0);
    await expect.poll(() => JSON.stringify(herdr.run(["pane", "list"]))).not.toContain(`"pane_id":"${sourcePane}"`);
    await screenshot(page, "pi-durable-sleep");
    const prior = fs.readFileSync(session, "utf8");
    await sleeping.getByRole("button", { name: "Wake agent" }).click();
    type Listed = { result: { agents: { pane_id: string; agent: string }[] } };
    let fresh = "";
    await expect.poll(() => {
      const agents = (herdr.run(["agent", "list"]) as Listed).result.agents;
      fresh = agents.find(agent => agent.agent === "pi" && agent.pane_id !== sourcePane)?.pane_id ?? "";
      return fresh;
    }, { timeout: 30_000 }).not.toBe("");
    await expect.poll(() => fs.readFileSync(path.join(herdr.root, "pi-launches.jsonl"), "utf8").trim().split("\n").map(line => JSON.parse(line) as string[])).toContainEqual(["--session", PI_ID]);
    reportPiWriter(herdr, fresh, session);
    await expect(sleeping).toHaveCount(0, { timeout: 30_000 });
    await expect(page.locator(`[data-agent-list] [data-pane="${fresh}"]`)).toContainText(PI_TITLE);
    expect(fs.readFileSync(session, "utf8")).toBe(prior);
    await screenshot(page, "pi-exact-wake");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
