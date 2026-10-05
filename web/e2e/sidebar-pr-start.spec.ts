// A sidebar row's pull request with no screen asking (issue #391): on an
// isolated pinned Herdr and hided with a fake `gh` that knows one pull request
// in each of three repositories. The first daemon only chooses a Workspace, so
// the next start opens on it with the right panel on Explorer: no Overview, no
// project Overview and no palette ever opens, and each page's wire carries no
// `github_request` and no `overview_refresh` (the wire it does carry is the
// terminal and the usage hint). Both rows still draw their lifecycle from the
// core's own read. A restart with `gh` down draws the previous run's pull
// requests muted as stale instead of falling back to a branch glyph, and a
// restart with `gh` back replaces them, still with no screen asking. The third
// repository's pull request is merged and names its head commit: it reattaches
// to the checkout on that commit from the saved answer alone. The repositories
// are tiny, so the worktree reader answers within milliseconds and this flow
// proves the end state; that the row does not wait for the reader is owned by
// the unit test in `herdr-core/src/runtime/tests/github_reads.rs`.

import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace } from "./wire";

test.describe.configure({ timeout: 180_000 });

const REPOSITORIES = [
  { name: "alpha", branch: "feature/alpha", number: 41, state: "OPEN" },
  { name: "bravo", branch: "feature/bravo", number: 42, state: "OPEN", draft: true },
  // Merged: GitHub names the head commit, and the row takes the pull request only while its checkout is still on that commit (#408).
  { name: "charlie", branch: "feature/charlie", number: 43, state: "MERGED" },
];

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false", ...args], { cwd, stdio: "ignore" });
}

/**
 * A `gh` that is logged in and lists the pull request of the repository it
 * runs in, until `gh-down` exists beside it; then every call fails the way a
 * missing network does.
 */
function fakeGh(dir: string, heads: Map<string, string>): { bin: string; down: string } {
  const bin = path.join(dir, "gh-bin");
  fs.mkdirSync(bin, { recursive: true });
  const down = path.join(bin, "gh-down");
  const answer = (repository: (typeof REPOSITORIES)[number]) =>
    JSON.stringify([
      {
        number: repository.number,
        title: `Work in ${repository.name}`,
        statusCheckRollup: [{ __typename: "CheckRun", status: "COMPLETED", conclusion: "SUCCESS", name: "verify" }],
        headRefName: repository.branch,
        headRefOid: heads.get(repository.name) ?? "",
        baseRefName: "main",
        state: repository.state,
        reviewDecision: null,
        isDraft: "draft" in repository,
        url: `https://example.invalid/${repository.name}/pull/${repository.number}`,
        mergedAt: repository.state === "MERGED" ? "2026-09-27T00:00:00Z" : null,
        updatedAt: "2026-09-27T00:00:00Z",
        closedAt: repository.state === "MERGED" ? "2026-09-27T00:00:00Z" : null,
        closingIssuesReferences: [],
      },
    ]);
  // The reader asks twice at once: every state without checks, and the open ones with only their number and checks.
  const openChecks = (repository: (typeof REPOSITORIES)[number]) =>
    JSON.stringify([{ number: repository.number, statusCheckRollup: [{ __typename: "CheckRun", status: "COMPLETED", conclusion: "SUCCESS", name: "verify" }] }]);
  const byRepository = REPOSITORIES.map((repository) => `    *"/${repository.name}") case "$*" in *"--state open"*) echo '${openChecks(repository)}' ;; *) echo '${answer(repository)}' ;; esac ;;`).join("\n");
  fs.writeFileSync(
    path.join(bin, "gh"),
    `#!/bin/sh
[ -e '${down}' ] && { echo "dial tcp: network is unreachable" >&2; exit 1; }
case "$1 $2" in
  "auth status") exit 0 ;;
  "pr list")
    case "$PWD" in
${byRepository}
      *) echo '[]' ;;
    esac ;;
  "repo view") echo '{"nameWithOwner":"acme/repo"}' ;;
  "issue list") echo '[]' ;;
  *) echo "unsupported: $*" >&2; exit 1 ;;
esac
`,
    { mode: 0o755 },
  );
  return { bin, down };
}

