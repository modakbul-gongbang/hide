import { expect, type ElectronApplication } from "@playwright/test";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { isolate, launch, screenshot, test } from "./fixture";

test("Agent edge drag splits the desktop column into two live tab groups", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "agent-groups");
  let app: ElectronApplication | null = null;
  try {
    const created = herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--no-focus"]) as { result: { tab: { tab_id: string }; root_pane: { pane_id: string } } };
    const launched = await launch(run.env);
    app = launched.app;
    const page = launched.page;
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setSize(1920, 1080));
    await enterWorkspace(page, "fixture");
    const tab = page.locator(`[data-agent-tab-bar] [data-tab="${created.result.tab.tab_id}"]`);
    await expect(tab).toBeVisible();
    const start = await tab.boundingBox();
    const body = await page.locator("[data-agent-body]").boundingBox();
    if (!start || !body) throw new Error("Agent drag target is missing");
    await page.mouse.move(start.x + start.width / 2, start.y + start.height / 2);
    await page.mouse.down();
    await page.mouse.move(start.x + start.width / 2 + 12, start.y + start.height / 2, { steps: 3 });
    await page.mouse.move(body.x + body.width * .92, body.y + body.height / 2, { steps: 12 });
    await expect(page.locator('[data-agent-drop="right"]')).toHaveText("Split right");
    await page.mouse.up();
    await expect(page.locator("[data-agent-area-id]")).toHaveCount(2);
    await expect(page.locator("[data-agent-tab-bar]")).toHaveCount(2);
    await expect(page.locator(`[data-canvas="${herdr.tab}"]`)).toBeVisible();
    await expect(page.locator(`[data-canvas="${created.result.tab.tab_id}"]`)).toBeVisible();
    for (const pane of [herdr.panes[0], created.result.root_pane.pane_id]) {
      const terminal = page.locator(`[data-terminal-host="${pane}"]`);
      await expect(terminal).toBeVisible();
      await terminal.click();
      await page.keyboard.type("printf 'desktop-group-live\\n'");
      await page.keyboard.press("Enter");
      await expect.poll(() => execFileSync(herdr.bin, ["pane", "read", pane, "--source", "visible", "--format", "text"], { env: herdr.env, encoding: "utf8", timeout: 10_000 })).toMatch(/(?:^|\n)desktop-group-live\r?(?:\n|$)/);
    }
    await expect(page.locator('[data-transport="released"]')).toHaveCount(0);
    await page.keyboard.down("Meta");
    await expect(page.locator(`[data-tab="${herdr.tab}"] [data-keycap="1"]`)).toBeVisible();
    await expect(page.locator(`[data-tab="${created.result.tab.tab_id}"] [data-keycap="2"]`)).toBeVisible();
    await page.keyboard.up("Meta");
    await expect(page.locator("[data-keycap]")).toHaveCount(0);
    await screenshot(page, "desktop-agent-groups-split");
  } finally {
    await app?.close().catch(() => undefined);
    run.cleanup();
    herdr.stop();
  }
});
