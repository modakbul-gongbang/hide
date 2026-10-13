// The native host's smallest window and largest text must contain all footer
// controls and the Usage sheet. The provider alone is a fixed external shim.
import { expect, type ElectronApplication } from "@playwright/test";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { usageShim } from "../../web/e2e/usage-fixture";
import { fitWindow, isolate, launchShell, test } from "./fixture";
import { captureNativeWindow } from "./native-window";

test("three usage chips, Help and Settings fit the minimum native window in English and Korean", async () => {
  const herdr = await startHerdr();
  const run = isolate(herdr, "usage");
  const usage = usageShim();
  run.env.PATH = usage.path;
  usage.release();
  let app: ElectronApplication | undefined;
  const failures: unknown[] = [];
  try {
    const opened = await launchShell(run.env);
    app = opened.app;
    const page = opened.page;
    await fitWindow(app, { width: 720, height: 480 });
    await expect(page.locator('[data-usage-chip="claude"]')).toContainText("62%", { timeout: 30_000 });
    await page.locator('[data-usage-chip="claude"]').click();
    await page.getByRole("button", { name: "Choose chips" }).click();
    await page.getByRole("switch", { name: "Claude Code 5 hours" }).click();
    await page.locator('[data-settings-tab="general"]').click();
    await page.locator('[data-font-size] [role="slider"]').press("End");
    const facts = { home: run.env.HOME, state: run.env.HIDE_STATE_DIR, socket: herdr.socket, provider: "external Claude CLI shim; Codex signed out", window: "720x480, maximum Interface text" };
    for (const language of ["en", "ko"] as const) {
      await page.locator('[data-interface-language]').click();
      await page.locator(`[data-language-option="${language}"]`).click();
      await page.locator(`[data-theme-option="${language === "en" ? "light" : "dark"}"]`).click();
      await page.locator('[data-settings-tab="usage"]').click();
      await expect(page.locator('[data-usage-settings]')).toBeVisible();
      await captureNativeWindow(app, `usage-settings-${language}-native`, facts);
      await page.keyboard.press("Escape");
      const chips = page.locator('[data-usage-chip]');
      await expect(chips).toHaveCount(3);
      const footer = page.locator('[data-sidebar-footer]');
      const boxes = await footer.evaluate((element) => {
        const bounds = element.getBoundingClientRect();
        return [...element.querySelectorAll("button")].map(button => {
          const box = button.getBoundingClientRect();
          return box.left >= bounds.left && box.right <= bounds.right && box.top >= bounds.top && box.bottom <= bounds.bottom;
        });
      });
      expect(boxes).toHaveLength(5);
      expect(boxes.every(Boolean)).toBe(true);
      await captureNativeWindow(app, `usage-footer-${language}-native`, facts);
      await chips.first().click();
      const popover = page.locator('[data-usage-popover]');
      await expect(popover.locator('[data-usage-window-row="five_hour"] [data-usage-value]')).toHaveText("4%");
      expect(await popover.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
      await captureNativeWindow(app, `usage-popover-${language}-native`, facts);
      await popover.getByRole("button").click();
      await page.locator('[data-settings-tab="general"]').click();
    }
  } catch (error) { failures.push(error); }
  finally {
    try { await app?.close(); } catch (error) { failures.push(error); }
    try { run.cleanup(); } catch (error) { failures.push(error); }
    try { herdr.stop(); } catch (error) { failures.push(error); }
    try { usage.cleanup(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "native Usage or teardown failed");
});
