// The Overview's disk cleanup sheet on an isolated pinned Herdr and hided (PRD
// disk-layers): a private repository with a merged worktree and one that is
// not, each holding the same ignored folders a build leaves (`target/` with a
// CACHEDIR.TAG, `node_modules/`, `dist/` beside a package.json, and an
// `agents/runs` folder no rule knows). The disk number opens the sheet with
// its layers on hover (B1), the table sorts and folds and reads a skeleton
// until measured (B4, B5), the other layer has no checkbox (B8), a cache-only
// cleanup runs at once and leaves what git tracks and what no rule names
// (B7, B17, B21), a worktree goes through one confirmation and the folder and
// its registration are gone while its branch stays (B15, B18, B21), and a
// press is one core event (B23, B27). Every deletion happens in the fixture
// root; nothing points at an operator project (CONTRIBUTING.md). Light and
// Dark captures land in HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 240_000 });

const SIGNATURE = "Signature: 8a477f597d28d172789f06886806bc55\n# Created by the disk cleanup e2e fixture\n";

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

/** What a build leaves in a checkout: ignored folders, sized so each layer has bytes. */
function build(dir: string): void {
  const file = (relative: string, kilobytes: number) => {
    const target = path.join(dir, relative);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, Buffer.alloc(kilobytes * 1024, 1));
  };
  fs.mkdirSync(path.join(dir, "target"), { recursive: true });
  fs.writeFileSync(path.join(dir, "target", "CACHEDIR.TAG"), SIGNATURE);
  file("target/debug/blob", 256);
  file("node_modules/pkg/index.js", 128);
  file("dist/bundle.js", 64);
  file("agents/runs/session.log", 64);
}

/** A capture in the other theme: the sheet is modal, so the class the theme setting toggles is toggled directly. */
async function showTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.evaluate((next) => {
    const root = document.documentElement;
    root.classList.toggle("dark", next === "dark");
    root.classList.toggle("light", next === "light");
  }, theme);
  await page.waitForTimeout(400);
}

