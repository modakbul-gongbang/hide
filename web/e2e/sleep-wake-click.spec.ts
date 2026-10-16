// A sleeping pane's Wake agent takes a mouse click (issue 1279): the controls
// drawn over a terminal sit above every layer xterm draws, so a click at the
// button's center wakes the agent rather than landing in the terminal.
//
// The desktop app draws terminals with WebGL, whose canvases carry their own
// z-index; under the suite's --disable-webgl the DOM renderer has no layer
// over the pane's center and the click would land either way, so this file
// runs a software WebGL context as `terminal-webgl.spec.ts` does.

import { expect, test } from "@playwright/test";
import { claudeProjects, setFixtureSession, startHerdr, writeFixtureTranscript } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { screenshot } from "./wire";

test.use({ launchOptions: { args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"] } });
test.describe.configure({ timeout: 120_000 });

const SESSION = "22222222-3333-4444-5555-666666666666";

type ProcessInfo = { result: { process_info: { foreground_processes: { argv?: string[] | null }[] } } };

test("a mouse click at the center of Wake agent over a WebGL terminal wakes the pane", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [, sleeper] = herdr.panes;
    writeFixtureTranscript(claudeProjects(herdr), SESSION, { task: "Agent two" });
    setFixtureSession(herdr, sleeper, SESSION);
    daemon = await startHided(herdr, "sleep-wake-click");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await page.locator("[data-checkout]").first().click();
    const pane = page.locator(`[data-pane-view="${sleeper}"]`);
    await expect(pane).toBeVisible({ timeout: 20_000 });

    await page.locator(`[data-pane-menu="${sleeper}"]`).click();
    const item = page.locator('[data-menu-item="sleep_agent"]');
    await expect(item).toBeEnabled({ timeout: 20_000 });
    await item.click();
    const wake = pane.locator(`[data-agent-wake="${sleeper}"]`);
    await expect(wake).toBeVisible({ timeout: 30_000 });
    // The terminal under the sleeping body draws with WebGL, whose link
    // layer is the canvas the operator's click landed on.
    await expect(pane.locator("canvas.xterm-link-layer")).toHaveCount(1);
    await screenshot(page, "sleep-wake-click-sleeping");

    // A pointer press at the button's center, as the operator clicks;
    // Playwright's locator click would wait for a covering layer instead.
    const box = await wake.boundingBox();
    if (!box) throw new Error("Wake agent has no box");
    await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);

    const resumed = () =>
      (herdr.run(["pane", "process-info", "--pane", sleeper]) as ProcessInfo).result.process_info.foreground_processes.map((process) => process.argv?.join(" "));
    await expect.poll(resumed, { timeout: 30_000 }).toContain(`claude --resume ${SESSION}`);
    await expect(pane.locator("[data-pane-sleep]")).toHaveCount(0, { timeout: 30_000 });
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
