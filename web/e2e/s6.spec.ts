// The S6 flow on an isolated pinned Herdr and hided: a first run on Main, a Project's
// Overview and its Workspace (B1, B2, B4), the three layouts from the icons
// and the palette (B5), the Explorer and History as independent tools (B10),
// a delegated child reached from its parent's chip and left by its Return
// (B14-B16), and a restart that brings the Workspace back with a View tab
// whose file is gone marked unavailable (B19, B20).

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
}

/** Declares `child` as spawned by `parent`, the way a spawner's hook does; report-metadata prints nothing. */
function declareChild(herdr: HerdrFixture, child: string, parent: string): void {
  execFileSync(herdr.bin, ["pane", "report-metadata", child, "--source", "e2e-lineage", "--token", `parent_pane=${parent}`], { env: herdr.env, timeout: 30_000 });
}

test("Main, Overview and a Workspace with its layouts, tools and delegated child", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [parent, child] = herdr.panes;
    // The Explorer names rows by the checkout's resolved path.
    const root = path.join(fs.realpathSync(herdr.root), "fixture");
    fs.writeFileSync(path.join(herdr.root, "fixture", "notes.md"), "# Notes\n\nkept across a restart\n");
    fs.writeFileSync(path.join(herdr.root, "fixture", "gone.txt"), "removed before the restart\n");
    daemon = await startHided(herdr, "s6");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await open(page, daemon);

    // A first run starts on Main (D-11), which lists this machine's Project
    // with real counts (B1).
    const workspace = page.locator("[data-workspace-screen]");
    await expect(page.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await expect(workspace).toHaveCount(0);
    const project = page.locator("[data-main-project]", { hasText: "fixture" });
    await expect(project).toBeVisible();
    await expect(project.locator("[data-workspace-count]")).toHaveText(/1 workspace/);
    await screenshot(page, "s6-main");

    // Its Overview lists the Workspace and both agents; an agent enters the
    // Workspace at that pane (B2).
    await project.click();
    await expect(page.locator("[data-overview-screen]")).toBeVisible();
    await expect(page.locator("[data-overview-workspace]")).toHaveCount(1);
    await expect(page.locator("[data-overview-agent]")).toHaveCount(2);
    await screenshot(page, "s6-overview");
    await page.locator(`[data-overview-agent="${parent}"]`).click();
    await expect(workspace).toBeVisible();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true");
    // A new Workspace starts with its agents alone and the Explorer shown.
    await expect(workspace).toHaveAttribute("data-layout", "agents");
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();

    // The icons and the palette choose a layout; each is one workspace_view (B5).
    const before = sent.get("workspace_view") ?? 0;
    await page.locator('[data-layout-choice="together"]').click();
    await expect(workspace).toHaveAttribute("data-layout", "together");
    // With nothing open the View areas take no space, in either layout that
    // shows them (web-pane-scroll-find-areas B8, B10).
    await expect(page.locator("[data-view-area]")).toHaveCount(0);
    await expect(page.locator("[data-agent-area]")).toBeVisible();
    await page.locator('[data-layout-choice="views"]').click();
    await expect(workspace).toHaveAttribute("data-layout", "views");
    await expect(page.locator("[data-view-area]")).toHaveCount(0);
    await expect(page.locator("[data-agent-area]")).toBeVisible();
    await page.keyboard.press("Meta+KeyK");
    await page.keyboard.type("Layout: Agents only");
    await page.keyboard.press("Enter");
    await expect(workspace).toHaveAttribute("data-layout", "agents");
    expect(sent.get("workspace_view")).toBe(before + 3);
    expect(last.get("workspace_view")).toEqual({ mode: "agents" });

    // Explorer and History open and close on their own (B10).
    await page.locator('[data-tool-toggle="changes"]').click();
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    await page.locator('[data-tool-close="explorer"]').click();
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await page.locator('[data-tool-toggle="explorer"]').click();
    await page.locator('[data-tool-toggle="changes"]').click();
    await expect(page.locator('[data-tool="changes"]')).toHaveCount(0);

    // A file opened from Agents only brings the View area back beside the
    // agents (B11).
    await page.locator(`[data-explorer-row="${path.join(root, "notes.md")}"]`).dblclick();
    await expect(workspace).toHaveAttribute("data-layout", "together");
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(1);
    await page.locator(`[data-explorer-row="${path.join(root, "gone.txt")}"]`).dblclick();
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(2);
    await screenshot(page, "s6-together");

    // A declared child shows as a chip under its parent's header; the core
    // moves the delegated pane to its own tab, so the chip crosses tabs with
    // one tracked focus, and the child's Return comes back the same way
    // (B14-B16).
    declareChild(herdr, child, parent);
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveCount(0, { timeout: 20_000 });
    const chip = page.locator(`[data-pane-children="${parent}"] [data-child-chip="${child}"]`);
    await expect(chip).toBeVisible({ timeout: 20_000 });
    await expect(chip).toHaveAttribute("title", /Agent two/);
    await screenshot(page, "s6-child-chip");
    await chip.click();
    await expect.poll(() => last.get("focus_pane")?.pane_id).toBe(child);
    expect(last.get("focus_pane")?.request_id).toEqual(expect.any(String));
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    // Herdr's layout confirms the move, and the in-flight line goes away.
    await expect(page.locator("[data-relation-status]")).toHaveCount(0, { timeout: 15_000 });
    const back = page.locator(`[data-pane-view="${child}"] [data-pane-return="${parent}"]`);
    await expect(back).toBeVisible();
    await screenshot(page, "s6-child-return");
    await back.click();
    await expect.poll(() => last.get("focus_pane")?.pane_id).toBe(parent);
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await expect(page.locator("[data-relation-status]")).toHaveCount(0, { timeout: 15_000 });

    // The pane menu lists the relative and opens it only when chosen (B16, B18).
    const menuButton = page.locator(`[data-pane-menu="${parent}"]`);
    const focusCount = sent.get("focus_pane") ?? 0;
    await menuButton.click();
    const openChild = page.locator(`[data-menu-item="open:${child}"]`);
    await expect(openChild).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(openChild).toHaveCount(0);
    expect(sent.get("focus_pane") ?? 0).toBe(focusCount);

    // A restart brings the Workspace back with its layout and View tabs; the
    // file that went away stays as an unavailable tab (B19, B20).
    await page.locator('[data-layout-choice="views"]').click();
    await expect(workspace).toHaveAttribute("data-layout", "views");
    fs.rmSync(path.join(herdr.root, "fixture", "gone.txt"));
    daemon = await daemon.restart();
    // Reopening the app is a fresh page, not a reconnect of this one.
    await page.goto("about:blank");
    await open(page, daemon);
    await expect(page.locator("[data-workspace-screen]")).toHaveAttribute("data-layout", "views", { timeout: 20_000 });
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(2, { timeout: 15_000 });
    await expect(page.locator('[data-view-area] [data-unavailable="true"]')).toHaveCount(1);
    await expect(page.locator("[data-view-area] [data-close-unavailable]")).toBeVisible();
    await screenshot(page, "s6-restored-unavailable");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
