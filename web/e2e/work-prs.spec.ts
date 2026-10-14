// The Work tool's PRs lens and the pull request decision card on an isolated
// pinned Herdr and hided (PRD work-tab-pr-lens): a Git project whose pull
// requests a fake `gh` answers and records every write of, so nothing reaches
// GitHub. The list groups by whose move it is with a reason line per row (B6,
// B8, B24); a row opens the card as a File Views tab (B17); Merge asks once and
// goes out conditioned on the head commit the card showed (B30, B31); a pull
// request that moved after the card was drawn is refused and the card says so
// (B33); Update branch reaches a behind pull request (B32). Light and Dark
// captures land in HIDE_E2E_SCREENSHOT_DIR.

import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { fixtureProgram } from "./platform-fixture";
import { chooseTheme, countSent, screenshot, showProjectRow, showTool } from "./wire";

test.describe.configure({ timeout: 180_000 });

function git(cwd: string, args: string[]): string {
  return execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, encoding: "utf8" });
}

const HOUR = 3_600_000;
const passing = [{ __typename: "CheckRun", name: "verify", status: "COMPLETED", conclusion: "SUCCESS" }];
const failing = [{ __typename: "CheckRun", name: "verify", status: "COMPLETED", conclusion: "FAILURE", detailsUrl: "https://github.com/acme/repo/actions/runs/7" }];

type Listed = {
  number: number;
  title: string;
  branch: string;
  author: string;
  state: "OPEN" | "MERGED";
  merge: string;
  review: string | null;
  checks: unknown[];
  draft?: boolean;
  asked?: boolean;
  hoursAgo: number;
};

const PULLS: Listed[] = [
  { number: 31, title: "머지할 수 있는 PR", branch: "feat/clean", author: "example-user", state: "OPEN", merge: "CLEAN", review: "APPROVED", checks: passing, hoursAgo: 3 },
  { number: 32, title: "base에 뒤처진 PR", branch: "feat/behind", author: "example-user", state: "OPEN", merge: "BEHIND", review: "APPROVED", checks: passing, hoursAgo: 5 },
  { number: 33, title: "내 리뷰를 기다리는 PR", branch: "feat/review", author: "ana", state: "OPEN", merge: "BLOCKED", review: "REVIEW_REQUIRED", checks: passing, asked: true, hoursAgo: 8 },
  { number: 34, title: "충돌이 난 PR", branch: "feat/conflict", author: "example-user", state: "OPEN", merge: "DIRTY", review: null, checks: passing, hoursAgo: 12 },
  { number: 35, title: "검사가 실패한 초안", branch: "feat/draft", author: "example-user", state: "OPEN", merge: "DRAFT", review: null, checks: failing, draft: true, hoursAgo: 20 },
  { number: 36, title: "Bump sha2 from 0.10.9 to 0.11.0", branch: "dependabot/cargo/sha2", author: "app/dependabot", state: "OPEN", merge: "BLOCKED", review: null, checks: passing, hoursAgo: 30 },
  { number: 37, title: "머지된 작업", branch: "feat/merged", author: "example-user", state: "MERGED", merge: "UNKNOWN", review: null, checks: passing, hoursAgo: 40 },
];

/**
 * A `gh` that is signed in as `example-user` for `acme/repo` and keeps its
 * state in files: each pull request's head commit and merge state, and a log
 * of every write it was asked for. A merge or a branch update whose head
 * condition does not match the state's head is refused the way GitHub
 * refuses it. Anything it is not taught is refused.
 */
