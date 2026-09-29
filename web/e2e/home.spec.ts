// This Mac's Home (PRD home-device-rail B16-B18, B21) against a private
// Herdr and hided: the Home row's `+` makes `~/hide` in the daemon's HOME with
// a link per registered project and hide's guide files, and opens a tab there;
// a `~/hide` that is not Hide's is left alone, and the reason shows where Home
// was opened, under the Home row and inside the start panel.

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

async function pinFixture(page: Page) {
  const row = page.locator("[data-project-row]").filter({ hasText: "fixture" }).first();
  await row.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Pin", exact: true }).click();
  await expect(page.locator("[data-home-count]")).toHaveText("1 project", { timeout: 20_000 });
}

async function openHomeTab(page: Page) {
  await page.locator("[data-home-destination]").hover();
  await page.locator("[data-home-new-tab]").click();
}

test("Home's + makes ~/hide with a link per project and opens a tab there", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "home");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await pinFixture(page);
    const home = path.join(daemon.home, "hide");
    // B18: no Home until it is first used.
    expect(fs.existsSync(home)).toBe(false);

    await openHomeTab(page);
    await expect.poll(() => fs.existsSync(path.join(home, ".hide-home.json")), { timeout: 30_000 }).toBe(true);
    for (const guide of ["AGENTS.md", "CLAUDE.md"]) expect(fs.existsSync(path.join(home, guide))).toBe(true);
    const link = path.join(home, "fixture");
    expect(fs.lstatSync(link).isSymbolicLink()).toBe(true);
    expect(fs.realpathSync(link)).toBe(fs.realpathSync(path.join(herdr.root, "fixture")));

    // B17: the tab opens in Home and the center goes there; the Home is still no project (B16).
    await expect(page.locator("[data-workspace-screen]")).toBeVisible({ timeout: 20_000 });
    await expect
      .poll(() => JSON.stringify(herdr.run(["api", "snapshot"])).includes(fs.realpathSync(home)), { timeout: 20_000 })
      .toBe(true);
    await expect(page.locator("[data-home-count]")).toHaveText("1 project");
    await expect(page.locator("[data-project-row]").filter({ hasText: /^hide$/ })).toHaveCount(0);
    await screenshot(page, "home-tab-opened");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("a ~/hide that is not Hide's is left alone, and the reason shows under the Home row and in the start panel", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "home-conflict");
    const home = path.join(daemon.home, "hide");
    fs.mkdirSync(home);
    fs.writeFileSync(path.join(home, "mine.txt"), "the operator's own folder\n");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");

    // B21: the Home row's + is refused where it was pressed, and the folder is untouched.
    await openHomeTab(page);
    const refusal = page.locator("[data-home-refusal]");
    await expect(refusal).toContainText("~/hide", { timeout: 20_000 });
    expect(fs.readdirSync(home)).toEqual(["mine.txt"]);
    await screenshot(page, "home-conflict-row");
    // A click on the Home row clears it.
    await page.locator("[data-home-destination]").click();
    await expect(refusal).toHaveCount(0);

    // B21: a start aimed at Home says the same inside the panel and keeps the text.
    await page.keyboard.press("Meta+KeyK");
    await expect(page.locator('[data-palette="Search"] [data-palette-input]')).toBeFocused();
    await page.keyboard.type("에이전트");
    await page.locator('[data-palette-row="command:start-agent"]').click();
    const panel = page.locator("[data-start-panel]");
    await expect(panel.locator("[data-start-target]")).toHaveAttribute("data-start-target", "home:local");
    await panel.locator("[data-start-text]").fill("홈에서 할 일");
    await panel.locator("[data-start-submit]").click();
    await expect(panel.locator("[data-start-failure]")).toContainText("~/hide", { timeout: 20_000 });
    await expect(panel.locator("[data-start-text]")).toHaveValue("홈에서 할 일");
    expect(fs.readdirSync(home)).toEqual(["mine.txt"]);
    await screenshot(page, "home-conflict-panel");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
