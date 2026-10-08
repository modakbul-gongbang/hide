import { openProjectOverview } from "./overview-entry";
// The link record on an isolated pinned Herdr and hided (PRD link-graph D-34):
// Claude Code session files under the daemon's HOME, one that printed the
// pull request's address when GitHub made it and two that worked on its
// branch afterwards, and a fake `gh` that lists the pull request. With no
// pane open on any of them, the PR panel shows the three sessions, the maker
// with its `Created PR` chip (B6, B7, B32); a line's `View conversation` opens
// the Sessions tab at that session (B11); the Sessions row carries the PR
// chip back to the panel (B36); `Resume` starts that session in the branch's
// worktree (B12); and a session whose file is deleted stays as a dim line
// whose Resume says why (B15). Light and Dark captures land in
// HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { agentsIn, claudeProjects, startHerdr } from "./herdr-fixture";
import { PI_ID, preparePiWriter } from "./session-reader-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { fixtureProgram } from "./platform-fixture";
import { countSent, screenshot } from "./wire";
import { chord } from "./chords";
import { animationsFinished } from "./wait";

test.describe.configure({ timeout: 240_000 });

function git(cwd: string, args: string[]): string {
  return execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, encoding: "utf8" });
}

const HOUR = 3_600_000;
const CREATED = Date.now() - 2 * HOUR;
const BRANCH = "prd/link";

/** A `gh` logged in for `acme/repo` that lists one open pull request on `BRANCH`, made at `CREATED`. */
function fakeGh(root: string): string {
  const bin = path.join(root, "gh-bin");
  const pulls = [
    {
      number: 31,
      title: "연결 기록 패널을 PR 옆에 붙입니다",
      statusCheckRollup: [{ __typename: "CheckRun", name: "verify", status: "COMPLETED", conclusion: "SUCCESS" }],
      headRefName: BRANCH,
      headRefOid: "1".repeat(40),
      isCrossRepository: false,
      baseRefName: "main",
      state: "OPEN",
      reviewDecision: "REVIEW_REQUIRED",
      isDraft: false,
      url: "https://github.com/acme/repo/pull/31",
      mergedAt: null,
      closedAt: null,
      createdAt: new Date(CREATED).toISOString(),
      updatedAt: new Date(CREATED + HOUR).toISOString(),
      closingIssuesReferences: [],
    },
  ];
  fixtureProgram(
    bin,
    "gh",
    `const args = process.argv.slice(2);
const out = (value) => process.stdout.write(JSON.stringify(value) + "\\n");
const [a, b] = args;
if (a === "auth" && b === "status") process.exit(0);
if (a === "repo" && b === "view") { out({ nameWithOwner: "acme/repo", id: "R_fixture" }); process.exit(0); }
if (a === "pr" && b === "list") { out(args.includes("merged") ? [] : ${JSON.stringify(pulls)}); process.exit(0); }
if (a === "issue" && b === "list") { out([]); process.exit(0); }
if (a === "api" && b === "graphql") { out({ data: {} }); process.exit(0); }
process.stderr.write("unsupported: " + args.join(" ") + "\\n");
process.exit(1);
`,
  );
  return bin;
}

const at = (ms: number) => new Date(ms).toISOString();

/** A person's turn as Claude Code writes it, on `BRANCH` in `cwd`. */
function request(id: string, cwd: string, ms: number, text: string, uuid: string): string {
  return JSON.stringify({
    type: "user", isSidechain: false, uuid, parentUuid: null, message: { role: "user", content: text }, timestamp: at(ms),
    promptId: `p-${uuid}`, origin: { kind: "human" }, userType: "external", entrypoint: "cli", cwd, sessionId: id, gitBranch: BRANCH,
  });
}

function answer(id: string, cwd: string, ms: number, text: string, uuid: string, parent: string): string {
  return JSON.stringify({ type: "assistant", isSidechain: false, uuid, parentUuid: parent, message: { role: "assistant", content: [{ type: "text", text }] }, timestamp: at(ms), cwd, sessionId: id, gitBranch: BRANCH });
}