function fakeGh(root: string): { bin: string; state: string } {
  const bin = path.join(root, "gh-bin");
  const state = path.join(root, "gh-state");
  fs.mkdirSync(bin, { recursive: true });
  fs.mkdirSync(state, { recursive: true });
  const now = Date.now();
  const sha = (number: number, generation: number) => `${number.toString(16)}${generation.toString(16)}`.padStart(40, "0");
  const heads: Record<number, string> = {};
  const merges: Record<number, string> = {};
  for (const pr of PULLS) {
    heads[pr.number] = sha(pr.number, 0);
    merges[pr.number] = pr.merge;
  }
  fs.writeFileSync(path.join(state, "heads.json"), JSON.stringify(heads));
  fs.writeFileSync(path.join(state, "merges.json"), JSON.stringify(merges));
  const pulls = PULLS.map((pr) => ({
    number: pr.number,
    title: pr.title,
    headRefName: pr.branch,
    isCrossRepository: false,
    baseRefName: "main",
    state: pr.state,
    reviewDecision: pr.review,
    isDraft: pr.draft ?? false,
    url: `https://github.com/acme/repo/pull/${pr.number}`,
    mergedAt: pr.state === "MERGED" ? new Date(now - pr.hoursAgo * HOUR).toISOString() : null,
    updatedAt: new Date(now - pr.hoursAgo * HOUR).toISOString(),
    createdAt: new Date(now - (pr.hoursAgo + 24) * HOUR).toISOString(),
    closedAt: null,
    closingIssuesReferences: [],
    author: { login: pr.author, is_bot: pr.author.startsWith("app/") },
    reviewRequests: pr.asked ? [{ __typename: "User", login: "example-user" }] : [],
  }));
  const checks = PULLS.filter((pr) => pr.state === "OPEN").map((pr) => ({ number: pr.number, statusCheckRollup: pr.checks }));
  const asked = PULLS.filter((pr) => pr.asked).map((pr) => ({ number: pr.number }));
  const script = `const fs = require("fs");
const path = require("path");
const state = ${JSON.stringify(state)};
const args = process.argv.slice(2);
const file = (name) => path.join(state, name);
const read = (name) => JSON.parse(fs.readFileSync(file(name), "utf8"));
const out = (value) => process.stdout.write((typeof value === "string" ? value : JSON.stringify(value)) + "\\n");
const fail = (why) => { process.stderr.write(why + "\\n"); process.exit(1); };
const pulls = ${JSON.stringify(pulls)};
const checks = ${JSON.stringify(checks)};
const asked = ${JSON.stringify(asked)};
const [a, b] = args;
const log = () => fs.appendFileSync(file("writes.log"), JSON.stringify(args) + "\\n");
if (a === "auth" && b === "status") process.exit(0);
if (a === "api" && b === "user") { out("example-user"); process.exit(0); }
if (a === "repo" && b === "view") { out({ nameWithOwner: "acme/repo" }); process.exit(0); }
if (a === "pr" && b === "list") {
  if (args.includes("merged")) { out([]); process.exit(0); }
  if (args.includes("review-requested:@me")) { out(asked); process.exit(0); }
  const heads = read("heads.json");
  const merges = read("merges.json");
  if (args.includes("number,statusCheckRollup")) { out(checks); process.exit(0); }
  out(pulls.map((pr) => ({ ...pr, headRefOid: heads[pr.number], mergeStateStatus: merges[pr.number] })));
  process.exit(0);
}
if (a === "issue" && b === "list") { out([]); process.exit(0); }
if (a === "pr" && b === "merge") {
  log();
  const number = args[2];
  const head = args[args.indexOf("--match-head-commit") + 1];
  if (read("heads.json")[number] !== head) fail("Head branch was modified. Review the changes here and try the merge again.");
  const merges = read("merges.json");
  merges[number] = "MERGED";
  fs.writeFileSync(file("merges.json"), JSON.stringify(merges));
  process.exit(0);
}
if (a === "api" && args[1] === "--method" && args[2] === "PUT") {
  log();
  const number = args[3].split("/")[4];
  const expected = args[args.indexOf("-f") + 1].split("=")[1];
  const heads = read("heads.json");
  if (heads[number] !== expected) fail("expected_head_sha does not match the pull request head");
  heads[number] = (heads[number].slice(0, 39) + "f");
  fs.writeFileSync(file("heads.json"), JSON.stringify(heads));
  const merges = read("merges.json");
  merges[number] = "CLEAN";
  fs.writeFileSync(file("merges.json"), JSON.stringify(merges));
  out({ message: "Updating pull request branch." });
  process.exit(0);
}
if (a === "api" && b === "graphql") {
  const query = args.join(" ");
  if (query.includes("HidePrFacts")) { out({ data: { repository: { pullRequests: { nodes: [] } } } }); process.exit(0); }
  const match = /pullRequest\\(number:(\\d+)\\)/.exec(query);
  if (query.includes("HidePrCard") && match) {
    const number = match[1];
    const heads = read("heads.json");
    out({ data: { repository: {
      mergeCommitAllowed: true, squashMergeAllowed: true, rebaseMergeAllowed: false, viewerPermission: "ADMIN",
      defaultBranchRef: { name: "main" }, mergeQueue: null, attributes: { text: "dist/** linguist-generated\\n" },
      pullRequest: {
        headRefOid: heads[number], baseRefName: "main", body: "## Summary\\n\\n본문입니다 #" + number + "\\n\\n## Test plan\\n\\n- [x] checked\\n",
        files: { totalCount: 3, nodes: [{ path: "src/app.ts", additions: 12, deletions: 3 }, { path: "src/lib.ts", additions: 4, deletions: 0 }, { path: "dist/app.js", additions: 900, deletions: 20 }] },
        reviewThreads: { nodes: [] },
      },
    } } });
    process.exit(0);
  }
  out({ data: {} });
  process.exit(0);
}
fail("unsupported: " + args.join(" "));
`;
  fixtureProgram(bin, "gh", script);
  return { bin, state };
}

