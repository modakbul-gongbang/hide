// The Project Overview board on an isolated pinned Herdr and hided (PRD
// web-project-overview): a Git project with a primary checkout and three
// worktrees at different stages, a folder project with agents, and a project
// with no agent at all. It enters by the project name and by ⌘⇧H (B1), reads
// the header facts (B2), the ad hoc strip and the Git columns with Merged
// folded (B4, B5), a needs-you card raised in its own column (B7), opens a
// checkout from a card header and a pane from an agent row (B8), switches to
// the Agents board (B9), and draws the folder and empty states (B10). Light
// and Dark captures land in HIDE_E2E_SCREENSHOT_DIR (B13, B14).

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

/** A Herdr workspace at `cwd`, with a fake `claude` agent titled `task` unless `agent` is false. */
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
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press("Alt+Comma");
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="appearance"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  // Controls fade their colors into the new theme; a capture waits them out.
  await page.waitForTimeout(400);
}

test("a project's Overview board: entry, columns, cards, Agents view and its states", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    // A repository with an origin, as a real one has: the core measures each
    // branch against origin's default. Two worktrees carry commits of their
    // own (one also uncommitted work), one was merged into main.
    const repo = path.join(herdr.root, "repo");
    const origin = path.join(herdr.root, "origin.git");
    fs.mkdirSync(repo);
    git(herdr.root, ["init", "--bare", origin]);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    git(repo, ["remote", "add", "origin", origin]);
    git(repo, ["push", "-q", "origin", "main"]);
    git(repo, ["remote", "set-head", "origin", "main"]);
    const tree = (name: string) => path.join(herdr.root, `repo-${name}`);
    git(repo, ["worktree", "add", "-b", "prd/web-overview-with-a-long-branch-name", tree("working")]);
    git(tree("working"), ["commit", "--allow-empty", "-m", "overview work"]);
    fs.writeFileSync(path.join(tree("working"), "notes.md"), "진행 중인 작업\n");
    git(repo, ["worktree", "add", "-b", "prd/asking", tree("asking")]);
    git(tree("asking"), ["commit", "--allow-empty", "-m", "asking work"]);
    git(repo, ["worktree", "add", "-b", "prd/shipped", tree("shipped")]);
    git(tree("shipped"), ["commit", "--allow-empty", "-m", "shipped work"]);
    git(repo, ["merge", "--ff-only", "prd/shipped"]);

    const mainPane = await workspaceAt(herdr, repo, "최신 hide 서버 웹 실행");
    const workingPane = await workspaceAt(herdr, tree("working"), "웹 디자인 시스템 리셋 구현");
    const askingPane = await workspaceAt(herdr, tree("asking"), "사이드바 상태 규칙 구현");
    await workspaceAt(herdr, tree("shipped"), "머지된 작업");
    // An agent asking the operator: its card is raised in 작업 중, not moved.
    execFileSync(herdr.bin, ["pane", "report-agent", askingPane, "--source", "e2e", "--agent", "claude", "--state", "blocked", "--message", "Done 그룹 회색 링을 기존 표시로 바꿔도 될까요?"], { env: herdr.env, timeout: 30_000 });
    // A project with only a shell: no agent at all.
    const quiet = path.join(herdr.root, "quiet");
    fs.mkdirSync(quiet);
    await workspaceAt(herdr, quiet, null);

    daemon = await startHided(herdr, "overview");
    await open(page, daemon);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });

    // The sidebar's project name opens that project's Overview on Tasks (B1).
    await page.locator('[data-sidebar-mode="projects"]').click();
    const repoRow = page.locator("[data-project-row]", { hasText: /^repo/ });
    await repoRow.click();
    const overview = page.locator("[data-overview-screen]");
    await expect(overview).toBeVisible();
    await expect(overview).toHaveAttribute("data-overview-state", "board");
    await expect(overview).toHaveAttribute("data-overview-view", "tasks");

    // Header facts (B2): three worktrees; no PR count, since GitHub never answered here.
    await expect(page.locator('[data-stat="worktrees"]')).toHaveText(/3 worktrees/);
    await expect(page.locator('[data-stat="open-prs"]')).toHaveCount(0);

    // The primary checkout is on the ad hoc strip; each worktree in its Git column (B4, B5).
    const adhoc = page.locator("[data-overview-adhoc]");
    await expect(adhoc.locator("[data-overview-card]")).toHaveCount(1);
    await expect(adhoc.locator(`[data-overview-agent="${mainPane}"]`)).toBeVisible();
    const column = (id: string) => page.locator(`[data-overview-column="${id}"]`);
    await expect(column("working").locator("[data-overview-card]")).toHaveCount(2, { timeout: 20_000 });
    await expect(column("ready").locator("[data-overview-card]")).toHaveCount(0);
    // The asking agent's card carries the halo and leads 작업 중 without leaving it (B7).
    const asking = column("working").locator("[data-overview-card]").first();
    await expect(asking).toHaveAttribute("data-needs-you", "true", { timeout: 20_000 });
    await expect(asking.locator(`[data-overview-agent="${askingPane}"]`)).toBeVisible();
    const working = column("working").locator("[data-overview-card]", { hasText: "prd/web-overview-with-a-long-branch-name" });
    await expect(working.locator('[data-overview-delivery="working"]')).toContainText("변경 1 · ↑1 커밋");
    // Merged starts folded and lists only its names until opened.
    await expect(column("merged")).toHaveAttribute("data-collapsed", "true");
    await expect(column("merged").locator("[data-overview-collapsed-names]")).toContainText("prd/shipped", { timeout: 20_000 });
    await column("merged").locator("[data-overview-column-toggle]").click();
    await expect(column("merged")).toHaveAttribute("data-collapsed", "false");
    await expect(column("merged").locator('[data-overview-delivery="merged"]')).toContainText("머지됨");
    await column("merged").locator("[data-overview-column-toggle]").click();

    // A long branch stays inside its card (B12).
    const within = await working.evaluate((card) => {
      const box = card.getBoundingClientRect();
      return [...card.querySelectorAll("*")].every((node) => node.getBoundingClientRect().right <= box.right + 1);
    });
    expect(within).toBe(true);

    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await screenshot(page, `overview-tasks-${theme}`);
    }

    // The Agents board: one card per lineage root in lifecycle columns (B9).
    await page.locator('[data-overview-tab="agents"]').click();
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    await expect(page.locator('[data-overview-columns="agents"] [data-overview-column]')).toHaveCount(3);
    await expect(page.locator('[data-overview-column="active"]').locator(`[data-overview-root="${askingPane}"]`)).toBeVisible();
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await screenshot(page, `overview-agents-${theme}`);
    }
    await page.locator('[data-overview-tab="tasks"]').click();

    // A card header opens its checkout; ⌘⇧H comes back; Esc returns to the Workspace (B1, B8).
    await working.locator("[data-overview-workspace]").click();
    const workspace = page.locator("[data-workspace-screen]");
    await expect(workspace).toBeVisible();
    await expect(page.locator(`[data-pane-view="${workingPane}"]`)).toBeVisible();
    await page.locator("body").click({ position: { x: 1, y: 1 } });
    await page.keyboard.press("Meta+Shift+KeyH");
    await expect(overview).toBeVisible();
    await expect(overview).toHaveAttribute("data-overview-view", "tasks");
    await page.keyboard.press("Escape");
    await expect(workspace).toBeVisible();

    // An agent row opens its pane (B8).
    await page.keyboard.press("Meta+Shift+KeyH");
    await expect(overview).toBeVisible();
    await page.locator(`[data-overview-agent="${mainPane}"]`).click();
    await expect(workspace).toBeVisible();
    await expect(page.locator(`[data-pane-view="${mainPane}"]`)).toHaveAttribute("data-focused", "true");

    // A folder with agents shows only the ad hoc strip (B10).
    await page.locator("[data-project-row]", { hasText: /^fixture/ }).click();
    await expect(overview).toHaveAttribute("data-overview-state", "adhoc");
    await expect(page.locator("[data-overview-adhoc] [data-overview-agent]")).toHaveCount(2);
    await expect(page.locator("[data-overview-column]")).toHaveCount(0);
    await screenshot(page, "overview-folder-light");

    // A project with no agent is the empty state with New agent; New agent
    // on a folder opens its Workspace (B3, B10).
    await page.locator("[data-project-row]", { hasText: /^quiet/ }).click();
    await expect(overview).toHaveAttribute("data-overview-state", "empty");
    await expect(page.locator("[data-overview-empty]")).toBeVisible();
    await screenshot(page, "overview-empty-light");
    await page.locator("[data-overview-empty-new-agent]").click();
    await expect(workspace).toBeVisible();

    // New agent on a Git project opens the New worktree flow; Cancel keeps the Overview (B3).
    await repoRow.click();
    await expect(overview).toHaveAttribute("data-overview-state", "board");
    await page.locator("[data-overview-new-agent]").click();
    await expect(page.locator("[data-new-worktree]")).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.locator("[data-new-worktree]")).toHaveCount(0);
    await expect(overview).toBeVisible();

    // While hided is down the board keeps the last snapshot and only the
    // connection line says so; nothing on the board turns into a banner (B11).
    const restarting = daemon.restart();
    await expect(page.locator("[data-connection]")).toBeVisible({ timeout: 10_000 });
    await expect(overview).toHaveAttribute("data-overview-state", "board");
    await expect(column("working").locator("[data-overview-card]")).toHaveCount(2);
    await expect(overview.locator('[role="alert"]')).toHaveCount(0);
    daemon = await restarting;
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
