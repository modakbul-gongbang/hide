// The PRs tab of a Project's Overview on an isolated pinned Herdr and hided
// (PRD overview-lenses-prs): a Git project whose pull requests a fake `gh`
// answers and records every write of, so nothing reaches GitHub. The tile
// counts the open ones and the tab groups them by whose move it is (B1, B2);
// it is a skeleton until GitHub answers (B22); rows keep still under the
// pointer and hovering publishes nothing (B3-B8, B24); the keyboard unfolds
// and moves (B5). An issue is linked with one confirmation that writes
// `Closes #N` into the body (B9, B10); a new issue made from a pull request
// keeps the issue when the body write fails and writes only the body again
// (B12, B13); a pull request whose checks failed is handed to an agent in a
// worktree of its branch, fetched from origin (B15, B16), with the feedback
// read failing once (B18); merged ones fold and offer 정리 (B19, B20); and
// the PR chips and the sidebar's PR card lead to the row (B21). Light and
// Dark captures land in HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { labelAgent, agentsIn, startHerdr, setFixtureLifecycle, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";
import { chord } from "./chords";

test.describe.configure({ timeout: 240_000 });

function git(cwd: string, args: string[]): string {
  return execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, encoding: "utf8" });
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
    labelAgent(herdr, pane, { task });
  }
  return pane;
}

const DAY = 86_400_000;
const passing = [{ __typename: "CheckRun", name: "verify", status: "COMPLETED", conclusion: "SUCCESS" }];
const failing = [{ __typename: "CheckRun", name: "verify", status: "COMPLETED", conclusion: "FAILURE", detailsUrl: "https://github.com/acme/repo/actions/runs/7" }];

/** A pull request as `gh pr list` lists it; its closing references come from its body, as GitHub reads them. */
type Listed = { number: number; title: string; branch: string; state: "OPEN" | "MERGED" | "CLOSED"; checks: unknown[]; review: string | null; body: string; mergedDaysAgo?: number; draft?: boolean };

const PULLS: Listed[] = [
  { number: 20, title: "머지된 작업", branch: "prd/merged", state: "MERGED", checks: passing, review: null, body: "", mergedDaysAgo: 30 },
  { number: 21, title: "리뷰를 기다리는 PR", branch: "prd/turn", state: "OPEN", checks: passing, review: "REVIEW_REQUIRED", body: "리뷰를 기다리는 본문" },
  { number: 22, title: "이슈로 만들 PR", branch: "prd/new-issue", state: "OPEN", checks: passing, review: "REVIEW_REQUIRED", body: "이슈로 옮길 PR 본문" },
  { number: 23, title: "에이전트가 고치는 PR", branch: "prd/fixing", state: "OPEN", checks: failing, review: "CHANGES_REQUESTED", body: "고치는 중" },
  { number: 24, title: "Bump sha2 from 0.10.9 to 0.11.0", branch: "dependabot/cargo/sha2", state: "OPEN", checks: failing, review: null, body: "Bumps sha2." },
  { number: 25, title: "닫힌 PR", branch: "prd/closed", state: "CLOSED", checks: passing, review: null, body: "" },
];

/**
 * A `gh` that is logged in for `acme/repo` and keeps its state in files:
 * each pull request's body, the issues (created ones added), and a log of
 * every write it was asked for. The first `pr list` waits until the spec
 * releases it, so the tab's skeleton can be seen; `pr edit 22` fails once;
 * the feedback read of #23 fails once. Anything else it is asked is refused.
 */