test("the disk cleanup sheet: layers, a cache-only cleanup at once, and a worktree after one confirmation", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    fs.writeFileSync(path.join(repo, "Cargo.toml"), '[package]\nname = "fixture"\n');
    fs.writeFileSync(path.join(repo, "package.json"), '{"name":"fixture"}\n');
    fs.writeFileSync(path.join(repo, ".gitignore"), "target/\nnode_modules/\ndist/\nagents/runs/\n");
    git(repo, ["add", "."]);
    git(repo, ["commit", "-m", "initial"]);
    // A repository with an origin, as a real one has: the core measures each branch against origin's default.
    const origin = path.join(herdr.root, "origin.git");
    git(herdr.root, ["init", "--bare", origin]);
    git(repo, ["remote", "add", "origin", origin]);
    git(repo, ["push", "-q", "origin", "main"]);
    git(repo, ["remote", "set-head", "origin", "main"]);
    const shipped = path.join(herdr.root, "repo-shipped");
    const active = path.join(herdr.root, "repo-active");
    git(repo, ["worktree", "add", "-b", "prd/shipped", shipped]);
    git(shipped, ["commit", "--allow-empty", "-m", "shipped work"]);
    git(repo, ["merge", "--ff-only", "prd/shipped"]);
    git(repo, ["worktree", "add", "-b", "prd/active", active]);
    git(active, ["commit", "--allow-empty", "-m", "active work"]);
    for (const dir of [repo, shipped, active]) build(dir);

    const created = herdr.run(["workspace", "create", "--cwd", repo, "--label", "repo", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as { result: { root_pane: { pane_id: string } } };
    await prompt(herdr, created.result.root_pane.pane_id);

    daemon = await startHided(herdr, "disk-cleanup");
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();
    await page.locator("[data-project-row]", { hasText: /^repo/ }).click();
    await expect(page.locator("[data-overview-screen]")).toBeVisible();

    // B1: the disk number lists the layers on hover and opens the sheet.
    const entrance = page.locator('[data-disk-entrance="true"]');
    await expect(entrance).toBeVisible({ timeout: 30_000 });
    await entrance.hover();
    const tooltip = page.locator('[data-disk-tooltip="true"]');
    await expect(tooltip).toContainText("빌드 캐시");
    await expect(tooltip).toContainText("의존성");
    await expect(tooltip.locator('[data-disk-tooltip-line="other"]')).toContainText("기타");
    await page.mouse.move(2, 998);
    const reviewsBefore = sent.get("cleanup_review") ?? 0;
    await entrance.click();
    const sheet = page.locator("[data-disk-sheet]");
    await expect(sheet).toBeVisible();
    // B27: opening the sheet is one review event, and hover, filters and ticks add none.
    await expect.poll(() => (sent.get("cleanup_review") ?? 0) - reviewsBefore).toBe(1);
    await expect(sheet).toHaveAttribute("data-disk-state", "ready", { timeout: 60_000 });

    // B4, D-21: main on top, and every fixture checkout is under 1 GB, so the rest fold.
    const row = (suffix: string) => sheet.locator(`[data-disk-row$="${suffix}"]`);
    await expect(row("/repo")).toBeVisible();
    await expect(sheet.locator("[data-disk-fold]")).toHaveAttribute("data-disk-fold", "closed");
    await sheet.locator("[data-disk-fold-toggle]").click();
    await expect(row("/repo-shipped")).toBeVisible();
    await expect(row("/repo-active")).toBeVisible();
    // D-22: the merged worktree is finished, the other rests, and each filter states its count.
    await expect(row("/repo-shipped")).toHaveAttribute("data-disk-bucket", "done");
    await expect(row("/repo-active")).toHaveAttribute("data-disk-bucket", "resting");
    await expect(sheet.locator('[data-disk-filter-item="done"]')).toContainText("1");

    // B7, B8: git-visible files are in no cell, the unknown folder is `other` and has no checkbox.
    const other = row("/repo-active").locator('[data-disk-cell$=":other"]');
    await expect(other).toContainText(/\d/);
    await expect(other.locator('input, button[role="checkbox"]')).toHaveCount(0);
    await expect(row("/repo-active").locator('[data-disk-check$=":build_cache"]')).toBeEnabled();
    await expect(row("/repo").locator('[data-disk-cell$=":worktree"]')).toHaveAttribute("data-disk-cell-state", "none");
    // B15: the worktree that is not merged cannot be ticked and says why.
    await expect(row("/repo-active").locator('[data-disk-cell$=":worktree"]')).toHaveAttribute("data-disk-cell-state", "blocked");
    await expect(row("/repo-shipped").locator('[data-disk-check$=":worktree"]')).toBeEnabled();

    // B10: the top-left checkbox ticks every visible cache; the footer counts them (B16).
    await sheet.locator('[data-disk-bundle="all"]').click();
    await expect(sheet.locator('[data-disk-bundle="all"]')).toHaveAttribute("data-disk-bundle-state", "checked");
    await expect(sheet.locator("[data-disk-summary]")).toContainText("빌드 캐시 3 · 의존성 3 · 워크트리 0");
    await expect(sheet.locator('[data-disk-note="dependencies"]')).toBeVisible();
    await showTheme(page, "light");
    await screenshot(page, "disk-cleanup-sheet-light");
    await showTheme(page, "dark");
    await screenshot(page, "disk-cleanup-sheet-dark");

    // B17, B21: cache-only runs at once, without a confirmation, and the tracked files stay.
    const confirmsBefore = sent.get("cleanup_confirm") ?? 0;
    await sheet.locator("[data-disk-clean]").click();
    await expect(sheet.locator("[data-disk-result]")).toBeVisible({ timeout: 60_000 });
    expect((sent.get("cleanup_confirm") ?? 0) - confirmsBefore).toBe(1);
    await expect(page.locator("[data-disk-confirm]")).toHaveCount(0);
    for (const dir of [repo, shipped, active]) {
      expect(fs.existsSync(path.join(dir, "target"))).toBe(false);
      expect(fs.existsSync(path.join(dir, "node_modules"))).toBe(false);
      expect(fs.existsSync(path.join(dir, "dist"))).toBe(false);
      expect(fs.existsSync(path.join(dir, "agents", "runs", "session.log"))).toBe(true);
      expect(fs.existsSync(path.join(dir, "README.md"))).toBe(true);
    }
    // B22: free space before and after, the allocated total beside it, a line for each cell.
    await expect(sheet.locator("[data-disk-free-change]")).toContainText("→");
    await expect(sheet.locator("[data-disk-allocated]")).toContainText("할당 합계");
    await expect(sheet.locator('[data-disk-result-line="removed"]').first()).toBeVisible();
    await showTheme(page, "light");
    await screenshot(page, "disk-cleanup-result-light");
    await showTheme(page, "dark");
    await screenshot(page, "disk-cleanup-result-dark");

    // B22, B24: review again measures again and returns to the table.
    await sheet.locator("[data-disk-review-again]").click();
    await expect(sheet).toHaveAttribute("data-disk-state", "ready", { timeout: 60_000 });
    // The fold keeps the way the operator left it (open).
    await expect(sheet.locator("[data-disk-fold]")).toHaveAttribute("data-disk-fold", "open");
    await expect(row("/repo-shipped").locator('[data-disk-cell$=":build_cache"]')).toHaveAttribute("data-disk-cell-state", "empty");

    // B11, B18: a worktree includes its caches, asks once, and Back deletes nothing.
    await sheet.locator('[data-disk-check$="/repo-shipped:worktree"]').click();
    await expect(sheet.locator("[data-disk-warning=\"worktree\"]")).toContainText("prd/shipped은 폴더째 지워진다");
    await sheet.locator("[data-disk-clean]").click();
    const confirm = page.locator("[data-disk-confirm]");
    await expect(confirm).toContainText("워크트리 1개를 폴더째 지운다");
    await expect(confirm).toContainText("prd/shipped");
    await expect(confirm.locator("[data-disk-confirm-run]")).toHaveText("워크트리 1개와 캐시 정리");
    // Neither button holds the keyboard when the confirmation opens (design 6).
    expect(await page.evaluate(() => document.activeElement?.hasAttribute("data-disk-confirm-run") || document.activeElement?.hasAttribute("data-disk-confirm-back"))).toBe(false);
    await showTheme(page, "light");
    await screenshot(page, "disk-cleanup-confirm-light");
    await showTheme(page, "dark");
    await screenshot(page, "disk-cleanup-confirm-dark");
    await confirm.locator("[data-disk-confirm-back]").click();
    await expect(confirm).toHaveCount(0);
    expect(fs.existsSync(shipped)).toBe(true);
    await sheet.locator("[data-disk-clean]").click();
    await confirm.locator("[data-disk-confirm-run]").click();
    await expect(sheet.locator("[data-disk-result]")).toBeVisible({ timeout: 60_000 });
    // B21: the folder and its registration go, the branch stays.
    expect(fs.existsSync(shipped)).toBe(false);
    expect(git(repo, ["worktree", "list", "--porcelain"])).not.toContain("repo-shipped");
    expect(git(repo, ["branch", "--list", "prd/shipped"]).trim()).not.toBe("");
    expect(fs.existsSync(active)).toBe(true);
    await sheet.locator("[data-disk-close]").click();
    await expect(sheet).toHaveCount(0);
    // B27: two presses of `정리` were two confirm events, one per cleanup.
    expect((sent.get("cleanup_confirm") ?? 0) - confirmsBefore).toBe(2);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("a checkout with a working agent cannot be ticked, and an open pane alone does not block main", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    fs.writeFileSync(path.join(herdr.root, "home", ".zshenv"), `export PATH="${path.join(herdr.root, "bin")}:$PATH"\n`);
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "package.json"), '{"name":"fixture"}\n');
    fs.writeFileSync(path.join(repo, ".gitignore"), "node_modules/\ndist/\n");
    git(repo, ["add", "."]);
    git(repo, ["commit", "-m", "initial"]);
    const busy = path.join(herdr.root, "repo-busy");
    git(repo, ["worktree", "add", "-b", "prd/busy", busy]);
    git(busy, ["commit", "--allow-empty", "-m", "busy work"]);
    for (const dir of [repo, busy]) build(dir);

    const at = async (cwd: string) => {
      const created = herdr.run(["workspace", "create", "--cwd", cwd, "--label", path.basename(cwd), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as { result: { root_pane: { pane_id: string } } };
      const pane = created.result.root_pane.pane_id;
      await prompt(herdr, pane);
      return pane;
    };
    await at(repo);
    const pane = await at(busy);
    herdr.run(["agent", "start", "agent-busy", "--kind", "claude", "--pane", pane]);
    execFileSync(herdr.bin, ["pane", "report-agent", pane, "--source", "e2e", "--agent", "claude", "--state", "working"], { env: herdr.env, timeout: 30_000 });

    daemon = await startHided(herdr, "disk-cleanup-busy");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();
    await page.locator("[data-project-row]", { hasText: /^repo/ }).click();
    await page.locator('[data-disk-entrance="true"]').click({ timeout: 30_000 });
    const sheet = page.locator("[data-disk-sheet]");
    await expect(sheet).toHaveAttribute("data-disk-state", "ready", { timeout: 60_000 });
    await sheet.locator("[data-disk-fold-toggle]").click();
    const busyRow = sheet.locator('[data-disk-row$="/repo-busy"]');
    await expect(busyRow).toHaveAttribute("data-disk-bucket", "working");
    await expect(busyRow.locator("[data-disk-in-use]")).toHaveText("에이전트 작업 중");
    await expect(busyRow.locator('[data-disk-check$=":build_cache"]')).toBeDisabled();
    await expect(busyRow.locator('[data-disk-cell$=":build_cache"]')).toHaveAttribute("data-disk-cell-state", "blocked");
    // Main has a pane open and is still ticked: an open pane alone blocks nothing (D-14).
    await expect(sheet.locator('[data-disk-row$="/repo"] [data-disk-check$=":build_cache"]')).toBeEnabled();
    await sheet.locator('[data-disk-bundle="all"]').click();
    await expect(sheet.locator("[data-disk-summary]")).toContainText("빌드 캐시 1");
    await sheet.locator("[data-disk-cancel]").click();
    await expect(sheet).toHaveCount(0);
    expect(fs.existsSync(path.join(busy, "node_modules"))).toBe(true);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
