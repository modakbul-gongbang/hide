// A private native transcript reaches the actual ledger, snapshot and both
// usage surfaces. Cache reads are deliberately much larger than new tokens.
import fs from "node:fs";
import path from "node:path";
import { expect, type ElectronApplication } from "@playwright/test";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { usageShim } from "../../web/e2e/usage-fixture";
import { fitWindow, isolate, launchShell, screenshot, test } from "./fixture";

test("native usage history preserves totals, hover geometry and the Overview filter", async () => {
  const herdr = await startHerdr();
  const run = isolate(herdr, "usage-history");
  const usage = usageShim();
  run.env.PATH = usage.path;
  usage.release();
  // General terminal fixtures link their transcripts; the ledger deliberately
  // refuses linked trees, so this accounting fixture owns a real directory.
  const projects = path.join(run.env.HOME!, ".claude", "projects");
  if (fs.lstatSync(projects).isSymbolicLink()) fs.unlinkSync(projects);
  const root = path.join(projects, "fixture");
  fs.mkdirSync(root, { recursive: true });
  const now = Date.now();
  fs.writeFileSync(path.join(root, "usage.jsonl"), Array.from({ length: 6 }, (_, i) => JSON.stringify({
    type: "assistant", sessionId: "fixture", requestId: `request-${i}`,
    timestamp: new Date(now - i * 86_400_000).toISOString(),
    message: { id: `message-${i}`, model: "fixture-unpriced-model", content: [], usage: { input_tokens: 10, output_tokens: 20, cache_creation_input_tokens: 30, cache_read_input_tokens: 900 } },
  })).join("\n") + "\n");
  let app: ElectronApplication | undefined;
  const failures: unknown[] = [];
  try {
    const opened = await launchShell(run.env);
    app = opened.app;
    const page = opened.page;
    await fitWindow(app, { width: 720, height: 480 });
    const chip = page.locator('[data-usage-chip="claude"]');
    await expect(chip).toBeVisible({ timeout: 30_000 });
    const popover = page.locator('[data-usage-popover]');
    const detail = popover.locator('[data-usage-detail]');
    const capture = async (name: string) => {
      const candidate = await app!.evaluate(({ BrowserWindow }) => ({ pid: process.pid, executable: process.execPath, windows: BrowserWindow.getAllWindows().length, switches: process.argv, bounds: BrowserWindow.getAllWindows()[0]!.getBounds() }));
      expect(candidate.windows).toBe(1);
      const directory = process.env.HIDE_E2E_SCREENSHOT_DIR;
      if (directory) fs.writeFileSync(path.join(directory, `${name}.json`), JSON.stringify({ candidate, home: run.env.HOME, socket: herdr.socket, fixture: "six native calls; new tokens 360, cache read 5400; unknown model", collector: "isolated Electron renderer screenshot" }, null, 2));
      await screenshot(page, name);
    };
    for (const language of ["en", "ko"] as const) {
      for (const large of [false, true]) {
        await page.locator('[data-open-settings]').click();
        await page.locator('[data-settings-tab="general"]').click();
        await page.locator('[data-interface-language]').click();
        await page.locator(`[data-language-option="${language}"]`).click();
        await page.locator(`[data-theme-option="${language === "en" ? "light" : "dark"}"]`).click();
        const slider = page.locator('[data-font-size] [role="slider"]');
        await slider.press(large ? "End" : "Home");
        if (!large) { await slider.press("ArrowRight"); await slider.press("ArrowRight"); }
        await page.keyboard.press("Escape");
        await chip.click();
        await expect(popover.locator('[data-usage-total]')).toHaveText("360", { timeout: 30_000 });
        const share = popover.locator('[data-usage-detail] [data-usage-filter="claude"]:visible');
        await share.scrollIntoViewIfNeeded();
        const target = await share.boundingBox();
        await share.hover();
        await expect(detail.locator('[data-usage-cost]:visible')).toContainText("fixture-unpriced-model");
        expect(await share.boundingBox()).toEqual(target);
        const before = await detail.boundingBox();
        await popover.locator('[data-usage-day][data-today="true"]').hover();
        await expect(detail.locator('[data-usage-cost]:visible')).toContainText("fixture-unpriced-model");
        const after = await detail.boundingBox();
        expect(after!.height).toBe(before!.height);
        expect(await popover.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
        await capture(`usage-history-${language}-${large ? "largest" : "default"}`);
        await popover.locator('[data-usage-history-link]').click();
        const overview = page.locator('[data-usage-overview]');
        await expect(overview).toBeVisible();
        await expect(overview.locator('[data-usage-total]')).toHaveText("360");
        const filter = overview.locator('[data-usage-filter="claude"]');
        await filter.click();
        await expect(filter).toHaveAttribute("aria-pressed", "true");
        await filter.click();
        await expect(filter).toHaveAttribute("aria-pressed", "false");
        expect(await overview.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
        await capture(`usage-overview-${language}-${large ? "largest" : "default"}`);
      }
    }
  } catch (error) { failures.push(error); }
  finally {
    try { await app?.close(); } catch (error) { failures.push(error); }
    try { run.cleanup(); } catch (error) { failures.push(error); }
    try { herdr.stop(); } catch (error) { failures.push(error); }
    try { usage.cleanup(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "native history or teardown failed");
});