/** The three sessions, as files under the fixture's Claude projects folder. Returns each file by session id. */
function writeSessions(projects: string, cwd: string): Record<string, string> {
  const folder = path.join(projects, "-repo-link");
  fs.mkdirSync(folder, { recursive: true });
  const file = (id: string) => path.join(folder, `${id}.jsonl`);
  const write = (id: string, lines: string[]) => fs.writeFileSync(file(id), lines.join("\n") + "\n");
  write("s-maker", [
    request("s-maker", cwd, CREATED - 30 * 60_000, "연결 기록 패널 만들어 줘", "m1"),
    answer("s-maker", cwd, CREATED - 20 * 60_000, "패널을 만들었습니다.", "m2", "m1"),
    request("s-maker", cwd, CREATED - 60_000, "검증 끝났으면 PR 올려 줘", "m3"),
    JSON.stringify({ type: "pr-link", sessionId: "s-maker", prNumber: 31, prRepository: "acme/repo", prUrl: "https://github.com/acme/repo/pull/31", timestamp: at(CREATED + 1_000) }),
  ]);
  write("s-review", [request("s-review", cwd, CREATED + 30 * 60_000, "리뷰 코멘트 두 개 반영해 줘", "r1"), answer("s-review", cwd, CREATED + 40 * 60_000, "반영했습니다.", "r2", "r1")]);
  write("s-gone", [request("s-gone", cwd, CREATED + 50 * 60_000, "e2e 스냅샷 다시 찍어 줘", "g1")]);
  return { "s-maker": file("s-maker"), "s-review": file("s-review"), "s-gone": file("s-gone") };
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press(chord("settings"));
  await page.locator('[data-settings-tab="general"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  await animationsFinished(page);
}

test("a pull request's panel shows the sessions that made it and worked on it after their panes closed", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    fs.writeFileSync(path.join(herdr.root, "home", ".zshenv"), `export PATH="${path.join(herdr.root, "bin")}:$PATH"\n`);
    const repo = path.join(fs.realpathSync(herdr.root), "repo");
    const tree = path.join(fs.realpathSync(herdr.root), "repo-link");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    git(repo, ["worktree", "add", "-b", BRANCH, tree]);
    const files = writeSessions(claudeProjects(herdr), tree);
    herdr.run(["workspace", "create", "--cwd", repo, "--label", "repo", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);

    const gh = fakeGh(herdr.root);
    daemon = await startHided(herdr, "link-graph", undefined, { PATH: `${gh}${path.delimiter}${herdr.fixturePath}` });
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
    await chooseTheme(page, "light");
    await page.locator('[data-sidebar-mode="projects"]').click();
    await openProjectOverview(page, "repo");
    const overview = page.locator("[data-overview-screen]");
    await overview.locator('[data-lens-tile-button="prs"]').click();

    // The panel: the maker first among the ended lines is the newest-first
    // order the record keeps, each with its request; the maker's line names
    // the request just before the address and carries `Created PR` (B6, B7).
    await overview.locator('[data-pr-row="31"]').click({ timeout: 30_000 });
    const panel = overview.locator('[data-pr-panel="31"]');
    await expect(panel).toBeVisible();
    const lines = panel.locator("[data-link-session]");
    await expect(lines).toHaveCount(3, { timeout: 60_000 });
    expect(await lines.evaluateAll((items) => items.map((item) => item.getAttribute("data-link-session")))).toEqual(["s-gone", "s-review", "s-maker"]);
    await expect(panel.locator("[data-link-sessions]")).toHaveAttribute("data-link-sessions", "3");
    const maker = panel.locator('[data-link-session="s-maker"]');
    await expect(maker).toHaveAttribute("data-link-role", "created");
    await expect(maker.locator('[data-link-request="s-maker"]')).toHaveText("검증 끝났으면 PR 올려 줘");
    await expect(maker.locator('[data-link-chip="created"]')).toBeVisible();
    await expect(panel.locator('[data-link-session="s-review"] [data-link-chip="worked"]')).toBeVisible();
    await expect(panel.locator(`[data-pr-panel-worktree="${tree}"]`)).not.toHaveAttribute("data-removed", "true");
    await expect(panel.locator("[data-link-loading]")).toHaveCount(0, { timeout: 60_000 });
    await page.mouse.move(2, 998);
    await screenshot(page, "link-graph-panel-light");

    // A line's buttons stand under it on the pointer; View conversation opens
    // the Sessions tab at that session (B10, B11).
    await maker.hover();
    await expect(maker.locator('[data-link-button="view"]')).toBeVisible();
    await expect(maker.locator('[data-link-button="resume"]')).toBeVisible();
    await maker.locator('[data-link-button="view"]').click();
    await expect(overview).toHaveAttribute("data-overview-view", "sessions");
    await expect(page.locator('[data-session-header="s-maker"]')).toBeVisible({ timeout: 30_000 });
    await expect(page.locator("[data-session-turns]")).toContainText("검증 끝났으면 PR 올려 줘");

    // The session's row and head carry its PR chip, which opens the panel again (B36).
    const chip = page.locator('[data-session="s-maker"] [data-session-pr="31"]');
    await expect(chip).toHaveAttribute("data-session-pr-created", "true", { timeout: 30_000 });
    await expect(page.locator('[data-session-header="s-maker"] [data-session-pr="31"]')).toBeVisible();
    await expect(page.locator('[data-session="s-review"] [data-session-pr="31"]')).not.toHaveAttribute("data-session-pr-created", "true");
    await chip.click();
    await expect(overview).toHaveAttribute("data-overview-view", "prs");
    await expect(panel).toBeVisible();

    // A session whose file is gone stays as a dim line, and Resume says why (B15);
    // the panel reads the record again when it opens.
    fs.rmSync(files["s-gone"]);
    await page.keyboard.press("Escape");
    await expect(panel).toHaveCount(0);
    await overview.locator('[data-pr-row="31"]').click();
    const gone = panel.locator('[data-link-session="s-gone"]');
    await expect(gone).toHaveAttribute("data-link-file", "missing", { timeout: 30_000 });
    await expect(gone.locator('[data-link-missing="s-gone"]')).toBeVisible();
    await gone.hover();
    await expect(gone.locator('[data-link-button="resume"]')).toHaveAttribute("data-link-blocked", "links.why.file");
    await expect(gone.locator('[data-link-button="view"]')).toHaveAttribute("aria-disabled", "true");

    await chooseTheme(page, "dark");
    await page.mouse.move(2, 998);
    await screenshot(page, "link-graph-panel-dark");

    // Resume starts that session in the branch's worktree, once (B12).
    await maker.hover();
    await maker.locator('[data-link-button="resume"]').click();
    await expect(page.locator("[data-workspace-screen]")).toBeVisible({ timeout: 60_000 });
    expect(sent.get("agent_start_in_checkout")).toBe(1);
    expect(last.get("agent_start_in_checkout")).toMatchObject({ checkout_path: tree, provider: "claude", resume_session_id: "s-maker" });
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

// B5/B8: the selected native file reaches the actual queued resume worker.
test("a Pi archive resume keeps its selected source and uses the current control", async ({ page }) => {
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    fs.writeFileSync(path.join(herdr.root, "home", ".zshenv"), `export PATH="${path.join(herdr.root, "bin")}:$PATH"\n`);
    const repo = path.join(fs.realpathSync(herdr.root), "repo");
    const tree = path.join(fs.realpathSync(herdr.root), "repo-link");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    git(repo, ["worktree", "add", "-b", BRANCH, tree]);
    const session = preparePiWriter(herdr, tree);
    const source = fs.readFileSync(path.join(herdr.root, "pi-session-seed.jsonl"), "utf8") + `${JSON.stringify({
      type: "message", id: "pr-tool-result", timestamp: at(CREATED + 1_000),
      message: { role: "toolResult", toolCallId: "pr-tool", toolName: "bash", isError: false,
        content: [{ type: "text", text: "https://github.com/acme/repo/pull/31" }], timestamp: CREATED + 1_000 },
    })}\n`;
    fs.writeFileSync(session, source);
    herdr.run(["workspace", "create", "--cwd", repo, "--label", "repo", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);
    const gh = fakeGh(herdr.root);
    daemon = await startHided(herdr, "pi-archive", herdr.env.HOME, { PATH: `${gh}${path.delimiter}${herdr.fixturePath}` });
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await page.locator('[data-sidebar-mode="projects"]').click();
    await openProjectOverview(page, "repo");
    const overview = page.locator("[data-overview-screen]");
    await overview.locator('[data-lens-tile-button="prs"]').click();
    await overview.locator('[data-pr-row="31"]').click({ timeout: 30_000 });
    const archived = page.locator(`[data-pr-panel="31"] [data-link-session="${PI_ID}"]`);
    await expect(archived).toBeVisible({ timeout: 60_000 });
    await archived.hover();
    await archived.locator('[data-link-button="resume"]').click();
    await expect.poll(() => agentsIn(herdr, tree), { timeout: 60_000 }).toContain("pi");
    await expect.poll(() => fs.existsSync(path.join(herdr.root, "pi-launches.jsonl"))).toBe(true);
    const launches = fs.readFileSync(path.join(herdr.root, "pi-launches.jsonl"), "utf8").trim().split("\n").map(line => JSON.parse(line) as string[]);
    expect(launches).toEqual([["--session", PI_ID]]);
    expect(fs.readFileSync(session, "utf8")).toBe(source);
    await screenshot(page, "pi-archive-resume");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
