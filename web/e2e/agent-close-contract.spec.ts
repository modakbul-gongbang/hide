// Pinned Herdr behavior, observed on a repository and worktree owned by this fixture.
import { expect, test, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

// Each contract has its own private Herdr and linked worktree; only the UI contracts start hided.
test.describe.configure({ timeout: 120_000 });

type Created = { result: { workspace: { workspace_id: string }; tab: { tab_id: string }; root_pane: { pane_id: string } } };
type Snapshot = { result: { snapshot: { tabs: { tab_id: string; workspace_id: string }[]; panes: { pane_id: string; tab_id: string }[]; workspaces: { workspace_id: string }[] } } };

// A primary checkout with one linked worktree, both opened as Herdr workspaces.
async function startFixture() {
  const herdr = await startHerdr({ agents: false });
  try {
    const repo = path.join(herdr.root, "primary");
    const linked = path.join(herdr.root, "linked");
    fs.mkdirSync(repo);
    const git = (...args: string[]) => execFileSync("git", ["-C", repo, ...args], { env: herdr.env, encoding: "utf8", stdio: "pipe" });
    git("init", "-b", "main");
    git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-m", "fixture");
    git("worktree", "add", "-b", "linked", linked);
    const open = (at: string) => herdr.run(["worktree", "open", "--cwd", repo, "--path", at, "--no-focus"]) as Created;
    const primary = open(repo);
    const sibling = open(linked);
    const snapshot = () => (herdr.run(["api", "snapshot"]) as Snapshot).result.snapshot;
    return { herdr, repo, open, primary, sibling, snapshot };
  } catch (error) {
    herdr.stop();
    throw error;
  }
}

async function openPrimary(page: Page, daemon: Daemon) {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await enterWorkspace(page, "primary");
  await page.locator('[data-sidebar-mode="projects"]').click();
  await page.locator('[data-project]', { hasText: "primary" }).locator('[data-checkout][aria-label^="main"]').click();
}

test("Herdr refuses to close the last tab or pane of a primary checkout without confirmation", async () => {
  const { herdr, open, primary, sibling, snapshot } = await startFixture();
  try {
    const close = (args: string[]) => {
      const result = spawnSync(herdr.bin, args, { env: herdr.env, encoding: "utf8", timeout: 10_000 });
      return { exit: result.status, stdout: result.stdout, stderr: result.stderr };
    };
    const before = snapshot();
    const tabClose = close(["tab", "close", primary.result.tab.tab_id]);
    expect(tabClose.exit).toBe(1);
    expect(JSON.parse(tabClose.stderr).error.code).toBe("confirmation_required");
    const afterTab = snapshot();
    const secondPrimary = open(path.join(herdr.root, "primary"));
    const beforePane = snapshot();
    const paneClose = close(["pane", "close", secondPrimary.result.root_pane.pane_id]);
    expect(paneClose.exit).toBe(1);
    expect(JSON.parse(paneClose.stderr).error.code).toBe("confirmation_required");
    const afterPane = snapshot();
    const output = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (output) {
      const record = { version: execFileSync(herdr.bin, ["--version"], { encoding: "utf8" }).trim(), primary: primary.result, linked: sibling.result, before, tabClose, afterTab, beforePane, paneClose, afterPane };
      fs.writeFileSync(path.join(output, "primary-close-contract.json"), JSON.stringify(record, null, 2));
    }
    expect(JSON.stringify(afterTab)).toContain(sibling.result.workspace.workspace_id);
    expect(JSON.stringify(afterPane)).toContain(sibling.result.workspace.workspace_id);
  } finally { herdr.stop(); }
});

test("closing a primary checkout's last tab in the UI leaves a replacement tab in the same area and keeps the linked workspace", async ({ page }) => {
  const { herdr, primary, sibling, snapshot } = await startFixture();
  let daemon: Daemon | null = null;
  try {
    const workspaceId = primary.result.workspace.workspace_id;
    daemon = await startHided(herdr, "replacement-close-tab");
    await openPrimary(page, daemon);
    const originalTab = page.locator(`[data-agent-tab-bar] [data-tab="${primary.result.tab.tab_id}"]`);
    await expect(originalTab).toBeVisible();
    const area = await originalTab.locator("xpath=ancestor::*[@data-agent-area-id]").getAttribute("data-agent-area-id");
    await originalTab.getByRole("button", { name: /^Close tab/ }).click();
    await expect(originalTab).toHaveCount(0);
    await expect.poll(() => snapshot().tabs.filter((tab) => tab.workspace_id === workspaceId).length).toBe(1);
    const replacement = snapshot().tabs.find((tab) => tab.workspace_id === workspaceId)!;
    const replacementTab = page.locator(`[data-agent-tab-bar] [data-tab="${replacement.tab_id}"]`);
    await expect(replacementTab).toBeVisible();
    await expect(replacementTab.locator("xpath=ancestor::*[@data-agent-area-id]")).toHaveAttribute("data-agent-area-id", area);
    expect(snapshot().workspaces.some((workspace) => workspace.workspace_id === sibling.result.workspace.workspace_id)).toBe(true);
    await screenshot(page, "agent-primary-replacement-tab");
  } finally { daemon?.stop(); herdr.stop(); }
});

test("closing a primary checkout's last pane in the UI keeps one tab for it and the linked workspace", async ({ page }) => {
  const { herdr, primary, sibling, snapshot } = await startFixture();
  let daemon: Daemon | null = null;
  try {
    const workspaceId = primary.result.workspace.workspace_id;
    const pane = primary.result.root_pane.pane_id;
    daemon = await startHided(herdr, "replacement-close-pane");
    await openPrimary(page, daemon);
    await page.locator(`[data-pane-view="${pane}"]`).getByRole("button", { name: /Close/ }).click();
    await expect.poll(() => snapshot().panes.some((candidate) => candidate.pane_id === pane)).toBe(false);
    await expect.poll(() => snapshot().tabs.filter((tab) => tab.workspace_id === workspaceId).length).toBe(1);
    expect(snapshot().workspaces.some((workspace) => workspace.workspace_id === sibling.result.workspace.workspace_id)).toBe(true);
    await screenshot(page, "agent-primary-replacement-pane");
  } finally { daemon?.stop(); herdr.stop(); }
});
