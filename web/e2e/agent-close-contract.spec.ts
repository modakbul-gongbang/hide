// Pinned Herdr behavior, observed on a repository and worktree owned by this fixture.
import { expect, test } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

// Private Herdr, linked worktrees and close probes all run before the UI assertions.
test.describe.configure({ timeout: 180_000 });

test("primary last tab and last pane close retain the linked workspace boundary", { tag: "@flaky", annotation: { type: "issue", description: "https://github.com/modakbul-gongbang/hide/issues/231" } }, async ({ page }) => {
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "primary");
    const linked = path.join(herdr.root, "linked");
    fs.mkdirSync(repo);
    const git = (...args: string[]) => execFileSync("git", ["-C", repo, ...args], { env: herdr.env, encoding: "utf8", stdio: "pipe" });
    git("init", "-b", "main");
    git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-m", "fixture");
    git("worktree", "add", "-b", "linked", linked);
    type Created = { result: { workspace: { workspace_id: string }; tab: { tab_id: string }; root_pane: { pane_id: string } } };
    const primary = herdr.run(["worktree", "open", "--cwd", repo, "--path", repo, "--no-focus"]) as Created;
    const sibling = herdr.run(["worktree", "open", "--cwd", repo, "--path", linked, "--no-focus"]) as Created;
    const close = (args: string[]) => {
      const result = spawnSync(herdr.bin, args, { env: herdr.env, encoding: "utf8", timeout: 10_000 });
      return { exit: result.status, stdout: result.stdout, stderr: result.stderr };
    };
    const before = herdr.run(["api", "snapshot"]);
    const tabClose = close(["tab", "close", primary.result.tab.tab_id]);
    expect(tabClose.exit).toBe(1);
    expect(JSON.parse(tabClose.stderr).error.code).toBe("confirmation_required");
    const afterTab = herdr.run(["api", "snapshot"]);
    const secondPrimary = herdr.run(["worktree", "open", "--cwd", repo, "--path", repo, "--no-focus"]) as Created;
    const beforePane = herdr.run(["api", "snapshot"]);
    const paneClose = close(["pane", "close", secondPrimary.result.root_pane.pane_id]);
    expect(paneClose.exit).toBe(1);
    expect(JSON.parse(paneClose.stderr).error.code).toBe("confirmation_required");
    const afterPane = herdr.run(["api", "snapshot"]);
    const record = { version: execFileSync(herdr.bin, ["--version"], { encoding: "utf8" }).trim(), primary: primary.result, linked: sibling.result, before, tabClose, afterTab, beforePane, paneClose, afterPane };
    const output = process.env.HIDE_E2E_SCREENSHOT_DIR;
    if (output) fs.writeFileSync(path.join(output, "primary-close-contract.json"), JSON.stringify(record, null, 2));
    console.log(JSON.stringify({ tabClose, paneClose }));
    expect(JSON.stringify(afterTab)).toContain(sibling.result.workspace.workspace_id);
    expect(JSON.stringify(afterPane)).toContain(sibling.result.workspace.workspace_id);
    const workspaceId = primary.result.workspace.workspace_id;
    const currentTabs = () => (herdr.run(["api", "snapshot"]) as { result: { snapshot: { tabs: { tab_id: string; workspace_id: string }[]; panes: { pane_id: string; tab_id: string }[]; workspaces: { workspace_id: string }[] } } }).result.snapshot;
    daemon = await startHided(herdr, "replacement-close");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "primary");
    await page.locator('[data-sidebar-mode="projects"]').click();
    await page.locator('[data-project]', { hasText: "primary" }).locator('[data-checkout][aria-label^="main"]').click();
    const originalTab = page.locator(`[data-agent-tab-bar] [data-tab="${primary.result.tab.tab_id}"]`);
    await expect(originalTab).toBeVisible();
    const area = await originalTab.locator("xpath=ancestor::*[@data-agent-area-id]").getAttribute("data-agent-area-id");
    await originalTab.getByRole("button", { name: /^Close tab/ }).click();
    await expect(originalTab).toHaveCount(0);
    await expect.poll(() => currentTabs().tabs.filter((tab) => tab.workspace_id === workspaceId).length).toBe(1);
    const replacement = currentTabs().tabs.find((tab) => tab.workspace_id === workspaceId)!;
    const replacementTab = page.locator(`[data-agent-tab-bar] [data-tab="${replacement.tab_id}"]`);
    await expect(replacementTab).toBeVisible();
    expect(await replacementTab.locator("xpath=ancestor::*[@data-agent-area-id]").getAttribute("data-agent-area-id")).toBe(area);
    expect(currentTabs().workspaces.some((workspace) => workspace.workspace_id === sibling.result.workspace.workspace_id)).toBe(true);
    await screenshot(page, "agent-primary-replacement-tab");
    const replacementPane = currentTabs().panes.find((pane) => pane.tab_id === replacement.tab_id)!;
    await page.locator(`[data-pane-view="${replacementPane.pane_id}"]`).getByRole("button", { name: /Close/ }).click();
    await expect.poll(() => currentTabs().panes.some((pane) => pane.pane_id === replacementPane.pane_id)).toBe(false);
    await expect.poll(() => currentTabs().tabs.filter((tab) => tab.workspace_id === workspaceId).length).toBe(1);
    expect(currentTabs().workspaces.some((workspace) => workspace.workspace_id === sibling.result.workspace.workspace_id)).toBe(true);
    await screenshot(page, "agent-primary-replacement-pane");

  } finally { daemon?.stop(); herdr.stop(); }
});