/** A repository with its `branch` checked out as a linked worktree, and a Herdr workspace on each. */
function repositoryWithWorktree(herdr: HerdrFixture, name: string, branch: string): string {
  const repo = path.join(herdr.root, name);
  fs.mkdirSync(repo);
  git(repo, ["init"]);
  fs.writeFileSync(path.join(repo, "README.md"), `# ${name}\n`);
  git(repo, ["add", "README.md"]);
  git(repo, ["commit", "-m", "initial"]);
  const worktree = path.join(herdr.root, `${name}-wt`);
  git(repo, ["worktree", "add", "-b", branch, worktree]);
  git(worktree, ["commit", "--allow-empty", "-m", "work"]);
  for (const cwd of [repo, worktree]) {
    herdr.run(["workspace", "create", "--cwd", cwd, "--label", path.basename(cwd), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);
  }
  return execFileSync("git", ["rev-parse", "HEAD"], { cwd: worktree, encoding: "utf8" }).trim();
}

test("the sidebar draws every project's pull request with no screen asking, and a restart keeps them as stale", { tag: "@flaky", annotation: { type: "issue", description: "https://github.com/modakbul-gongbang/hide/issues/417" } }, async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const herdr = await startHerdr();
  let daemon = null as Daemon | null;
  try {
    const heads = new Map<string, string>();
    for (const { name, branch } of REPOSITORIES) heads.set(name, repositoryWithWorktree(herdr, name, branch));
    const gh = fakeGh(herdr.root, heads);
    daemon = await startHided(herdr, "sidebar-pr-start", undefined, { PATH: `${gh.bin}:${herdr.fixturePath}` });

    // The first run opens on All projects and chooses a Workspace, so the next start resumes on it.
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "alpha");
    const statePath = path.join(daemon.stateDir, "core-state.json");
    await expect.poll(() => (JSON.parse(fs.readFileSync(statePath, "utf8")) as { focused_checkout_id?: string | null }).focused_checkout_id ?? null).not.toBeNull();

    const row = (branch: string) => page.locator(`[data-checkout][aria-label^="${branch}"]`);
    const glyph = (number: number) => page.locator(`[data-checkout-pr-glyph="${number}"]`);
    /** A fresh page on the daemon: on the Workspace, the sidebar's Projects tab, and nothing else opened. */
    const attach = async (next: Daemon) => {
      const sent = countSent(page);
      await page.goto(`${next.origin}/#token=${next.token}`);
      await expect(page.locator("[data-workspace-screen]")).toBeVisible({ timeout: 20_000 });
      await page.locator('[data-sidebar-mode="projects"]').click();
      return sent;
    };
    /** A merged pull request's checkout is settled, so its row sits in the project's Inactive fold; open it once, the core keeps it open. */
    const mergedGlyph = async () => {
      const fold = page.locator('[data-inactive-checkouts$="/charlie"]');
      await expect(fold).toBeVisible({ timeout: 30_000 });
      if ((await fold.getAttribute("aria-expanded")) !== "true") await fold.click();
      return glyph(43);
    };
    const nothingAsked = (sent: Map<string, number>) => {
      expect(sent.get("github_request") ?? 0).toBe(0);
      expect(sent.get("overview_refresh") ?? 0).toBe(0);
    };

    // Start 1: GitHub answers. Both rows draw their lifecycle, fresh.
    daemon = await daemon.restart((stateDir) => {
      const file = path.join(stateDir, "core-state.json");
      const state = JSON.parse(fs.readFileSync(file, "utf8")) as Record<string, unknown>;
      fs.writeFileSync(file, JSON.stringify({ ...state, right_panel_visible: true, right_panel_section: "explorer" }));
    });
    let sent = await attach(daemon);
    await expect(row("feature/alpha")).toHaveAttribute("data-checkout-kind", "pr_open", { timeout: 30_000 });
    await expect(row("feature/bravo")).toHaveAttribute("data-checkout-kind", "pr_draft", { timeout: 30_000 });
    await expect(glyph(41)).not.toHaveClass(/text-muted-foreground/);
    await expect(await mergedGlyph()).toHaveAttribute("aria-label", "Open pull request #43");
    await expect(glyph(43)).not.toHaveClass(/text-muted-foreground/);
    nothingAsked(sent);

    // Start 2: GitHub is unreachable. The previous run's answer is drawn and kept, muted.
    fs.writeFileSync(gh.down, "");
    daemon = await daemon.restart();
    sent = await attach(daemon);
    await expect(row("feature/alpha")).toHaveAttribute("data-checkout-kind", "pr_open", { timeout: 30_000 });
    await expect(row("feature/bravo")).toHaveAttribute("data-checkout-kind", "pr_draft", { timeout: 30_000 });
    await expect(glyph(41)).toHaveClass(/text-muted-foreground/);
    await expect(glyph(42)).toHaveClass(/text-muted-foreground/);
    // The merged one is drawn from the saved answer too: the checkout's head comes from Git's files, not from the slow worktree catalog.
    await expect(await mergedGlyph()).toHaveClass(/text-muted-foreground/);
    nothingAsked(sent);

    // Start 3: GitHub is back. The core's own read replaces the stale answer.
    fs.rmSync(gh.down);
    daemon = await daemon.restart();
    sent = await attach(daemon);
    await expect(glyph(41)).not.toHaveClass(/text-muted-foreground/, { timeout: 30_000 });
    await expect(glyph(42)).not.toHaveClass(/text-muted-foreground/);
    await expect(await mergedGlyph()).not.toHaveClass(/text-muted-foreground/);
    nothingAsked(sent);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