function fakeGh(root: string): { bin: string; state: string } {
  const bin = path.join(root, "gh-bin");
  const state = path.join(root, "gh-state");
  fs.mkdirSync(bin, { recursive: true });
  fs.mkdirSync(state, { recursive: true });
  const now = Date.now();
  fs.writeFileSync(path.join(state, "bodies.json"), JSON.stringify(Object.fromEntries(PULLS.map((pr) => [pr.number, pr.body]))));
  fs.writeFileSync(
    path.join(state, "issues.json"),
    JSON.stringify([
      { number: 5, title: "PR과 이을 이슈", url: "https://github.com/acme/repo/issues/5", state: "OPEN", projectItems: [], updatedAt: "2026-09-26T00:00:00Z" },
      { number: 6, title: "다른 이슈", url: "https://github.com/acme/repo/issues/6", state: "OPEN", projectItems: [], updatedAt: "2026-09-25T00:00:00Z" },
    ]),
  );
  const pulls = PULLS.map((pr) => ({
    number: pr.number,
    title: pr.title,
    statusCheckRollup: pr.checks,
    headRefName: pr.branch,
    baseRefName: "main",
    state: pr.state,
    reviewDecision: pr.review,
    isDraft: pr.draft ?? false,
    url: `https://github.com/acme/repo/pull/${pr.number}`,
    mergedAt: pr.mergedDaysAgo === undefined ? null : new Date(now - pr.mergedDaysAgo * DAY).toISOString(),
    updatedAt: new Date(now - pr.number * 60_000).toISOString(),
  }));
  const reviews = { 23: [{ author: { login: "ana" }, state: "CHANGES_REQUESTED", body: "Split the reader." }], 24: [] };
  const script = `#!${process.execPath}
const fs = require("fs");
const path = require("path");
const state = ${JSON.stringify(state)};
const args = process.argv.slice(2);
const file = (name) => path.join(state, name);
const read = (name) => JSON.parse(fs.readFileSync(file(name), "utf8"));
const out = (value) => process.stdout.write((typeof value === "string" ? value : JSON.stringify(value)) + "\\n");
const once = (name) => { if (fs.existsSync(file(name))) return false; fs.writeFileSync(file(name), ""); return true; };
const fail = (why) => { process.stderr.write(why + "\\n"); process.exit(1); };
const pulls = ${JSON.stringify(pulls)};
const reviews = ${JSON.stringify(reviews)};
const [a, b] = args;
if (a === "auth" && b === "status") process.exit(0);
if (a === "repo" && b === "view") { out({ nameWithOwner: "acme/repo" }); process.exit(0); }
if (a === "pr" && b === "list") {
  if (args.includes("merged")) { out([]); process.exit(0); }
  if (once("listed")) { while (!fs.existsSync(file("release"))) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100); }
  const bodies = read("bodies.json");
  out(pulls.map((pr) => ({ ...pr, closingIssuesReferences: [...(bodies[pr.number] ?? "").matchAll(/Closes #(\\d+)/g)].map((m) => ({ url: "https://github.com/acme/repo/issues/" + m[1] })) })));
  process.exit(0);
}
if (a === "pr" && b === "view") {
  const number = args[2];
  const fields = args[4];
  const body = read("bodies.json")[number] ?? "";
  if (fields === "body") { out({ body }); process.exit(0); }
  if (number === "23" && once("feedback-23-failed")) fail("HTTP 502: feedback");
  const pr = pulls.find((row) => String(row.number) === number);
  out({ body, statusCheckRollup: pr ? pr.statusCheckRollup : [], reviews: reviews[number] ?? [] });
  process.exit(0);
}
if (a === "pr" && b === "edit") {
  const number = args[2];
  fs.appendFileSync(file("writes.log"), JSON.stringify(args) + "\\n");
  if (number === "22" && once("edit-22-failed")) fail("HTTP 502: edit");
  const bodies = read("bodies.json");
  bodies[number] = args[4];
  fs.writeFileSync(file("bodies.json"), JSON.stringify(bodies));
  process.exit(0);
}
if (a === "issue" && b === "create") {
  fs.appendFileSync(file("writes.log"), JSON.stringify(args) + "\\n");
  const issues = read("issues.json");
  const number = Math.max(8, ...issues.map((issue) => issue.number)) + 1;
  issues.unshift({ number, title: args[3], url: "https://github.com/acme/repo/issues/" + number, state: "OPEN", projectItems: [], updatedAt: new Date().toISOString() });
  fs.writeFileSync(file("issues.json"), JSON.stringify(issues));
  out("https://github.com/acme/repo/issues/" + number);
  process.exit(0);
}
if (a === "issue" && b === "list") { out(read("issues.json")); process.exit(0); }
if (a === "issue" && b === "view") { out({ body: "이슈 본문", labels: [], author: { login: "hoyeon" }, assignees: [], createdAt: "2026-09-19T00:00:00Z", comments: [] }); process.exit(0); }
if (a === "api" && b === "graphql") { out({ data: { r0: { nameWithOwner: "acme/repo" } } }); process.exit(0); }
fail("unsupported: " + args.join(" "));
`;
  fs.writeFileSync(path.join(bin, "gh"), script, { mode: 0o755 });
  return { bin, state };
}

