// This Mac's Home (PRD home-device-rail B16-B18, B21) against a private
// Herdr and hided: the Home row's `+` makes `~/hide` in the daemon's HOME with
// a link per registered project and hide's guide files, and opens a tab there;
// a `~/hide` that is not Hide's is left alone, and the reason shows where Home
// was opened, under the Home row and inside the start panel.

import { expect, test, type Page } from "@playwright/test";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";
import { chord } from "./chords";

test.describe.configure({ timeout: 120_000 });

async function pinFixture(page: Page) {
  // The fixture is a plain folder, which the sidebar draws as one row.
  const row = page.locator("nav[data-sidebar]").getByRole("button", { name: "fixture", exact: true });
  await row.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Pin", exact: true }).click();
  await expect(page.locator("[data-home-count]")).toHaveText("1 project", { timeout: 20_000 });
}

async function openHomeTab(page: Page) {
  await page.locator("[data-home-destination]").hover();
  await page.locator("[data-home-new-tab]").click();
}

type SnapshotPane = { pane_id: string; workspace_id: string; tab_id: string; cwd?: string | null; foreground_cwd?: string | null };
type HerdrSnapshot = { result: { snapshot: { workspaces: { workspace_id: string; label?: string }[]; panes: SnapshotPane[] } } };
type PaneProcess = { result: { process_info: { shell_pid: number; foreground_process_group_id: number } } };

/**
 * Written only when the test fails, so the next failure says where each pane's
 * cwd came from. Herdr's `cwd` is a pane's OSC 7 report when it made one and
 * the shell's /proc cwd otherwise, and `foreground_cwd` is always /proc; the
 * shell's /proc cwd read here (Linux) and the per-process cwds Herdr lists in
 * `process-info` tell which of the two was wrong. Hide's side is where the
 * path back landed. Each read stands alone, so one that
 * fails is recorded instead of hiding the rest.
 */
async function recordFailure(page: Page, herdr: HerdrFixture, daemon: Daemon | null) {
  const read = <T,>(what: () => T): T | { error: string } => {
    try {
      return what();
    } catch (error) {
      return { error: String(error) };
    }
  };
  const snapshot = read(() => herdr.run(["api", "snapshot"]) as HerdrSnapshot);
  const panes = "error" in snapshot ? [] : snapshot.result.snapshot.panes;
  const diagnostics = {
    daemonHome: daemon?.home ?? null,
    hide: {
      location: await page.locator('nav[aria-label="Location"]').textContent({ timeout: 1_000 }).catch((error: unknown) => `unreadable: ${String(error)}`),
      project: await page.locator("[data-go-overview]").getAttribute("data-go-overview", { timeout: 1_000 }).catch(() => null),
      kind: await page.locator("[data-workspace-location]").getAttribute("data-workspace-location", { timeout: 1_000 }).catch(() => null),
    },
    workspaces: "error" in snapshot ? snapshot : snapshot.result.snapshot.workspaces,
    panes: panes.map((pane) => {
      const info = read(() => (herdr.run(["pane", "process-info", "--pane", pane.pane_id]) as PaneProcess).result.process_info);
      const procCwd = (pid: number) => (process.platform === "linux" ? read(() => fs.readlinkSync(`/proc/${pid}/cwd`)) : "no /proc on this system");
      return {
        ...pane,
        process: info,
        shellProcCwd: "error" in info ? null : procCwd(info.shell_pid),
        foregroundProcCwd: "error" in info ? null : procCwd(info.foreground_process_group_id),
        visibleText: read(() => {
          const result = spawnSync(herdr.bin, ["pane", "read", pane.pane_id, "--source", "visible", "--format", "text"], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
          return result.status === 0 ? result.stdout : `pane read exited ${result.status}: ${result.stderr.trim()}`;
        }),
      };
    }),
  };
  const file = test.info().outputPath("home-diagnostics.json");
  fs.writeFileSync(file, JSON.stringify(diagnostics, null, 2));
  await test.info().attach("home-diagnostics", { path: file, contentType: "application/json" });
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
    // B17: the tab opens in Home once Home's sync answered, so everything the
    // sync writes is on disk by the time Herdr has a pane there.
    await expect
      .poll(() => fs.existsSync(home) && JSON.stringify(herdr.run(["api", "snapshot"])).includes(fs.realpathSync(home)), { timeout: 30_000 })
      .toBe(true);
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    for (const file of [".hide-home.json", "AGENTS.md", "CLAUDE.md"]) expect(fs.existsSync(path.join(home, file))).toBe(true);
    const link = path.join(home, "fixture");
    expect(fs.lstatSync(link).isSymbolicLink()).toBe(true);
    expect(fs.realpathSync(link)).toBe(fs.realpathSync(path.join(herdr.root, "fixture")));

    // The Home is still no project (B16).
    await expect(page.locator("[data-home-count]")).toHaveText("1 project");
    await expect(page.locator("nav[data-sidebar]").getByRole("button", { name: /^(hide|Home)$/ })).toHaveCount(0);
    // The path back names Home and its folder, never Home as a project.
    await expect(page.locator('nav[aria-label="Location"]')).toHaveText("Home/~/hide");
    await screenshot(page, "home-tab-opened");
  } catch (error) {
    await recordFailure(page, herdr, daemon);
    throw error;
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
    await page.keyboard.press(chord("search"));
    await expect(page.locator('[data-palette="Search"] [data-palette-input]')).toBeFocused();
    await page.keyboard.type("Start an agent");
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
