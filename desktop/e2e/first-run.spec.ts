// The first-run wizard in the desktop app (PRD first-run-onboarding): a HOME
// whose kit has never run opens it over the shell, its steps move only
// through their own buttons, and the notification step and Settings share the
// switch the core keeps. The development host has no permission helper, so
// macOS's permission reads as unreadable here; the packaged helper is checked
// natively. The page's test notification is caught in the main process so the
// run never puts a banner on the machine's screen.

import { expect, type ElectronApplication } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import { hostLog, isolate, launch, screenshot, test, type Isolated } from "./fixture";

let herdr: HerdrFixture;
let run: Isolated;
let app: ElectronApplication | null = null;

test.describe.configure({ timeout: 180_000 });

test.beforeAll(async () => {
  herdr = await startHerdr();
});

test.afterAll(() => {
  herdr?.stop();
});

test.beforeEach(() => {
  run = isolate(herdr, "first-run");
  // A machine the kit has never run on (`seedKitRecord`): the first run's question waits for the wizard.
  fs.rmSync(path.join(run.env.HOME!, ".hide", "kit", "installed.json"));
});

test.afterEach(async () => {
  const info = test.info();
  if (info.status !== info.expectedStatus) console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
});

test("the wizard walks agents, notifications and the first project, never returns, and Help reopens it as a closable walk", async () => {
  let page;
  ({ app, page } = await launch(run.env));
  await app.evaluate(({ Notification }) => {
    Notification.prototype.show = function show() {};
  });
  const wizard = page.locator("[data-first-run]");
  // B3: the agents step over the shell, with the three-part progress.
  await expect(wizard).toHaveAttribute("data-first-run", "agents", { timeout: 60_000 });
  await expect(page.locator('[data-first-run-progress="1"]')).toContainText("1 of 3");
  await screenshot(page, "first-run-agents");
  // B5: Escape does not close it.
  await page.keyboard.press("Escape");
  await expect(wizard).toHaveAttribute("data-first-run", "agents");

  // B6: Continue moves on once this Mac's kit pass for it has landed.
  await page.locator("[data-onboarding-apply]").click();
  await expect(wizard).toHaveAttribute("data-first-run", "notifications", { timeout: 60_000 });
  await expect(page.locator('[data-first-run-next="notifications"]')).toHaveText("Not now");
  await page.locator("[data-notification-turn-on]").click();
  // B17: no helper in a development host, so the setting cannot be read; the test still goes, once.
  await expect(page.locator('[data-notification-line="unreadable"]')).toBeVisible();
  await expect.poll(() => hostLog(run.env).filter((line) => line.event === "notify.shown").length).toBe(1);
  expect(hostLog(run.env).some((line) => line.event === "notify.permission_unreadable" && line.reason === "helper_missing")).toBe(true);
  await screenshot(page, "first-run-notifications");
  await page.locator('[data-first-run-next="notifications"]').click();

  await expect(wizard).toHaveAttribute("data-first-run", "project");
  await expect(page.locator("[data-add-project-browse]")).toBeVisible();
  await screenshot(page, "first-run-project");
  // B25: Skip for now ends it, and it does not return (B6).
  await page.locator("[data-first-run-skip]").click();
  await expect(wizard).toHaveCount(0);
  await page.reload();
  await expect(page.locator("[data-main-screen], [data-workspace-screen]").first()).toBeVisible({ timeout: 30_000 });
  await expect(wizard).toHaveCount(0);

  // B30: Settings > General keeps the switch the step turned on.
  await page.locator("[data-open-settings]").click();
  await expect(page.locator('[data-settings-group="notifications"] [data-notification-switch]')).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");

  // B45, B46: Help > Set up Hide opens the steps from the first, and Escape closes them.
  await app.evaluate(({ Menu }) => Menu.getApplicationMenu()!.getMenuItemById("setup_hide")!.click());
  await expect(wizard).toHaveAttribute("data-first-run-mode", "setup");
  await expect(wizard).toHaveAttribute("data-first-run", "agents");
  await page.keyboard.press("Escape");
  await expect(wizard).toHaveCount(0);
});