/** Every write the fake `gh` was asked for, in order. */
function writes(state: string): string[][] {
  const log = path.join(state, "writes.log");
  if (!fs.existsSync(log)) return [];
  return fs
    .readFileSync(log, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as string[]);
}

function body(state: string, number: number): string {
  return (JSON.parse(fs.readFileSync(path.join(state, "bodies.json"), "utf8")) as Record<string, string>)[number] ?? "";
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press(chord("settings"));
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="appearance"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  await page.waitForTimeout(400);
}

async function atRest(page: Page): Promise<void> {
  await page.mouse.move(2, 998, { steps: 4 });
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

/** Rests the pointer on `target`: a second small move is what a hand makes, and what a tooltip that was left behind needs to let go. */
async function rest(page: Page, target: import("@playwright/test").Locator): Promise<void> {
  await target.hover();
  const box = await target.boundingBox();
  if (box) await page.mouse.move(box.x + box.width / 2 + 1, box.y + box.height / 2, { steps: 2 });
}

function total(sent: Map<string, number>): number {
  let sum = 0;
  for (const count of sent.values()) sum += count;
  return sum;
}

test("a project's PRs tab: grouped pull requests, 이슈 잇기, 맡기기 and 정리", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    // Panes created from here on find the fixture's `claude` first, so an
    // agent a pull request is handed to is the shim, never a real one.
    fs.writeFileSync(path.join(herdr.root, "home", ".zshenv"), `export PATH="${path.join(herdr.root, "bin")}:$PATH"\n`);
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
    for (const [name, branch] of [
      ["turn", "prd/turn"],
      ["new-issue", "prd/new-issue"],
      ["fixing", "prd/fixing"],
      ["merged", "prd/merged"],
    ] as const) {
      git(repo, ["worktree", "add", "-b", branch, tree(name)]);
      git(tree(name), ["commit", "--allow-empty", "-m", `${name} work`]);
    }
    git(repo, ["merge", "--ff-only", "prd/merged"]);
    // The dependency bump exists only on origin, as a bot's branch does.
    git(repo, ["worktree", "add", "-b", "dependabot/cargo/sha2", tree("bot")]);
    git(tree("bot"), ["commit", "--allow-empty", "-m", "bump sha2"]);
    git(tree("bot"), ["push", "-q", "origin", "dependabot/cargo/sha2"]);
    git(repo, ["worktree", "remove", tree("bot")]);
    git(repo, ["branch", "-D", "dependabot/cargo/sha2"]);

    await workspaceAt(herdr, repo, null);
    const fixingPane = await workspaceAt(herdr, tree("fixing"), "리뷰 반영 작업 진행");
    await setFixtureLifecycle(herdr, fixingPane, "working");

    const gh = fakeGh(herdr.root);
    daemon = await startHided(herdr, "overview-prs", undefined, { PATH: `${gh.bin}:${herdr.fixturePath}` });
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
    await chooseTheme(page, "light");

    // The project row still opens Agents › 체크아웃 (B23).
    await page.locator('[data-sidebar-mode="projects"]').click();
    const repoRow = page.locator("[data-project-row]", { hasText: /^repo/ });
    await repoRow.click();
    const overview = page.locator("[data-overview-screen]");
    await expect(overview).toHaveAttribute("data-overview-view", "agents");
    const tile = (id: string) => overview.locator(`[data-lens-tile="${id}"]`);
    expect(await overview.locator("[data-lens-tile]").evaluateAll((tiles) => tiles.map((value) => value.getAttribute("data-lens-tile")))).toEqual(["agents", "issues", "prs", "sessions"]);

    // Until GitHub answers the tab is three skeleton rows and the tile has no number (B22).
    await tile("prs").locator("[data-lens-tile-button]").click();
    await expect(overview).toHaveAttribute("data-overview-view", "prs");
    await expect(overview.locator('[data-prs-view="reading"] [data-pr-skeleton]')).toHaveCount(3);
    await expect(tile("prs").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "unread");
    fs.writeFileSync(path.join(gh.state, "release"), "");

    // The groups by whose move it is; the closed one is not there, the merged one folded (B1, B2, B19).
    const groups = overview.locator("[data-pr-group]");
    await expect(groups).toHaveCount(4, { timeout: 30_000 });
    expect(await groups.evaluateAll((sections) => sections.map((section) => section.getAttribute("data-pr-group")))).toEqual(["turn", "fixing", "blocked", "merged"]);
    const rowsOf = (group: string) => overview.locator(`[data-pr-group="${group}"] [data-pr]`).evaluateAll((rows) => rows.map((row) => row.getAttribute("data-pr")));
    expect(await rowsOf("turn")).toEqual(["21", "22"]);
    expect(await rowsOf("fixing")).toEqual(["23"]);
    expect(await rowsOf("blocked")).toEqual(["24"]);
    expect(await rowsOf("merged")).toEqual([]);
    await expect(overview.locator('[data-pr="25"]')).toHaveCount(0);
    await expect(overview.locator('[data-pr-group="merged"]')).toHaveAttribute("data-folded", "true");
    await expect(tile("prs").locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "4");
    await expect(tile("prs")).toContainText("열림");
    await expect(tile("prs").locator("[data-lens-tile-badge]")).toHaveText("2");
    await expect(tile("prs").locator("[data-lens-tile-bar]")).toHaveAttribute("data-lens-tile-bar", "turn:2 fixing:1 blocked:1");
    await tile("prs").locator("[data-lens-tile-badge]").hover();
    await expect(page.getByRole("tooltip")).toContainText("내 차례 2");
    await expect(page.getByRole("tooltip")).toContainText("리뷰 2");

    // A row at rest (B3): state glyph, number, title, the empty issue cell,
    // the agent marks, the branch, CI and the review word; only 변경 요청 is yellow.
    const row = (number: number) => overview.locator(`[data-pr="${number}"]`);
    await expect(row(21).locator("[data-pr-state]")).toHaveAttribute("data-pr-state", "open");
    await expect(row(21).locator('[data-pr-issue="none"]')).toBeVisible();
    await expect(row(21).locator("[data-pr-branch]")).toHaveText("prd/turn");
    await expect(row(21).locator("[data-pr-review]")).toHaveText("리뷰 필요");
    await expect(row(23).locator("[data-pr-review]")).toHaveClass(/text-warning/);
    await expect(row(23).locator(`[data-pr-agent="${fixingPane}"]`)).toBeVisible();
    await expect(row(24).locator('[data-pr-checks-open="failed"]')).toBeVisible();

    // Under the pointer the time slot holds the buttons and nothing moves;
    // resting on the parts opens their cards; none of it is sent (B6, B7, B24).
    const before = total(sent);
    const title = row(21).locator("[data-pr-row]");
    const titleBox = await row(21).locator("[data-pr-branch]").boundingBox();
    await title.hover();
    await expect(row(21).locator('[data-pr-actions="default"]')).toBeVisible();
    await expect(row(21).locator("[data-pr-age]")).toBeHidden();
    expect(await row(21).locator("[data-pr-branch]").boundingBox()).toEqual(titleBox);
    await expect(row(21).locator("[data-pr-link-icon]")).toBeHidden();
    await row(21).locator('[data-pr-issue="none"]').hover();
    await expect(row(21).locator("[data-pr-link-icon]")).toBeVisible();
    await expect(page.getByRole("tooltip")).toContainText("이 PR을 이슈에 잇는다");
    await row(21).locator("[data-pr-number]").hover();
    await expect(page.locator('[data-checkout-card="pull_request"]')).toContainText("#21");
    await row(24).locator("[data-pr-row]").hover();
    await expect(row(24).locator("[data-pr-delegate-open]")).toBeVisible();
    await rest(page, row(24).locator("[data-pr-delegate-open]"));
    await expect(page.getByRole("tooltip")).toContainText("첫 지시 = 실패한 검사 · 리뷰 코멘트");
    await rest(page, row(23).locator(`[data-pr-agent="${fixingPane}"]`));
    await expect(page.locator(`[data-lens-message="${fixingPane}"]`)).toBeVisible();
    expect(total(sent)).toBe(before);
    await page.mouse.move(2, 998, { steps: 4 });
    await expect(page.locator(`[data-lens-message="${fixingPane}"]`)).toHaveCount(0);

    // The keyboard (B5): → unfolds, ← folds, ↓ moves; an unfolded row shows
    // the branch's agents and GitHub, Workspace and 이슈 잇기.
    await row(23).locator("[data-pr-row]").focus();
    await page.keyboard.press("ArrowRight");
    await expect(row(23)).toHaveAttribute("data-open", "true");
    await expect(row(23).locator(`[data-card-agent="${fixingPane}"]`)).toBeVisible();
    await expect(row(23).locator("[data-pr-unfolded-github], [data-pr-unfolded-workspace]")).toHaveCount(2);
    await page.keyboard.press("ArrowLeft");
    await expect(row(23)).not.toHaveAttribute("data-open", "true");
    await page.keyboard.press("ArrowDown");
    await expect(row(24).locator("[data-pr-row]")).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(row(24)).toHaveAttribute("data-open", "true");
    await expect(row(24).locator("[data-pr-unfolded-workspace]")).toHaveCount(0);
    await page.keyboard.press("Enter");

    // The CI mark is the checks on GitHub (B8).
    await page.context().route("https://github.com/**", (route) => route.fulfill({ body: "" }));
    const checks = page.waitForEvent("popup");
    await row(24).locator("[data-pr-checks-open]").click();
    expect((await checks).url()).toBe("https://github.com/acme/repo/pull/24/checks");
    await (await checks).close();

    // 이슈 잇기 with a GitHub issue (B9, B10): the project's open issues,
    // searchable; one confirmation, 그만두기 first; confirmed, one event, one
    // body write, and the issue cell filled.
    await row(21).locator("[data-pr-row]").hover();
    await row(21).locator("[data-pr-link-open]").click();
    const picker = page.locator('[data-pr-link-picker="21"]');
    await expect(picker.locator("[data-pr-link-choice]")).toHaveCount(2);
    await page.keyboard.type("이을");
    await expect(picker.locator("[data-pr-link-choice]")).toHaveCount(1);
    await picker.locator('[data-pr-link-choice="github:acme/repo#5"]').click();
    const confirm = page.locator('[data-pr-link="21"]');
    await expect(confirm.locator("[data-pr-link-confirm]")).toHaveText('PR #21 본문에 "Closes #5"을 씁니다. 머지되면 GitHub가 이슈를 닫습니다.');
    await expect(confirm.locator("[data-pr-link-cancel]")).toBeFocused();
    expect(writes(gh.state)).toEqual([]);
    await screenshot(page, "prs-link-confirm-light");
    await confirm.locator('[data-pr-link-write="write"]').click();
    await expect(confirm).toHaveCount(0, { timeout: 30_000 });
    expect(sent.get("pr_link_issue")).toBe(1);
    expect(last.get("pr_link_issue")).toMatchObject({ pr_number: 21, issue_key: "github:acme/repo#5" });
    expect(writes(gh.state)).toEqual([["pr", "edit", "21", "--body", "리뷰를 기다리는 본문\n\nCloses #5"]]);
    await expect(row(21).locator('[data-pr-issue="github:acme/repo#5"]')).toBeVisible({ timeout: 30_000 });

    // 새 이슈 만들기 (B12, B13): the pull request's title and body, one
    // confirmation; the body write fails, the issue stays made and linked,
    // and 본문 다시 쓰기 writes only the body.
    await row(22).locator("[data-pr-row]").hover();
    await row(22).locator("[data-pr-link-open]").click();
    await page.locator('[data-pr-link-picker="22"] [data-pr-link-new]').click();
    const made = page.locator('[data-pr-new-issue="22"]');
    await expect(made.locator("[data-pr-new-issue-title]")).toHaveValue("이슈로 만들 PR");
    await expect(made.locator("[data-pr-new-issue-body]")).toHaveValue("이슈로 옮길 PR 본문", { timeout: 20_000 });
    await expect(made.locator("[data-pr-new-issue-confirm]")).toHaveText('이슈를 만들고 PR #22 본문에 "Closes #(새 번호)"를 씁니다');
    await made.locator('[data-pr-new-issue-submit="create"]').click();
    await expect(made.locator("[data-pr-link-failure]")).toContainText("이슈 #9은 만들었고 PR 본문 쓰기는 실패했습니다", { timeout: 30_000 });
    await expect(made.locator('[data-pr-new-issue-submit="retry"]')).toHaveText(/본문 다시 쓰기/);
    await screenshot(page, "prs-new-issue-failed-light");
    await made.locator('[data-pr-new-issue-submit="retry"]').click();
    await expect(made).toHaveCount(0, { timeout: 30_000 });
    const written = writes(gh.state);
    expect(written.filter((args) => args[0] === "issue")).toEqual([["issue", "create", "--title", "이슈로 만들 PR", "--body", "이슈로 옮길 PR 본문"]]);
    expect(written.filter((args) => args[0] === "pr" && args[2] === "22")).toHaveLength(2);
    expect(body(gh.state, 22)).toBe("이슈로 옮길 PR 본문\n\nCloses #9");
    await expect(row(22).locator('[data-pr-issue="github:acme/repo#9"]')).toBeVisible({ timeout: 30_000 });

    // The PR chip on an issue card opens the PRs tab at its row, unfolded;
    // ⌘-click is GitHub (B21).
    await tile("issues").locator("[data-lens-tile-button]").click();
    const chip = overview.locator('[data-overview-card][data-task-key="github:acme/repo#5"] [data-lens-pr-chip="21"]');
    await expect(chip).toBeVisible({ timeout: 30_000 });
    const github = page.waitForEvent("popup");
    await chip.click({ modifiers: ["Meta"] });
    expect((await github).url()).toBe("https://github.com/acme/repo/pull/21");
    await (await github).close();
    await chip.click();
    await expect(overview).toHaveAttribute("data-overview-view", "prs");
    await expect(row(21)).toHaveAttribute("data-open", "true");
    await expect(row(21).locator("[data-pr-row]")).toBeFocused();

    // The sidebar's PR card has `PRs 탭에서 보기` (B21).
    await tile("agents").locator("[data-lens-tile-button]").click();
    const sidebarCheckout = page.locator('[data-project-list] [data-checkout][aria-label^="prd/new-issue"]');
    await sidebarCheckout.hover();
    const toTab = page.locator('[data-checkout-card-prs-tab="22"]');
    await expect(toTab).toBeVisible();
    await toTab.click();
    await expect(overview).toHaveAttribute("data-overview-view", "prs");
    await expect(row(22)).toHaveAttribute("data-open", "true");

    // 맡기기 whose feedback read fails (B18): the prompt stays empty with why
    // and 다시 읽기, which fills it; the start is never held back.
    await row(23).locator("[data-pr-row]").hover();
    await row(23).locator("[data-pr-menu]").click();
    await page.locator("[data-pr-menu-delegate]").click();
    const retrying = page.locator('[data-pr-delegate="23"]');
    await expect(retrying.locator("[data-pr-delegate-read-failed]")).toContainText("HTTP 502", { timeout: 20_000 });
    await expect(retrying.locator("[data-pr-delegate-prompt]")).toHaveValue("");
    await expect(retrying.locator("[data-pr-delegate-submit]")).toBeEnabled();
    await retrying.locator("[data-pr-delegate-reread]").click();
    await expect(retrying.locator("[data-pr-delegate-prompt]")).toHaveValue(/- ana: Split the reader\./, { timeout: 20_000 });
    await expect(retrying.locator("[data-pr-delegate-new-worktree]")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(retrying).toHaveCount(0);

    // ▷ 맡기기 on a bot's pull request with no checkout here (B15, B16): the
    // branch is fixed, the dialog says it makes the worktree, the prompt is
    // the failed check with its link; started, a worktree of that branch
    // exists, fetched from origin, and the screen is the agent's pane.
    await row(24).locator("[data-pr-row]").hover();
    await row(24).locator("[data-pr-delegate-open]").click();
    const delegate = page.locator('[data-pr-delegate="24"]');
    await expect(delegate.locator("[data-pr-delegate-branch]")).toHaveValue("dependabot/cargo/sha2");
    await expect(delegate.locator("[data-pr-delegate-branch]")).toBeDisabled();
    await expect(delegate.locator("[data-pr-delegate-new-worktree]")).toHaveText("이 브랜치의 워크트리를 만들고 시작합니다");
    await expect(delegate.locator("[data-pr-delegate-prompt]")).toHaveValue(
      "PR #24 (dependabot/cargo/sha2)의 CI 실패와 변경 요청을 고쳐줘: Bump sha2 from 0.10.9 to 0.11.0\n\n실패한 검사:\n- verify https://github.com/acme/repo/actions/runs/7",
      { timeout: 20_000 },
    );
    await expect(delegate.locator('[data-agent-kind="claude"], [data-agent-kind="codex"]')).toHaveCount(1);
    await expect(delegate.locator('[data-agent-kind="terminal"]')).toHaveCount(0);
    await screenshot(page, "prs-delegate-light");
    await delegate.locator("[data-pr-delegate-submit]").click();
    await expect(delegate).toHaveCount(0, { timeout: 60_000 });
    expect(sent.get("pr_delegate")).toBe(1);
    expect(last.get("pr_delegate")).toMatchObject({ pr_number: 24, provider: "claude" });
    await expect(page.locator("[data-workspace-screen]")).toBeVisible({ timeout: 30_000 });
    const worktrees = git(repo, ["worktree", "list", "--porcelain"]);
    expect(worktrees).toContain("branch refs/heads/dependabot/cargo/sha2");
    expect(git(repo, ["rev-parse", "--abbrev-ref", "dependabot/cargo/sha2@{upstream}"]).trim()).toBe("origin/dependabot/cargo/sha2");
    // The agent really runs in that worktree's pane: Herdr lists it there,
    // and the start never turned into a failure banner.
    const delegated = worktrees.split("\n\n").find((entry) => entry.includes("branch refs/heads/dependabot/cargo/sha2"))?.match(/^worktree (.+)$/m)?.[1];
    expect(delegated).toBeTruthy();
    await expect.poll(() => agentsIn(herdr, delegated as string), { timeout: 60_000 }).toContain("claude");
    await expect(page.locator('[data-task-agent="failed"]')).toHaveCount(0);

    // 최근 머지 unfolds from its header, dimmed; 정리 opens the existing
    // Delete worktree dialog, whose cancel changes nothing (B19, B20).
    await repoRow.click();
    await tile("prs").locator("[data-lens-tile-button]").click();
    await overview.locator('[data-pr-group-toggle="merged"]').click();
    await expect(row(20)).toBeVisible();
    await expect(row(20).locator(":scope > div")).toHaveClass(/opacity-/);
    await row(20).locator("[data-pr-row]").hover();
    await row(20).locator('[data-pr-cleanup="worktree"]').click();
    const remove = page.locator("[data-delete-worktree]");
    await expect(remove).toBeVisible();
    await remove.locator("[data-delete-cancel]").click();
    await expect(remove).toHaveCount(0);
    expect(fs.existsSync(tree("merged"))).toBe(true);

    await row(23).locator("[data-pr-row]").click();
    await expect(row(23)).toHaveAttribute("data-open", "true");
    await atRest(page);
    await screenshot(page, "prs-tab-light");
    await chooseTheme(page, "dark");
    await atRest(page);
    await screenshot(page, "prs-tab-dark");
    await row(21).locator("[data-pr-number]").hover();
    await expect(page.locator('[data-checkout-card="pull_request"]')).toBeVisible();
    await screenshot(page, "prs-pr-card-dark");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
