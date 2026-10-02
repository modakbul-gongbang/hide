// ⌘K as a navigation palette (PRD cmdk-navigation) on an isolated pinned
// Herdr and hided. Three flows:
//
// - a Workspace with no GitHub project: an empty ⌘K lists what is connected
//   to the agent in front, a name finds an agent and Enter goes there, `에이전트
//   시작…` is the one command left and opens the start panel, and nothing is
//   asked of GitHub (B2, B4, B20-B22, B16, B27);
// - a project a fake `gh` answers for: the issue, checkout, pull request and
//   agent connected to the agent in front, `#273` finding the issue and the
//   pull request numbered so, and the explicit GitHub search row with its
//   working, failed and empty answers, never run while typing (B2, B12, B16-B20);
// - ⌘P: ⌘↵ opens the highlighted file beside the area in use (B23);
// - Recent (PRD cmdk-recent): the checkouts last brought to the front, under
//   Related, kept across a daemon restart and shown alone on Settings.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { labelAgent, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, registerFolder, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

const input = (page: Page) => page.locator('[data-palette="Search"] [data-palette-input]');
const rows = (page: Page) => page.locator('[data-palette="Search"] [data-palette-row]');
const rowIds = (page: Page) => rows(page).evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-palette-row") ?? ""));

/** Opens ⌘K with the query field focused. */
async function openSearch(page: Page): Promise<void> {
  await page.keyboard.press("Meta+KeyK");
  await expect(input(page)).toBeFocused();
}

/** Gives the keyboard to one agent pane, which is what ⌘K then relates to. */
async function focusAgent(page: Page, pane: string): Promise<void> {
  await page.locator(`[data-pane-view="${pane}"] .xterm-helper-textarea`).focus();
  await expect(page.locator(`[data-pane-view="${pane}"]`)).toHaveAttribute("data-focused", "true");
}

test("⌘K lists what is connected to the agent in front, goes to an agent by name, keeps only 에이전트 시작…, and asks GitHub nothing", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [one, two] = herdr.panes;
    daemon = await startHided(herdr, "cmdk-names");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);

    // B2, B4: empty, the Related list is the checkout and the agent in front,
    // marked `여기`; the other agent of the checkout is not in its lineage, so
    // it is not listed. There is no command row.
    await focusAgent(page, two);
    await openSearch(page);
    await expect(page.locator('[data-palette-group="related"]')).toBeVisible();
    await expect(page.locator('[data-palette="Search"] [cmdk-group-heading]')).toHaveText(["Related"]);
    await expect.poll(() => rowIds(page)).toEqual([expect.stringMatching(/^checkout:/), `agent:${two}`]);
    const here = page.locator(`[data-palette-row="agent:${two}"]`);
    await expect(here.locator("[data-palette-here]")).toHaveText("여기");
    await expect(page.locator(`[data-palette-row="agent:${one}"]`)).toHaveCount(0);
    await expect(page.locator('[data-palette-row^="command:"]')).toHaveCount(0);

    // B21: Enter on `여기` goes nowhere; the detail says so and the palette stays.
    const focuses = sent.get("focus_pane") ?? 0;
    await here.hover();
    await expect(here).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-palette-detail-pane]")).toContainText("지금 보고 있는 에이전트");
    await expect(input(page)).toBeVisible();
    expect(sent.get("focus_pane") ?? 0).toBe(focuses);
    await page.keyboard.press("Escape");
    await expect(input(page)).toHaveCount(0);

    // B20: a name finds an agent and Enter goes to its pane, closing the palette.
    await openSearch(page);
    await page.keyboard.type("Agent one");
    await expect.poll(() => rowIds(page).then((ids) => ids[0])).toBe(`agent:${one}`);
    await page.keyboard.press("Enter");
    await expect(input(page)).toHaveCount(0);
    await expect.poll(() => last.get("focus_pane")?.pane_id).toBe(one);
    // Reopening ⌘K at once must keep every key: the pane's focus lands when the
    // core answers, and it may not take the keyboard from the palette (CI once
    // dropped the first letters of the next query to it).
    // B22: a command that ⌘K used to carry is not found by its name.
    await openSearch(page);
    await page.keyboard.type("Split right");
    await expect(input(page)).toHaveValue("Split right");
    await expect(page.locator('[data-palette-state="no-match"]')).toHaveText("일치하는 항목 없음");
    await expect(page.locator('[data-palette-row^="command:"]')).toHaveCount(0);
    // B16: no GitHub project on this Mac, so no GitHub search row to offer.
    await expect(page.locator('[data-palette-row="github-search"]')).toHaveCount(0);

    // B22: `에이전트 시작…` is the one command, and it opens the start panel.
    await input(page).fill("에이전트 시작");
    await expect.poll(() => rowIds(page)).toEqual(["command:start-agent"]);
    await expect(page.locator('[data-palette-group="commands"]')).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(input(page)).toHaveCount(0);
    // The text box's focus is start-panel.spec's claim (B33).
    await expect(page.locator("[data-start-panel]")).toBeVisible();

    // B27: this Workspace is not a Git project, so nothing was asked of GitHub.
    expect(sent.get("github_request") ?? 0).toBe(0);
    expect(sent.get("github_search") ?? 0).toBe(0);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

