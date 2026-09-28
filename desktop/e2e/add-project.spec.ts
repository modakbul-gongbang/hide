// Add a project in the desktop app: the dialog's Browse folder asks the main
// process for macOS's folder picker, which this spec replaces with a queue of
// answers, so a pick, a cancel and a refused folder run with no native sheet.

import { expect, test, type ElectronApplication, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { sendEvent } from "../../web/e2e/wire";
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

    // A device's folders are not this Mac's to browse: its Host gets a ~/ field,
    // and what the device path answers (here: no connection) stays in the dialog.
    const state = JSON.parse(fs.readFileSync(path.join(run.env.HIDE_STATE_DIR!, "hided.json"), "utf8")) as { token: string };
    await sendEvent(page, { origin: new URL(page.url()).origin, token: state.token }, "register_device", {
      id: "ssh-none", label: "Offline box", ssh_alias: "hide-e2e-no-such-host.invalid", host_consent: false,
    });
    await page.keyboard.press("Meta+Shift+KeyN");
    await dialog.locator("[data-add-project-host]").click();
    await page.locator('[data-host-option="ssh-none"]').click();
    await expect(dialog).toHaveAttribute("data-add-project", "ssh-none");
    await expect(browse).toHaveCount(0);
    const field = dialog.getByLabel("Folder on Offline box");
    await expect(field).toBeFocused();
    await expect(field).toHaveValue("~/");
    await field.fill("~/projects/app");
    await field.press("Enter");
    await expect(alert).toHaveAttribute("data-registration-reason", "core");
    await expect(alert).toHaveAttribute("data-registration-path", "~/projects/app");
    await expect(alert).toContainText("needs its connection");
    await captureWindow(app, page, "add-project-device");
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
  } finally {
    await app?.close();
    run.cleanup();
    herdr.stop();
  }
});

test("Create new project makes a Git repository and adds it; a name already taken is refused before anything is sent", async () => {
  const herdr = await startHerdr();
  const run = isolate(herdr, "create-project");
  let app: ElectronApplication | undefined;
  try {
    const home = run.env.HOME!;
    fs.mkdirSync(path.join(home, "projects", "taken"), { recursive: true });
    fs.writeFileSync(path.join(home, "projects", "taken", "notes.md"), "mine\n");
    const projects = fs.realpathSync(path.join(home, "projects"));
    const launched = await launch(run.env, { switches: ["--disable-backgrounding-occluded-windows"] });
    app = launched.app;
    const page = launched.page;
    await expect(page.locator("[data-main-screen], [data-workspace-screen]")).toBeVisible({ timeout: 30_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();
    const dialog = page.locator("[data-add-project]");
    const view = dialog.locator("[data-create-project]");
    const name = view.getByLabel("Name");
    const location = view.locator("[data-create-project-location]");
    const preview = view.locator("[data-create-project-path]");
    const problem = view.locator("[data-create-project-problem]");
    const submit = view.locator("[data-create-project-submit]");

    // Under Browse folder, Create new project opens its own view with the name field focused;
    // with no project yet the folder goes in home.
    await page.keyboard.press("Meta+Shift+KeyN");
    await expect(dialog.locator("[data-add-project-other-ways]")).toContainText("Other ways to add");
    await dialog.locator('[data-add-project-way="create"]').click();
    await expect(view.getByRole("heading", { name: "Create a new project" })).toBeVisible();
    await expect(name).toBeFocused();
    await expect(location).toContainText("Git repository in ~");
    await expect(submit).toBeDisabled();

    // The location row opens the native picker; the full path follows the name as it is typed.
    await stubPicker(app, [{ canceled: false, filePaths: [projects] }]);
    await location.click();
    await expect(location).toContainText("Git repository in ~/projects");
    await expect(preview).toHaveAttribute("data-create-project-path", path.join(projects, "project-name"));
    await name.fill("taken");
    await expect(problem).toHaveAttribute("data-create-project-problem", "already_exists");
    await expect(submit).toBeDisabled();
    await name.fill("a/b");
    await expect(problem).toHaveAttribute("data-create-project-problem", "name");
    await expect(submit).toBeDisabled();
    await name.fill("fresh");
    await expect(preview).toHaveAttribute("data-create-project-path", path.join(projects, "fresh"));
    await expect(problem).toHaveCount(0);
    await expect(submit).toBeEnabled();
    await captureWindow(app, page, "create-project");
    expect(fs.existsSync(path.join(projects, "fresh"))).toBe(false);

    // Enter creates it: the folder is a repository of its own and the project appears.
    await name.press("Enter");
    await expect(dialog).toHaveCount(0, { timeout: 20_000 });
    await expect(page.locator("[data-project-list]")).toContainText("fresh", { timeout: 20_000 });
    expect(fs.statSync(path.join(projects, "fresh", ".git")).isDirectory()).toBe(true);
    expect(fs.readFileSync(path.join(projects, "taken", "notes.md"), "utf8")).toBe("mine\n");

    // The next create starts beside it, and the same name again is the project already added.
    await page.keyboard.press("Meta+Shift+KeyN");
    await dialog.locator('[data-add-project-way="create"]').click();
    await expect(location).toContainText("Git repository in ~/projects");
    await name.fill("fresh");
    await expect(problem).toHaveAttribute("data-create-project-problem", "already_registered");
    await expect(submit).toBeDisabled();

    // Back returns to the first view, the keyboard on the way it came from.
    await view.locator("[data-create-project-back]").click();
    await expect(dialog.getByRole("heading", { name: "Add a project" })).toBeVisible();
    await expect(dialog.locator('[data-add-project-way="create"]')).toBeFocused();
    await page.keyboard.press("Escape");
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
