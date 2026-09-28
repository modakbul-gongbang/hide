// The Project Overview board on an isolated pinned Herdr and hided (PRD
// web-project-overview, task-agents-views, and the issue-first rework of
// 2026-09-28): a Git project with a primary checkout and four worktrees at
// different stages whose issues a fake `gh` answers, a folder project with
// agents, and a project with no agent at all. It enters by the project name
// and by ⌘⇧H (B1), reads the header facts (B2), the Backlog, In progress and
// Done columns, a task card headed by its issue and a worktree with none by
// its branch with 이슈 연결 on hover, a needs-you card raised in its own
// column and counted on the Agents tab, the List mode, opens a checkout from a
// card header and a pane from an agent row, the Agents inbox, the All
// projects boards, a Local issue made with C in a folder project and closed
// again, Settings › Issues, and an issue started into a worktree that lands
// linked in In progress. The sidebar is the scope picker: All projects on
// top, the project row marked on its Overview, and the view kept from one
// project to the next, Sessions included. Light and Dark captures land in
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

/**
 * A `gh` that is logged in and answers for `acme/repo`: two open issues and
 * no pull request, so the core reads them as the project's tasks the way it
 * reads the real one's. Only the read-only calls the core makes are answered.
 */
function fakeGh(dir: string): string {
  const bin = path.join(dir, "gh-bin");
  fs.mkdirSync(bin, { recursive: true });
  const issues = JSON.stringify([
    { number: 2, title: "태스크 출처 어댑터", url: "https://github.com/acme/repo/issues/2", state: "OPEN", projectItems: [], updatedAt: "2026-09-26T00:00:00Z" },
    { number: 3, title: "Graph 뷰", url: "https://github.com/acme/repo/issues/3", state: "OPEN", projectItems: [], updatedAt: "2026-09-25T00:00:00Z" },
  ]);
  // Issue 3 waits on issue 2, the relation GitHub's blockedBy records (task-agents-views B8).
  const dependencies = JSON.stringify({
    data: {
      r0: {
        nameWithOwner: "acme/repo",
        i2: { number: 2, blockedBy: { nodes: [] } },
        i3: { number: 3, blockedBy: { nodes: [{ number: 2, state: "OPEN", repository: { nameWithOwner: "acme/repo" } }] } },
      },
    },
  });
  fs.writeFileSync(
    path.join(bin, "gh"),
    `#!/bin/sh
case "$1 $2" in
  "auth status") exit 0 ;;
  "pr list") echo '[]' ;;
  "repo view") echo '{"nameWithOwner":"acme/repo"}' ;;
  "issue list") echo '${issues}' ;;
  "issue view") echo '{"body":"그래프 뷰의 본문"}' ;;
  "api graphql") echo '${dependencies}' ;;
  *) echo "unsupported: $*" >&2; exit 1 ;;
esac
`,
    { mode: 0o755 },
  );
  return bin;
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
    // A worktree for issue 2, named by the branch convention, with a commit
    // of its own: a branch with none reads as merged once its base resolves.
    git(repo, ["worktree", "add", "-b", "2-task-source", tree("linked")]);
    git(tree("linked"), ["commit", "--allow-empty", "-m", "task source"]);

    const mainPane = await workspaceAt(herdr, repo, "최신 hide 서버 웹 실행");
    const workingPane = await workspaceAt(herdr, tree("working"), "웹 디자인 시스템 리셋 구현");
    const askingPane = await workspaceAt(herdr, tree("asking"), "사이드바 상태 규칙 구현");
    await workspaceAt(herdr, tree("shipped"), "머지된 작업");
    // An agent asking the operator: its card is raised in 작업 중, not moved.
    // The question sentence is the label plugin's `expected_reply` token.
    execFileSync(herdr.bin, ["pane", "report-agent", askingPane, "--source", "e2e", "--agent", "claude", "--state", "blocked"], { env: herdr.env, timeout: 30_000 });
    execFileSync(herdr.bin, ["pane", "report-metadata", askingPane, "--source", "e2e", "--token", "expected_reply=Done 그룹 회색 링을 바꿔도 될까요?"], { env: herdr.env, timeout: 30_000 });
    // A project with only a shell: no agent at all.
    const quiet = path.join(herdr.root, "quiet");
    fs.mkdirSync(quiet);
    await workspaceAt(herdr, quiet, null);

    daemon = await startHided(herdr, "overview", undefined, { PATH: `${fakeGh(herdr.root)}:${process.env.PATH ?? ""}` });
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
    // The Overview child marks this scope; the project header never takes selection.
    const overviewRow = repoRow.locator("xpath=ancestor::li[@data-project]").locator("[data-project-overview]");
    await expect(overviewRow).toHaveAttribute("aria-current", "page");
    await expect(repoRow).not.toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-overview-destination]")).not.toHaveAttribute("aria-current", "page");
    await expect(page.locator('[data-project-list] [data-checkout][aria-current="true"]')).toHaveCount(0);
    // The project row takes the checkout rule: on its own open, unfolded
    // Overview a click folds the project and the Overview stays; the next
    // click unfolds it again.
    const repoToggle = repoRow.locator("xpath=ancestor::li[@data-project]").locator("[data-project-toggle]");
    await expect(repoToggle).toHaveAttribute("aria-expanded", "true");
    await repoRow.click();
    await expect(repoToggle).toHaveAttribute("aria-expanded", "false");
    await expect(overviewRow).toHaveCount(0);
    await expect(overview).toBeVisible();
    await repoRow.click();
    await expect(repoToggle).toHaveAttribute("aria-expanded", "true");
    await expect(overviewRow).toHaveAttribute("aria-current", "page");

    // Header facts (B2): four worktrees; no open PR, as GitHub answered; the
    // size the Overview asked the core to measure; the merged worktree,
    // which opens the Done column.
    // The worktree with no Herdr workspace can be listed after the ones Herdr
    // reports; a slow runner showed three at five seconds.
    await expect(page.locator('[data-stat="worktrees"]')).toHaveText(/4 worktrees/, { timeout: 20_000 });
    await expect(page.locator('[data-stat="open-prs"]')).toHaveText("0 open PRs", { timeout: 20_000 });
    await expect(page.locator('[data-stat="disk"]')).toHaveText(/^\d+(\.\d)? (B|KB|MB|GB)$/, { timeout: 30_000 });
    await expect(page.locator('[data-stat="merged"]')).toHaveText("1 merged → 정리", { timeout: 20_000 });
    await expect(page.locator('[data-stat="open-issues"]')).toHaveText(/^2 open issues\s*· GitHub$/, { timeout: 20_000 });

    // The primary checkout works on no issue, so it is on the Agents inbox
    // only; each worktree is in its Git column (B4, B5).
    await expect(page.locator(`[data-overview-columns="tasks"] [data-agent-open="${mainPane}"]`)).toHaveCount(0);
    const column = (id: string) => page.locator(`[data-overview-column="${id}"]`);
    await expect(column("working").locator("[data-overview-card]")).toHaveCount(3, { timeout: 20_000 });
    // The open issue no checkout works on is the backlog; the one a worktree
    // is named for heads that worktree's card, its id opening the issue and
    // its title the checkout (task-agents-views B1, B5).
    await expect(column("backlog").locator("[data-overview-card]")).toHaveCount(1, { timeout: 20_000 });
    await expect(column("backlog")).toContainText("Graph 뷰");
    // It waits on issue 2, so it carries the lock line naming it (B8).
    await expect(column("backlog").locator("[data-blocked-by]")).toHaveAttribute("data-blocked-by", "github:acme/repo#2", { timeout: 20_000 });
    await expect(column("backlog").locator("[data-blocked-by]")).toHaveText("#2");
    const linked = column("working").locator('[data-overview-card][data-task-key="github:acme/repo#2"]');
    await expect(linked).toContainText("태스크 출처 어댑터");
    await expect(linked.locator("[data-task-id]")).toHaveAttribute("href", "https://github.com/acme/repo/issues/2");
    await expect(linked.locator("[data-card-link]")).toHaveCount(0);
    // The asking agent's card carries the halo and leads 작업 중 without leaving it (B7).
    const asking = column("working").locator("[data-overview-card]").first();
    await expect(asking).toHaveAttribute("data-needs-you", "true", { timeout: 20_000 });
    // Its row carries the question on a second line (B8).
    await expect(asking.locator(`[data-pane="${askingPane}"] [data-agent-line="request"]`)).toContainText("Done 그룹 회색 링을 바꿔도 될까요?");
    // No band under the header: the Agents tab counts what waits on the operator.
    await expect(overview.locator("[data-waiting-band]")).toHaveCount(0);
    await expect(overview.locator('[data-overview-tab="agents"] [data-agents-waiting]')).toBeVisible();
    const working = column("working").locator("[data-overview-card]", { hasText: "prd/web-overview-with-a-long-branch-name" });
    // A worktree with no issue leads with its branch and offers 이슈 연결 on hover (D-07).
    await expect(working.locator('[data-fact="files"]')).toHaveText("1 files");
    await expect(working.locator('[data-fact="ahead"]')).toHaveText("↑1");
    await expect(working.locator("[data-card-branch]")).toHaveText("prd/web-overview-with-a-long-branch-name");
    await working.hover();
    await expect(working.locator("[data-card-link]")).toBeVisible();
    await working.locator("[data-card-link]").click();
    await expect(page.locator("[data-link-issue]")).toBeVisible();
    await expect(page.locator("[data-link-issue-item]")).toHaveCount(2);
    await screenshot(page, "overview-link-issue-dark");
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-link-issue]")).toHaveCount(0);
    // Done starts folded to one line per card until opened; the merged fact opens it.
    await expect(column("done")).toHaveAttribute("data-collapsed", "true");
    await expect(column("done").locator("[data-overview-collapsed-names]")).toContainText("prd/shipped", { timeout: 20_000 });
    await page.locator('[data-stat="merged"]').click();
    await expect(column("done")).toHaveAttribute("data-collapsed", "false");
    await expect(column("done").locator('[data-overview-card][data-stage="done"]')).toHaveCount(1);
    await column("done").locator("[data-overview-column-toggle]").click();

    // A long branch stays inside its card (B12).
    const within = await working.evaluate((card) => {
      const box = card.getBoundingClientRect();
      return [...card.querySelectorAll("*")].every((node) => node.getBoundingClientRect().right <= box.right + 1);
    });
    expect(within).toBe(true);

    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      // Settings hands focus back to the Done toggle; the capture shows it at rest.
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await screenshot(page, `overview-tasks-${theme}`);
    }

    // Dependencies draws the same task cards left to right: issue 2, which
    // its worktree works on, before issue 3 it blocks, one arrow between them
    // and each card's stage word; untracked checkouts stay on the Board (D-09).
    await page.locator('[data-tasks-mode-item="dependencies"]').click();
    const graph = page.locator("[data-dependency-graph]");
    await expect(graph.locator("[data-dependency-layer]")).toHaveCount(2);
    await expect(graph.locator('[data-dependency-layer="0"] [data-overview-card]')).toHaveAttribute("data-task-key", "github:acme/repo#2");
    await expect(graph.locator('[data-dependency-layer="1"] [data-overview-card]')).toHaveAttribute("data-task-key", "github:acme/repo#3");
    await expect(graph.locator('[data-dependency-layer="1"] [data-overview-card]')).toHaveAttribute("data-blocked", "true");
    await expect(graph.locator('[data-dependency-layer="0"] [data-card-status]')).toHaveText("진행 중");
    await expect(graph.locator('[data-dependency-layer="1"] [data-card-status]')).toHaveText("백로그");
    await expect(graph.locator("[data-dependency-edge]")).toHaveCount(1);
    await expect(graph.locator("[data-dependency-edge]")).toHaveAttribute("d", /^M \S+ \S+ C /);
    await expect(page.locator("[data-dependency-unrelated]")).toHaveCount(0);
    await expect(page.locator("[data-tasks-dependencies] [data-overview-card]:not([data-task-key])")).toHaveCount(0);
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await screenshot(page, `overview-dependencies-${theme}`);
    }
    // The mode is the page's: All projects opens on it too, with each card's
    // project beside its id (B9, D-10).
    await page.locator("[data-go-main]").click();
    const mainScreen = page.locator("[data-main-screen]");
    await expect(mainScreen.locator("[data-tasks-mode]")).toHaveAttribute("data-tasks-mode", "dependencies");
    await expect(mainScreen.locator("[data-dependency-edge]")).toHaveCount(1);
    await expect(mainScreen.locator('[data-dependency-layer="0"] [data-card-project]')).toHaveText("repo");
    await chooseTheme(page, "light");
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await screenshot(page, "all-projects-dependencies-light");
    // The List mode: the same tasks one row each, the moving work first, and a
    // row waiting on the operator unfolded to its agents.
    await page.locator('[data-tasks-mode-item="list"]').click();
    await repoRow.click();
    await expect(overview).toHaveAttribute("data-overview-view", "tasks");
    const list = page.locator("[data-tasks-list]");
    await expect(list.locator("[data-list-group]").first()).toHaveAttribute("data-list-group", "working");
    await expect(list.locator('[data-list-row][data-needs-you="true"]').first()).toContainText("Done 그룹 회색 링을 바꿔도 될까요?");
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await screenshot(page, `overview-list-${theme}`);
    }
    await page.locator('[data-tasks-mode-item="board"]').click();
    await expect(page.locator('[data-overview-columns="tasks"]')).toBeVisible();

    // The Agents inbox: the agents waiting on the operator first, each naming
    // its issue or, with none, its checkout (B11).
    await page.locator('[data-overview-tab="agents"]').click();
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    await expect(page.locator("[data-inbox-group]").first()).toHaveAttribute("data-inbox-group", "needs");
    const askingCard = page.locator('[data-inbox-group="needs"]').locator(`[data-overview-root="${askingPane}"]`);
    await expect(askingCard).toBeVisible();
    await expect(askingCard).toContainText("prd/asking");
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await screenshot(page, `overview-agents-${theme}`);
    }

    // The Sessions tab is the project's Sessions, and the view stays when
    // another project opens; All projects is its own scope, marked in turn.
    await page.locator('[data-overview-tab="sessions"]').click();
    await expect(overview).toHaveAttribute("data-overview-view", "sessions");
    await expect(page.locator("[data-sessions-screen]")).toBeVisible();
    await expect(page.locator("[data-sessions-state]")).toHaveAttribute("data-sessions-state", /^(empty|rows)$/, { timeout: 20_000 });
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      // Settings hands focus back to the control that was clicked; the capture shows it at rest.
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await screenshot(page, `overview-sessions-${theme}`);
    }
    const allProjects = page.locator("[data-overview-destination]");
    await allProjects.click();
    await expect(page.locator("[data-main-screen]")).toBeVisible();
    await expect(allProjects).toHaveAttribute("aria-current", "page");
    await expect(allProjects).toContainText("3 projects");
    await expect(repoRow).not.toHaveAttribute("aria-current", "page");
    await expect(page.locator('[data-main-stats] [data-stat="projects"]')).toHaveText("3 projects");
    await expect(page.locator('[data-main-stats] [data-stat="merged"]')).toHaveText("1 merged");
    // All projects has no Sessions, so it opens on its first view, Tasks: every
    // project's tasks on one board (D-01, B15).
    const main = page.locator("[data-main-screen]");
    await expect(main).toHaveAttribute("data-main-view", "tasks");
    // The page's view is still Sessions, so the project opens on it again.
    await repoRow.click();
    await expect(overview).toHaveAttribute("data-overview-view", "sessions");
    await allProjects.click();
    await expect(main).toHaveAttribute("data-main-view", "tasks");
    await expect(main.locator('[data-overview-column="backlog"] [data-overview-card]')).toHaveCount(1, { timeout: 20_000 });
    await expect(main.locator('[data-overview-column="backlog"] [data-card-project]')).toHaveText("repo");
    await expect(main.locator("[data-unconnected-project]")).toHaveCount(0);
    await expect(main.locator('[data-main-tab="agents"] [data-agents-waiting]')).toBeVisible();
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await screenshot(page, `all-projects-tasks-${theme}`);
    }
    // Its Agents view names each agent's project before its checkout.
    await page.locator('[data-main-tab="agents"]').click();
    await expect(main.locator(`[data-agent-card="${askingPane}"]`)).toContainText("repo · prd/asking");
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await screenshot(page, `all-projects-agents-${theme}`);
    }
    await page.locator('[data-main-tab="projects"]').click();
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      // Settings hands focus back to the control that was clicked; the capture shows it at rest.
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await screenshot(page, `all-projects-${theme}`);
    }
    await repoRow.click();
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

    // An agent row opens its pane (B8); the primary checkout's agent works on
    // no issue, so its row is on the Agents inbox.
    await page.keyboard.press("Meta+Shift+KeyH");
    await expect(overview).toBeVisible();
    await page.locator('[data-overview-tab="agents"]').click();
    await page.locator(`[data-overview-screen] [data-agent-open="${mainPane}"]`).click();
    await expect(workspace).toBeVisible();
    await expect(page.locator(`[data-pane-view="${mainPane}"]`)).toHaveAttribute("data-focused", "true");

    // An Agents inbox row opens the asking agent's pane, where it is answered (B10).
    await page.keyboard.press("Meta+Shift+KeyH");
    await expect(overview).toBeVisible();
    await page.locator('[data-overview-tab="agents"]').click();
    await page.locator(`[data-agent-open="${askingPane}"]`).click();
    await expect(workspace).toBeVisible();
    await expect(page.locator(`[data-pane-view="${askingPane}"]`)).toHaveAttribute("data-focused", "true");

    // A folder's issues are Local ones kept by Hide. Its agents work on no
    // issue, so its board is the empty Backlog; C makes an issue, 만들고 바로
    // 시작 goes on to the Start dialog with its first prompt, and the card's
    // menu closes the issue again. Its sidebar row opens its checkout, so its
    // Overview is reached from All projects.
    await page.locator("[data-go-main]").click();
    await page.locator('[data-main-tab="projects"]').click();
    await page.locator("[data-main-project]", { hasText: /^fixture/ }).click();
    await page.locator('[data-overview-tab="tasks"]').click();
    await expect(overview).toHaveAttribute("data-overview-state", "empty");
    await expect(page.locator('[data-overview-column="backlog"] [data-backlog-empty]')).toBeVisible();
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await page.keyboard.press("c");
    const newIssue = page.locator("[data-new-issue]");
    await expect(newIssue).toBeVisible();
    await expect(newIssue.locator("[data-new-issue-project]")).toContainText("fixture · Local");
    await newIssue.locator("[data-new-issue-title]").fill("폴더 이슈");
    await newIssue.locator("[data-new-issue-body]").fill("폴더에서 할 일");
    await newIssue.locator("[data-new-issue-start]").click();
    await screenshot(page, "new-issue-light");
    await newIssue.locator("[data-new-issue-create]").click();
    const start = page.locator("[data-start-issue]");
    await expect(start).toBeVisible();
    await expect(start.locator("[data-start-name]")).toHaveCount(0);
    await expect(start.locator("[data-start-prompt]")).toHaveValue("로컬 이슈 L-1를 해결해줘: 폴더 이슈\n\n폴더에서 할 일");
    await page.getByRole("button", { name: "취소" }).click();
    const localCard = page.locator('[data-overview-column="backlog"] [data-overview-card]');
    await expect(localCard).toHaveCount(1);
    await expect(localCard.locator("[data-task-id]")).toHaveText("L-1");
    await screenshot(page, "overview-local-issue-light");
    await localCard.hover();
    await localCard.locator("[data-card-menu]").click();
    await page.locator('[data-card-issue-open="close"]').click();
    await expect(page.locator('[data-overview-column="backlog"] [data-overview-card]')).toHaveCount(0);

    // A project with no agent and no issue is the empty Backlog with the way
    // to make the first issue (B14); New agent on a folder opens its Workspace.
    await page.locator("[data-go-main]").click();
    await page.locator("[data-main-project]", { hasText: /^quiet/ }).click();
    await expect(overview).toHaveAttribute("data-overview-state", "empty");
    await expect(page.locator("[data-waiting-band]")).toHaveCount(0);
    await expect(page.locator("[data-backlog-empty-new]")).toBeVisible();
    await screenshot(page, "overview-empty-light");
    await page.locator("[data-overview-new-agent]").click();
    await expect(workspace).toBeVisible();

    // New agent on a Git project opens the New worktree flow; Cancel keeps the Overview (B3).
    await repoRow.click();
    await expect(overview).toHaveAttribute("data-overview-state", "board");
    await page.locator("[data-overview-new-agent]").click();
    await expect(page.locator("[data-new-worktree]")).toBeVisible();
    // Escape in the dialog closes only the dialog; the Overview stays (B1, B3).
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-new-worktree]")).toHaveCount(0);
    await expect(overview).toBeVisible();
    await page.locator("[data-overview-new-agent]").click();
    await expect(page.locator("[data-new-worktree]")).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.locator("[data-new-worktree]")).toHaveCount(0);
    await expect(overview).toBeVisible();

    // Settings › Issues: the sources, each project's source, and how work starts.
    await page.keyboard.press("Alt+Comma");
    await page.locator('[data-settings-tab="issues"]').click();
    await expect(page.locator("[data-settings-issues]")).toBeVisible();
    await expect(page.getByRole("combobox", { name: "repo 이슈 출처" })).toHaveText("자동 (GitHub)");
    await expect(page.getByRole("combobox", { name: "quiet 이슈 출처" })).toHaveText("자동 (Local)");
    await screenshot(page, "settings-issues-light");
    await chooseTheme(page, "dark");
    await page.keyboard.press("Alt+Comma");
    await page.locator('[data-settings-tab="issues"]').click();
    await screenshot(page, "settings-issues-dark");
    await page.keyboard.press("Escape");

    // 시작 on the backlog issue: the Start dialog names the worktree for the
    // issue and fills the first prompt from its body; the worktree it makes is
    // linked to the issue, so the card moves from Backlog to In progress.
    const backlogCard = column("backlog").locator('[data-overview-card][data-task-key="github:acme/repo#3"]');
    await backlogCard.hover();
    await backlogCard.locator("[data-card-start]").click();
    await expect(start).toBeVisible();
    await expect(start.locator("[data-start-name]")).toHaveValue(/^3-/);
    await expect(start.locator("[data-start-prompt]")).toHaveValue("Issue #3를 해결해줘: Graph 뷰\n\n그래프 뷰의 본문\n\n완료되면 이 이슈를 닫는 PR을 열어줘 (PR 본문에 Closes #3).");
    await screenshot(page, "start-issue-dark");
    await start.locator("[data-start-name]").fill("3-graph-view");
    await start.locator('[data-start-agent="terminal"]').click();
    await start.locator('[data-start-submit="start"]').click();
    await expect(start).toHaveCount(0, { timeout: 30_000 });
    // The new worktree's Workspace comes to the front at its pane; ⌘⇧H
    // then opens its project's Overview.
    await expect(workspace).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(`[data-checkout][aria-label^="3-graph-view"]`)).toHaveAttribute("aria-current", "true");
    await page.keyboard.press("Meta+Shift+KeyH");
    await expect(overview).toBeVisible();
    await expect(column("backlog").locator("[data-overview-card]")).toHaveCount(0, { timeout: 30_000 });
    await expect(column("working").locator('[data-overview-card][data-task-key="github:acme/repo#3"]')).toContainText("3-graph-view", { timeout: 30_000 });

    // While hided is down the board keeps the last snapshot and only the
    // connection line says so; nothing on the board turns into a banner (B11).
    const restarting = daemon.restart();
    await expect(page.locator("[data-connection]")).toBeVisible({ timeout: 10_000 });
    await expect(overview).toHaveAttribute("data-overview-state", "board");
    await expect(column("working").locator("[data-overview-card]")).toHaveCount(4);
    await expect(overview.locator('[role="alert"]')).toHaveCount(0);
    daemon = await restarting;
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