const BRANCH = "prd/cmdk-nav";
const SEARCHED_PR = "https://github.com/acme/repo/pull/90";

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: "ignore" });
}

/**
 * A `gh` that is logged in for `acme/repo` and knows: pull request 180 on
 * `BRANCH`, which closes issue 5; an issue and a pull request both numbered
 * 273 and a pull request whose title only holds 273. `gh search` answers a
 * held pull request again, an old one and a closed issue; a query holding
 * `fail` fails and one holding `nothing` finds nothing. Every call is logged.
 */
function fakeGh(root: string): { bin: string; log: string } {
  const bin = path.join(root, "gh-bin");
  const log = path.join(root, "gh-calls.log");
  fs.mkdirSync(bin, { recursive: true });
  const passing = [{ __typename: "CheckRun", name: "verify", status: "COMPLETED", conclusion: "SUCCESS" }];
  const pull = (number: number, title: string, branch: string, closes: number[]) => ({
    number,
    title,
    statusCheckRollup: passing,
    headRefName: branch,
    baseRefName: "main",
    state: "OPEN",
    reviewDecision: "REVIEW_REQUIRED",
    isDraft: false,
    url: `https://github.com/acme/repo/pull/${number}`,
    mergedAt: null,
    updatedAt: "2026-10-01T00:00:00Z",
    closingIssuesReferences: closes.map((issue) => ({ url: `https://github.com/acme/repo/issues/${issue}` })),
  });
  const issue = (number: number, title: string) => ({ number, title, url: `https://github.com/acme/repo/issues/${number}`, state: "OPEN", projectItems: [], updatedAt: "2026-10-01T00:00:00Z" });
  const pulls = [pull(180, "⌘K를 이동 팔레트로", BRANCH, [5]), pull(273, "번호로 찾는 PR", "prd/by-number", []), pull(274, "273 이후의 정리", "prd/after", [])];
  const issues = [issue(5, "팔레트 이동 이슈"), issue(273, "번호로 찾는 이슈")];
  const hit = (kind: string, number: number, title: string, state: string, url: string) => ({ number, title, state, url, isDraft: false, repository: { name: "repo", nameWithOwner: "acme/repo" }, kind });
  const searchPrs = [hit("pr", 90, "오래된 PR", "merged", SEARCHED_PR), hit("pr", 180, "⌘K를 이동 팔레트로", "open", "https://github.com/acme/repo/pull/180")];
  const searchIssues = [hit("issue", 91, "닫힌 이슈", "closed", "https://github.com/acme/repo/issues/91")];
  fs.writeFileSync(
    path.join(bin, "gh"),
    `#!/bin/sh
echo "$*" >> '${log}'
case "$1 $2" in
  "auth status") exit 0 ;;
  "repo view") echo '{"nameWithOwner":"acme/repo"}'; exit 0 ;;
  "issue list") echo '${JSON.stringify(issues)}'; exit 0 ;;
  "pr list")
    case " $* " in
      *" merged "*) echo '[]' ;;
      *) echo '${JSON.stringify(pulls)}' ;;
    esac
    exit 0 ;;
  "search prs"|"search issues")
    # The query is the words after the double dash.
    case " $* " in
      *" fail "*) echo "boom" >&2; exit 1 ;;
      *" nothing "*) echo '[]'; exit 0 ;;
    esac
    if [ "$2" = "prs" ]; then echo '${JSON.stringify(searchPrs)}'; else echo '${JSON.stringify(searchIssues)}'; fi
    exit 0 ;;
  "api graphql") echo '{"data":{"r0":{"nameWithOwner":"acme/repo"}}}'; exit 0 ;;
esac
echo "unsupported: $*" >&2
exit 1
`,
    { mode: 0o755 },
  );
  return { bin, log };
}

