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
    await expect(page.locator("[data-overview-screen] [data-agent-open]")).toHaveCount(2);
    await screenshot(page, "s6-overview");
    await page.locator(`[data-overview-screen] [data-agent-open="${parent}"]`).click();
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

    // A file opened from Agents only floats the View areas as a panel over
    // the agents' right side; the Agent area keeps its full size underneath,
    // so no terminal resizes, and the agents left of the panel stay live
    // (issue 170).
    const agentArea = page.locator("[data-agent-area]");
    const panel = page.locator("[data-views-over-agents] [data-views-over-agents]");
    const agentBox = (await agentArea.boundingBox())!;
    const resizes = () => sent.get("terminal_resize") ?? 0;
    const resizesBefore = resizes();
    await page.locator(`[data-explorer-row="${path.join(root, "notes.md")}"]`).dblclick();
    await expect(workspace).toHaveAttribute("data-layout", "agents");
    await expect(workspace).toHaveAttribute("data-views-over-agents", "true");
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(1);
    await page.locator(`[data-explorer-row="${path.join(root, "gone.txt")}"]`).dblclick();
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(2);
    const inset = await page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--spacing-sm")));
    const panelBox = (await panel.boundingBox())!;
    expect(panelBox.x + panelBox.width).toBeCloseTo(agentBox.x + agentBox.width - inset, 0);
    expect(panelBox.y).toBeCloseTo(agentBox.y + inset, 0);
    expect(panelBox.y + panelBox.height).toBeCloseTo(agentBox.y + agentBox.height - inset, 0);
    expect(panelBox.width).toBeLessThan(agentBox.width - inset);
    expect(await agentArea.boundingBox()).toEqual(agentBox);

    // Its left edge resizes it like any divider: one step per arrow key, a
    // guide line while dragging with nothing resized, one change on release.
    const edge = panel.locator("[data-views-over-divider]");
    await edge.focus();
    const shareBefore = Number(await edge.getAttribute("aria-valuenow"));
    await page.keyboard.press("ArrowLeft");
    await expect(edge).toHaveAttribute("aria-valuenow", String(shareBefore + 5));
    await expect.poll(async () => (await panel.boundingBox())!.width).toBeGreaterThan(panelBox.width);
    const widened = (await panel.boundingBox())!;
    const edgeBox = (await edge.boundingBox())!;
    await page.mouse.move(edgeBox.x + edgeBox.width / 2, edgeBox.y + 200);
    await page.mouse.down();
    const target = agentBox.x + agentBox.width - inset - 300;
    await page.mouse.move(target, edgeBox.y + 200, { steps: 8 });
    await expect(page.locator("[data-area-guide]")).toBeVisible();
    expect(await panel.boundingBox()).toEqual(widened);
    expect(resizes()).toBe(resizesBefore);
    await page.mouse.up();
    await expect(page.locator("[data-area-guide]")).toHaveCount(0);
    await expect.poll(async () => (await panel.boundingBox())!.width).toBeCloseTo(300, -1);
    expect(last.get("workspace_view")).toEqual({ views_over_share: expect.any(Number) });
    expect(await agentArea.boundingBox()).toEqual(agentBox);
    expect(resizes()).toBe(resizesBefore);

    // The agents beside it are live: a click focuses the pane there, typing
    // reaches it, the panel stays up, and ⌘F finds in that pane rather than
    // in the document; in the panel ⌘F is the document's.
    const childHost = page.locator(`[data-pane-view="${child}"] [data-terminal-host]`);
    await childHost.click({ position: { x: 40, y: 60 } });
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    expect(last.get("focus_pane")).toMatchObject({ pane_id: child, in_place: true });
    await page.keyboard.type("typed beside the panel");
    await expect.poll(() => fs.readFileSync(herdr.inputLogs[1], "utf8"), { timeout: 10_000 }).toContain("typed beside the panel");
    await expect(workspace).toHaveAttribute("data-views-over-agents", "true");
    expect(resizes()).toBe(resizesBefore);
    // The pane's find bar takes a row of the pane itself, so the count starts
    // again after it.
    await page.keyboard.press("Meta+f");
    await expect(page.locator("[data-find-bar]")).toBeVisible();
    await expect(page.locator(".cm-search")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-find-bar]")).toHaveCount(0);
    await panel.locator("[data-editor-body] .cm-content").click();
    await page.keyboard.press("Meta+f");
    await expect(page.locator(".cm-search")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(workspace).toHaveAttribute("data-views-over-agents", "true");
    expect(await agentArea.boundingBox()).toEqual(agentBox);
    await screenshot(page, "s6-views-over-agents");
    // The resize the closed find bar gives back lands before the count starts.
    await expect
      .poll(async () => {
        const seen = resizes();
        await page.waitForTimeout(300);
        return resizes() === seen;
      })
      .toBe(true);
    const resizesAfterFind = resizes();

    // The toggle takes the panel down without closing a view and brings it
    // back; neither way resizes a terminal.
    await page.locator('[data-views-over-toggle="on"]').click();
    await expect(workspace).toHaveAttribute("data-views-over-agents", "false");
    await expect(page.locator("[data-view-area]")).toHaveCount(0);
    expect(last.get("workspace_view")).toEqual({ views_over_agents: false });
    // Down, the keyboard is back on the pane the core has focused.
    await expect
      .poll(() => page.evaluate(() => document.activeElement?.closest("[data-pane-view]")?.getAttribute("data-pane-view") ?? null))
      .toBe(child);
    await screenshot(page, "s6-views-over-agents-down");
    await page.locator('[data-views-over-toggle="off"]').click();
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(2);
    await expect.poll(async () => (await panel.boundingBox())!.width).toBeCloseTo(300, -1);
    // An agent chosen from the sidebar takes it down.
    await page.locator(`[data-agent-open="${parent}"]`).first().click();
    await expect(workspace).toHaveAttribute("data-views-over-agents", "false");
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await expect(page.locator('[data-views-over-toggle="off"]')).toBeVisible();
    expect(await agentArea.boundingBox()).toEqual(agentBox);
    expect(resizes()).toBe(resizesAfterFind);

    // A declared child shows as a chip under its parent's header; the core
    // moves the delegated pane to its own tab, so the chip crosses tabs with
    // one tracked focus, and the child's Return comes back the same way
    // (B14-B16).
    declareChild(herdr, child, parent);
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveCount(0, { timeout: 20_000 });
    const chip = page.locator(`[data-pane-children="${parent}"] [data-child-chip="${child}"]`);
    await expect(chip).toBeVisible({ timeout: 20_000 });
    await expect(chip).toHaveAttribute("aria-label", /Agent two/);
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
    await page.mouse.move(0, 0);
    await page.keyboard.press("Escape");
    await expect(openChild).toHaveCount(0);
    expect(sent.get("focus_pane") ?? 0).toBe(focusCount);
    // The focus the closed menu hands back to its button does not bring the
    // button's hint up after it; the hint waits for the pointer to come back.
    await page.waitForTimeout(800);
    await expect(page.locator('[data-slot="tooltip-content"]')).toHaveCount(0);

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
