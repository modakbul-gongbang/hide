import { openProjectOverview } from "./overview-entry";
// The Project Overview on an isolated pinned Herdr and hided (PRD
// overview-lenses-tiles-agents, on top of web-project-overview,
// task-agents-views and the issue-first rework of 2026-09-28): a Git project
// with a primary checkout and six worktrees at different stages whose issues
// and pull requests a fake `gh` answers, an Observer on main that delegated to an Implementor in
// a worktree, a folder project with agents, and a project with no agent at
// all. Every entry opens the request view (overview-request-view D-05), and
// the Agents tile the graph with the front checkout's box selected
// (agents-graph-view B1); the tiles in the tab row's place count
// agents, issues and today's sessions (B1-B6); boxes are columned, ordered,
// folded, lined and headed as the PRD draws them (B2-B23); a row's popover
// opens the agent's whole message (B19); the filter (B24-B28), the arrow keys
// (B35), hover publishing nothing (B38), an unchanged snapshot moving nothing
// (B39), ⌥` restoring the lens (B29) and the All projects graph (B30) follow. The Issues tile is the issues-only board
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
import { agentsIn, continueFixtureTranscript, declareParent, labelAgent, sessionOf, startHerdr, setFixtureLifecycle, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { fixtureProgram } from "./platform-fixture";
import { countSent, screenshot } from "./wire";
import { chord, field } from "./chords";
import { animationsFinished, quietFor, unchangedForFrames } from "./wait";

test.describe.configure({ timeout: 240_000 });

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "commit.gpgsign=false", "-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: "ignore" });
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
    labelAgent(herdr, pane, { task });
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
    author: { login: "example-user" },
    assignees: [{ login: "example-user" }],
    createdAt: "2026-09-20T00:00:00Z",
    comments: [comment("a", 21, "첫 댓글"), comment("b", 22, "둘째 댓글"), comment("c", 23, "셋째 댓글"), comment("d", 24, "마지막 댓글")],
  });
  const plain = (body: string) => JSON.stringify({ body, labels: [], author: { login: "example-user" }, assignees: [], createdAt: "2026-09-19T00:00:00Z", comments: [] });
  // Issue 3 waits on issue 2, the relation GitHub's blockedBy records (task-agents-views B8).
  // Issue 2 has two sub-issues, GitHub's `completed` of `total` 1 of 2: #4, open and
  // closed by pull request 11, and #9, already closed.
  const none = { subIssuesSummary: { total: 0, completed: 0 }, subIssues: { nodes: [] } };
  const dependencies = JSON.stringify({
    data: {
      r0: {
        nameWithOwner: "acme/repo",
        i2: {
          number: 2,
          blockedBy: { nodes: [] },
          subIssuesSummary: { total: 2, completed: 1 },
          subIssues: {
            nodes: [
              { number: 4, title: "리뷰 중인 이슈", state: "OPEN", repository: { nameWithOwner: "acme/repo" } },
              { number: 9, title: "끝난 조각", state: "CLOSED", repository: { nameWithOwner: "acme/repo" } },
            ],
          },
        },
        i3: { number: 3, blockedBy: { nodes: [{ number: 2, state: "OPEN", repository: { nameWithOwner: "acme/repo" } }] }, ...none },
        i4: { number: 4, blockedBy: { nodes: [] }, ...none },
      },
    },
  });
  const failedOnce = path.join(bin, "issue-4-failed");
  fixtureProgram(
    bin,
    "gh",
    `const fs = require("fs");
const args = process.argv.slice(2);
const key = args.slice(0, 2).join(" ");
const done = (text) => { console.log(text); process.exit(0); };
if (args.join(" ").includes("--state merged")) done("[]");
if (key === "auth status") process.exit(0);
if (key === "pr list") { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 1000); done(${JSON.stringify(pulls)}); }
if (key === "repo view") done('{"nameWithOwner":"acme/repo"}');
if (key === "issue list") done(${JSON.stringify(issues)});
if (key === "issue view") {
  const number = args[2];
  if (number === "2") done(${JSON.stringify(detail)});
  if (number === "4") {
    const failedOnce = ${JSON.stringify(failedOnce)};
    if (fs.existsSync(failedOnce)) done(${JSON.stringify(plain("리뷰할 본문"))});
    fs.writeFileSync(failedOnce, "");
    console.error("HTTP 502");
    process.exit(1);
  }
  done(${JSON.stringify(plain("그래프 뷰의 본문"))});
}
if (key === "api graphql") done(${JSON.stringify(dependencies)});
console.error("unsupported: " + args.join(" "));
process.exit(1);
`,
  );
  return bin;
}

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press(chord("settings"));
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="appearance"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  // Controls fade their colors into the new theme; a capture waits them out.
  await animationsFinished(page);
}

/**
 * Moves the pointer off a hover card the way a hand does, in a run of moves
 * rather than two, until `gone` has left the page. Radix closes a hoverable
 * card only on a move that arrives after its leave listener is in place; two
 * bare moves on a busy runner can both land before it and leave the card
 * open for good (overview.spec on CI, 2026-09-29..10-01).
 */
async function leaveHoverCard(page: Page, gone: Locator): Promise<void> {
  // eslint-disable-next-line hide-e2e/no-action-in-poll -- Radix hoverable tooltip arms its grace-area pointermove listener on the next render; no DOM state reports it, so a synthetic leave is repeated
  await expect(async () => {
    await page.mouse.move(2, 998);
    await page.mouse.move(4, 996, { steps: 4 });
    await expect(gone).toHaveCount(0, { timeout: 500 });
  }).toPass({ timeout: 10_000 });
}