/** The `gh search` calls logged so far for pull requests and for issues. */
function searches(log: string): { prs: number; issues: number } {
  const lines = fs.existsSync(log) ? fs.readFileSync(log, "utf8").split("\n") : [];
  return { prs: lines.filter((line) => line.startsWith("search prs ")).length, issues: lines.filter((line) => line.startsWith("search issues ")).length };
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

/** A Herdr workspace at `cwd` with a fake `claude` agent titled `task`; its pane. */
async function workspaceAt(herdr: HerdrFixture, cwd: string, task: string): Promise<string> {
  const created = herdr.run(["workspace", "create", "--cwd", cwd, "--label", path.basename(cwd), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as {
    result: { root_pane: { pane_id: string } };
  };
  const pane = created.result.root_pane.pane_id;
  await prompt(herdr, pane);
  herdr.run(["agent", "start", `agent-${path.basename(cwd)}`, "--kind", "claude", "--pane", pane]);
  labelAgent(herdr, pane, { task });
  return pane;
}

test("⌘K relates the agent in front to its issue and pull request, finds them by #number, and searches GitHub only from its own row", async ({ page, context }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    const worktree = path.join(herdr.root, "repo-nav");
    git(repo, ["worktree", "add", "-b", BRANCH, worktree]);
    git(worktree, ["commit", "--allow-empty", "-m", "navigation"]);
    await workspaceAt(herdr, repo, "메인 체크아웃 정리");
    const agent = await workspaceAt(herdr, worktree, "이동 팔레트 구현");

    const gh = fakeGh(herdr.root);
    daemon = await startHided(herdr, "cmdk-github", undefined, { PATH: `${gh.bin}:${herdr.fixturePath}` });
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();
    await page.locator(`[data-checkout][aria-label^="${BRANCH}"]`).first().click();
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await focusAgent(page, agent);

    // B2: the issue the pull request closes on top, the checkout's group head,
    // its pull request, then the agent in front. ⌘K read the project once if
    // nothing had, so the issue and the pull request arrive after the open.
    await openSearch(page);
    await expect(page.locator('[data-palette="Search"] [cmdk-group-heading]')).toHaveText(["Related"]);
    await expect
      .poll(() => rowIds(page), { timeout: 30_000 })
      .toEqual([expect.stringMatching(/^issue:/), expect.stringMatching(/^checkout:/), expect.stringMatching(/^pr:.*:180$/), `agent:${agent}`]);
    await expect(rows(page).first()).toContainText("팔레트 이동 이슈");
    await expect(page.locator('[data-palette-row^="pr:"]')).toContainText("⌘K를 이동 팔레트로");
    await expect(page.locator('[data-palette-row^="pr:"]')).toContainText("Open");
    await expect(page.locator(`[data-palette-row="agent:${agent}"] [data-palette-here]`)).toBeVisible();
    await page.keyboard.press("Escape");
    // Opening ⌘K again, and typing in it, reads no project again (B10, B27).
    const reads = sent.get("github_request") ?? 0;
    await openSearch(page);
    await page.keyboard.type("repo");
    await expect(rows(page).first()).toBeVisible();
    await page.keyboard.press("Escape");
    expect(sent.get("github_request") ?? 0).toBe(reads);

    // B12: `#273` puts the issue and the pull request numbered so first, each
    // its own row, above the pull request whose title only holds 273.
    await openSearch(page);
    await page.keyboard.type("#273");
    await expect.poll(() => rowIds(page)).toEqual([expect.stringMatching(/^issue:.*273$/), expect.stringMatching(/^pr:.*:273$/), expect.stringMatching(/^pr:.*:274$/), "github-search"]);
    await expect(page.locator('[data-palette="Search"] [cmdk-group-heading]')).toHaveText(["Issues", "Pull requests"]);
    // B20: Enter on the pull request opens it in its Project's Overview.
    await page.keyboard.press("ArrowDown");
    await expect(rows(page).nth(1)).toHaveAttribute("aria-selected", "true");
    await expect(page.locator('[data-palette-detail-pane="pr"]')).toContainText("번호로 찾는 PR");
    await page.keyboard.press("Enter");
    await expect(input(page)).toHaveCount(0);
    await expect(page.locator("[data-overview-screen]")).toBeVisible();

    // B16: a query nothing holds says so, and the last row offers the GitHub
    // search. Typing called GitHub no more than reading the project did.
    const before = searches(gh.log);
    await openSearch(page);
    await page.keyboard.type("zzz-old");
    await expect(page.locator('[data-palette-state="no-match"]')).toHaveText("일치하는 항목 없음");
    const github = page.locator('[data-palette-row="github-search"]');
    await expect(github).toHaveText(/GitHub에서 "zzz-old" 검색/);
    await expect(github).toHaveAttribute("data-github-state", "idle");
    expect(searches(gh.log)).toEqual(before);
    expect(sent.get("github_search") ?? 0).toBe(0);

    // B17: choosing it searches once; the answer is the GitHub group, and the
    // held pull request 180 is not listed twice.
    await page.keyboard.press("Enter");
    const group = page.locator('[data-palette-group="github"]');
    await expect.poll(() => group.locator("[data-palette-row]").count(), { timeout: 30_000 }).toBe(2);
    expect(await group.locator("[data-palette-row]").evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-palette-row")))).toEqual(["github:pr:acme/repo#90", "github:issue:acme/repo#91"]);
    expect(sent.get("github_search")).toBe(1);
    expect(searches(gh.log)).toEqual({ prs: before.prs + 1, issues: before.issues + 1 });
    await expect(page.locator('[data-palette-state="no-match"]')).toHaveCount(0);

    // B19: a changed query drops the answer, and typing calls GitHub no more.
    await page.keyboard.type("x");
    await expect(group).toHaveCount(0);
    await expect(github).toHaveAttribute("data-github-state", "idle");
    expect(searches(gh.log)).toEqual({ prs: before.prs + 1, issues: before.issues + 1 });

    // B20: a GitHub result opens on GitHub, as a ⌘-click does, and closes the palette.
    await context.route("https://github.com/**", (route) => route.fulfill({ contentType: "text/html", body: "<title>GitHub</title>" }));
    await input(page).fill("zzz-old");
    await github.click();
    await expect(group.locator("[data-palette-row]")).toHaveCount(2, { timeout: 30_000 });
    const opened = context.waitForEvent("page");
    await page.locator('[data-palette-row="github:pr:acme/repo#90"]').click();
    expect((await opened).url()).toBe(SEARCHED_PR);
    await expect(input(page)).toHaveCount(0);

    // B18: a failed search says so on its row and a second pick runs it again;
    // a search that finds nothing says `GitHub에도 없음`.
    await openSearch(page);
    await page.keyboard.type("will fail");
    await expect(github).toHaveAttribute("data-github-state", "idle");
    const beforeFail = searches(gh.log).prs;
    await page.keyboard.press("Enter");
    await expect(github).toHaveAttribute("data-github-state", "failed", { timeout: 30_000 });
    await expect(github).toContainText("GitHub 검색 실패 · 다시 시도");
    await page.keyboard.press("Enter");
    await expect.poll(() => searches(gh.log).prs).toBe(beforeFail + 2);
    await expect(github).toHaveAttribute("data-github-state", "failed", { timeout: 30_000 });
    await input(page).fill("nothing here");
    await page.keyboard.press("Enter");
    await expect(github).toHaveAttribute("data-github-state", "none", { timeout: 30_000 });
    await expect(github).toContainText("GitHub에도 없음");
    await page.keyboard.press("Escape");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("⌘P then ⌘↵ opens the highlighted file beside the area in use, and ↵ alone opens it in the preview", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const root = path.join(herdr.root, "fixture");
    const git = (...args: string[]) => execFileSync("git", ["-C", root, ...args], { env: herdr.env });
    fs.writeFileSync(path.join(root, "first.txt"), "first\n");
    fs.writeFileSync(path.join(root, "second.txt"), "second\n");
    git("init", "-q");
    git("add", ".");
    git("-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-qm", "Initial files");
    daemon = await startHided(herdr, "cmdk-beside");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");

    // ↵ alone opens the pick in the checkout's preview tab.
    await page.keyboard.press("Meta+KeyP");
    const files = page.locator('[data-palette="Open file"] [data-palette-input]');
    await expect(files).toBeFocused();
    await expect(page.locator('[data-palette-hint="beside"]')).toHaveText("⌘↵ 옆에 열기");
    await page.keyboard.type("first.txt");
    await expect(page.locator('[data-palette-row$="/first.txt"]')).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("Enter");
    await expect(files).toHaveCount(0);
    await expect(page.locator('[data-view-tab-bar] [role="tab"]')).toHaveCount(1);
    expect(last.get("file_open")).toMatchObject({ preview: true });
    expect(last.get("file_open")).not.toHaveProperty("beside");
    const opens = sent.get("file_open") ?? 0;

    // ⌘↵ opens the highlighted row beside, pinned: one event, a second area.
    await page.keyboard.press("Meta+KeyP");
    await expect(files).toBeFocused();
    await page.keyboard.type("second.txt");
    const second = page.locator('[data-palette-row$="/second.txt"]');
    await expect(second).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("Meta+Enter");
    await expect(files).toHaveCount(0);
    await expect.poll(() => sent.get("file_open")).toBe(opens + 1);
    expect(last.get("file_open")).toMatchObject({ beside: true, preview: false });
    await expect(page.locator("[data-view-area-id]")).toHaveCount(2);
    await expect(page.locator('[data-view-tab-bar] [role="tab"]')).toHaveCount(2);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("⌘K lists the checkouts last brought to the front under Recent, keeps them across a restart, shows them alone on Settings, and goes back with Enter", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "cmdk-recent");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const fixture = await page.locator("[data-project] [data-checkout]").first().getAttribute("data-checkout");
    expect(fixture).toBeTruthy();

    // B6: a second project's checkout brought to the front goes to the top of the list.
    await registerFolder(page, daemon, `${daemon.home}/projects/alpha`);
    await expect(page.locator("[data-project]")).toHaveCount(2, { timeout: 20_000 });
    await page.locator("[data-project]", { hasText: "alpha" }).locator("[data-checkout]").first().click();
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();

    // B1, B4: Recent follows Related and holds the checkout left behind, not the one in front.
    await openSearch(page);
    await expect(page.locator('[data-palette="Search"] [cmdk-group-heading]')).toHaveText(["Related", "Recent"]);
    const recent = page.locator('[data-palette-group="recent"] [data-palette-row]');
    await expect(recent).toHaveCount(1);
    await expect(recent).toHaveAttribute("data-palette-row", `checkout:${fixture}`);
    await expect(recent.locator("[data-palette-enter]")).toBeAttached();
    await screenshot(page, "cmdk-recent-related");

    // B11: a typed query hides Recent, and clearing it brings it back.
    await page.keyboard.type("alp");
    await expect(page.locator('[data-palette-group="recent"]')).toHaveCount(0);
    await page.keyboard.press("ControlOrMeta+KeyA");
    await page.keyboard.press("Backspace");
    await expect(recent).toHaveCount(1);
    await page.keyboard.press("Escape");

    // B7: the list is on disk, so the first ⌘K after a restart still has it; B5: on Settings it stands alone.
    // B9: a record of a device that cannot be reached stays, dimmed; the test registers one whose address does not resolve.
    daemon = await daemon.restart((stateDir) => {
      const file = path.join(stateDir, "core-state.json");
      const stored = JSON.parse(fs.readFileSync(file, "utf8")) as { device_registrations: unknown[]; recent_checkouts: unknown[] };
      stored.device_registrations.push({ id: "ghost", label: "ghost-box", ssh_alias: "hide-e2e-unreachable.invalid", herdr_socket_path: null, host_consent: null });
      stored.recent_checkouts.push({ device_id: "ghost", checkout_id: "remote:ghost:checkout:abc", project_name: "api", branch: "release", device_name: "ghost-box" });
      fs.writeFileSync(file, JSON.stringify(stored));
    });
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-workspace-screen]")).toBeVisible({ timeout: 30_000 });
    await page.keyboard.press("Alt+Comma");
    await expect(page.locator("[data-settings]")).toBeVisible();
    await page.keyboard.press("Meta+KeyK");
    await expect(input(page)).toBeFocused();
    await expect(page.locator('[data-palette="Search"] [cmdk-group-heading]')).toHaveText(["Recent"]);
    await expect(page.locator('[data-palette-group="recent"] [data-palette-row]')).toHaveCount(3);
    const ghost = page.locator('[data-palette-row="checkout:remote:ghost:checkout:abc"]');
    await expect(ghost).toHaveAttribute("data-palette-dim", "true");
    await expect(ghost).toContainText("release");
    await expect(ghost).toContainText("ghost-box");
    await expect(ghost.locator("[data-palette-enter]")).toHaveCount(0);
    await screenshot(page, "cmdk-recent-settings");

    // B9, B12: arrows pass over the dimmed row, and Enter on it changes nothing and closes nothing.
    await ghost.hover();
    await expect(ghost).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("[data-palette-action]")).toHaveCount(0);
    await page.keyboard.press("Enter");
    await expect(input(page)).toBeVisible();
    await expect(ghost).toHaveAttribute("aria-selected", "true");

    // B3: Enter on a Recent row opens that checkout.
    await page.locator(`[data-palette-row="checkout:${fixture}"]`).click();
    await expect(input(page)).toHaveCount(0);
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await expect(page.locator(`[data-checkout="${fixture}"]`)).toBeVisible();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