function writes(state: string): string[][] {
  const log = path.join(state, "writes.log");
  if (!fs.existsSync(log)) return [];
  return fs.readFileSync(log, "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line) as string[]);
}

function head(state: string, number: number): string {
  return (JSON.parse(fs.readFileSync(path.join(state, "heads.json"), "utf8")) as Record<string, string>)[number]!;
}

function moveHead(state: string, number: number, to: string): void {
  const file = path.join(state, "heads.json");
  const heads = JSON.parse(fs.readFileSync(file, "utf8")) as Record<string, string>;
  heads[number] = to;
  fs.writeFileSync(file, JSON.stringify(heads));
}

test("the Work tab's PRs lens groups by whose move it is and the card merges and updates on the head it showed", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Awaited<ReturnType<typeof startHided>> | null = null;
  try {
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
    herdr.run(["workspace", "create", "--cwd", repo, "--label", "repo", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);

    const gh = fakeGh(herdr.root);
    daemon = await startHided(herdr, "work-prs", undefined, { PATH: `${gh.bin}${path.delimiter}${herdr.fixturePath}` });
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await (await showProjectRow(page, page.locator("[data-project]", { hasText: "repo" }).locator("[data-checkout]").first())).click();
    await chooseTheme(page, "light");

    // The Tools column's first tab is Work; its lens switch holds Sessions and PRs (B1, B2).
    await showTool(page, "work");
    await expect(page.locator('[data-column-row="tools"] [data-tool-tab]')).toHaveCount(4);
    const work = page.locator("[data-work-panel]");
    await expect(work.locator("[data-work-lens]")).toHaveCount(2, { timeout: 30_000 });
    await work.locator('[data-work-lens="prs"]').click();
    await expect(work).toHaveAttribute("data-work-panel", "prs");
    expect(sent.get("workspace_view")).toBeGreaterThan(0);

    // The groups by whose move it is; each row says why in its second line (B6, B8, B24).
    const groups = work.locator("[data-pr-group]");
    await expect(groups.first()).toBeVisible({ timeout: 30_000 });
    expect(await groups.evaluateAll((sections) => sections.map((section) => section.getAttribute("data-pr-group")))).toEqual(["needs_you", "blocked", "drafts", "bots", "merged"]);
    const rowsOf = (group: string) => work.locator(`[data-pr-group="${group}"] [data-pr]`).evaluateAll((rows) => rows.map((row) => row.getAttribute("data-pr")));
    await expect(work.locator('[data-pr-group="needs_you"] [data-pr]')).toHaveCount(3);
    expect((await rowsOf("needs_you")).sort()).toEqual(["31", "32", "33"]);
    expect(await rowsOf("blocked")).toEqual(["34"]);
    // Waiting, Drafts, Bots and merged start folded (B7); a group opens from its header.
    for (const group of ["drafts", "bots", "merged"]) await expect(work.locator(`[data-pr-group="${group}"]`)).toHaveAttribute("data-folded", "true");
    await work.locator('[data-pr-group="drafts"] [data-pr-focus="group"]').click();
    expect(await rowsOf("drafts")).toEqual(["35"]);
    const reason = (number: number) => work.locator(`[data-pr="${number}"] [data-pr-reason]`);
    await expect(reason(31)).toHaveAttribute("data-pr-reason", "merge");
    await expect(reason(32)).toHaveAttribute("data-pr-reason", "update");
    await expect(reason(33)).toHaveAttribute("data-pr-reason", "review");
    await expect(reason(34)).toHaveAttribute("data-pr-reason", "conflict");
    await expect(work.locator('[data-work-lens="prs"] [data-work-dot]')).toBeVisible();
    await screenshot(page, "work-prs-light");

    // A row opens the card as a File Views tab, and merge asks once, then goes
    // out conditioned on the head commit the card showed (B17, B30, B31).
    await work.locator('[data-pr-row="31"]').click();
    const card = page.locator('[data-pr-card="31"]');
    await expect(card).toBeVisible({ timeout: 30_000 });
    await expect(card.locator('[data-pr-card-reason="merge"]')).toBeVisible();
    await expect(card.locator("[data-pr-card-files]")).toHaveAttribute("data-pr-card-files", "3", { timeout: 30_000 });
    await expect(card.locator("[data-pr-card-generated]")).toBeVisible();
    await expect(card.locator("[data-pr-card-outline]")).toBeVisible();
    await screenshot(page, "work-pr-card-light");
    await chooseTheme(page, "dark");
    await screenshot(page, "work-pr-card-dark");
    await chooseTheme(page, "light");
    const merge = card.locator("[data-pr-card-merge]");
    await expect(merge).toBeEnabled();
    expect(writes(gh.state)).toEqual([]);
    await merge.click();
    await expect(card.locator("[data-pr-card-confirm]")).toBeVisible();
    expect(writes(gh.state)).toEqual([]);
    await card.locator("[data-pr-card-confirm-yes]").click();
    await expect(card.locator('[data-pr-card-write="done"]')).toBeVisible({ timeout: 30_000 });
    expect(sent.get("pr_merge")).toBe(1);
    expect(last.get("pr_merge")).toMatchObject({ pr_number: 31, head: head(gh.state, 31) });
    expect(writes(gh.state)).toEqual([["pr", "merge", "31", "--merge", "--match-head-commit", head(gh.state, 31)]]);

    // Update branch on a behind pull request whose head moved after the card
    // was drawn: GitHub refuses, the card says it changed, Look again reads it
    // and the update then goes out on the new head (B32, B33).
    await work.locator('[data-pr-row="32"]').click();
    const behind = page.locator('[data-pr-card="32"]');
    await expect(behind).toBeVisible({ timeout: 30_000 });
    await expect(behind.locator('[data-pr-card-reason="update"]')).toBeVisible();
    await expect(behind.locator("[data-pr-card-files]")).toBeVisible({ timeout: 30_000 });
    const shown = head(gh.state, 32);
    moveHead(gh.state, 32, shown.slice(0, 39) + "e");
    await behind.locator("[data-pr-card-update]").click();
    await behind.locator('[data-pr-card-confirm-yes="update"]').click();
    await expect(behind.locator('[data-pr-card-write="failed"]')).toHaveAttribute("data-pr-card-write-reason", "head_moved", { timeout: 30_000 });
    await screenshot(page, "work-pr-card-moved-light");
    await behind.locator("[data-pr-card-look-again]").click();
    await expect(behind.locator('[data-pr-card-write="failed"]')).toHaveCount(0, { timeout: 30_000 });
    await behind.locator("[data-pr-card-update]").click();
    await behind.locator('[data-pr-card-confirm-yes="update"]').click();
    await expect(behind.locator('[data-pr-card-write="done"]')).toBeVisible({ timeout: 30_000 });
    const updates = writes(gh.state).filter((args) => args[0] === "api");
    expect(updates).toHaveLength(2);
    expect(updates[1]).toEqual(["api", "--method", "PUT", "repos/acme/repo/pulls/32/update-branch", "-f", `expected_head_sha=${shown.slice(0, 39)}e`]);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