/**
 * Rests on `target` until its tooltip says every one of `texts`. A tooltip opens
 * on a pointer move that reaches its trigger and then waits its 500 ms; a move
 * that is lost on a busy runner opens nothing and nothing retries it (a CI
 * snapshot showed the trigger present and no tooltip). Each attempt leaves the
 * page first so the pointer enters the trigger afresh. Three attempts wait
 * 1.5 s each for the tooltip, less than the five seconds a single wait had; the
 * moves themselves keep their own action timeouts, so a slow runner that is
 * late to move is not counted against the tooltip. A control that only shows
 * while its card is hovered names that card as `within`, which each attempt
 * hovers again before the control.
 */
async function restOn(page: Page, target: Locator, texts: string[], within?: Locator): Promise<void> {
  const tooltip = page.getByRole("tooltip");
  let lastFailure: unknown;
  for (let attempt = 0; attempt < 3; attempt++) {
    await page.mouse.move(2, 998);
    if (within) await within.hover();
    await target.hover();
    try {
      for (const text of texts) await expect(tooltip).toContainText(text, { timeout: 1500 });
      return;
    } catch (failure) {
      lastFailure = failure;
    }
  }
  throw lastFailure;
}

/** The graph draws frames only while it glides: thirty animation frames with no graph frame drawn is rest. */
const graphAtRest = (page: Page, canvas: Locator) => unchangedForFrames(page, () => canvas.getAttribute("data-graph-frames"));

