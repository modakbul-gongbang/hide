// The S6 flow on an isolated pinned Herdr and hided: a first run on Main, a Project's
// Overview and its Workspace (B1, B2, B4), the Agent Views, File Views and
// Tools columns from the toolbar, its menu and their chords, with a terminal
// resized once per column change and never inside File Views (PRD
// three-column-panel), the Explorer and History as one tool at a time (B10),
// a delegated child reached from its parent's chip and left by its Return
// (B14-B16), and a restart that brings the Workspace back with a View tab
// whose file is gone marked unavailable (B19, B20).

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { declareParent, herdrHasFocus, startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { chooseColumn, countSent, screenshot } from "./wire";
import { chord, commandLabel } from "./chords";
import { openCurrentProjectOverview } from "./overview-entry";
import { quietFor, unchangedForFrames } from "./wait";

test.describe.configure({ timeout: 120_000 });

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
}

test("Main, Overview and a Workspace with its columns, tools and delegated child", async ({ page }) => {
  // Wide enough for all three columns at their default widths.
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

    // #350's project card opens its checkout. Shared Overview's scope
    // opens on the request view (overview-request-view D-05);
    // the Agents graph's rows list both agents, and an agent enters the
    // Workspace at that pane (B2).
    await project.click();
    await openCurrentProjectOverview(page, "fixture");
    await expect(page.locator('[data-overview-screen][data-overview-view="requests"]')).toBeVisible();
    await page.locator('[data-lens-tile-button="agents"]').click();
    await expect(page.locator('[data-overview-screen][data-overview-view="agents"]')).toBeVisible();
    await expect(page.locator("[data-overview-screen] [data-graph-open]")).toHaveCount(2);
    await screenshot(page, "s6-overview");
    await page.locator(`[data-overview-screen] [data-graph-open="${parent}"]`).click();
    await expect(workspace).toBeVisible();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true");
    // A new Workspace shows Agent Views alone: File Views and Tools are off
    // and Tools holds the Explorer (B2). The toolbar spans the body and ends
    // with Overview, Open server, File Views and Tools as icons only (#349 D-07).
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    await expect(workspace).toHaveAttribute("data-tools", "off");
    await expect(page.locator('[data-column="views"], [data-column="tools"]')).toHaveCount(0);
    const toggles = page.locator("[data-workspace-toolbar] [data-column-toggles] button");
    await expect(toggles).toHaveCount(4);
    expect(await toggles.evaluateAll((buttons) => buttons.map((button) => [button.getAttribute("aria-label"), button.textContent?.trim() ?? ""]))).toEqual([
      ["Overview", ""],
      ["Open server", ""],
      ["File Views", ""],
      ["Tools", ""],
    ]);
    const body = page.locator('[data-column-row="true"]');
    const toolbarBox = (await page.locator("[data-workspace-toolbar]").boundingBox())!;
    const bodyBox = (await body.boundingBox())!;
    expect(toolbarBox.width).toBeCloseTo(bodyBox.width, 0);
    expect(toolbarBox.y + toolbarBox.height).toBeLessThanOrEqual(bodyBox.y + 1);

    // B7, B8: each icon names itself with its chord in the tooltip and
    // reads pressed only while its column shows.
    const viewsToggle = page.locator('[data-column-toggle="views"]');
    const toolsToggle = page.locator('[data-column-toggle="tools"]');
    await expect(viewsToggle).toHaveAttribute("aria-pressed", "false");
    await toolsToggle.hover();
    await expect(page.locator('[data-slot="tooltip-content"]')).toContainText("Tools");
    await expect(page.locator('[data-slot="tooltip-content"]')).toContainText(commandLabel("toggle_explorer"));
    await viewsToggle.hover();
    await expect(page.locator('[data-slot="tooltip-content"]')).toContainText("File Views");
    await expect(page.locator('[data-slot="tooltip-content"]')).toContainText(commandLabel("toggle_right_panel"));
    await page.mouse.move(bodyBox.x + 40, bodyBox.y + 40);

    // The Tools icon turns Tools on by result value, alone beside the agents
    // (B12, Tools only is a state), and the toolbar's menu turns it off.
    const toolsColumn = page.locator('[data-column="tools"]');
    const viewsColumn = page.locator('[data-column="views"]');
    const agentArea = page.locator('[data-column="agents"]');
    const before = sent.get("workspace_view") ?? 0;
    await toolsToggle.click();
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await expect(toolsToggle).toHaveAttribute("aria-pressed", "true");
    expect(last.get("workspace_view")).toEqual({ tools: true });
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    const toolColumn = await page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--size-panel-ideal")));
    expect((await toolsColumn.boundingBox())!.width).toBeCloseTo(toolColumn, 0);
    await chooseColumn(page, "tools");
    await expect(workspace).toHaveAttribute("data-tools", "off");
    expect(sent.get("workspace_view")).toBe(before + 2);
    expect(last.get("workspace_view")).toEqual({ tools: false });

    // ⌘E turns Tools on; its icon tabs swap the one tool it holds, and a
    // tool kept while Tools is off comes back with it.
    await page.keyboard.press(chord("toggle_explorer"));
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await toolsColumn.locator('[data-tool-tab="changes"]').click();
    expect(last.get("workspace_view")).toEqual({ tool: "changes" });
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await expect(page.locator('[data-tool="explorer"]')).toHaveCount(0);
    await page.keyboard.press(chord("toggle_explorer"));
    await expect(toolsColumn).toHaveCount(0);
    await page.keyboard.press(chord("toggle_explorer"));
    await expect(page.locator('[data-tool="changes"]')).toBeVisible();
    await toolsColumn.locator('[data-tool-tab="explorer"]').click();
    await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
    // The sidebar switch keeps no chord of its own: the chords above left it on Projects.
    await expect(page.locator("[data-sidebar]")).toHaveAttribute("data-sidebar", "projects");

    // Tools' divider shows its grip on hover and while dragging, moves a
    // guide during the drag, and lands one width on release (B20, B21).
    const toolsDivider = page.locator('[data-column-divider="tools"]');
    const grip = toolsDivider.locator("[data-column-grip]");
    const opacity = () => grip.evaluate((element) => getComputedStyle(element).opacity);
    expect(await opacity()).toBe("0");
    const toolsDividerBox = (await toolsDivider.boundingBox())!;
    await page.mouse.move(toolsDividerBox.x + toolsDividerBox.width / 2, toolsDividerBox.y + 200);
    await expect.poll(opacity).toBe("1");
    await page.mouse.down();
    await page.mouse.move(bodyBox.x + bodyBox.width - 500, toolsDividerBox.y + 200, { steps: 8 });
    await expect(page.locator("[data-column-guide] [data-column-grip]")).toBeVisible();
    await page.mouse.up();
    await expect(page.locator("[data-column-guide]")).toHaveCount(0);
    expect(last.get("workspace_view")).toEqual({ tools_width: expect.any(Number) });
    await expect.poll(async () => (await toolsColumn.boundingBox())!.width).toBeCloseTo(496, -1);
    await page.mouse.move(bodyBox.x + 40, bodyBox.y + 40);
    await expect(toolsDivider).toHaveAttribute("role", "separator");

    // ⌘⇧B with no view open opens File Views on the New tab page (B16), and
    // closing that untouched tab turns File Views off again; Tools stays.
    await page.keyboard.press(chord("toggle_right_panel"));
    expect(last.get("workspace_view")).toEqual({ views: true });
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(viewsColumn.locator("[data-new-tab-page]")).toBeVisible();
    await viewsColumn.locator('button[aria-label^="Close view"]').first().click();
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    await expect(workspace).toHaveAttribute("data-tools", "shown");

    // A file opened from the Explorer turns File Views on between the agents
    // and Tools; the agents resize once to the narrower width (B3, B4, B18).
    const resizes = () => sent.get("terminal_resize") ?? 0;
    const settled = () => unchangedForFrames(page, resizes);
    await settled();
    const agentsBeforeViews = (await agentArea.boundingBox())!;
    const resizesBeforeViews = resizes();
    await page.locator(`[data-explorer-row="${path.join(root, "notes.md")}"]`).dblclick();
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(viewsColumn.locator('[data-tab-kind="file"]')).toHaveCount(1);
    await expect.poll(async () => (await agentArea.boundingBox())!.width).toBeLessThan(agentsBeforeViews.width - 300);
    await expect.poll(resizes, { timeout: 10_000 }).toBeGreaterThan(resizesBeforeViews);
    await settled();
    // Once: one resize for each pane on screen, none for a frame between.
    const panesShown = await page.locator("[data-pane-view]").count();
    expect(resizes() - resizesBeforeViews).toBe(panesShown);
    // Left to right, docked edge to edge: Agent Views, File Views, Tools.
    const agentsBox = (await agentArea.boundingBox())!;
    const viewsBox = (await viewsColumn.boundingBox())!;
    const toolsBox = (await toolsColumn.boundingBox())!;
    expect(agentsBox.x + agentsBox.width).toBeLessThanOrEqual(viewsBox.x);
    expect(viewsBox.x + viewsBox.width).toBeLessThanOrEqual(toolsBox.x);
    expect(toolsBox.x + toolsBox.width).toBeCloseTo(bodyBox.x + bodyBox.width, 0);
    for (const box of [agentsBox, viewsBox, toolsBox]) expect(box.height).toBeCloseTo(bodyBox.height, 0);

    // Inside File Views nothing resizes a terminal: opening another file,
    // switching its tabs (B19).
    const resizesInViews = resizes();
    await page.locator(`[data-explorer-row="${path.join(root, "gone.txt")}"]`).dblclick();
    await expect(viewsColumn.locator('[data-tab-kind="file"]')).toHaveCount(2);
    await viewsColumn.locator('[data-tab-kind="file"]').first().click();
    await quietFor(page, 500, "opening a file view sends nothing more");
    expect(resizes()).toBe(resizesInViews);
    expect(await agentArea.boundingBox()).toEqual(agentsBox);

    // File Views' divider: one step per arrow key, a guide while dragging
    // with nothing resized, one change on release (B19, B20).
    const viewsDivider = page.locator('[data-column-divider="views"]');
    await viewsDivider.focus();
    const widthBefore = Number(await viewsDivider.getAttribute("aria-valuenow"));
    await page.keyboard.press("ArrowRight");
    await expect(viewsDivider).toHaveAttribute("aria-valuenow", String(widthBefore - 32));
    expect(last.get("workspace_view")).toEqual({ views_width: widthBefore - 32, width_request_id: expect.any(String) });
    await settled();
    const widened = (await viewsColumn.boundingBox())!;
    const resizesBeforeDrag = resizes();
    const viewsDividerBox = (await viewsDivider.boundingBox())!;
    await page.mouse.move(viewsDividerBox.x + viewsDividerBox.width / 2, viewsDividerBox.y + 200);
    await page.mouse.down();
    await page.mouse.move(viewsDividerBox.x + viewsDividerBox.width / 2 + 150, viewsDividerBox.y + 200, { steps: 8 });
    await expect(page.locator("[data-column-guide]")).toBeVisible();
    expect(await viewsColumn.boundingBox()).toEqual(widened);
    expect(resizes()).toBe(resizesBeforeDrag);
    await page.mouse.up();
    await expect(page.locator("[data-column-guide]")).toHaveCount(0);
    await expect.poll(async () => (await viewsColumn.boundingBox())!.width).toBeCloseTo(widened.width - 150, -1);
    expect(last.get("workspace_view")).toEqual({ views_width: expect.any(Number) });

    // The divider between File Views and Tools trades their widths, so the
    // agents keep theirs and no terminal resizes.
    await settled();
    const agentsBeforeTrade = (await agentArea.boundingBox())!;
    const resizesBeforeTrade = resizes();
    const tradeBox = (await toolsDivider.boundingBox())!;
    await page.mouse.move(tradeBox.x + tradeBox.width / 2, tradeBox.y + 200);
    await page.mouse.down();
    await page.mouse.move(tradeBox.x + 60, tradeBox.y + 200, { steps: 6 });
    await page.mouse.up();
    await expect.poll(() => last.get("workspace_view")).toEqual({ views_width: expect.any(Number), tools_width: expect.any(Number) });
    await quietFor(page, 500, "the width commit is the last one");
    expect(await agentArea.boundingBox()).toEqual(agentsBeforeTrade);
    expect(resizes()).toBe(resizesBeforeTrade);
    await page.mouse.move(bodyBox.x + 40, bodyBox.y + 40);

    // The agents beside the columns are live: a click focuses the pane,
    // typing reaches it, and ⌘F finds in that pane; in a View area ⌘F is the
    // document's (B23).
    const childHost = page.locator(`[data-pane-view="${child}"] [data-terminal-host]`);
    await childHost.click({ position: { x: 40, y: 60 } });
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    expect(last.get("focus_pane")).toEqual({ pane_id: child, origin: "operator" });
    await page.keyboard.type("typed beside the columns");
    await expect.poll(() => fs.readFileSync(herdr.inputLogs[1], "utf8"), { timeout: 10_000 }).toContain("typed beside the columns");
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    const mouseReports = () => fs.readFileSync(herdr.inputLogs[1], "latin1").split("\x1b[<0;").length - 1;
    const reportsBefore = mouseReports();
    await childHost.click({ position: { x: 40, y: 60 } });
    await expect.poll(mouseReports, { timeout: 10_000 }).toBe(reportsBefore + 2);
    const heardBefore = fs.readFileSync(herdr.inputLogs[1], "latin1").length;
    await page.keyboard.press(chord("find_in_pane"));
    await expect.poll(() => JSON.stringify(fs.readFileSync(herdr.inputLogs[1], "latin1").slice(heardBefore)), { timeout: 10_000 }).toBe(JSON.stringify("\x0f/"));
    expect(last.get("pane_find_open")).toMatchObject({ pane_id: child });
    await expect(page.locator(".cm-search")).toHaveCount(0);
    await expect(page.locator("[data-find-bar]")).toHaveCount(0);
    await viewsColumn.locator("[data-editor-body] .cm-content").click();
    await page.keyboard.press(chord("find_in_pane"));
    await expect(page.locator(".cm-search")).toBeVisible();
    await page.keyboard.press("Escape");
    await screenshot(page, "s6-columns-wide");

    // ⌘⇧B turns File Views off without closing a view; the icon counts the
    // views it keeps, and the keyboard that was in File Views goes back to
    // the focused pane (B9, B11, B24). Turning it on brings the same tabs.
    await settled();
    const resizesBeforeHide = resizes();
    await viewsColumn.locator("[data-editor-body] .cm-content").click();
    await page.keyboard.press(chord("toggle_right_panel"));
    await expect(workspace).toHaveAttribute("data-file-views", "off");
    expect(last.get("workspace_view")).toEqual({ views: false });
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await expect(viewsToggle).toHaveAttribute("aria-pressed", "false");
    await expect(page.locator("[data-column-badge]")).toHaveText("2");
    await expect(viewsToggle).toHaveAttribute("aria-description", "2 views open");
    // The badge is drawn whole on the icon's top-right corner, inside the toolbar.
    const badge = (await page.locator("[data-column-badge]").boundingBox())!;
    const toggleBox = (await viewsToggle.boundingBox())!;
    expect(badge.y).toBeGreaterThanOrEqual(toolbarBox.y);
    expect(badge.x).toBeGreaterThanOrEqual(toggleBox.x + toggleBox.width / 2);
    expect(badge.y).toBeLessThan(toggleBox.y + toggleBox.height / 2);
    await expect
      .poll(() => page.evaluate(() => document.activeElement?.closest("[data-pane-view]")?.getAttribute("data-pane-view") ?? null))
      .not.toBeNull();
    await expect.poll(resizes, { timeout: 10_000 }).toBeGreaterThan(resizesBeforeHide);
    await settled();
    expect(resizes() - resizesBeforeHide).toBe(panesShown);
    await screenshot(page, "s6-columns-views-off");
    await viewsToggle.click();
    expect(last.get("workspace_view")).toEqual({ views: true });
    await expect(viewsColumn.locator('[data-tab-kind="file"]')).toHaveCount(2);
    await expect(page.locator("[data-column-badge]")).toHaveCount(0);

    // An agent chosen from the sidebar changes no column (B22, D-17).
    await page.locator('[data-sidebar-mode="agents"]').click();
    await page.locator(`[data-agent-open="${parent}"]`).first().click();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await expect.poll(() => herdrHasFocus(herdr, parent), { timeout: 15_000 }).toBe(true);

    // B25-B27: the body decides how many columns show, and a narrow body
    // stores nothing. Between the steps Tools gives way to File Views and
    // comes back in its place when called; below them one column shows.
    const sentBeforeNarrow = sent.get("workspace_view") ?? 0;
    await page.setViewportSize({ width: 1300, height: 1000 });
    await expect(workspace).toHaveAttribute("data-workspace-body", "mid");
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(workspace).toHaveAttribute("data-tools", "hidden");
    await page.keyboard.press(chord("toggle_explorer"));
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await expect(workspace).toHaveAttribute("data-file-views", "hidden");
    await expect(toolsToggle).toHaveAttribute("aria-pressed", "true");
    await expect(viewsToggle).toHaveAttribute("aria-pressed", "false");
    await page.keyboard.press(chord("toggle_right_panel"));
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(workspace).toHaveAttribute("data-tools", "hidden");
    expect(sent.get("workspace_view") ?? 0).toBe(sentBeforeNarrow);
    await screenshot(page, "s6-columns-mid");
    await page.setViewportSize({ width: 1000, height: 1000 });
    await expect(workspace).toHaveAttribute("data-workspace-body", "narrow");
    await expect(workspace).toHaveAttribute("data-file-views", "hidden");
    await expect(workspace).toHaveAttribute("data-tools", "hidden");
    await expect(agentArea).toBeVisible();
    await viewsToggle.click();
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    expect((await viewsColumn.boundingBox())!.width).toBeCloseTo((await body.boundingBox())!.width, 0);
    await screenshot(page, "s6-columns-narrow");
    // The same icon again gives the body back to Agent Views and stores nothing.
    await viewsToggle.click();
    await expect(workspace).toHaveAttribute("data-file-views", "hidden");
    await expect(agentArea).toBeVisible();
    await page.keyboard.press(chord("toggle_explorer"));
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    // An agent chosen from the sidebar returns the body to Agent Views.
    await page.locator(`[data-agent-open="${child}"]`).first().click();
    await expect(workspace).toHaveAttribute("data-tools", "hidden");
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await expect.poll(() => herdrHasFocus(herdr, child), { timeout: 15_000 }).toBe(true);
    expect(sent.get("workspace_view") ?? 0).toBe(sentBeforeNarrow);
    // Widening brings back what the Workspace stores.
    await page.setViewportSize({ width: 1920, height: 1000 });
    await expect(workspace).toHaveAttribute("data-file-views", "shown");
    await expect(workspace).toHaveAttribute("data-tools", "shown");
    await page.locator(`[data-agent-open="${parent}"]`).first().click();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await expect.poll(() => herdrHasFocus(herdr, parent), { timeout: 15_000 }).toBe(true);

    // A declared child shows as a chip under its parent's header; the core
    // moves the delegated pane to its own tab, so the chip crosses tabs with
    // one tracked focus, and the child's Return comes back the same way
    // (B14-B16).
    declareParent(herdr, child, parent);
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
    await quietFor(page, 800, "the closed menu's button does not bring its hint up");
    await expect(page.locator('[data-slot="tooltip-content"]')).toHaveCount(0);

    // A restart brings the Workspace back with its columns, widths and View
    // tabs; the file that went away stays as an unavailable tab (S6 B19,
    // B20; PRD three-column-panel B29).
    await chooseColumn(page, "tools");
    await expect(workspace).toHaveAttribute("data-tools", "off");
    const viewsWidth = (await viewsColumn.boundingBox())!.width;
    // The file that goes away is the one in front, so its state shows after the restart.
    await viewsColumn.locator('[data-tab-kind="file"]', { hasText: "gone.txt" }).click();
    fs.rmSync(path.join(herdr.root, "fixture", "gone.txt"));
    daemon = await daemon.restart();
    // Reopening the app is a fresh page, not a reconnect of this one.
    await page.goto("about:blank");
    await open(page, daemon);
    await expect(page.locator("[data-workspace-screen]")).toHaveAttribute("data-file-views", "shown", { timeout: 20_000 });
    await expect(page.locator("[data-workspace-screen]")).toHaveAttribute("data-tools", "off");
    await expect.poll(async () => (await page.locator('[data-column="views"]').boundingBox())!.width).toBeCloseTo(viewsWidth, 0);
    await expect(page.locator('[data-view-area] [data-tab-kind="file"]')).toHaveCount(2, { timeout: 15_000 });
    await expect(page.locator('[data-view-area] [data-unavailable="true"]')).toHaveCount(1);
    await expect(page.locator("[data-view-area] [data-close-unavailable]")).toBeVisible();
    await screenshot(page, "s6-restored-unavailable");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
