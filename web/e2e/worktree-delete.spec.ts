// Delete worktree… on a worktree that would lose work: the menu item is
// enabled, the dialog names the dirt, the unmerged commit and the agent it
// stops, Delete waits for the discard checkbox, and with both boxes ticked
// the row is dimmed with a spinner while it goes, then the folder and its
// unmerged branch go and the row leaves the sidebar.

import { expect, test } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { labelAgent, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

const BRANCH = "feature/delete";

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

test("a dirty worktree with an unmerged branch and an agent is deleted once both boxes are ticked", async ({ page }) => {
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
    // The base is read from origin/HEAD, as a clone has it.
    git(repo, ["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(repo, ["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
    const worktree = path.join(herdr.root, "repo-delete");
    git(repo, ["worktree", "add", "-b", BRANCH, worktree]);
    git(worktree, ["commit", "--allow-empty", "-m", "not merged anywhere"]);
    fs.writeFileSync(path.join(worktree, "draft.txt"), "work in progress\n");

    const created = herdr.run(["workspace", "create", "--cwd", worktree, "--label", "repo-delete", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as {
      result: { root_pane: { pane_id: string } };
    };
    const pane = created.result.root_pane.pane_id;
    await prompt(herdr, pane);
    herdr.run(["agent", "start", "agent-delete", "--kind", "claude", "--pane", pane]);
    labelAgent(herdr, pane, { task: "삭제 대상 작업 정리" });

    daemon = await startHided(herdr, "worktree-delete");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await page.locator('[data-sidebar-mode="projects"]').click();

    const feature = page.locator("[data-checkout-row]").filter({ has: page.locator(`[data-checkout][aria-label^="${BRANCH}"]`) });
    await expect(feature).toBeVisible({ timeout: 30_000 });
    await feature.locator("[data-checkout-menu]").click({ button: "right" });
    const menu = page.getByRole("menu", { name: `${BRANCH} actions` });
    const item = menu.locator('[data-menu-item="delete_worktree"]');
    await expect(item).not.toHaveAttribute("data-disabled", "");
    await item.click();

    const dialog = page.locator("[data-delete-worktree]");
    await expect(dialog).toBeVisible();
    // The row may arrive after the dialog opened; the consequences follow it.
    const consequences = dialog.locator("[data-delete-consequences]");
    await expect(consequences).toContainText("1 changed file not committed", { timeout: 30_000 });
    await expect(consequences).toContainText("ahead 1 unmerged");
    await expect(consequences).toContainText(/stops 1 agent: 삭제 대상 작업 정리 \(/);
    await expect(dialog.locator("[data-delete-branch-warning]")).toHaveText("1 commit not on main is lost with it");
    await expect(dialog).toContainText("Discard 1 changed file");

    const confirm = dialog.locator("[data-delete-confirm]");
    await expect(confirm).toHaveText("Close 1 pane and delete");
    await expect(confirm).toBeDisabled();
    await screenshot(page, "worktree-delete-dialog-unticked");
    await dialog.locator("[data-delete-branch]").click();
    await dialog.locator("[data-delete-discard]").click();
    await expect(confirm).toBeEnabled();
    await screenshot(page, "worktree-delete-dialog-ticked");
    // Every state the row passes through, however briefly it is drawn.
    await page.evaluate((branch) => {
      const seen: string[] = [];
      (window as unknown as { __removingSeen: string[] }).__removingSeen = seen;
      new MutationObserver(() => {
        const row = [...document.querySelectorAll("[data-checkout-row]")].find((li) => li.querySelector(`[data-checkout][aria-label^="${branch}"]`));
        if (row?.getAttribute("data-checkout-removing") === "true" && row.querySelector("[data-checkout-removing-mark]") && row.querySelector("[data-checkout]:disabled")) seen.push("removing");
      }).observe(document.body, { subtree: true, childList: true, attributes: true });
    }, BRANCH);
    await confirm.click();

    await expect(dialog.locator('[data-delete-result="finished"]')).toBeVisible({ timeout: 60_000 });
    await expect(feature).toHaveCount(0, { timeout: 30_000 });
    expect(await page.evaluate(() => (window as unknown as { __removingSeen: string[] }).__removingSeen.length)).toBeGreaterThan(0);
    expect(fs.existsSync(worktree)).toBe(false);
    expect(git(repo, ["branch", "--list", BRANCH]).trim()).toBe("");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