/** Clears hover and keyboard focus so a capture shows the page at rest. */
async function atRest(page: Page): Promise<void> {
  await page.mouse.move(2, 998);
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

test("a project's Overview: tiles, the Agents graph, and the Issues board", async ({ page }) => {
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
    const shippedPane = await workspaceAt(herdr, tree("shipped"), "머지된 작업 마무리");
    // The Observer on main delegated the working worktree's Implementor,
    // which is working, so the Observer waits on it (B14, B21).
    declareParent(herdr, workingPane, mainPane);
    await setFixtureLifecycle(herdr, workingPane, "working");
    // An agent at an approval prompt: its label's one line says what it
    // asks; the node and its popover show it (B21, B22; overview-request-view
    // B47). The approval comes from Herdr, not from the label.
    await setFixtureLifecycle(herdr, askingPane, "blocked");
    labelAgent(herdr, askingPane, {
      task: "사이드바 상태 규칙 구현",
      progress: "Done 그룹 회색 링을 바꿔도 될까요?",
    });
    // A project with only a shell: no agent at all.
    const quiet = path.join(herdr.root, "quiet");
    fs.mkdirSync(quiet);
    await workspaceAt(herdr, quiet, null);

    daemon = await startHided(herdr, "overview", undefined, { PATH: `${fakeGh(herdr.root)}${path.delimiter}${herdr.fixturePath}` });
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await open(page, daemon);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });

    // Shared Overview's current-project scope opens the request view.
    // Its Agents tile keeps the front checkout's box selected (B1).
    await page.locator('[data-sidebar-mode="projects"]').click();
    const repoRow = page.locator("[data-project-row]", { hasText: /^repo/ });
    const refreshesBefore = sent.get("sessions_refresh") ?? 0;
    await openProjectOverview(page, "repo");
    const overview = page.locator("[data-overview-screen]");
    const tile = (id: string) => overview.locator(`[data-lens-tile="${id}"]`);
    /** Every way in lands on the request view; the graph is one tile away. */
    const toGraph = async () => {
      await tile("agents").locator("[data-lens-tile-button]").click();
      await expect(overview).toHaveAttribute("data-overview-view", "agents");
    };
    await expect(overview).toBeVisible();
    await expect(overview).toHaveAttribute("data-overview-view", "requests");
    await expect.poll(() => sent.get("request_view") ?? 0).toBeGreaterThanOrEqual(1);
    expect(last.get("request_view")?.observing).toBe(true);
    await tile("agents").locator("[data-lens-tile-button]").click();
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    expect(last.get("request_view")?.observing).toBe(false);
    await expect.poll(() => sent.get("overview_refresh") ?? 0).toBe(1);
    await expect(overview.locator('[data-overview-refreshing="true"]')).toBeVisible();
    await expect(overview.locator('[data-overview-refreshing="true"] svg')).toHaveClass(/animate-spin/);
    await overview.screenshot({ path: path.join(process.env.HIDE_E2E_SCREENSHOT_DIR ?? herdr.root, "overview-refreshing-stats.png") });
    const boxes = overview.locator("[data-graph-box]");
    const box = (branch: string) => overview.locator("[data-graph-box]", { has: page.locator("[data-graph-head]", { hasText: branch }) });
    const row = (pane: string) => overview.locator(`[data-graph-row="${pane}"]`);
    const canvas = overview.locator("[data-graph-canvas]");
    await expect(overview.locator('[data-graph-box][data-selected="true"]')).toHaveCount(1);
    // The coordinates the layout drew, by the box's own place.
    const place = async (locator: ReturnType<typeof box>) => (await locator.boundingBox())!;
    // Opening the Overview reads the project's session history once (B5).
    await expect.poll(() => (sent.get("sessions_refresh") ?? 0) - refreshesBefore).toBe(1);
    expect(last.get("sessions_refresh")?.workspace_id).toEqual(await overview.getAttribute("data-overview-screen"));
    // Overview selection belongs to the shared row, with no project destination.
    await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
    await expect(repoRow).not.toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-project-overview]")).toHaveCount(0);

    // The facts line keeps worktrees, disk and merged, and no issue or PR
    // count; the mode control sits at its right end (B8).
    await expect(page.locator('[data-stat="worktrees"]')).toHaveText(/6 worktrees/, { timeout: 20_000 });
    await expect(page.locator('[data-stat="disk"]')).toHaveText(/^\d+(\.\d)? (B|KB|MB|GB)$/, { timeout: 30_000 });
    await expect(page.locator('[data-stat="merged"]')).toHaveText("1 merged → Clean up", { timeout: 20_000 });
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
    // The filter is at the right end of that line: three status chips and a search, and no device choice with one device (B24).
    await toGraph();
    await expect(overview.locator("[data-graph-chip]")).toHaveCount(3);
    await expect(overview.locator("[data-graph-search]")).toBeVisible();
    await expect(overview.locator("[data-graph-devices]")).toHaveCount(0);
    // New agent and New issue stay on the title row (B9).
    await expect(overview.locator("[data-overview-new-agent]")).toBeVisible();

    // The tiles where the tab row was: Agents chosen, four agents, one of
    // them the operator's turn, the bar by bucket; Issues two open, the bar
    // by stage; Sessions today's count once the history is read (B1-B5).
    await expect(overview.locator("[data-lens-tile]")).toHaveCount(5);
    expect(await overview.locator("[data-lens-tile]").evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-lens-tile")))).toEqual(["requests", "agents", "issues", "prs", "sessions"]);
    await expect(tile("agents")).toHaveAttribute("data-selected", "true");
    await expect(tile("agents").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "4", { timeout: 20_000 });
    await expect(tile("agents").locator("[data-lens-tile-badge]")).toHaveText("1");
    await expect(tile("agents").locator("[data-lens-tile-bar]")).toHaveAttribute("data-lens-tile-bar", "turn:1 working:1 delegating:1 resting:1");
    await expect(tile("issues").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "3", { timeout: 20_000 });
    await expect(tile("issues")).toContainText("open");
    await expect(tile("issues").locator("[data-lens-tile-bar]")).toHaveAttribute("data-lens-tile-bar", "backlog:1 working:1 review:1");
    await expect(tile("issues").locator("[data-lens-tile-badge]")).toHaveCount(0);
    await expect(tile("sessions").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "0", { timeout: 20_000 });
    await expect(tile("sessions")).toContainText("today");
    // Resting on the bar shows its legend and on the badge its breakdown (B4).
    await restOn(page, tile("agents").locator("[data-lens-tile-bar]"), ["My turn 1"]);
    await restOn(page, tile("agents").locator("[data-lens-tile-badge]"), ["Approval 1"]);

    // Boxes (B2-B5, D-37): the asking worktree's band stands first, above main's, because
    // its question outranks main's band; main holds the Observer and the Implementor it
    // delegated one column right at its row's height; the merged worktree and the three
    // with no agent fold (B21, B23).
    await expect(boxes).toHaveCount(3, { timeout: 20_000 });
    const mainBox = overview.locator('[data-graph-box][data-graph-primary="true"]');
    const workingBox = box("prd/web-overview-with-a-long-branch-name");
    const askingBox = box("prd/asking");
    await expect(mainBox).toHaveAttribute("data-graph-col", "0");
    await expect(workingBox).toHaveAttribute("data-graph-col", "1");
    await expect(askingBox).toHaveAttribute("data-graph-col", "0");
    const [mainRect, workingRect, askingRect] = [await place(mainBox), await place(workingBox), await place(askingBox)];
    expect(Math.abs(workingRect.y - mainRect.y)).toBeLessThan(1);
    expect(workingRect.x).toBeGreaterThan(mainRect.x + mainRect.width);
    expect(askingRect.y + askingRect.height).toBeLessThan(mainRect.y);
    // main's head says how many agents it has (B12); a worktree head: the branch, ↑N and its changed files, dirty in the warning tone.
    await expect(mainBox.locator("[data-graph-head-agents]")).toHaveText("1 agent");
    await expect(workingBox.locator("[data-graph-head-distance]")).toHaveText("↑1");
    await expect(workingBox.locator("[data-graph-head-files]")).toHaveText("1 file");
    await expect(workingBox.locator("[data-graph-head-files]")).toHaveClass(/text-warning/);
    // Rows: the Observer waits on its child, the Implementor works, the asking row is the operator's turn and the only one with a second line (B17).
    await expect(row(workingPane)).toHaveAttribute("data-bucket", "working");
    await expect(row(mainPane)).toHaveAttribute("data-bucket", "delegating");
    await expect(row(askingPane)).toHaveAttribute("data-bucket", "turn");
    await expect(row(askingPane)).toHaveAttribute("data-attention", "0");
    await expect(row(askingPane).locator(`[data-graph-row-line="${askingPane}"]`)).toHaveText("Done 그룹 회색 링을 바꿔도 될까요?");
    await expect(overview.locator("[data-graph-row-line]")).toHaveCount(1);
    // One line from the Observer's row to the Implementor's, blue with flowing dashes while it works (B8, B9).
    const edge = overview.locator(`[data-graph-edge="${mainPane}>${workingPane}"]`);
    await expect(edge).toHaveAttribute("data-edge-kind", "flow");
    await expect(edge.locator(".graph-edge-base")).toHaveAttribute("d", /^M \S+ \S+ /);
    await expect(overview.locator(`[data-graph-flow="${mainPane}>${workingPane}"]`)).toHaveCount(1);
    await expect(overview.locator("[data-graph-edge]")).toHaveCount(1);
    // The folds: one line each, per project, a click unfolding it in place (B21, B23).
    const emptyFold = overview.locator('[data-graph-fold="empty"]');
    const cleanupFold = overview.locator('[data-graph-fold="cleanup"]');
    await expect(emptyFold).toHaveText(/Worktrees without agents 3/);
    await expect(cleanupFold).toHaveText(/Ready to clean up 1/);
    await emptyFold.click();
    await expect(emptyFold).toHaveAttribute("aria-expanded", "true");
    await expect(boxes).toHaveCount(6);
    // The linked worktree's head carries its issue chip, which opens the
    // Issues tab at that card (B14).
    const linkedBox = box("2-task-source");
    await expect(linkedBox.locator("[data-lens-issue-chip]")).toHaveAttribute("data-lens-issue-chip", "github:acme/repo#2");
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await atRest(page);
      await screenshot(page, `overview-agents-graph-${theme}`);
    }

    // Hover, focus and a half-second rest publish nothing (B38).
    const quietBefore = [...sent.values()].reduce((sum, count) => sum + count, 0);
    await workingBox.locator("[data-graph-head-open]").hover();
    // ↵ Workspace appears at the end of the branch line without moving it (B15).
    await expect(workingBox.locator("[data-graph-head-hint]")).toBeVisible();
    // Resting on a head opens the checkout card, whose ↵ Workspace is the click.
    const card = page.locator("[data-checkout-card]");
    await expect(card).toBeVisible();
    await expect(card.locator('[data-checkout-card-row="base"]')).toContainText("↑1");
    await expect(card.locator('[data-checkout-card-row="changes"]')).toContainText("1 file");
    await expect(card.locator("[data-checkout-card-workspace]")).toBeVisible();
    await screenshot(page, "overview-box-card-light");
    // The pointer leaves the card first: Radix clears its in-transit mark from a hoverable card on a move after the one that left it, and a trigger ignores moves while that mark is set.
    await leaveHoverCard(page, card);
    // Hovering a row keeps its delegation chain bright and fades the rest, then restores (B19).
    await page.mouse.move(2, 998);
    await page.mouse.move(4, 996);
    await row(workingPane).locator("[data-graph-open]").hover();
    await expect(row(mainPane)).toHaveCSS("opacity", "1");
    await expect(row(askingPane)).not.toHaveCSS("opacity", "1");
    await expect(edge).toHaveCSS("opacity", "1");
    await expect(row(workingPane).locator("[data-graph-row-hint]")).toHaveText("↵ Panel");
    await page.mouse.move(2, 998);
    await page.mouse.move(4, 996);
    await expect(row(askingPane)).toHaveCSS("opacity", "1");
    // Resting on the asking row opens the agent's whole message with where it stands (B19).
    await row(askingPane).locator("[data-graph-open]").hover();
    const message = page.locator(`[data-lens-message="${askingPane}"]`);
    await expect(message).toBeVisible();
    await expect(message).toContainText("Done 그룹 회색 링을 바꿔도 될까요?");
    await expect(message.locator("[data-lens-message-context]")).toContainText("prd/asking");
    await expect(message.locator("[data-lens-message-context]")).toContainText("Started directly");
    await expect(row(askingPane).locator("[data-graph-row-hint]")).toHaveText("↵ Answer");
    await screenshot(page, "overview-message-light");
    expect([...sent.values()].reduce((sum, count) => sum + count, 0)).toBe(quietBefore);
    // A delegated row's popover names who delegated it.
    await page.mouse.move(2, 998);
    await page.mouse.move(4, 996);
    await row(workingPane).locator("[data-graph-open]").hover();
    await expect(page.locator(`[data-lens-message="${workingPane}"] [data-lens-message-context]`)).toContainText("최신 hide 서버 웹 실행");
    // ...and says in words what colour its line is (B36).
    await expect(page.locator(`[data-lens-message="${workingPane}"] [data-lens-message-context]`)).toContainText("LineWorking");
    await page.mouse.move(2, 998);
    await page.mouse.move(4, 996);
    await row(askingPane).locator("[data-graph-open]").hover();
    await expect(message).toBeVisible();
    // Its ↵ Answer in panel is the row's click: that agent's pane (B18, B19).
    await message.locator(`[data-lens-message-open="${askingPane}"]`).click();
    const workspace = page.locator("[data-workspace-screen]");
    await expect(workspace).toBeVisible();
    await expect(page.locator(`[data-pane-view="${askingPane}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });

    // ⌘⇧H from that Workspace opens the request view, and the graph its box selected (B1).
    await page.locator("body").click({ position: { x: 1, y: 1 } });
    await page.locator("[data-go-main]").click();
    await page.getByRole("tab", { name: "repo", exact: true }).click();
    await toGraph();
    await expect(askingBox).toHaveAttribute("data-selected", "true");
    await expect(overview.locator('[data-graph-box][data-selected="true"]')).toHaveCount(1);
    // A row's click is that agent's pane (B18); the way back selects its box again.
    await overview.locator(`[data-graph-open="${workingPane}"]`).click();
    await expect(page.locator(`[data-pane-view="${workingPane}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await page.locator("body").click({ position: { x: 1, y: 1 } });
    await page.locator("[data-go-main]").click();
    await page.getByRole("tab", { name: "repo", exact: true }).click();
    await toGraph();
    await expect(workingBox).toHaveAttribute("data-selected", "true");
    // The keyboard: a head takes focus, ↓ moves to its row, ← to the row drawn beside it, ↵ opens it (B35).
    await workingBox.locator("[data-graph-head-open]").focus();
    await page.keyboard.press("ArrowDown");
    await expect(overview.locator(`[data-graph-open="${workingPane}"]`)).toBeFocused();
    // Focus shows the same chain a hover does.
    await expect(row(askingPane)).not.toHaveCSS("opacity", "1");
    await page.keyboard.press("ArrowLeft");
    await expect(overview.locator(`[data-graph-open="${mainPane}"]`)).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(workspace).toBeVisible();
    // A head's click is its Workspace, main's included (B15).
    await page.locator("[data-go-main]").click();
    await page.getByRole("tab", { name: "repo", exact: true }).click();
    await toGraph();
    await mainBox.locator("[data-graph-head-open]").click();
    await expect(page.locator(`[data-pane-view="${mainPane}"]`)).toBeVisible();
    await page.locator("[data-go-main]").click();
    await page.getByRole("tab", { name: "repo", exact: true }).click();
    await toGraph();

    // `N merged → Clean up` opens the disk cleanup sheet on the finished filter and
    // leaves the Ready to clean up fold as it was; Escape closes the sheet, the
    // fold's own line unfolds it (disk-layers B3). The merged box is dimmed
    // with the merge glyph and offers Clean up, whose popover says what it
    // removes; Clean up opens the Delete worktree dialog, and cancelling changes
    // nothing (B16).
    await page.locator('[data-stat="merged"]').click();
    await expect(page.locator("[data-disk-sheet]")).toHaveAttribute("data-disk-filter", "done");
    await expect(cleanupFold).toHaveAttribute("aria-expanded", "false");
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-disk-sheet]")).toHaveCount(0);
    await cleanupFold.click();
    await expect(cleanupFold).toHaveAttribute("aria-expanded", "true");
    const shippedBox = box("prd/shipped");
    await expect(row(shippedPane)).toHaveAttribute("data-bucket", "resting");
    const cleanup = shippedBox.locator("[data-graph-cleanup]");
    await restOn(page, cleanup, ["Remove merged worktrees"]);
    await cleanup.click();
    const dialog = page.locator("[data-delete-worktree]");
    await expect(dialog).toBeVisible();
    await screenshot(page, "overview-cleanup-dialog-light");
    // Pane closing is counted on the button, the branch kept by default (B16).
    await expect(dialog.locator("[data-delete-confirm]")).toHaveText("Close 1 pane and delete", { timeout: 15_000 });
    await dialog.locator("[data-delete-cancel]").click();
    await expect(dialog).toHaveCount(0);
    await expect(shippedBox).toBeVisible();

    // The filter (B24-B28). A tile bar's segment lights its status chip alone
    // (B25); chips are OR, search and chips AND; folds and non-matching rows go,
    // the parent chain stays dimmed (B27); nothing matching says so with a way back (B28).
    const chip = (name: string) => overview.locator(`[data-graph-chip="${name}"]`);
    const rows = overview.locator("[data-graph-row]");
    await chip("working").click();
    await chip("turn").click();
    await expect(rows).toHaveCount(3);
    await tile("agents").locator('[data-lens-tile-segment="resting"]').click();
    await expect(chip("resting")).toHaveAttribute("data-state", "on");
    await expect(chip("turn")).toHaveAttribute("data-state", "off");
    await expect(chip("working")).toHaveAttribute("data-state", "off");
    await expect(rows).toHaveCount(1);
    await expect(row(shippedPane)).toBeVisible();
    await expect(overview.locator("[data-graph-fold]")).toHaveCount(0);
    await chip("resting").click();
    await chip("turn").click();
    await expect(rows).toHaveCount(1);
    await expect(row(askingPane)).toBeVisible();
    await chip("working").click();
    await expect(rows).toHaveCount(3);
    await chip("turn").click();
    await expect(rows).toHaveCount(2);
    await expect(boxes).toHaveCount(2);
    await chip("working").click();
    const search = overview.locator("[data-graph-search]");
    // `웹 디자인` is the Implementor's title: its Observer stays only as the dimmed chain to it (B26, B27).
    await search.fill("웹 디자인");
    await expect(rows).toHaveCount(2);
    await expect(row(workingPane)).toHaveCSS("opacity", "1");
    await expect(row(mainPane)).not.toHaveCSS("opacity", "1");
    await search.fill("ASKING");
    await expect(rows).toHaveCount(1);
    await expect(row(askingPane)).toBeVisible();
    await search.fill("zzzz");
    await expect(overview.locator("[data-graph-filter-empty]")).toBeVisible();
    await expect(boxes).toHaveCount(0);
    await atRest(page);
    await screenshot(page, "overview-agents-filter-empty-light");
    await overview.locator("[data-graph-filter-clear]").click();
    await expect(search).toHaveValue("");
    await expect(rows).toHaveCount(4);
    // The first Escape in the search clears only it, chips staying; on an empty field Escape leaves the Overview (B29).
    await chip("turn").click();
    await search.fill("zz");
    await search.press("Escape");
    await expect(search).toHaveValue("");
    await expect(chip("turn")).toHaveAttribute("data-state", "on");
    await expect(overview).toBeVisible();
    await chip("turn").click();
    await expect(rows).toHaveCount(4);

    // Nothing moves while the picture is the same, and a changed one moves once (B33, B39);
    // that new text alone keeps the picture is the graph unit test's (D-26).
    // The graph's own relayouts and animation frames are counted on its canvas;
    // the last glide (a chip just changed the picture) is let finish first.
    await atRest(page);
    await graphAtRest(page, canvas);
    const revision = async () => Number(await canvas.getAttribute("data-graph-revision"));
    const frames = async () => Number(await canvas.getAttribute("data-graph-frames"));
    const revisionBefore = await revision();
    const framesBefore = await frames();
    await quietFor(page, 1500, "an unchanged picture is neither relaid out nor redrawn");
    expect(await revision()).toBe(revisionBefore);
    expect(await frames()).toBe(framesBefore);
    // The Implementor asks: its row grows a second line, its line turns orange, the graph is laid out once and glides.
    // Its own session asks, so the relationship declared for it holds.
    continueFixtureTranscript(herdr, sessionOf(herdr, workingPane), { task: "웹 디자인 시스템 리셋 구현", reply: "이 줄을 그대로 둬도 될까요?" });
    await setFixtureLifecycle(herdr, workingPane, "blocked");
    await expect(edge).toHaveAttribute("data-edge-kind", "ask", { timeout: 20_000 });
    await expect(row(workingPane).locator(`[data-graph-row-line="${workingPane}"]`)).toHaveText("이 줄을 그대로 둬도 될까요?", { timeout: 20_000 });
    await expect.poll(revision).toBeGreaterThan(revisionBefore);
    await expect(row(workingPane)).toHaveAttribute("data-attention", "0");
    await setFixtureLifecycle(herdr, workingPane, "working");
    await expect(edge).toHaveAttribute("data-edge-kind", "flow", { timeout: 20_000 });
    // Reduced motion draws every change at once and stops the flow (B34).
    const flowSelector = `[data-graph-flow="${mainPane}>${workingPane}"]`;
    const flowPath = overview.locator(flowSelector);
    // The canvas names whether its dashes are stepping (the stepping itself, its pace and its stop, is the unit test's); the line the dashes belong to is inside it.
    const flowing = canvas.and(page.locator('[data-graph-flowing="true"]'));
    await expect(flowing.locator(flowSelector)).toHaveCount(1);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await expect(flowPath).toHaveCSS("display", "none");
    await expect(canvas).not.toHaveAttribute("data-graph-flowing", "true");
    await page.emulateMedia({ reducedMotion: "no-preference" });
    // ...and turning the setting off lets the dashes step again on the line that is drawn (B9, B34).
    await expect(flowPath).toHaveAttribute("d", /.+/);
    await expect(flowing.locator(flowSelector)).toHaveCount(1);

    // The shared scope retains its lens within this window across a close.
    await chip("working").click();
    await expect(cleanupFold).toHaveCount(0);
    await page.locator('[data-checkout][aria-label^="prd/asking"]').first().click();
    await expect(workspace).toBeVisible();
    await page.locator("[data-open-overview]").click();
    await expect(overview).toBeVisible();
    await expect(chip("working")).toHaveAttribute("data-state", "on");
    await chip("working").click();
    // Continue the existing wide-board content checks on the central page.
    await page.keyboard.press("Escape");
    await page.locator("[data-go-main]").click();
    await page.getByRole("tab", { name: "repo", exact: true }).click();
    await toGraph();

    // The issue chip opens the Issues tab with that issue's panel beside the
    // board (PRD overview-lenses-issues B10); the board is issues only: a
    // card per issue in Backlog, In progress and Review, and the worktree and
    // pull request with no issue as one line each under their columns (B1-B4).
    await emptyFold.click();
    await linkedBox.locator("[data-lens-issue-chip]").click();
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
    await expect(column("working").locator("[data-loose-worktrees]")).toHaveText(/2 worktrees without an issue/);
    await expect(column("review").locator("[data-loose-prs]")).toHaveText(/1 PR without an issue/);
    await expect(column("backlog")).toContainText("Graph 뷰");
    await expect(column("backlog").locator("[data-blocked-by]")).toHaveAttribute("data-blocked-by", "github:acme/repo#2", { timeout: 20_000 });
    // A card is glyph, id and title; in progress its checkout chip (B1, B2);
    // in review its pull request chip (B2, B3).
    await expect(issueCard(2).locator("[data-task-id]")).toHaveText("#2");
    await expect(issueCard(2).locator("[data-card-title]")).toHaveText("태스크 출처 어댑터");
    await expect(issueCard(2).locator("[data-card-checkout]")).toContainText("2-task-source");
    await expect(issueCard(4).locator("[data-lens-pr-chip]")).toHaveAttribute("data-lens-pr-chip", "11");
    await expect(issueCard(4)).toContainText("Review required");
    await expect(issueCard(3).locator("[data-card-chips]")).toHaveCount(0);
    // GitHub's sub-issue progress on the card, and none on an issue without sub-issues.
    await expect(issueCard(2).locator("[data-sub-issues]")).toHaveAttribute("data-sub-issues", "1/2");
    await expect(issueCard(2).locator("[data-sub-issues]")).toHaveText("Sub-issues 1/2");
    await expect(issueCard(3).locator("[data-sub-issues]")).toHaveCount(0);

    // The panel (B11-B15): head, the in-progress actions, the properties the
    // read brought, what was done for it, the Markdown body and the latest
    // three of four comments; a GitHub issue has no edit (B19).
    await expect(panel).toHaveAttribute("data-issue-source", "github");
    await expect(panel.locator("[data-issue-state]")).toHaveText("Open");
    await expect(panel.locator("[data-issue-panel-title]")).toHaveText("태스크 출처 어댑터");
    await expect(panel.locator("[data-issue-panel-workspace]")).toBeVisible();
    await expect(panel.locator("[data-issue-panel-github]")).toBeVisible();
    await expect(panel.locator("[data-issue-panel-start], [data-issue-panel-edit]")).toHaveCount(0);
    await expect(panel.locator('[data-issue-property="stage"]')).toHaveText("In progress");
    await expect(panel.locator('[data-issue-property="labels"] [data-issue-label]')).toHaveCount(2, { timeout: 20_000 });
    await expect(panel.locator('[data-issue-property="author"]')).toHaveText("example-user · Sep 20");
    await expect(panel.locator('[data-issue-property="assignees"]')).toHaveText("example-user");
    await expect(panel.locator('[data-issue-property="created"]')).toHaveCount(0);
    await expect(panel.locator("[data-issue-work-checkout]")).toContainText("2-task-source");
    // The sub-issues: each one's state, and the pull request that closes it as a chip.
    await expect(panel.locator("[data-issue-sub-issues]")).toHaveAttribute("data-issue-sub-issues", "1/2");
    await expect(panel.locator('[data-sub-issue="github:acme/repo#4"]')).toHaveAttribute("data-sub-issue-state", "open");
    await expect(panel.locator('[data-sub-issue="github:acme/repo#4"] [data-sub-issue-pr]')).toHaveAttribute("data-sub-issue-pr", "11");
    await expect(panel.locator('[data-sub-issue="github:acme/repo#9"]')).toHaveAttribute("data-sub-issue-state", "closed");
    await expect(panel.locator('[data-sub-issue="github:acme/repo#9"] [data-sub-issue-pr]')).toHaveCount(0);
    await expect(panel.locator("[data-issue-body=ready] [data-markdown-text]")).toContainText("출처를 어댑터로 나눈다.");
    await expect(panel.locator("[data-issue-comments]")).toHaveAttribute("data-issue-comments", "4");
    await expect(panel.locator("[data-issue-comment]")).toHaveCount(3);
    await expect(panel.locator("[data-issue-comment]").last()).toContainText("마지막 댓글");
    await expect(panel.locator("[data-issue-comments]")).toContainText("Write comments on GitHub");
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
    // Issue 4's first read fails: one line of why and Retry in the body's
    // place, the rest of the panel standing (B16); Retry reads it again.
    await expect(panel.locator("[data-issue-body=failed] [data-issue-body-failure]")).toBeVisible({ timeout: 20_000 });
    await expect(panel.locator('[data-issue-property="stage"]')).toHaveText("Review");
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
    await quietFor(page, 600, "the card height holds at rest");
    expect([...sent.values()].reduce((sum, count) => sum + count, 0)).toBe(quietIssues);
    // Each button says what it does (B7).
    await restOn(page, issueCard(2).locator("[data-card-workspace]"), ["Open Workspace"], issueCard(2));
    await issueCard(3).hover();
    await expect(issueCard(3).locator("[data-card-start]")).toBeVisible();
    await restOn(page, issueCard(3).locator("[data-card-start]"), ["Create a worktree and agent for this issue"], issueCard(3));
    await leaveHoverCard(page, page.getByRole("tooltip"));
    const previewBefore = sent.get("issue_detail_request") ?? 0;
    await issueCard(2).locator("[data-task-id]").hover();
    const preview = page.locator('[data-issue-preview="github:acme/repo#2"]');
    await expect(preview).toBeVisible();
    await expect(preview.locator("[data-issue-preview-body]")).toHaveText(/^배경\s+출처를 어댑터로 나눈다\.\s+GitHub/);
    await expect(preview).toContainText("example-user · Sep 20 · 4 comments");
    await expect(preview).toContainText("Open");
    await screenshot(page, "overview-issue-preview-light");
    await leaveHoverCard(page, preview);
    await issueCard(2).locator("[data-task-id]").hover();
    await expect(preview).toBeVisible();
    await leaveHoverCard(page, preview);
    expect((sent.get("issue_detail_request") ?? 0) - previewBefore).toBeLessThanOrEqual(1);

    // A line of work with no issue (B4): the worktree line's popover says
    // where it goes and names them, its click is the graph with that line open; the pull
    // request line's is the PRs tab, where each has an issue cell to link
    // (overview-lenses-prs B21).
    const looseWorktrees = column("working").locator("[data-loose-worktrees]");
    await restOn(page, looseWorktrees, ["View in the Agents graph", "prd/asking"]);
    await looseWorktrees.click();
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    await expect(emptyFold).toHaveAttribute("aria-expanded", "true");
    await tile("issues").locator("[data-lens-tile-button]").click();
    const loosePrs = column("review").locator("[data-loose-prs]");
    await restOn(page, loosePrs, ["View on the PRs tab", "#12"]);
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
    await openProjectOverview(page, "repo");
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

    // All projects keeps its tab row; its Agents view is the same graph, a
    // band per project under its name, the one with the operator's turn first (B30).
    const allProjects = page.locator("[data-home-destination]");
    await allProjects.click();
    await expect(main).toBeVisible();
    await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
    await expect(allProjects).not.toHaveAttribute("aria-current", "page");
    await expect(page.locator('[data-main-stats] [data-stat="projects"]')).toHaveText("3 projects");
    await expect(main.locator("[data-main-tab]")).toHaveCount(4);
    await expect(main).toHaveAttribute("data-main-view", "requests");
    await expect(main.locator('[data-main-tab="agents"] [data-agents-waiting]')).toBeVisible();
    await page.locator('[data-main-tab="agents"]').click();
    await expect(main.locator("[data-graph=all]")).toBeVisible();
    await expect(main.locator("[data-graph-project]").first()).toHaveText("repo");
    await expect(main.locator("[data-graph-section]", { has: page.locator("[data-graph-project]", { hasText: "repo" }) }).locator(`[data-graph-row="${askingPane}"]`)).toHaveAttribute("data-bucket", "turn");
    await expect(main.locator("[data-graph-devices]")).toHaveCount(0);
    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await atRest(page);
      await screenshot(page, `all-projects-agents-${theme}`);
    }
    // The filter applies to every project: `ASKING` keeps one row of one project (B30).
    await main.locator("[data-graph-search]").fill("asking");
    await expect(main.locator("[data-graph-row]")).toHaveCount(1);
    await main.locator("[data-graph-search]").fill("");
    await expect(main.locator("[data-graph-row]")).not.toHaveCount(1);

    // A folder's issues are Local ones kept by Hide: C makes an issue, Create and
    // start immediately goes on to the Start dialog, and the card's menu closes it.
    await page.locator('[data-main-tab="projects"]').click();
    await page.locator("[data-main-project]", { hasText: /^fixture/ }).click();
    await expect(workspace).toBeVisible();
    await page.locator("[data-go-main]").click();
    await page.getByRole("tab", { name: "fixture", exact: true }).click();
    await expect(overview).toHaveAttribute("data-overview-view", "requests");
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
    await expect(start.locator("[data-start-prompt]")).toHaveValue("Solve local issue L-1: 폴더 이슈\n\n폴더에서 할 일");
    await page.getByRole("button", { name: "Cancel" }).click();
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
    await page.keyboard.press(field("Enter"));
    await expect(editor.locator("[data-issue-editor-failure]")).toBeVisible();
    await expect(editor.locator("[data-issue-editor-body]")).toHaveValue("고친 본문");
    await expect(localCard.locator("[data-card-title]")).toHaveText("폴더 이슈");
    const updatesBefore = sent.get("local_issue_update") ?? 0;
    await editor.locator("[data-issue-editor-title]").fill("고친 폴더 이슈");
    await page.keyboard.press(field("Enter"));
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

    // A project with no agent: its main box stays, counting none
    // (B13); New agent on a folder opens its Workspace.
    await page.locator("[data-go-main]").click();
    await page.locator('[data-main-tab="projects"]').click();
    await page.locator("[data-main-project]", { hasText: /^quiet/ }).click();
    await expect(workspace).toBeVisible();
    await page.locator("[data-go-main]").click();
    await page.getByRole("tab", { name: "quiet", exact: true }).click();
    // The request view of a project with no agent is one line and New agent (overview-request-view B40).
    await expect(overview).toHaveAttribute("data-overview-state", "empty");
    await expect(overview.locator("[data-requests-empty]")).toBeVisible();
    await expect(overview.locator("[data-requests-new-agent]")).toBeVisible();
    await expect(tile("requests").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "0");
    await toGraph();
    await expect(overview).toHaveAttribute("data-overview-state", "empty");
    await expect(boxes).toHaveCount(1);
    await expect(boxes.first().locator("[data-graph-head-agents]")).toHaveText("0 agents");
    await expect(boxes.first().locator("[data-graph-row]")).toHaveCount(0);
    await expect(tile("agents").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "0");
    await expect(tile("agents").locator("[data-lens-tile-badge]")).toHaveCount(0);
    await atRest(page);
    await screenshot(page, "overview-empty-light");
    await page.locator("[data-overview-new-agent]").click();
    await expect(workspace).toBeVisible();

    // New agent on a Git project opens the New worktree flow; Cancel keeps the Overview (B9).
    await openProjectOverview(page, "repo");
    await page.locator("[data-overview-new-agent]").click();
    await expect(page.locator("[data-new-worktree]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-new-worktree]")).toHaveCount(0);
    await expect(overview).toBeVisible();

    // Settings › Issues: the sources, each project's source, and how work starts.
    await page.keyboard.press(chord("settings"));
    await page.locator('[data-settings-tab="issues"]').click();
    await expect(page.locator("[data-settings-issues]")).toBeVisible();
    await expect(page.getByRole("combobox", { name: "Issue source for repo" })).toHaveText("Automatic (GitHub)");
    await expect(page.getByRole("combobox", { name: "Issue source for quiet" })).toHaveText("Automatic (Local)");
    await page.keyboard.press("Escape");

    // Start on the backlog issue makes a worktree linked to it, so the card
    // moves from Backlog to In progress; its Workspace comes to the front,
    // and ⌘⇧H selects its box.
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
    await page.locator("[data-go-main]").click();
    await page.getByRole("tab", { name: "repo", exact: true }).click();
    await toGraph();
    // A fresh agent only rests, so its box is folded away: main's box carries the selection until the fold is opened (B1).
    await expect(box("3-graph-view")).toHaveCount(0);
    await expect(overview.locator("[data-graph-box][data-selected]")).toHaveAttribute("data-graph-primary", "true");
    await overview.locator('[data-graph-fold="resting"]').click();
    await expect(box("3-graph-view").locator("[data-graph-row]")).toHaveCount(1);
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
