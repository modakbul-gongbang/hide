// The sidebar's Projects tab on an isolated pinned Herdr and hided: a Git
// project with an agent in its primary checkout and one in a worktree, and a
// plain folder with only a shell. The rows name their kind and last-commit
// age; an opened checkout lists its agents and a closed one names them on its
// second line; a project folds its checkouts; both folds are the core's
// ui state, so they survive a reload. Light and Dark captures land in
// HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: "ignore" });
}

async function prompt(herdr: HerdrFixture, pane: string): Promise<void> {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const read = spawnSync(herdr.bin, ["pane", "read", pane, "--source", "visible", "--format", "text"], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
    if (read.status === 0 && read.stdout.includes("fixture %")) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`no prompt in pane ${pane}`);
}

/** A Herdr workspace at `cwd`, with a fake `claude` agent titled `task` unless it is null. */
async function workspaceAt(herdr: HerdrFixture, cwd: string, task: string | null): Promise<string> {
  const created = herdr.run(["workspace", "create", "--cwd", cwd, "--label", path.basename(cwd), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as {
    result: { root_pane: { pane_id: string } };
  };
  const pane = created.result.root_pane.pane_id;
  await prompt(herdr, pane);
  if (task) {
    herdr.run(["agent", "start", `agent-${path.basename(cwd)}`, "--kind", "claude", "--pane", pane]);
    execFileSync(herdr.bin, ["pane", "report-metadata", pane, "--source", "e2e", "--token", `task=${task}`], { env: herdr.env, timeout: 30_000 });
  }
  return pane;
}

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
  await page.locator('[data-sidebar-mode="projects"]').click();
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press("Alt+Comma");
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="appearance"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  await page.waitForTimeout(400);
}

test("the Projects tab: kind, age, agent line, opened checkouts and folded projects", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    const worktree = path.join(herdr.root, "repo-rows");
    git(repo, ["worktree", "add", "-b", "feature/sidebar-rows", worktree]);
    git(worktree, ["commit", "--allow-empty", "-m", "rows"]);
    // The branch description is a purpose the core reads from Git itself.
    git(repo, ["config", "branch.feature/sidebar-rows.description", "Projects 탭 행 다시 그리기"]);
    const notes = path.join(herdr.root, "notes");
    fs.mkdirSync(notes);

    await workspaceAt(herdr, repo, "메인 체크아웃 정리");
    const rowsPane = await workspaceAt(herdr, worktree, "사이드바 행 구현");
    await workspaceAt(herdr, notes, null);

    daemon = await startHided(herdr, "projects-sidebar");
    await open(page, daemon);

    const project = page.locator("[data-project]").filter({ has: page.locator("[data-project-row]", { hasText: /^repo/ }) });
    const projectToggle = project.locator("[data-project-toggle]");
    await expect(projectToggle).toHaveAttribute("aria-expanded", "true");
    const primary = project.locator("[data-checkout-row]", { hasText: /^main/ });
    const feature = project.locator("[data-checkout-row]", { hasText: "feature/sidebar-rows" });
    await expect(primary.locator("[data-checkout]")).toHaveAttribute("data-checkout-kind", "primary");
    await expect(feature.locator("[data-checkout]")).toHaveAttribute("data-checkout-kind", "branch");
    // The worktree's commit was just made: its age is the first minute.
    await expect(feature.locator("[data-checkout-age]")).toHaveText("now");

    // A checkout's agent rows start open (the core's collapsed set is empty):
    // its agent is listed under it and line two gives way to the rows.
    const featureToggle = feature.locator("[data-checkout-toggle]");
    await expect(featureToggle).toHaveAttribute("aria-expanded", "true");
    await expect(feature.locator(`[data-checkout-agents-open] [data-pane="${rowsPane}"]`)).toBeVisible();
    await expect(feature.locator("[data-checkout-agents]")).toHaveCount(0);
    await screenshot(page, "projects-sidebar-open");

    // Closed, line two names the one agent, then the checkout's purpose.
    await featureToggle.click();
    await expect(featureToggle).toHaveAttribute("aria-expanded", "false");
    await expect(feature.locator("[data-checkout-agents-open]")).toHaveCount(0);
    await expect(feature.locator('[data-checkout-agents="1"]')).toBeVisible();
    await expect(feature.locator("[data-purpose]")).toHaveText("Projects 탭 행 다시 그리기");

    // A plain folder with only a shell: a folder glyph, one line, nothing to open.
    const folder = page.locator("[data-project]").filter({ has: page.locator("[data-project-row]", { hasText: /^notes/ }) });
    await expect(folder.locator("[data-checkout]")).toHaveAttribute("data-checkout-kind", "folder");
    await expect(folder.locator("[data-checkout-toggle]")).toHaveCount(0);

    // The row itself still opens the checkout.
    await feature.locator("[data-checkout]").click();
    await expect(feature.locator("[data-checkout]")).toHaveAttribute("aria-current", "true");

    // Folding the project hides its checkouts; both folds survive a reload.
    await projectToggle.click();
    await expect(projectToggle).toHaveAttribute("aria-expanded", "false");
    await expect(project.locator("[data-checkout]")).toHaveCount(0);
    await open(page, daemon);
    await expect(projectToggle).toHaveAttribute("aria-expanded", "false");
    await expect(project.locator("[data-checkout]")).toHaveCount(0);
    await projectToggle.click();
    await expect(featureToggle).toHaveAttribute("aria-expanded", "false");

    for (const theme of ["light", "dark"] as const) {
      await chooseTheme(page, theme);
      await page.locator("[data-project-list]").hover({ position: { x: 1, y: 1 } });
      await screenshot(page, `projects-sidebar-${theme}`);
    }
  } finally {
    await daemon?.stop();
    await herdr.stop();
  }
});
