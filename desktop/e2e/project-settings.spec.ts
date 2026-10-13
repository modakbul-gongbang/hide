// Scoped saves, failure recovery and restart use a real private core and
// filesystem. Provider discovery is the fixture's PATH, never the account's.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { expect, type ElectronApplication } from "@playwright/test";
import { startHerdr } from "../../web/e2e/herdr-fixture";
import { captureNativeWindow } from "./native-window";
import { fitWindow, isolate, launch, screenshot, test } from "./fixture";

test("Project settings keep confirmed values through a failed save and restart", async () => {
  const herdr = await startHerdr({ agents: false });
  const run = isolate(herdr, "project-settings");
  let app: ElectronApplication | null = null;
  try {
    const repo = path.join(run.env.HOME!, "projects", "sample");
    fs.mkdirSync(repo);
    execFileSync("git", ["init", "-q", "-b", "main"], { cwd: repo });
    fs.writeFileSync(path.join(repo, "README.md"), "Fixture\n");
    execFileSync("git", ["add", "README.md"], { cwd: repo });
    execFileSync("git", ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "Initial"], { cwd: repo });
    herdr.run(["workspace", "create", "--cwd", repo, "--label", "sample", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);
    let launched = await launch(run.env);
    app = launched.app;
    let page = launched.page;
    const gear = () => page.locator('[data-project]', { hasText: "sample" }).locator('[data-row-settings]');
    await gear().click();
    let sheet = page.locator('[data-project-settings]');
    await expect(sheet).toBeVisible();
    await expect(sheet.getByRole("tab")).toHaveText(["Connections", "Working style", "Memory"]);
    // The real discovery answer on a repository without a GitHub remote.
    await expect(sheet).toContainText("Local");
    await sheet.getByRole("combobox", { name: "Issue source", exact: true }).click();
    await page.getByRole("option", { name: "Local", exact: true }).click();
    await expect(sheet).toContainText("Saved");
    const stateFile = path.join(run.env.HIDE_STATE_DIR!, "core-state.json");
    const stored = () => JSON.parse(fs.readFileSync(stateFile, "utf8")) as { project_settings: Record<string, { device_id: string; root: string; issue_source?: string; new_work?: string }> };
    await expect.poll(() => Object.values(stored().project_settings).find((record) => record.root === repo)?.issue_source).toBe("local");
    await sheet.getByRole("tab", { name: "Working style" }).click();
    await expect(sheet).toContainText("Unsupported");
    const location = sheet.getByRole("combobox", { name: "Start new work in" });
    await expect(location).toContainText("Existing checkout");
    // A real atomic-replacement failure, confined to this fixture's state file.
    const backup = `${stateFile}.before-failure`;
    fs.renameSync(stateFile, backup);
    fs.mkdirSync(stateFile);
    try {
      await location.click();
      await page.getByRole("option", { name: "New worktree", exact: true }).click();
      await expect(sheet).toContainText("Couldn't save");
      await expect(location).toContainText("Existing checkout");
      await screenshot(page, "project-settings-save-failed");
    } finally {
      fs.rmdirSync(stateFile);
      fs.renameSync(backup, stateFile);
    }
    await sheet.getByRole("button", { name: "Try again" }).click();
    await expect(location).toContainText("New worktree");
    await expect(sheet).toContainText("Saved");
    await expect.poll(() => Object.values(stored().project_settings).find((record) => record.root === repo)?.new_work).toBe("worktree");
    await captureNativeWindow(app, "project-settings-work-native", { state: run.env.HIDE_STATE_DIR, home: run.env.HOME, socket: herdr.socket, daemonPid: run.daemonPid(), build: "worktree debug", provider: "fixture discovery, no external GitHub" });
    await page.keyboard.press("Escape");
    await expect(sheet).toHaveCount(0);
    await expect(gear()).toBeFocused();
    await app.close();
    app = null;
    expect(run.hide(["stop"]).status).toBe(0);
    launched = await launch(run.env);
    app = launched.app;
    page = launched.page;
    await gear().click();
    sheet = page.locator('[data-project-settings]');
    await sheet.getByRole("tab", { name: "Working style" }).click();
    await expect(sheet.getByRole("combobox", { name: "Start new work in" })).toContainText("New worktree");
    // Actual narrow window and largest supported interface text in all scripts.
    await fitWindow(app, { width: 720, height: 480 });
    for (const language of ["en", "ko", "ja", "zh-CN"]) {
      await page.keyboard.press("Escape");
      await page.locator('[data-open-settings]').click();
      await page.locator('[data-interface-language]').click();
      await page.locator(`[data-language-option="${language}"]`).click();
      await expect(page.locator("html")).toHaveAttribute("lang", language);
      await page.keyboard.press("Escape");
      await page.evaluate(() => document.documentElement.style.setProperty("--interface-scale", String(17 / 13)));
      await gear().click();
      const tabs = sheet.getByRole("tab");
      await tabs.first().focus();
      await page.keyboard.press("ArrowDown");
      await expect(tabs.nth(1)).toHaveAttribute("data-state", "active");
      for (const theme of ["light", "dark"]) {
        await page.evaluate((theme) => { document.documentElement.classList.toggle("dark", theme === "dark"); document.documentElement.classList.toggle("light", theme === "light"); }, theme);
        for (let index = 0; index < 3; index++) {
          await tabs.nth(index).click();
          await expect.poll(() => sheet.evaluate((node) => {
            const panel = node.querySelector('[role=tabpanel]')!;
            const nav = node.querySelector("nav")!;
            return node.scrollWidth <= node.clientWidth && panel.scrollWidth <= panel.clientWidth && nav.scrollHeight <= nav.clientHeight;
          })).toBe(true);
          await screenshot(page, `project-settings-${language}-${theme}-${index}-narrow`);
        }
      }
    }
    // B18: project + opens the same confirmation, never starts blindly.
    await page.keyboard.press("Escape");
    await page.locator('[data-project]', { hasText: "sample" }).locator('[data-row-start]').click();
    const confirmation = page.locator('[data-start-worktree]');
    await expect(confirmation).toBeVisible();
    await expect(confirmation.getByRole("combobox")).toContainText("main");
    await expect(confirmation.getByRole("textbox")).toHaveValue("");
  } finally {
    if (app) await app.close();
    run.cleanup();
    herdr.stop();
  }
});
