// The Project Overview on an isolated pinned Herdr and hided (PRD
// overview-lenses-tiles-agents, on top of web-project-overview,
// task-agents-views and the issue-first rework of 2026-09-28): a Git project
// with a primary checkout and six worktrees at different stages whose issues
// and pull requests a fake `gh` answers, an Observer on main that delegated to an Implementor in
// a worktree, a folder project with agents, and a project with no agent at
// all. Every entry opens the Agents lens on its checkout lanes with the front
// checkout's lane selected (B11, B12); the tiles in the tab row's place count
// agents, issues and today's sessions (B1-B6); lanes are ordered, folded,
// lined and headed as the PRD draws them (B13-B21); a node's line opens the
// agent's whole message (B22); the lineage mode (B23-B25), the arrow keys
// (B26), hover publishing nothing (B27), ⌥` restoring the lens (B11) and the
// All projects lanes (B30) follow. The Issues tile is the issues-only board
// of PRD overview-lenses-issues: issue cards with the lines of work that have
// none (B1-B4), the issue panel beside the board with its read, failure and
// retry (B10-B16, B19), the keyboard (B20), the preview and quiet hover (B6-B8,
// B22), the filter (B21), a Local issue made with C and edited in its panel
// (B18), List and Dependencies, Settings › Issues, and an issue started into a
// worktree (B23). Light and Dark captures land in HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Locator, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { agentsIn, startHerdr, setFixtureLifecycle, type HerdrFixture, declareParent } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { bindChordlessCommand, countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 240_000 });

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
 * A `gh` that is logged in and answers for `acme/repo`: three open issues,
 * one pull request that closes issue 4 and one that closes none, so the core
 * reads them as the project's tasks the way it reads the real one's. Issue 2
 * reads with labels, an author, an assignee and four comments; issue 4's
 * first read fails and its second succeeds (B16). Only the read-only calls
 * the core makes are answered.
 */
