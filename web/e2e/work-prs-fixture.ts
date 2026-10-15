// The pull requests a fake `gh` answers for the Work tool specs (PRD
// work-tab-pr-lens): the web spec (`work-prs.spec.ts`) and the desktop host's
// native captures share them, so nothing reaches GitHub in either.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fixtureProgram } from "./platform-fixture";

export function git(cwd: string, args: string[]): string {
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
export function fakeGh(root: string): { bin: string; state: string } {
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
  fs.writeFileSync(path.join(state, "first-heads.json"), JSON.stringify(heads));
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
  const first = read("first-heads.json");
  // A list read after a push says so in the title, which is how a spec sees the list was read again.
  out(pulls.map((pr) => ({ ...pr, title: heads[pr.number] === first[pr.number] ? pr.title : pr.title + " (moved)", headRefOid: heads[pr.number], mergeStateStatus: merges[pr.number] })));
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
    const heads = fs.existsSync(file("card-heads.json")) ? read("card-heads.json") : read("heads.json");
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

export function writes(state: string): string[][] {
  const log = path.join(state, "writes.log");
  if (!fs.existsSync(log)) return [];
  return fs.readFileSync(log, "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line) as string[]);
}

export function head(state: string, number: number): string {
  return (JSON.parse(fs.readFileSync(path.join(state, "heads.json"), "utf8")) as Record<string, string>)[number]!;
}

/** The card's own read answers another head than the list's, as a push between the two reads does. */
export function skewCardHead(state: string, number: number, to: string): void {
  const heads = JSON.parse(fs.readFileSync(path.join(state, "heads.json"), "utf8")) as Record<string, string>;
  heads[number] = to;
  fs.writeFileSync(path.join(state, "card-heads.json"), JSON.stringify(heads));
}

export function moveHead(state: string, number: number, to: string): void {
  const file = path.join(state, "heads.json");
  const heads = JSON.parse(fs.readFileSync(file, "utf8")) as Record<string, string>;
  heads[number] = to;
  fs.writeFileSync(file, JSON.stringify(heads));
}
