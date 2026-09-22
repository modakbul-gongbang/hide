// S3 slice-3 flow on an isolated pinned Herdr (PRD web-shell-pivot-s3 B1, B3):
// a real checkout with a git repository, listed lazily in the Explorer, its
// folders expanded through the core's ui state, a file opened into the
// checkout's preview tab, and a Git decoration taken from the core's
// changed-file set. Every command runs against a private server; the
// operator's Herdr is never touched.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

type Daemon = { origin: string; token: string; home: string; stop: () => void };

/** A committed file the working tree then changes, so the core reports it. */
function gitFixture(dir: string): void {
  execFileSync("git", ["init", "-q"], { cwd: dir });
  fs.writeFileSync(path.join(dir, "tracked.txt"), "one\n");
  execFileSync("git", ["add", "-A"], { cwd: dir });
  execFileSync("git", ["-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "commit", "-qm", "init"], { cwd: dir });
  fs.writeFileSync(path.join(dir, "tracked.txt"), "two\n");
}

async function focusRepo(page: Page, repoDir: string): Promise<void> {
  await page.locator('[data-sidebar-mode="projects"]').click();
  const project = page.locator("[data-project]", { hasText: path.basename(repoDir) });
  const row = project.locator("[data-checkout]").first();
  await row.click();
  await expect(row).toHaveAttribute("aria-current", "true");
}

test.describe.configure({ timeout: 90_000 });

test("the Explorer lists a checkout, expands a folder and opens a preview tab", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    // A second checkout, this one a git repository with one modified file.
    const repoDir = path.join(herdr.root, "repo");
    fs.mkdirSync(path.join(repoDir, "src"), { recursive: true });
    fs.writeFileSync(path.join(repoDir, "src", "main.ts"), "export const answer = 41;\n");
    fs.writeFileSync(path.join(repoDir, "README.md"), "# repo\n");
    fs.writeFileSync(path.join(repoDir, ".gitignore"), "node_modules\n");
    gitFixture(repoDir);
    const repo = fs.realpathSync(repoDir);
    herdr.run([
      "workspace", "create", "--cwd", repoDir, "--label", "repo",
      "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]);

    daemon = await startHided(herdr, "s3");
    const lastSent = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, lastSent);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);

    await focusRepo(page, repoDir);
    await page.locator('[data-sidebar-mode="explorer"]').click();

    // The tree lists the root: directories first, hidden names shown, `.git`
    // and the escaping names left out (B1).
    await expect(page.locator("[data-explorer]")).toHaveAttribute("data-explorer", repo);
    await expect(page.locator(`[data-explorer-row="${repo}/src"]`)).toBeVisible();
    await expect(page.locator(`[data-explorer-row="${repo}/.gitignore"]`)).toBeVisible();
    await expect(page.locator(`[data-explorer-row="${repo}/README.md"]`)).toBeVisible();
    await expect(page.locator(`[data-explorer-row="${repo}/tracked.txt"]`)).toBeVisible();
    await expect(page.locator(`[data-explorer-row="${repo}/.git"]`)).toHaveCount(0);
    await screenshot(page, "s3-explorer-root");

    // The core's changed-file set colors the modified row (B1, D-06).
    await expect(page.locator(`[data-explorer-row="${repo}/tracked.txt"]`)).toHaveAttribute("data-decoration", "modified", { timeout: 20_000 });

    // Expanding a folder is one ui_state_update and one file_list; its
    // children appear without a reload (B1, B3).
    await page.locator(`[data-explorer-row="${repo}/src"]`).click();
    await expect.poll(() => sent.get("file_list")).toBeGreaterThanOrEqual(2);
    expect(lastSent.get("file_list")).toMatchObject({ root: repo, path: `${repo}/src` });
    await expect.poll(() => sent.get("ui_state_update")).toBeGreaterThanOrEqual(2);
    await expect(page.locator(`[data-explorer-row="${repo}/src/main.ts"]`)).toBeVisible();
    await screenshot(page, "s3-explorer-expanded");

    // A single click opens the checkout's preview tab and the editor surface.
    await page.locator(`[data-explorer-row="${repo}/src/main.ts"]`).click();
    await expect.poll(() => sent.get("file_open")).toBe(1);
    expect(lastSent.get("file_open")).toMatchObject({ path: `${repo}/src/main.ts`, preview: true });
    const fileTab = page.locator('[data-tab-kind="file"]');
    await expect(fileTab).toHaveCount(1);
    await expect(fileTab).toHaveAttribute("data-preview", "true");
    await expect(page.locator("[data-editor-text]")).toContainText("export const answer = 41;");
    await screenshot(page, "s3-preview-tab");

    // ⌘⇧K promotes the preview to an ordinary tab (B3).
    await page.keyboard.press("Meta+Shift+KeyK");
    await expect.poll(() => sent.get("file_keep_open")).toBe(1);
    await expect(fileTab).toHaveAttribute("data-preview", "false");

    // Closing the tab is one file_close and the terminal canvas returns.
    await fileTab.hover();
    await fileTab.locator("button").click();
    await expect.poll(() => sent.get("file_close")).toBe(1);
    await expect(page.locator('[data-tab-kind="file"]')).toHaveCount(0);
    await expect(page.locator("[data-canvas]")).toBeVisible();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