function fakeGh(dir: string): string {
  const bin = path.join(dir, "gh-bin");
  fs.mkdirSync(bin, { recursive: true });
  const issues = JSON.stringify([
    { number: 2, title: "태스크 출처 어댑터", url: "https://github.com/acme/repo/issues/2", state: "OPEN", projectItems: [], updatedAt: "2026-09-26T00:00:00Z", createdAt: "2026-09-20T00:00:00Z" },
    { number: 3, title: "Graph 뷰", url: "https://github.com/acme/repo/issues/3", state: "OPEN", projectItems: [], updatedAt: "2026-09-25T00:00:00Z" },
    { number: 4, title: "리뷰 중인 이슈", url: "https://github.com/acme/repo/issues/4", state: "OPEN", projectItems: [], updatedAt: "2026-09-24T00:00:00Z" },
  ]);
  const pr = (number: number, branch: string, title: string, closes: number[], review: string | null) => ({
    number,
    title,
    statusCheckRollup: [],
    headRefName: branch,
    baseRefName: "main",
    state: "OPEN",
    reviewDecision: review,
    isDraft: false,
    url: `https://github.com/acme/repo/pull/${number}`,
    mergedAt: null,
    updatedAt: "2026-09-27T00:00:00Z",
    closingIssuesReferences: closes.map((issue) => ({ url: `https://github.com/acme/repo/issues/${issue}` })),
  });
  const pulls = JSON.stringify([pr(11, "prd/reviewing", "리뷰 이슈 구현", [4], "REVIEW_REQUIRED"), pr(12, "prd/loose-pr", "이슈 없는 정리", [], null)]);
  const comment = (login: string, day: number, body: string) => ({ author: { login }, createdAt: `2026-09-${day}T00:00:00Z`, body });
  const detail = JSON.stringify({
    body: "## 배경\n\n출처를 **어댑터**로 나눈다.\n\n- GitHub\n- Local",
    labels: [
      { name: "enhancement", color: "a2eeef" },
      { name: "ui", color: "not-hex" },
    ],
    author: { login: "hoyeon" },
    assignees: [{ login: "hoyeon" }],
    createdAt: "2026-09-20T00:00:00Z",
    comments: [comment("a", 21, "첫 댓글"), comment("b", 22, "둘째 댓글"), comment("c", 23, "셋째 댓글"), comment("d", 24, "마지막 댓글")],
  });
  const plain = (body: string) => JSON.stringify({ body, labels: [], author: { login: "hoyeon" }, assignees: [], createdAt: "2026-09-19T00:00:00Z", comments: [] });
  // Issue 3 waits on issue 2, the relation GitHub's blockedBy records (task-agents-views B8).
  const dependencies = JSON.stringify({
    data: {
      r0: {
        nameWithOwner: "acme/repo",
        i2: { number: 2, blockedBy: { nodes: [] } },
        i3: { number: 3, blockedBy: { nodes: [{ number: 2, state: "OPEN", repository: { nameWithOwner: "acme/repo" } }] } },
        i4: { number: 4, blockedBy: { nodes: [] } },
      },
    },
  });
  const failedOnce = path.join(bin, "issue-4-failed");
  fs.writeFileSync(
    path.join(bin, "gh"),
    `#!/bin/sh
case "$*" in
  *"--state merged"*) printf '%s\\n' '[]'; exit 0 ;;
esac
case "$1 $2" in
  "auth status") exit 0 ;;
  "pr list") sleep 1; printf '%s\\n' '${pulls}' ;;
  "repo view") printf '%s\\n' '{"nameWithOwner":"acme/repo"}' ;;
  "issue list") printf '%s\\n' '${issues}' ;;
  "issue view")
    case "$3" in
      2) printf '%s\\n' '${detail}' ;;
      4)
        if [ -e '${failedOnce}' ]; then printf '%s\\n' '${plain("리뷰할 본문")}'; else : > '${failedOnce}'; echo "HTTP 502" >&2; exit 1; fi ;;
      *) printf '%s\\n' '${plain("그래프 뷰의 본문")}' ;;
    esac ;;
  "api graphql") printf '%s\\n' '${dependencies}' ;;
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

/**
 * Moves the pointer off a hover card the way a hand does, in a run of moves
 * rather than two, until `gone` has left the page. Radix closes a hoverable
 * card only on a move that arrives after its leave listener is in place; two
 * bare moves on a busy runner can both land before it and leave the card
 * open for good (overview.spec on CI, 2026-09-29..10-01).
 */
async function leaveHoverCard(page: Page, gone: Locator): Promise<void> {
  await expect(async () => {
    await page.mouse.move(2, 998);
    await page.mouse.move(4, 996, { steps: 4 });
    await expect(gone).toHaveCount(0, { timeout: 500 });
  }).toPass({ timeout: 10_000 });
}

/** Clears hover and keyboard focus so a capture shows the page at rest. */
async function atRest(page: Page): Promise<void> {
  await page.mouse.move(2, 998);
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

test("a project's Overview: tiles, checkout lanes, lineage, and the Issues board", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    // Panes created from here on find the fixture's `claude` first, so the
    // agent an issue starts is the shim, never a real one.
    fs.writeFileSync(path.join(herdr.root, "home", ".zshenv"), `export PATH="${path.join(herdr.root, "bin")}:$PATH"\n`);
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
    git(repo, ["branch", "--set-upstream-to=origin/main", "main"]);
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
    // Two worktrees with open pull requests: one closes issue 4, the other no issue.
    git(repo, ["worktree", "add", "-b", "prd/reviewing", tree("reviewing")]);
    git(tree("reviewing"), ["commit", "--allow-empty", "-m", "reviewing work"]);
    git(repo, ["worktree", "add", "-b", "prd/loose-pr", tree("loose")]);
    git(tree("loose"), ["commit", "--allow-empty", "-m", "loose work"]);

    const mainPane = await workspaceAt(herdr, repo, "최신 hide 서버 웹 실행");
    const workingPane = await workspaceAt(herdr, tree("working"), "웹 디자인 시스템 리셋 구현");
    const askingPane = await workspaceAt(herdr, tree("asking"), "사이드바 상태 규칙 구현");
    const shippedPane = await workspaceAt(herdr, tree("shipped"), "머지된 작업");
    // The Observer on main delegated the working worktree's Implementor,
    // which is working, so the Observer waits on it (B14, B21).
    declareParent(herdr, workingPane, mainPane);
    await setFixtureLifecycle(herdr, workingPane, "working");
    // An agent asking the operator: the label plugin's `expected_reply` is
    // its question, and its `progress` the rest of what it said; the node
    // shows the question, its popover the whole message (B21, B22).
    await setFixtureLifecycle(herdr, askingPane, "blocked");
    execFileSync(herdr.bin, ["pane", "report-metadata", askingPane, "--source", "e2e", "--token", "expected_reply=Done 그룹 회색 링을 바꿔도 될까요?"], { env: herdr.env, timeout: 30_000 });
    execFileSync(herdr.bin, ["pane", "report-metadata", askingPane, "--source", "e2e", "--token", "progress=사이드바 상태 규칙을 세 곳에 적용했고 Done 그룹만 남았습니다."], { env: herdr.env, timeout: 30_000 });
    // A project with only a shell: no agent at all.
    const quiet = path.join(herdr.root, "quiet");
    fs.mkdirSync(quiet);
    await workspaceAt(herdr, quiet, null);

    daemon = await startHided(herdr, "overview", undefined, { PATH: `${fakeGh(herdr.root)}:${herdr.fixturePath}` });
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await open(page, daemon);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });

    // The sidebar's project name opens that project's Overview on Agents ›
    // 체크아웃, with no front checkout in it so main's lane selected (B11, B12).
    await page.locator('[data-sidebar-mode="projects"]').click();
    const repoRow = page.locator("[data-project-row]", { hasText: /^repo/ });
    const refreshesBefore = sent.get("sessions_refresh") ?? 0;
    await repoRow.click();
    const overview = page.locator("[data-overview-screen]");
    await expect(overview).toBeVisible();
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    await expect.poll(() => sent.get("overview_refresh") ?? 0).toBe(1);
    await expect(overview.locator('[data-overview-refreshing="true"]')).toBeVisible();
    await expect(overview.locator('[data-overview-refreshing="true"] svg')).toHaveClass(/animate-spin/);
    await overview.screenshot({ path: path.join(process.env.HIDE_E2E_SCREENSHOT_DIR ?? herdr.root, "overview-refreshing-stats.png") });
    await expect(overview).toHaveAttribute("data-agents-mode", "checkouts");
    const lanes = overview.locator("[data-lens-mode=checkouts] [data-lens-lane]");
    const lane = (branch: string) => overview.locator("[data-lens-lane]", { has: page.locator("[data-lens-head]", { hasText: branch }) });
    await expect(overview.locator('[data-lens-lane][data-selected="true"]')).toHaveAttribute("data-lane-rank", "primary");
    // Opening the Overview reads the project's session history once (B5).
    await expect.poll(() => (sent.get("sessions_refresh") ?? 0) - refreshesBefore).toBe(1);
    expect(last.get("sessions_refresh")?.workspace_id).toEqual(await overview.getAttribute("data-overview-screen"));
    // The Overview child marks this scope; the project header never takes selection.
    const overviewRow = repoRow.locator("xpath=ancestor::li[@data-project]").locator("[data-project-overview]");
    await expect(overviewRow).toHaveAttribute("aria-current", "page");
    await expect(repoRow).not.toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-home-destination]")).not.toHaveAttribute("aria-current", "page");
    await expect(page.locator('[data-project-list] [data-checkout][aria-current="true"]')).toHaveCount(0);
    // The project row takes the checkout rule: on its own open, unfolded
    // Overview a click folds the project and the Overview stays.
    const repoToggle = repoRow.locator("xpath=ancestor::li[@data-project]").locator("[data-project-toggle]");
    await expect(repoToggle).toHaveAttribute("aria-expanded", "true");
    await repoRow.click();
    await expect(repoToggle).toHaveAttribute("aria-expanded", "false");
    await expect(overviewRow).toHaveCount(0);
    await expect(overview).toBeVisible();
    await repoRow.click();
    await expect(repoToggle).toHaveAttribute("aria-expanded", "true");
    await expect(overviewRow).toHaveAttribute("aria-current", "page");

    // The facts line keeps worktrees, disk and merged, and no issue or PR
    // count; the mode control sits at its right end (B8).
    await expect(page.locator('[data-stat="worktrees"]')).toHaveText(/6 worktrees/, { timeout: 20_000 });
    await expect(page.locator('[data-stat="disk"]')).toHaveText(/^\d+(\.\d)? (B|KB|MB|GB)$/, { timeout: 30_000 });
    await expect(page.locator('[data-stat="merged"]')).toHaveText("1 merged → 정리", { timeout: 20_000 });
    await expect(overview.locator('[data-overview-refreshing="true"]')).toHaveCount(0, { timeout: 20_000 });
    // A local ref move refreshes the catalog while Overview stays open.
    git(repo, ["switch", "-c", "behind-test"]);
    git(repo, ["commit", "--allow-empty", "-m", "ahead on origin"]);
    git(repo, ["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(repo, ["switch", "main"]);
    await expect(overview.locator('[data-stat="behind"]')).toHaveText("main ↓1 behind origin", { timeout: 20_000 });
    git(repo, ["update-ref", "refs/remotes/origin/main", "main"]);
    await expect(overview.locator('[data-stat="behind"]')).toHaveCount(0, { timeout: 20_000 });
    await expect(overview.locator('[data-stat="open-prs"], [data-stat="open-issues"]')).toHaveCount(0);
    await expect(overview.locator("[data-agents-mode-item]")).toHaveCount(2);
    // New agent and 새 이슈 stay on the title row (B9).
    await expect(overview.locator("[data-overview-new-agent]")).toBeVisible();

    // The tiles where the tab row was: Agents chosen, four agents, one of
    // them the operator's turn, the bar by bucket; Issues two open, the bar
    // by stage; Sessions today's count once the history is read (B1-B5).
    const tile = (id: string) => overview.locator(`[data-lens-tile="${id}"]`);
    await expect(overview.locator("[data-lens-tile]")).toHaveCount(4);
    expect(await overview.locator("[data-lens-tile]").evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-lens-tile")))).toEqual(["agents", "issues", "prs", "sessions"]);
    await expect(overview.locator("[data-overview-tab], [data-inbox-group], [data-waiting-band]")).toHaveCount(0);
    await expect(tile("agents")).toHaveAttribute("data-selected", "true");
    await expect(tile("agents").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "4", { timeout: 20_000 });
    await expect(tile("agents").locator("[data-lens-tile-badge]")).toHaveText("1");
    await expect(tile("agents").locator("[data-lens-tile-bar]")).toHaveAttribute("data-lens-tile-bar", "turn:1 working:1 delegating:1 resting:1");
    await expect(tile("issues").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "3", { timeout: 20_000 });
    await expect(tile("issues")).toContainText("열림");
    await expect(tile("issues").locator("[data-lens-tile-bar]")).toHaveAttribute("data-lens-tile-bar", "backlog:1 working:1 review:1");
    await expect(tile("issues").locator("[data-lens-tile-badge]")).toHaveCount(0);
    await expect(tile("sessions").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "0", { timeout: 20_000 });
    await expect(tile("sessions")).toContainText("오늘");
    // Resting on the bar shows its legend and on the badge its breakdown (B4).
    await tile("agents").locator("[data-lens-tile-bar]").hover();
    await expect(page.getByRole("tooltip")).toContainText("내 차례 1");
    await tile("agents").locator("[data-lens-tile-badge]").hover();
    await expect(page.getByRole("tooltip")).toContainText("승인 1");

    // Lanes: main pinned on top, then the asking agent's lane, then the
    // working one; the merged worktree and the one with no agent fold (B13, B20).
    await expect(lanes).toHaveCount(3, { timeout: 20_000 });
    expect(await lanes.evaluateAll((rows) => rows.map((row) => row.getAttribute("data-lane-rank")))).toEqual(["primary", "turn", "working"]);
    await expect(lanes.nth(1)).toContainText("prd/asking");
    await expect(lanes.nth(2)).toContainText("prd/web-overview-with-a-long-branch-name");
    // main's head is the house, main and its agent count (B15).
    await expect(lanes.nth(0).locator("[data-lens-head-agents]")).toHaveText("에이전트 1");
    // A worktree head: the branch, ↑N and its changed files, dirty in the warning tone (B15).
    const workingLane = lane("prd/web-overview-with-a-long-branch-name");
    await expect(workingLane.locator("[data-lens-head-distance]")).toHaveText("↑1");
    await expect(workingLane.locator("[data-lens-head-files]")).toHaveText("1 file");
    await expect(workingLane.locator("[data-lens-head-files]")).toHaveClass(/text-warning/);
    // Nodes: the Implementor stands in its worktree's lane, and one line
    // runs from the Observer on main down to it (B14); the Observer says how
    // its child is doing, the asking node is the only one outlined (B21).
    await expect(workingLane.locator(`[data-lens-node="${workingPane}"]`)).toHaveAttribute("data-bucket", "working");
    const observer = lanes.nth(0).locator(`[data-lens-node="${mainPane}"]`);
    await expect(observer).toHaveAttribute("data-bucket", "delegating");
    await expect(observer).toContainText("일하는 중 1");
    await expect(overview.locator(`[data-lens-line="${mainPane}>${workingPane}"]`)).toHaveAttribute("d", /^M \S+ \S+ V /);
    const askingNode = overview.locator(`[data-lens-node="${askingPane}"]`);
    await expect(askingNode).toHaveAttribute("data-bucket", "turn");
    await expect(askingNode).toHaveClass(/border-warning/);
    await expect(askingNode.locator(`[data-lens-node-line="${askingPane}"]`)).toHaveText("Done 그룹 회색 링을 바꿔도 될까요?");
    await expect(observer).not.toHaveClass(/border-warning/);
    // The folds: one line each, a click unfolding it in place (B20).
    const emptyFold = overview.locator('[data-lens-fold="empty"]');
    const cleanupFold = overview.locator('[data-lens-fold="cleanup"]');
    await expect(emptyFold).toHaveText(/에이전트 없는 워크트리 3/);
    await expect(cleanupFold).toHaveText(/정리할 것 1/);
    await emptyFold.click();
    await expect(emptyFold).toHaveAttribute("aria-expanded", "true");
    await expect(lanes).toHaveCount(6);
    // The linked worktree's head carries its issue chip, which opens the
    // Issues tab at that card (B17).
    const linkedLane = lane("2-task-source");
    await expect(linkedLane.locator("[data-lens-issue-chip]")).toHaveAttribute("data-lens-issue-chip", "github:acme/repo#2");
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await atRest(page);
      await screenshot(page, `overview-agents-checkouts-${theme}`);
    }

    // Hover, focus and a half-second rest publish nothing (B27).
    const quietBefore = [...sent.values()].reduce((sum, count) => sum + count, 0);
    await workingLane.locator("[data-lens-head-open]").hover();
    // Resting on a lane head opens the checkout card, whose ↵ Workspace is the click (B16).
    const card = page.locator("[data-checkout-card]");
    await expect(card).toBeVisible();
    await expect(card.locator('[data-checkout-card-row="base"]')).toContainText("↑1");
    await expect(card.locator('[data-checkout-card-row="changes"]')).toContainText("1 file");
    await expect(card.locator("[data-checkout-card-workspace]")).toBeVisible();
    await screenshot(page, "overview-lane-card-light");
    // Resting on the asking node's line opens its whole message (B22). The
    // pointer leaves the card first: Radix clears its in-transit mark from a
    // hoverable card on a move after the one that left it, and a trigger
    // ignores moves while that mark is set.
    await leaveHoverCard(page, card);
    await askingNode.locator(`[data-lens-node-line="${askingPane}"]`).hover();
    const message = page.locator(`[data-lens-message="${askingPane}"]`);
    await expect(message).toBeVisible();
    await expect(message).toContainText("Done 그룹 회색 링을 바꿔도 될까요?");
    await expect(message).toContainText("사이드바 상태 규칙을 세 곳에 적용했고 Done 그룹만 남았습니다.");
    await screenshot(page, "overview-message-light");
    expect([...sent.values()].reduce((sum, count) => sum + count, 0)).toBe(quietBefore);
    // Its ↵ 패널에서 답하기 is the node's click: that agent's pane (B22).
    await message.locator(`[data-lens-message-open="${askingPane}"]`).click();
    const workspace = page.locator("[data-workspace-screen]");
    await expect(workspace).toBeVisible();
    await expect(page.locator(`[data-pane-view="${askingPane}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });

    // ⌘⇧H from that Workspace opens Agents › 체크아웃 with its lane selected (B11, B12).
    await page.locator("body").click({ position: { x: 1, y: 1 } });
    await page.keyboard.press("Meta+Shift+KeyH");
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    await expect(lane("prd/asking")).toHaveAttribute("data-selected", "true");
    await expect(overview.locator('[data-lens-lane][data-selected="true"]')).toHaveCount(1);
    // A node's click is that agent's pane (B22); Esc leaves the Overview (B26).
    await overview.locator(`[data-lens-open="${workingPane}"]`).click();
    await expect(page.locator(`[data-pane-view="${workingPane}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await page.locator("body").click({ position: { x: 1, y: 1 } });
    await page.keyboard.press("Meta+Shift+KeyH");
    await expect(lane("prd/web-overview-with-a-long-branch-name")).toHaveAttribute("data-selected", "true");
    // The keyboard: a lane head takes focus, → moves to its node, ↵ opens it (B26).
    await lane("prd/web-overview-with-a-long-branch-name").locator("[data-lens-head-open]").focus();
    await page.keyboard.press("ArrowRight");
    await expect(overview.locator(`[data-lens-open="${workingPane}"]`)).toBeFocused();
    // ↑ goes to the node drawn straight above it: its Observer, whose line
    // keeps that column free in the asking lane between them.
    await page.keyboard.press("ArrowUp");
    await expect(overview.locator(`[data-lens-open="${mainPane}"]`)).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(workspace).toBeVisible();
    // A lane head's click is its Workspace, main's included (B17).
    await page.keyboard.press("Meta+Shift+KeyH");
    await lanes.nth(0).locator("[data-lens-head-open]").click();
    await expect(page.locator(`[data-pane-view="${mainPane}"]`)).toBeVisible();
    await page.keyboard.press("Meta+Shift+KeyH");

    // `N merged → 정리` opens the disk cleanup sheet on the finished filter and
    // leaves the lane's 정리할 것 fold as it was; Escape closes the sheet, the
    // fold's own line unfolds it (disk-layers B3). The merged lane is dimmed
    // with the merge glyph and offers 정리, whose popover says what it
    // removes; 정리 opens the Delete worktree dialog, and cancelling changes
    // nothing (B18, B19).
    await page.locator('[data-stat="merged"]').click();
    await expect(page.locator("[data-disk-sheet]")).toHaveAttribute("data-disk-filter", "done");
    await expect(cleanupFold).toHaveAttribute("aria-expanded", "false");
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-disk-sheet]")).toHaveCount(0);
    await cleanupFold.click();
    await expect(cleanupFold).toHaveAttribute("aria-expanded", "true");
    const shippedLane = lane("prd/shipped");
    await expect(shippedLane.locator(`[data-lens-node="${shippedPane}"]`)).toHaveAttribute("data-bucket", "resting");
    const cleanup = shippedLane.locator("[data-lens-cleanup]");
    await cleanup.hover();
    await expect(page.getByRole("tooltip")).toContainText("지운다");
    await cleanup.click();
    const dialog = page.locator("[data-delete-worktree]");
    await expect(dialog).toBeVisible();
    await screenshot(page, "overview-cleanup-dialog-light");
    // Pane closing is counted on the button, the branch kept by default (B19).
    await expect(dialog.locator("[data-delete-confirm]")).toHaveText("Close 1 pane and delete", { timeout: 15_000 });
    await dialog.locator("[data-delete-cancel]").click();
    await expect(dialog).toHaveCount(0);
    await expect(shippedLane).toBeVisible();

    // The lineage mode: Observer, Implementor, then children; the asking
    // lineage first; one arrow from parent to child; each node's third line
    // names its checkout, issue and PR (B23, B24); resting lineages fold (B25).
    await overview.locator('[data-agents-mode-item="lineage"]').click();
    await expect(overview).toHaveAttribute("data-agents-mode", "lineage");
    const lineages = overview.locator("[data-lens-lineage]");
    await expect(lineages.first()).toHaveAttribute("data-lens-lineage", askingPane);
    await expect(lineages.first()).toHaveAttribute("data-lineage-rank", "turn");
    const observerLineage = overview.locator(`[data-lens-lineage="${mainPane}"]`);
    await expect(observerLineage.locator("[data-lens-node]")).toHaveCount(2);
    await expect(overview.locator(`[data-lens-line="${mainPane}>${workingPane}"]`)).toHaveAttribute("d", /^M \S+ \S+ H /);
    await expect(overview.locator("[data-lens-mode=lineage]")).toContainText("Observer · 보통 main");
    await expect(overview.locator("[data-lens-mode=lineage]")).toContainText("Implementor · 워크트리");
    await expect(overview.locator(`[data-lens-node="${workingPane}"] [data-lens-checkout-chip]`)).toContainText("prd/web-overview-with-a-long-branch-name");
    await expect(overview.locator('[data-lens-mode=lineage] [data-lens-fold="cleanup"]')).toHaveText(/정리할 것 1/);
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await atRest(page);
      await screenshot(page, `overview-agents-lineage-${theme}`);
    }
    // A checkout chip's click is its Workspace (B24).
    await overview.locator(`[data-lens-node="${askingPane}"] [data-lens-checkout-chip]`).click();
    await expect(workspace).toBeVisible();
    // Recent Panels brings back the Overview as it was left: lineage mode,
    // 정리할 것 open (B11). ⌥` now cycles the focused area, so the global
    // command has no chord until one is bound in Settings (focused-area-tab-cycle D-05).
    await bindChordlessCommand(page, "recent_panel", "Alt+Shift+KeyP");
    await page.keyboard.press("Alt+Shift+KeyP");
    await expect(overview).toBeVisible();
    await expect(overview).toHaveAttribute("data-agents-mode", "lineage");
    await expect(overview.locator('[data-lens-mode=lineage] [data-lens-fold="cleanup"]')).toHaveAttribute("aria-expanded", "true");
    // Any other entry resets it to Agents › 체크아웃 (B11).
    await repoRow.click();
    await repoRow.click();
    await expect(overview).toHaveAttribute("data-agents-mode", "checkouts");
    await expect(cleanupFold).toHaveAttribute("aria-expanded", "false");

    // The issue chip opens the Issues tab with that issue's panel beside the
    // board (PRD overview-lenses-issues B10); the board is issues only: a
    // card per issue in Backlog, In progress and Review, and the worktree and
    // pull request with no issue as one line each under their columns (B1-B4).
    await emptyFold.click();
    await linkedLane.locator("[data-lens-issue-chip]").click();
    await expect(overview).toHaveAttribute("data-overview-view", "issues");
    await expect(tile("issues")).toHaveAttribute("data-selected", "true");
    const column = (id: string) => page.locator(`[data-overview-column="${id}"]`);
    const issueCard = (number: number) => page.locator(`[data-issue-card="github:acme/repo#${number}"]`);
    const panel = page.locator("[data-issue-panel]");
    await expect(panel).toHaveAttribute("data-issue-panel", "github:acme/repo#2");
    await expect(overview.locator("[data-issues-split]")).toBeVisible();
    await expect(issueCard(2)).toHaveAttribute("data-selected", "true");
    await expect(issueCard(2).locator('[data-issue-created-age]')).toHaveText(/^\d+d$/);
    await expect(issueCard(3).locator('[data-issue-created-age]')).toHaveCount(0);
    await issueCard(2).screenshot({ path: path.join(process.env.HIDE_E2E_SCREENSHOT_DIR ?? herdr.root, "overview-issue-created-age.png") });
    await expect(overview.locator("[data-tasks-mode-item]")).toHaveCount(3);
    await expect(column("working").locator("[data-overview-card]")).toHaveCount(1, { timeout: 20_000 });
    await expect(column("backlog").locator("[data-overview-card]")).toHaveCount(1);
    await expect(column("review").locator("[data-overview-card]")).toHaveCount(1, { timeout: 20_000 });
    await expect(overview.locator("[data-overview-card]:not([data-issue-card])")).toHaveCount(0);
    await expect(column("working").locator("[data-loose-worktrees]")).toHaveAttribute("data-loose-worktrees", "2");
    await expect(column("working").locator("[data-loose-worktrees]")).toHaveText(/이슈 없는 워크트리 2/);
    await expect(column("review").locator("[data-loose-prs]")).toHaveText(/이슈 없는 PR 1/);
    await expect(column("backlog")).toContainText("Graph 뷰");
    await expect(column("backlog").locator("[data-blocked-by]")).toHaveAttribute("data-blocked-by", "github:acme/repo#2", { timeout: 20_000 });
    // A card is glyph, id and title; in progress its checkout chip (B1, B2);
    // in review its pull request chip (B2, B3).
    await expect(issueCard(2).locator("[data-task-id]")).toHaveText("#2");
    await expect(issueCard(2).locator("[data-card-title]")).toHaveText("태스크 출처 어댑터");
    await expect(issueCard(2).locator("[data-card-checkout]")).toContainText("2-task-source");
    await expect(issueCard(4).locator("[data-lens-pr-chip]")).toHaveAttribute("data-lens-pr-chip", "11");
    await expect(issueCard(4)).toContainText("리뷰 필요");
    await expect(issueCard(3).locator("[data-card-chips]")).toHaveCount(0);

    // The panel (B11-B15): head, the in-progress actions, the properties the
    // read brought, what was done for it, the Markdown body and the latest
    // three of four comments; a GitHub issue has no edit (B19).
    await expect(panel).toHaveAttribute("data-issue-source", "github");
    await expect(panel.locator("[data-issue-state]")).toHaveText("Open");
    await expect(panel.locator("[data-issue-panel-title]")).toHaveText("태스크 출처 어댑터");
    await expect(panel.locator("[data-issue-panel-workspace]")).toBeVisible();
    await expect(panel.locator("[data-issue-panel-github]")).toBeVisible();
    await expect(panel.locator("[data-issue-panel-start], [data-issue-panel-edit]")).toHaveCount(0);
    await expect(panel.locator('[data-issue-property="stage"]')).toHaveText("진행 중");
    await expect(panel.locator('[data-issue-property="labels"] [data-issue-label]')).toHaveCount(2, { timeout: 20_000 });
    await expect(panel.locator('[data-issue-property="author"]')).toHaveText("hoyeon · 9월 20일");
    await expect(panel.locator('[data-issue-property="assignees"]')).toHaveText("hoyeon");
    await expect(panel.locator('[data-issue-property="created"]')).toHaveCount(0);
    await expect(panel.locator("[data-issue-work-checkout]")).toContainText("2-task-source");
    await expect(panel.locator("[data-issue-body=ready] [data-markdown-text]")).toContainText("출처를 어댑터로 나눈다.");
    await expect(panel.locator("[data-issue-comments]")).toHaveAttribute("data-issue-comments", "4");
    await expect(panel.locator("[data-issue-comment]")).toHaveCount(3);
    await expect(panel.locator("[data-issue-comment]").last()).toContainText("마지막 댓글");
    await expect(panel.locator("[data-issue-comments]")).toContainText("쓰기는 GitHub에서");
    // The read is cached, so the card shows the labels it brought (B1).
    await expect(issueCard(2).locator("[data-card-labels] [data-issue-label]")).toHaveCount(2);
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await atRest(page);
      await screenshot(page, `overview-issues-panel-${theme}`);
    }

    // The keyboard moves the panel with the card (B10, B20): → to the
    // neighbouring column's card at the same height, which is review's; a
    // column's end keeps it; ← twice reaches the backlog.
    await issueCard(2).focus();
    await page.keyboard.press("ArrowRight");
    await expect(issueCard(4)).toBeFocused();
    await expect(panel).toHaveAttribute("data-issue-panel", "github:acme/repo#4");
    // Issue 4's first read fails: one line of why and 재시도 in the body's
    // place, the rest of the panel standing (B16); 재시도 reads it again.
    await expect(panel.locator("[data-issue-body=failed] [data-issue-body-failure]")).toBeVisible({ timeout: 20_000 });
    await expect(panel.locator('[data-issue-property="stage"]')).toHaveText("리뷰");
    await expect(panel.locator("[data-issue-work-pr]")).toHaveAttribute("data-issue-work-pr", "11");
    // In review the panel's first action is the card's: the pull request, not the Workspace (B6, B11).
    await expect(panel.locator('[data-issue-panel-pr="11"]')).toBeVisible();
    await expect(panel.locator("[data-issue-panel-workspace]")).toHaveCount(0);
    await expect(page.locator('[role="alert"]')).toHaveCount(0);
    const detailsBefore = sent.get("issue_detail_request") ?? 0;
    await panel.locator("[data-issue-body-retry]").click();
    await expect(panel.locator("[data-issue-body=ready]")).toContainText("리뷰할 본문", { timeout: 20_000 });
    expect((sent.get("issue_detail_request") ?? 0) - detailsBefore).toBe(1);
    expect(last.get("issue_detail_request")?.task_key).toBe("github:acme/repo#4");
    await issueCard(4).focus();
    await page.keyboard.press("ArrowDown");
    await expect(issueCard(4)).toBeFocused();
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    await expect(issueCard(3)).toBeFocused();
    await expect(panel).toHaveAttribute("data-issue-panel", "github:acme/repo#3");
    await expect(panel.locator("[data-issue-panel-start]")).toBeVisible();
    await expect(panel.locator('[data-issue-property="blocked"]')).toContainText("#2");
    // Esc closes the panel first and leaves the Overview where it is (B10).
    await page.keyboard.press("Escape");
    await expect(panel).toHaveCount(0);
    await expect(overview).toHaveAttribute("data-overview-view", "issues");
    await expect(overview.locator("[data-issues-split]")).toHaveCount(0);
    await expect(issueCard(3)).toBeFocused();
    // ↵ opens it again; the same issue opens on its cache and reads again (B15, B20).
    const againBefore = sent.get("issue_detail_request") ?? 0;
    await page.keyboard.press("Enter");
    await expect(panel).toHaveAttribute("data-issue-panel", "github:acme/repo#3");
    await expect(panel.locator("[data-issue-body=ready]")).toContainText("그래프 뷰의 본문");
    await expect.poll(() => (sent.get("issue_detail_request") ?? 0) - againBefore).toBe(1);
    await panel.locator("[data-issue-panel-close]").click();
    await expect(panel).toHaveCount(0);

    // Hover, focus and a half-second rest on a card publish nothing, and its
    // buttons fill the id line's reserved slot without changing its height
    // (B6, B22). Resting on the id opens the preview, which reads the issue
    // once however often it opens (B8, D-42).
    await atRest(page);
    const quietIssues = [...sent.values()].reduce((sum, count) => sum + count, 0);
    const restHeight = (await issueCard(2).boundingBox())?.height;
    await issueCard(2).hover();
    await expect(issueCard(2).locator("[data-card-workspace]")).toBeVisible();
    expect((await issueCard(2).boundingBox())?.height).toBe(restHeight);
    await page.waitForTimeout(600);
    expect([...sent.values()].reduce((sum, count) => sum + count, 0)).toBe(quietIssues);
    // Each button says what it does (B7).
    await issueCard(2).locator("[data-card-workspace]").hover();
    await expect(page.getByRole("tooltip")).toContainText("Workspace 열기");
    await issueCard(3).hover();
    await expect(issueCard(3).locator("[data-card-start]")).toBeVisible();
    await issueCard(3).locator("[data-card-start]").hover();
    await expect(page.getByRole("tooltip")).toContainText("이 이슈로 워크트리와 에이전트를 만든다");
    await leaveHoverCard(page, page.getByRole("tooltip"));
    const previewBefore = sent.get("issue_detail_request") ?? 0;
    await issueCard(2).locator("[data-task-id]").hover();
    const preview = page.locator('[data-issue-preview="github:acme/repo#2"]');
    await expect(preview).toBeVisible();
    await expect(preview.locator("[data-issue-preview-body]")).toHaveText(/^배경\s+출처를 어댑터로 나눈다\.\s+GitHub/);
    await expect(preview).toContainText("hoyeon · 9월 20일 · 댓글 4");
    await expect(preview).toContainText("Open");
    await screenshot(page, "overview-issue-preview-light");
    await leaveHoverCard(page, preview);
    await issueCard(2).locator("[data-task-id]").hover();
    await expect(preview).toBeVisible();
    await leaveHoverCard(page, preview);
    expect((sent.get("issue_detail_request") ?? 0) - previewBefore).toBeLessThanOrEqual(1);

    // A line of work with no issue (B4): the worktree line's popover says
    // where it goes and names them, its click is Agents › 체크아웃; the pull
    // request line's is the PRs tab, where each has an issue cell to link
    // (overview-lenses-prs B21).
    const looseWorktrees = column("working").locator("[data-loose-worktrees]");
    await looseWorktrees.hover();
    await expect(page.getByRole("tooltip")).toContainText("Agents › 체크아웃에서 보기");
    await expect(page.getByRole("tooltip")).toContainText("prd/asking");
    await looseWorktrees.click();
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    await expect(overview).toHaveAttribute("data-agents-mode", "checkouts");
    await tile("issues").locator("[data-lens-tile-button]").click();
    const loosePrs = column("review").locator("[data-loose-prs]");
    await loosePrs.hover();
    await expect(page.getByRole("tooltip")).toContainText("PRs 탭에서 보기");
    await expect(page.getByRole("tooltip")).toContainText("#12");
    await loosePrs.click();
    await expect(overview).toHaveAttribute("data-overview-view", "prs");
    await expect(overview.locator('[data-pr="12"] [data-pr-issue="none"]')).toBeVisible();
    await tile("issues").locator("[data-lens-tile-button]").click();

    // The filter at the facts line's right end, beside the mode control
    // (B21): words in the id or title keep the matching cards, and with none
    // left the backlog says so with a way back.
    const filter = overview.locator("[data-issues-controls] [data-issue-filter]");
    await expect(filter).toHaveAttribute("data-issue-filter", "none");
    await filter.click();
    await page.locator("[data-issue-filter-query]").fill("graph");
    await expect(overview.locator("[data-issue-card]")).toHaveCount(1);
    await expect(issueCard(3)).toBeVisible();
    await page.locator("[data-issue-filter-query]").fill("없는 이슈");
    await expect(overview.locator("[data-issue-card]")).toHaveCount(0);
    await expect(column("backlog").locator("[data-filter-empty]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(filter).toHaveAttribute("data-issue-filter", "active");
    await column("backlog").locator("[data-filter-clear]").click();
    await expect(filter).toHaveAttribute("data-issue-filter", "none");
    await expect(overview.locator("[data-issue-card]")).toHaveCount(3);
    // Done work with no issue is no card, so Done has no column here.
    await expect(column("done")).toHaveCount(0);
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await atRest(page);
      await screenshot(page, `overview-issues-${theme}`);
    }

    // Dependencies draws the same task cards left to right, one arrow between them.
    await page.locator('[data-tasks-mode-item="dependencies"]').click();
    const graph = page.locator("[data-dependency-graph]");
    await expect(graph.locator("[data-dependency-layer]")).toHaveCount(2);
    await expect(graph.locator('[data-dependency-layer="0"] [data-overview-card]')).toHaveAttribute("data-task-key", "github:acme/repo#2");
    await expect(graph.locator('[data-dependency-layer="1"] [data-overview-card]')).toHaveAttribute("data-task-key", "github:acme/repo#3");
    await expect(graph.locator("[data-dependency-edge]")).toHaveCount(1);
    await expect(graph.locator("[data-dependency-edge]")).toHaveAttribute("d", /^M \S+ \S+ C /);
    // The Issues mode is the page's: All projects' Tasks opens on it too (task-agents-views B9, D-10).
    await page.locator("[data-go-main]").click();
    const main = page.locator("[data-main-screen]");
    await page.locator('[data-main-tab="tasks"]').click();
    await expect(main.locator("[data-tasks-mode]")).toHaveAttribute("data-tasks-mode", "dependencies");
    await expect(main.locator('[data-dependency-layer="0"] [data-card-project]')).toHaveText("repo");
    await page.locator('[data-tasks-mode-item="list"]').click();
    // The List mode on the project's Issues tab.
    await repoRow.click();
    await tile("issues").locator("[data-lens-tile-button]").click();
    const list = page.locator("[data-tasks-list]");
    await expect(list.locator("[data-list-group]").first()).toHaveAttribute("data-list-group", "working");
    await expect(list.locator('[data-issue-row="github:acme/repo#2"] [data-issue-created-age]')).toHaveText(/^\d+d$/);
    await expect(list.locator('[data-issue-row="github:acme/repo#3"] [data-issue-created-age]')).toHaveCount(0);
    await page.locator('[data-tasks-mode-item="board"]').click();

    // The Sessions tile is the project's Sessions (B5).
    await tile("sessions").locator("[data-lens-tile-button]").click();
    await expect(overview).toHaveAttribute("data-overview-view", "sessions");
    await expect(page.locator("[data-sessions-state]")).toHaveAttribute("data-sessions-state", /^(empty|rows)$/, { timeout: 20_000 });
    await atRest(page);
    await screenshot(page, "overview-sessions-light");

    // All projects keeps its tab row; its Agents view is the same lanes,
    // each head carrying its project's name (B30).
    const allProjects = page.locator("[data-home-destination]");
    await allProjects.click();
    await expect(main).toBeVisible();
    await expect(allProjects).toHaveAttribute("aria-current", "page");
    await expect(page.locator('[data-main-stats] [data-stat="projects"]')).toHaveText("3 projects");
    await expect(main.locator("[data-main-tab]")).toHaveCount(3);
    await expect(main.locator('[data-main-tab="agents"] [data-agents-waiting]')).toBeVisible();
    await page.locator('[data-main-tab="agents"]').click();
    await expect(main.locator("[data-lens-mode=checkouts]")).toBeVisible();
    await expect(main.locator("[data-lens-lane]", { has: page.locator("[data-lens-head]", { hasText: "prd/asking" }) }).locator("[data-lens-head-project]")).toHaveText("repo");
    await expect(main.locator(`[data-lens-node="${askingPane}"]`)).toHaveAttribute("data-bucket", "turn");
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await atRest(page);
      await screenshot(page, `all-projects-agents-${theme}`);
    }
    await page.locator('[data-agents-mode-item="lineage"]').click();
    await expect(main.locator("[data-lens-mode=lineage]")).toBeVisible();
    await atRest(page);
    await screenshot(page, "all-projects-lineage-light");
    await page.locator('[data-agents-mode-item="checkouts"]').click();

    // A folder's issues are Local ones kept by Hide: C makes an issue, 만들고
    // 바로 시작 goes on to the Start dialog, and the card's menu closes it.
    await page.locator('[data-main-tab="projects"]').click();
    await page.locator("[data-main-project]", { hasText: /^fixture/ }).click();
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    await tile("issues").locator("[data-lens-tile-button]").click();
    await expect(overview).toHaveAttribute("data-overview-state", "empty");
    await expect(page.locator('[data-overview-column="backlog"] [data-backlog-empty]')).toBeVisible();
    await atRest(page);
    await page.keyboard.press("c");
    const newIssue = page.locator("[data-new-issue]");
    await expect(newIssue).toBeVisible();
    await expect(newIssue.locator("[data-new-issue-project]")).toContainText("fixture · Local");
    await newIssue.locator("[data-new-issue-title]").fill("폴더 이슈");
    await newIssue.locator("[data-new-issue-body]").fill("폴더에서 할 일");
    await newIssue.locator("[data-new-issue-start]").click();
    await newIssue.locator("[data-new-issue-create]").click();
    const start = page.locator("[data-start-issue]");
    await expect(start).toBeVisible();
    await expect(start.locator("[data-start-prompt]")).toHaveValue("로컬 이슈 L-1를 해결해줘: 폴더 이슈\n\n폴더에서 할 일");
    await page.getByRole("button", { name: "취소" }).click();
    const localCard = page.locator('[data-overview-column="backlog"] [data-overview-card]');
    await expect(localCard).toHaveCount(1);
    await expect(localCard.locator("[data-task-id]")).toHaveText("L-1");
    // A Local issue's panel (B12, B14, B18): no labels, author or comments,
    // the day it was made; its title edits in place, Esc cancels with the
    // card unchanged, an empty title is refused in place with the text kept,
    // and ⌘↵ saves as one event.
    await localCard.click();
    const localPanel = page.locator("[data-issue-panel]");
    await expect(localPanel).toHaveAttribute("data-issue-source", "local");
    await expect(localPanel.locator('[data-issue-property="created"]')).toBeVisible();
    await expect(localPanel.locator('[data-issue-property="labels"], [data-issue-property="author"], [data-issue-property="assignees"], [data-issue-comments]')).toHaveCount(0);
    await expect(localPanel.locator("[data-issue-body=ready]")).toContainText("폴더에서 할 일");
    await expect(localPanel.locator("[data-issue-panel-edit]")).toBeVisible();
    await localPanel.locator("[data-issue-panel-title]").click();
    const editor = localPanel.locator("[data-issue-editor]");
    await expect(editor.locator("[data-issue-editor-title]")).toHaveValue("폴더 이슈");
    await editor.locator("[data-issue-editor-title]").fill("바뀌면 안 되는 제목");
    await page.keyboard.press("Escape");
    await expect(editor).toHaveCount(0);
    await expect(localPanel).toBeVisible();
    await expect(localCard.locator("[data-card-title]")).toHaveText("폴더 이슈");
    await localPanel.locator("[data-issue-panel-edit]").click();
    await editor.locator("[data-issue-editor-title]").fill("");
    await editor.locator("[data-issue-editor-body]").fill("고친 본문");
    await page.keyboard.press("Meta+Enter");
    await expect(editor.locator("[data-issue-editor-failure]")).toBeVisible();
    await expect(editor.locator("[data-issue-editor-body]")).toHaveValue("고친 본문");
    await expect(localCard.locator("[data-card-title]")).toHaveText("폴더 이슈");
    const updatesBefore = sent.get("local_issue_update") ?? 0;
    await editor.locator("[data-issue-editor-title]").fill("고친 폴더 이슈");
    await page.keyboard.press("Meta+Enter");
    await expect(editor).toHaveCount(0);
    await expect(localCard.locator("[data-card-title]")).toHaveText("고친 폴더 이슈");
    await expect(localPanel.locator("[data-issue-body=ready]")).toContainText("고친 본문");
    expect((sent.get("local_issue_update") ?? 0) - updatesBefore).toBe(1);
    expect(last.get("local_issue_update")).toMatchObject({ title: "고친 폴더 이슈", body: "고친 본문" });
    await atRest(page);
    await screenshot(page, "overview-issue-panel-local-light");
    await page.keyboard.press("Escape");
    await expect(localPanel).toHaveCount(0);
    await localCard.hover();
    await localCard.locator("[data-card-menu]").click();
    await page.locator('[data-card-issue-open="close"]').click();
    await expect(page.locator('[data-overview-column="backlog"] [data-overview-card]')).toHaveCount(0);

    // A project with no agent: its main lane stays pinned, counting none
    // (B13); New agent on a folder opens its Workspace.
    await page.locator("[data-go-main]").click();
    await page.locator('[data-main-tab="projects"]').click();
    await page.locator("[data-main-project]", { hasText: /^quiet/ }).click();
    await expect(overview).toHaveAttribute("data-overview-state", "empty");
    await expect(lanes).toHaveCount(1);
    await expect(lanes.first().locator("[data-lens-head-agents]")).toHaveText("에이전트 0");
    await expect(lanes.first().locator("[data-lens-node]")).toHaveCount(0);
    await expect(tile("agents").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "0");
    await expect(tile("agents").locator("[data-lens-tile-badge]")).toHaveCount(0);
    await atRest(page);
    await screenshot(page, "overview-empty-light");
    await page.locator("[data-overview-new-agent]").click();
    await expect(workspace).toBeVisible();

    // New agent on a Git project opens the New worktree flow; Cancel keeps the Overview (B9).
    await repoRow.click();
    await page.locator("[data-overview-new-agent]").click();
    await expect(page.locator("[data-new-worktree]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-new-worktree]")).toHaveCount(0);
    await expect(overview).toBeVisible();

    // Settings › Issues: the sources, each project's source, and how work starts.
    await page.keyboard.press("Alt+Comma");
    await page.locator('[data-settings-tab="issues"]').click();
    await expect(page.locator("[data-settings-issues]")).toBeVisible();
    await expect(page.getByRole("combobox", { name: "repo 이슈 출처" })).toHaveText("자동 (GitHub)");
    await expect(page.getByRole("combobox", { name: "quiet 이슈 출처" })).toHaveText("자동 (Local)");
    await page.keyboard.press("Escape");

    // 시작 on the backlog issue makes a worktree linked to it, so the card
    // moves from Backlog to In progress; its Workspace comes to the front,
    // and ⌘⇧H selects its lane.
    await tile("issues").locator("[data-lens-tile-button]").click();
    const backlogCard = column("backlog").locator('[data-overview-card][data-task-key="github:acme/repo#3"]');
    await backlogCard.hover();
    await backlogCard.locator("[data-card-start]").click();
    await expect(start).toBeVisible();
    await expect(start.locator("[data-start-name]")).toHaveValue(/^3-/);
    await start.locator("[data-start-name]").fill("3-graph-view");
    // The kind and model start from the remembered choice, Claude before any (PRD home-device-rail B35).
    await expect(start.locator('[data-agent-kind="claude"]')).toBeVisible();
    await start.locator('[data-start-submit="start"]').click();
    await expect(start).toHaveCount(0, { timeout: 30_000 });
    await expect(workspace).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(`[data-checkout][aria-label^="3-graph-view"]`)).toHaveAttribute("aria-current", "true");
    // The agent really runs in the new worktree's pane: Herdr lists it
    // there, and the start never turned into a failure banner.
    const started = execFileSync("git", ["worktree", "list", "--porcelain"], { cwd: repo, encoding: "utf8" }).split("\n\n").find((entry) => entry.includes("branch refs/heads/3-graph-view"))?.match(/^worktree (.+)$/m)?.[1];
    expect(started).toBeTruthy();
    await expect.poll(() => agentsIn(herdr, started as string), { timeout: 60_000 }).toContain("claude");
    await expect(page.locator('[data-task-agent="failed"]')).toHaveCount(0);
    await page.keyboard.press("Meta+Shift+KeyH");
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    await expect(lane("3-graph-view")).toHaveAttribute("data-selected", "true");
    await expect(lane("3-graph-view").locator("[data-lens-node]")).toHaveCount(1);
    await tile("issues").locator("[data-lens-tile-button]").click();
    await expect(column("backlog").locator("[data-overview-card]")).toHaveCount(0, { timeout: 30_000 });
    await expect(column("working").locator('[data-overview-card][data-task-key="github:acme/repo#3"]')).toContainText("3-graph-view", { timeout: 30_000 });

    // While hided is down the Overview keeps the last snapshot and only the
    // connection line says so; nothing turns into a banner.
    const restarting = daemon.restart();
    await expect(page.locator("[data-connection]")).toBeVisible({ timeout: 10_000 });
    await expect(column("working").locator("[data-overview-card]")).toHaveCount(2);
    await expect(overview.locator('[role="alert"]')).toHaveCount(0);
    daemon = await restarting;
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
