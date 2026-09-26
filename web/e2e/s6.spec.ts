// The S6 flow on an isolated pinned Herdr and hided: a first run on Main, a Project's
// Overview and its Workspace (B1, B2, B4), the side panel's states from the
// toolbar, its strip and the palette with no terminal resized until it is
// pinned (issue 170), the Explorer and History as independent tools (B10),
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

test("Main, Overview and a Workspace with its side panel, tools and delegated child", async ({ page }) => {
  // Wide enough that the right-hand pane shows beside the panel at its
  // minimum (a View area and the tool column).
  await page.setViewportSize({ width: 1920, height: 1000 });
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

    // A first run starts on Main (D-11), whose Projects view lists this
    // machine's Project with real counts (B1).
    const workspace = page.locator("[data-workspace-screen]");
    await expect(page.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await expect(workspace).toHaveCount(0);
    await page.locator('[data-main-tab="projects"]').click();
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
    // A new Workspace starts with its agents alone: the side panel is
    // closed, and the Explorer it will show is stored on (issue 170).
    await expect(workspace).toHaveAttribute("data-panel", "closed");
    await expect(page.locator("[data-side-panel]")).toHaveCount(0);
    await expect(page.locator('[data-tool-toggle="explorer"]')).toHaveAttribute("aria-pressed", "false");

    // The toolbar toggle and the palette choose the panel's state; each is
    // one workspace_view. With no view open the panel is only as wide as the
    // Explorer column.
    const panel = page.locator("[data-side-panel]");
    const body = page.locator("[data-workspace-body]");
    const before = sent.get("workspace_view") ?? 0;
    await page.locator('[data-panel-toggle="off"]').click();
    await expect(workspace).toHaveAttribute("data-panel", "open");
    await expect(panel).toHaveAttribute("data-panel-content", "tools");
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    const toolColumn = await page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--size-panel-ideal")));
    expect((await panel.boundingBox())!.width).toBeCloseTo(toolColumn, 0);
    await page.keyboard.press("Meta+KeyK");
    await page.keyboard.type("Close side panel");
    await page.keyboard.press("Enter");
    await expect(workspace).toHaveAttribute("data-panel", "closed");
    expect(sent.get("workspace_view")).toBe(before + 2);
    expect(last.get("workspace_view")).toEqual({ panel: "closed" });

    // Explorer and History open and close on their own (B10), and one pressed
    // while the panel is closed opens the panel with it.
    await page.locator('[data-tool-toggle="changes"]').click();
    await expect(workspace).toHaveAttribute("data-panel", "open");
    expect(last.get("workspace_view")).toEqual({ changes: true });
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    await page.locator('[data-tool-close="explorer"]').click();
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    // With no tool and no view the panel says nothing is open, and offers
    // the Explorer back.
    await page.locator('[data-tool-toggle="changes"]').click();
    await expect(page.locator('[data-tool="changes"]')).toHaveCount(0);
    await expect(panel).toHaveAttribute("data-panel-content", "empty");
    await expect(panel.getByText("No file or diff is open in this Workspace.")).toBeVisible();
    await panel.locator("[data-empty-show-explorer]").click();
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    await expect(panel).toHaveAttribute("data-panel-content", "tools");

    // A file opened from the Explorer widens the panel to its stored width,
    // over agents that keep the body's width underneath, so no terminal
    // resizes; the agents left of the panel stay live (issue 170).
    const agentArea = page.locator("[data-agent-area]");
    const agentBox = (await agentArea.boundingBox())!;
    const bodyBox = (await body.boundingBox())!;
    expect(agentBox.width).toBeCloseTo(bodyBox.width, 0);
    const resizes = () => sent.get("terminal_resize") ?? 0;
    // Nothing still settling from the Workspace's first draw counts.
    await expect
      .poll(async () => {
        const seen = resizes();
        await page.waitForTimeout(300);
        return resizes() === seen;
      })
      .toBe(true);
    const resizesBefore = resizes();
    await page.locator(`[data-explorer-row="${path.join(root, "notes.md")}"]`).dblclick();
    await expect(panel).toHaveAttribute("data-panel-content", "views");
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(1);
    await page.locator(`[data-explorer-row="${path.join(root, "gone.txt")}"]`).dblclick();
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(2);
    const panelBox = (await panel.boundingBox())!;
    // Docked to the body's right edge, full height, the tool column inside it.
    expect(panelBox.x + panelBox.width).toBeCloseTo(bodyBox.x + bodyBox.width, 0);
    expect(panelBox.y).toBeCloseTo(bodyBox.y, 0);
    expect(panelBox.height).toBeCloseTo(bodyBox.height, 0);
    expect(panelBox.width).toBeGreaterThan(toolColumn);
    await expect(panel.locator('[data-tool="explorer"]')).toBeVisible();
    // The panel's actions sit at the right end of its one strip, above the
    // tool column.
    await expect(panel.locator('[data-workspace-tools] [data-panel-actions] [data-panel-pin="off"]')).toBeVisible();
    expect(await agentArea.boundingBox()).toEqual(agentBox);

    // Its left edge resizes it like any divider: one step per arrow key, a
    // guide line while dragging with nothing resized, one change on release.
    const edge = panel.locator("[data-panel-edge]");
    await edge.focus();
    const shareBefore = Number(await edge.getAttribute("aria-valuenow"));
    await page.keyboard.press("ArrowRight");
    await expect(edge).toHaveAttribute("aria-valuenow", String(shareBefore - 5));
    await expect.poll(async () => (await panel.boundingBox())!.width).toBeLessThan(panelBox.width);
    const narrowed = (await panel.boundingBox())!;
    const edgeBox = (await edge.boundingBox())!;
    await page.mouse.move(edgeBox.x + edgeBox.width / 2, edgeBox.y + 200);
    await page.mouse.down();
    const target = bodyBox.x + bodyBox.width - 600;
    await page.mouse.move(target, edgeBox.y + 200, { steps: 8 });
    await expect(page.locator("[data-panel-guide]")).toBeVisible();
    expect(await panel.boundingBox()).toEqual(narrowed);
    expect(resizes()).toBe(resizesBefore);
    await page.mouse.up();
    await expect(page.locator("[data-panel-guide]")).toHaveCount(0);
    await expect.poll(async () => (await panel.boundingBox())!.width).toBeCloseTo(600, -1);
    expect(last.get("workspace_view")).toEqual({ views_over_share: expect.any(Number) });
    const opened = (await panel.boundingBox())!;
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
    await expect(workspace).toHaveAttribute("data-panel", "open");
    // A tab of the Agent area's own strip is chosen in place too.
    await page.locator('[data-agent-tab-bar] [data-tab-kind="herdr"]').first().click();
    await expect.poll(() => last.get("focus_tab")).toMatchObject({ in_place: true });
    await expect(workspace).toHaveAttribute("data-panel", "open");
    await childHost.click({ position: { x: 40, y: 60 } });
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
    await expect(workspace).toHaveAttribute("data-panel", "open");
    expect(await agentArea.boundingBox()).toEqual(agentBox);
    await screenshot(page, "s6-side-panel-open");
    // The resize the closed find bar gives back lands before the count starts.
    await expect
      .poll(async () => {
        const seen = resizes();
        await page.waitForTimeout(300);
        return resizes() === seen;
      })
      .toBe(true);
    const resizesAfterFind = resizes();

    // Expand gives the panel the whole body over the agents, and Restore
    // width brings back its width; neither resizes a terminal.
    await panel.locator('[data-panel-expand="off"]').click();
    await expect(workspace).toHaveAttribute("data-panel", "expanded");
    expect(last.get("workspace_view")).toEqual({ panel: "expanded" });
    await expect.poll(async () => (await panel.boundingBox())!.width).toBeCloseTo(bodyBox.width, 0);
    expect(await agentArea.boundingBox()).toEqual(agentBox);
    await screenshot(page, "s6-side-panel-expanded");
    await panel.locator('[data-panel-expand="on"]').click();
    await expect(workspace).toHaveAttribute("data-panel", "open");
    await expect.poll(async () => (await panel.boundingBox())!.width).toBeCloseTo(opened.width, 0);

    // The toggle closes the panel without closing a view, and says how many
    // it keeps; reopened, the panel comes back at its width. Neither way
    // resizes a terminal.
    await page.locator('[data-panel-toggle="on"]').click();
    await expect(workspace).toHaveAttribute("data-panel", "closed");
    await expect(page.locator("[data-view-area]")).toHaveCount(0);
    expect(last.get("workspace_view")).toEqual({ panel: "closed" });
    await expect(page.locator("[data-panel-badge]")).toHaveText("2");
    // Closed, the keyboard is back on the pane the core has focused.
    await expect
      .poll(() => page.evaluate(() => document.activeElement?.closest("[data-pane-view]")?.getAttribute("data-pane-view") ?? null))
      .toBe(child);
    await screenshot(page, "s6-side-panel-closed");
    await page.locator('[data-panel-toggle="off"]').click();
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(2);
    await expect.poll(async () => (await panel.boundingBox())!.width).toBeCloseTo(opened.width, 0);
    expect(await agentArea.boundingBox()).toEqual(agentBox);
    await expect
      .poll(async () => {
        await page.waitForTimeout(300);
        return resizes();
      })
      .toBe(resizesAfterFind);

    // Pin docks the panel: the agents end at its left edge and their
    // terminals resize to the narrower width; unpinning floats it again and
    // gives them the body back.
    await panel.locator('[data-panel-pin="off"]').click();
    expect(last.get("workspace_view")).toEqual({ pinned: true });
    await expect(workspace).toHaveAttribute("data-panel-docked", "true");
    await expect.poll(async () => (await agentArea.boundingBox())!.width).toBeCloseTo(bodyBox.width - opened.width, 0);
    await expect.poll(resizes, { timeout: 10_000 }).toBeGreaterThan(resizesAfterFind);
    await screenshot(page, "s6-side-panel-pinned");
    // A pinned panel covers no agent, so an agent chosen from the sidebar
    // leaves it up.
    await page.locator(`[data-agent-open="${parent}"]`).first().click();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await expect(workspace).toHaveAttribute("data-panel", "open");
    const resizesPinned = resizes();
    await panel.locator('[data-panel-pin="on"]').click();
    await expect(workspace).toHaveAttribute("data-panel-docked", "false");
    await expect.poll(async () => (await agentArea.boundingBox())!.width).toBeCloseTo(bodyBox.width, 0);
    await expect.poll(resizes, { timeout: 10_000 }).toBeGreaterThan(resizesPinned);

    // Floating again, an agent chosen from the sidebar closes the panel.
    await page.locator(`[data-agent-open="${child}"]`).first().click();
    await expect(workspace).toHaveAttribute("data-panel", "closed");
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await expect(page.locator('[data-panel-toggle="off"]')).toBeVisible();
    expect(await agentArea.boundingBox()).toEqual(agentBox);
    await page.locator(`[data-agent-open="${parent}"]`).first().click();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });

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

    // A restart brings the Workspace back with its side panel and View
    // tabs; the file that went away stays as an unavailable tab (B19, B20).
    await page.keyboard.press("Meta+KeyK");
    await page.keyboard.type("Expand side panel");
    await page.keyboard.press("Enter");
    await expect(workspace).toHaveAttribute("data-panel", "expanded");
    fs.rmSync(path.join(herdr.root, "fixture", "gone.txt"));
    daemon = await daemon.restart();
    // Reopening the app is a fresh page, not a reconnect of this one.
    await page.goto("about:blank");
    await open(page, daemon);
    await expect(page.locator("[data-workspace-screen]")).toHaveAttribute("data-panel", "expanded", { timeout: 20_000 });
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(2, { timeout: 15_000 });
    await expect(page.locator('[data-view-area] [data-unavailable="true"]')).toHaveCount(1);
    await expect(page.locator("[data-view-area] [data-close-unavailable]")).toBeVisible();
    await screenshot(page, "s6-restored-unavailable");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
