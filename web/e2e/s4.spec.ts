// S4's checkout-scoped History and read-only diff through an isolated pinned
// Herdr, hided and browser. The fixture is disposable and never uses an
// operator checkout, socket or app.

import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 90_000 });

test("History opens a scoped patch, then updates after editing the original file", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Awaited<ReturnType<typeof startHided>> | null = null;
  try {
    const repoDir = path.join(herdr.root, "history-repo");
    fs.mkdirSync(repoDir);
    const repo = fs.realpathSync(repoDir);
    const file = path.join(repo, "한글 notes.txt");
    execFileSync("git", ["init", "-q", "-b", "main"], { cwd: repo });
    fs.writeFileSync(file, "first\n");
    execFileSync("git", ["add", "-A"], { cwd: repo });
    execFileSync("git", ["-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "-c", "commit.gpgsign=false", "commit", "-qm", "base"], { cwd: repo });
    // The worktree reader takes the default from origin/HEAD, never guesses
    // it from the main worktree's current branch.
    execFileSync("git", ["update-ref", "refs/remotes/origin/main", "main"], { cwd: repo });
    execFileSync("git", ["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"], { cwd: repo });
    execFileSync("git", ["switch", "-q", "-c", "feature"], { cwd: repo });
    fs.writeFileSync(path.join(repo, "branch.txt"), "branch\n");
    execFileSync("git", ["add", "-A"], { cwd: repo });
    execFileSync("git", ["-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "-c", "commit.gpgsign=false", "commit", "-qm", "branch"], { cwd: repo });
    fs.writeFileSync(file, "first\nsecond <script>window.__executed = true</script>\n");
    herdr.run(["workspace", "create", "--cwd", repo, "--label", "history-repo", "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]);

    daemon = await startHided(herdr, "s4");
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await page.locator('[data-sidebar-mode="projects"]').click();
    await page.locator("[data-project]", { hasText: "history-repo" }).locator("[data-checkout]").first().click();
    await page.keyboard.press("Meta+Shift+KeyB");
    await expect(page.locator('[data-right-panel-section="explorer"]')).toBeVisible();
    await page.getByRole("button", { name: "History", exact: true }).click();
    await expect(page.locator('[data-right-panel-section="changes"]')).toBeVisible();
    await expect(page.locator('[data-history-group-section="working"]')).toBeVisible();
    await expect(page.locator('[data-history-group-section="committed"]')).toBeVisible();
    const row = page.locator('[data-history-group="working"][data-history-path="한글 notes.txt"]');
    await expect(row).toHaveAttribute("aria-label", /Uncommitted: 한글 notes.txt, Modified/);
    await row.click();
    await expect(page.locator('[data-editor-kind="diff"]')).toBeVisible();
    await expect(page.locator('[data-patch-view] .cm-content')).toContainText("second <script>window.__executed = true</script>");
    expect(await page.evaluate(() => (window as Window & { __executed?: boolean }).__executed ?? false)).toBe(false);
    await expect(page.locator('[data-tab-kind="diff"]')).toHaveAttribute("data-preview", "true");
    expect(sent.get("file_draft") ?? 0).toBe(0);
    expect(sent.get("file_save") ?? 0).toBe(0);
    await screenshot(page, "s4-working-diff");

    await page.locator('[data-history-group="committed"][data-history-path="branch.txt"]').click();
    await expect(page.locator('[data-diff-group="committed"] [data-patch-view] .cm-content')).toContainText("+branch");
    await row.click();
    await expect(page.locator('[data-diff-group="working"]')).toBeVisible();
    await page.locator('[data-right-panel-collapse="true"]').click();
    await expect(page.locator("[data-right-panel]")).toHaveCount(0);
    await page.keyboard.press("Meta+Shift+KeyB");
    await expect(page.locator('[data-right-panel-section="changes"]')).toBeVisible();

    await page.getByRole("button", { name: "Explorer" }).click();
    const fileRow = page.locator(`[data-explorer-row="${file}"]`);
    await expect(fileRow).toBeVisible();
    await fileRow.click();
    const content = page.locator('[data-editor-codemirror] .cm-content');
    await content.click();
    await page.keyboard.press("Meta+KeyA");
    await page.keyboard.type("first\nthird\n");
    await expect.poll(() => fs.readFileSync(file, "utf8")).toBe("first\nthird\n");
    await page.getByRole("button", { name: "History", exact: true }).click();
    await row.click();
    await expect(page.locator('[data-patch-view] .cm-content')).toContainText("+third");
    await expect(page.locator('[data-patch-view] .cm-content')).not.toContainText("second <script>");
    await expect(page.locator('[data-tab-kind="file"]')).toHaveAttribute("data-preview", "false"); // edited preview remains open
    await row.dblclick();
    await expect(page.locator('[data-tab-kind="diff"]')).toHaveAttribute("data-preview", "false");
    expect(sent.get("file_draft")).toBeGreaterThanOrEqual(1);
    expect(sent.get("file_save")).toBeGreaterThanOrEqual(1);
    await screenshot(page, "s4-updated-diff");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
