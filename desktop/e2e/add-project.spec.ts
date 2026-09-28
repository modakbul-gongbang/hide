// Add a project in the desktop app: the dialog's Browse folder asks the main
// process for macOS's folder picker, which this spec replaces with a queue of
// answers, so a pick, a cancel and a refused folder run with no native sheet.

import { expect, test, type ElectronApplication, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { isolate, launch } from "./fixture";

type Pick = { canceled: boolean; filePaths: string[] };

/** Replaces the picker with `answers`, one per call; returns how many calls it has had. */
async function stubPicker(app: ElectronApplication, answers: Pick[]): Promise<void> {
  await app.evaluate(({ dialog }, queued) => {
    const state = globalThis as unknown as { __picks: Pick[]; __pickCalls: number };
    state.__picks = queued;
    state.__pickCalls ??= 0;
    dialog.showOpenDialog = (async () => {
      state.__pickCalls += 1;
      const next = state.__picks.shift();
      if (!next) throw new Error("no queued pick");
      return next;
    }) as typeof dialog.showOpenDialog;
  }, answers);
}

const pickCalls = (app: ElectronApplication) => app.evaluate(() => (globalThis as unknown as { __pickCalls: number }).__pickCalls);

test("Add a project picks a folder with the native picker, and a cancel or a refusal keeps the dialog", async () => {
  const herdr = await startHerdr();
  const run = isolate(herdr, "add-project");
  let app: ElectronApplication | undefined;
  try {
    const home = run.env.HOME!;
    fs.mkdirSync(path.join(home, "projects", "alpha"), { recursive: true });
    fs.mkdirSync(path.join(run.root, "outside"), { recursive: true });
    // The picker answers real paths; hided keeps the canonical spelling it checked.
    const alpha = fs.realpathSync(path.join(home, "projects", "alpha"));
    const outside = fs.realpathSync(path.join(run.root, "outside"));
    const launched = await launch(run.env, { switches: ["--disable-backgrounding-occluded-windows"] });
    app = launched.app;
    const page = launched.page;
    await expect(page.locator("[data-main-screen], [data-workspace-screen]")).toBeVisible({ timeout: 30_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();
    const dialog = page.locator("[data-add-project]");
    const browse = page.locator("[data-add-project-browse]");
    const alert = dialog.locator("[data-registration-reason]");

    // The strip's + opens the dialog on this Mac, Browse folder holding the keyboard.
    const add = page.locator("[data-sidebar-new-workspace]");
    await expect(add).toHaveAccessibleName("Add project");
    const [addBox, searchBox] = [(await add.boundingBox())!, (await page.locator("[data-sidebar-search]").boundingBox())!];
    expect(addBox.x).toBeLessThan(searchBox.x);
    await add.click();
    await expect(dialog).toHaveAttribute("data-add-project", "local");
    await expect(dialog.getByRole("heading", { name: "Add a project" })).toBeVisible();
    await expect(dialog.locator("[data-add-project-host]")).toHaveText(/This Mac/);
    await expect(browse).toBeFocused();

    // A cancelled pick changes nothing: the dialog stays, with no alert and nothing pending.
    await stubPicker(app, [{ canceled: true, filePaths: [] }]);
    await page.keyboard.press("Enter");
    await expect.poll(() => pickCalls(app!)).toBe(1);
    await expect(dialog).toBeVisible();
    await expect(alert).toHaveCount(0);
    await expect(dialog.locator("[data-registration-pending]")).toHaveCount(0);

    // A folder outside home is refused by hided; the alert names it and the dialog stays for another pick.
    await stubPicker(app, [{ canceled: false, filePaths: [outside] }]);
    await browse.click();
    await expect(alert).toHaveAttribute("data-registration-reason", "outside_home");
    await expect(alert).toHaveAttribute("data-registration-path", outside);
    await expect(alert).toContainText(outside);
    await expect(browse).toBeEnabled();
    await captureWindow(app, page, "add-project-refused");

    // A folder under home registers: the dialog closes and the project appears.
    await stubPicker(app, [{ canceled: false, filePaths: [alpha] }]);
    await browse.click();
    await expect(dialog).toHaveCount(0, { timeout: 20_000 });
    await expect(page.locator("[data-project-list]")).toContainText("alpha", { timeout: 20_000 });

    // ⌘⇧N opens it again; the same folder is refused by the shell before anything is sent.
    await page.keyboard.press("Meta+Shift+KeyN");
    await expect(dialog).toBeVisible();
    await stubPicker(app, [{ canceled: false, filePaths: [alpha] }]);
    await page.keyboard.press("Enter");
    await expect(alert).toHaveAttribute("data-registration-reason", "already_registered");
    await expect(alert).toContainText(alpha);
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);

    // The Overview's Add project opens the same dialog, and its × closes it.
    await page.locator("[data-overview-destination]").click();
    await page.locator("[data-main-add-project]").click();
    await expect(dialog).toBeVisible();
    await captureWindow(app, page, "add-project-dialog");
    await dialog.getByRole("button", { name: "Close" }).click();
    await expect(dialog).toHaveCount(0);
  } finally {
    await app?.close();
    run.cleanup();
    herdr.stop();
  }
});

/** A capture of the candidate window by its own id, when a run directory was named. */
async function captureWindow(app: ElectronApplication, page: Page, name: string): Promise<void> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!dir) return;
  // The window paints on its own frame; capture after two, so the state just asserted is on screen.
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await page.waitForTimeout(200);
  const source = await app.evaluate(({ BrowserWindow }) => {
    const windows = BrowserWindow.getAllWindows();
    if (windows.length !== 1) throw new Error("Expected exactly one candidate window");
    return windows[0]!.getMediaSourceId();
  });
  fs.mkdirSync(dir, { recursive: true });
  execFileSync("/usr/sbin/screencapture", ["-x", "-o", "-l", source.split(":")[1]!, path.join(dir, `${name}.png`)]);
}
