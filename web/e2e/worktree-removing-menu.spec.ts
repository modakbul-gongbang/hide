// A checkout row mid-removal: with the Delete worktree dialog hidden while Git
// still works, a right-click on the dimmed row draws no empty
// popover, and the row leaves once the removal finishes.

import { expect, test } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

const BRANCH = "feature/slow";

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

/**
 * A `git` first on the daemon's PATH that holds `git worktree remove` for a
 * while, as a removal Git does in place does, and runs the real Git for
 * everything else.
 */
function slowGit(root: string): string {
  const real = execFileSync("sh", ["-c", "command -v git"], { encoding: "utf8" }).trim();
  const bin = path.join(root, "slow-git");
  fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, "git"), `#!/bin/sh\ncase " $* " in *" worktree remove "*) sleep 8 ;; esac\nexec "${real}" "$@"\n`, { mode: 0o755 });
  return `${bin}:${process.env.PATH ?? ""}`;
}

test("a checkout row being deleted opens no empty menu", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    git(repo, ["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(repo, ["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
    const worktree = path.join(herdr.root, "repo-slow");
    git(repo, ["worktree", "add", "-b", BRANCH, worktree]);

    // A pane in main keeps the project listed once the worktree's pane closes.
    for (const [cwd, label] of [[repo, "repo"], [worktree, "repo-slow"]]) {
      const created = herdr.run(["workspace", "create", "--cwd", cwd, "--label", label, "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as {
        result: { root_pane: { pane_id: string } };
      };
      await prompt(herdr, created.result.root_pane.pane_id);
    }

    daemon = await startHided(herdr, "worktree-removing-menu", undefined, { PATH: slowGit(herdr.root) });
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();

    const feature = page.locator("[data-checkout-row]").filter({ has: page.locator(`[data-checkout][aria-label^="${BRANCH}"]`) });
    await expect(feature).toBeVisible({ timeout: 30_000 });
    await feature.locator("[data-checkout-menu]").click({ button: "right" });
    await page.getByRole("menu", { name: `${BRANCH} actions` }).locator('[data-menu-item="delete_worktree"]').click();

    const dialog = page.locator("[data-delete-worktree]");
    const confirm = dialog.locator("[data-delete-confirm]");
    await expect(confirm).toBeEnabled({ timeout: 30_000 });
    await confirm.click();
    await expect(dialog.locator('[data-delete-phase="removing"]')).toBeVisible({ timeout: 30_000 });
    // The operator stops watching; the removal goes on under the row.
    await dialog.locator("[data-delete-cancel]").click();
    await expect(dialog).toHaveCount(0);
    await expect(feature).toHaveAttribute("data-checkout-removing", "true");

    await feature.locator("[data-checkout-menu]").click({ button: "right" });
    await screenshot(page, "worktree-removing-right-click");
    await expect(page.locator('[role="menu"]')).toHaveCount(0);
    await expect(feature.locator("[data-checkout-menu]")).toHaveAttribute("data-state", "closed");
    // Still mid-removal, so the right-click above landed on the dimmed row.
    await expect(feature).toHaveAttribute("data-checkout-removing", "true");

    await expect(feature).toHaveCount(0, { timeout: 30_000 });
    expect(fs.existsSync(worktree)).toBe(false);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
