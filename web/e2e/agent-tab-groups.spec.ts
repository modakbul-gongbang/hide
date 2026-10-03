import { expect, test, type Locator, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, declareParent } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";
import { chord } from "./chords";

test.describe.configure({ timeout: 180_000 });
test.use({ actionTimeout: 15_000 });
async function box(node: Locator) { const rect = await node.boundingBox(); if (!rect) throw new Error("Missing drag target"); return rect; }
async function drag(page: Page, tab: Locator, target: { x: number; y: number }, during?: () => Promise<void>) {
  await tab.scrollIntoViewIfNeeded();
  const rect = await box(tab);
  const start = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  await page.mouse.move(start.x + 12, start.y + 2, { steps: 3 });
  await page.mouse.move(target.x, target.y, { steps: 10 });
  await during?.();
  await page.mouse.up();
}
const areas = (page: Page) => page.locator("[data-agent-area-id]");
const tab = (page: Page, id: string) => page.locator(`[data-agent-tab-bar] [data-tab="${id}"]`);
async function shape(page: Page) {
  return areas(page).evaluateAll((nodes) => nodes.map((area) => ({
    id: area.getAttribute("data-agent-area-id"), active: area.getAttribute("data-active-area"),
    tabs: [...area.querySelectorAll("[role=tab][data-tab]")].map((tab) => tab.getAttribute("data-tab")),
    shown: area.querySelector("[data-canvas]")?.getAttribute("data-canvas"),
  })));
}

// Quarantined: runs in CI without blocking `verify` until #303 is fixed.
test("Agent pointer drags split live canvases, reorder, move, cancel, resize, collapse and restore", { tag: "@flaky", annotation: { type: "issue", description: "https://github.com/modakbul-gongbang/hide/issues/303" } }, async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const created = ["second", "third"].map((label) => herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", label, "--no-focus"]) as { result: { tab: { tab_id: string }; root_pane: { pane_id: string } } });
    const [first, second, third] = [herdr.tab, ...created.map((row) => row.result.tab.tab_id)];
    daemon = await startHided(herdr, "agent-groups");
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(tab(page, third!)).toBeVisible();
    const original = await shape(page);
    expect(original[0]?.tabs).toEqual([first, second, third]);
    const body = await box(page.locator("[data-agent-body]").first());
    const beforeResize = sent.get("terminal_resize") ?? 0;
    await drag(page, tab(page, third!), { x: body.x + body.width * .92, y: body.y + body.height / 2 }, async () => {
      await expect(page.locator('[data-agent-drop="right"]')).toHaveText("Split right");
      await expect(page.locator("[data-agent-drag-tab]")).toBeVisible();
      expect(await shape(page)).toEqual(original);
      expect(sent.get("terminal_resize") ?? 0).toBe(beforeResize);
      await screenshot(page, "agent-groups-split-preview");
    });
    await expect(areas(page)).toHaveCount(2);
    await expect(areas(page).nth(0).locator("[data-canvas]")).toHaveAttribute("data-canvas", first!);
    await expect(areas(page).nth(1).locator("[data-canvas]")).toHaveAttribute("data-canvas", third!);
    expect(sent.get("agent_layout.split")).toBe(1);
    await expect(page.locator('[data-transport="released"]')).toHaveCount(0);
    // Both shown tabs keep receiving fresh output while the other owns focus.
    const live = [herdr.panes[0], created[1]!.result.root_pane.pane_id];
    for (const [index, pane] of live.entries()) execFileSync(herdr.bin, ["pane", "run", pane, `for n in 1 2 3; do printf 'area-${index}-live-%s\\n' "$n"; sleep 0.1; done`], { env: herdr.env, timeout: 10_000 });
    for (const [index, pane] of live.entries()) await expect.poll(() => page.evaluate((id) => window.__hideProbe?.paneText(id) ?? "", pane)).toContain(`area-${index}-live-3`);
    await screenshot(page, "agent-groups-two-live-areas");
    await page.keyboard.press(chord("settings"));
    await page.locator('[data-settings-tab="appearance"]').click();
    await page.locator('[data-theme-option="light"]').click();
    await expect(page.locator("html")).toHaveClass(/(^|\s)light(\s|$)/);
    await page.keyboard.press("Escape");
    await screenshot(page, "agent-groups-two-live-areas-light");
    const ownBody = await box(areas(page).nth(1).locator("[data-agent-body]"));
    const beforeInvalid = await shape(page);
    await drag(page, tab(page, third!), { x: ownBody.x + 5, y: ownBody.y + ownBody.height / 2 }, async () => {
      await expect(page.locator("[data-agent-drop]")).toHaveCount(0);
      await expect(page.locator("html")).toHaveAttribute("data-agent-drag", "forbidden");
    });
    expect(await shape(page)).toEqual(beforeInvalid);
    const right = await box(tab(page, third!));
    await drag(page, tab(page, second!), { x: right.x + right.width + 20, y: right.y + right.height / 2 });
    await expect.poll(async () => (await shape(page))[1]?.tabs).toEqual([third, second]);
    const last = await box(tab(page, second!));
    await drag(page, tab(page, third!), { x: last.x + last.width + 15, y: last.y + last.height / 2 });
    await expect.poll(async () => (await shape(page))[1]?.tabs).toEqual([second, third]);
    const beforeCancel = await shape(page);
    const target = await box(tab(page, first!));
    await drag(page, tab(page, third!), { x: target.x + target.width + 15, y: target.y + target.height / 2 }, async () => {
      await expect(page.locator('[data-agent-drop="bar"]')).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(page.locator("[data-agent-drop]")).toHaveCount(0);
    });
    expect(await shape(page)).toEqual(beforeCancel);
    const divider = page.locator("[data-agent-divider]");
    const boundary = await box(divider);
    await page.mouse.move(boundary.x + boundary.width / 2, boundary.y + 150);
    await page.mouse.down();
    await page.mouse.move(boundary.x - 130, boundary.y + 150, { steps: 12 });
    await expect(page.locator("[data-agent-resize-guide]")).toBeVisible();
    await page.mouse.up();
    await expect.poll(async () => Number(await divider.getAttribute("aria-valuenow"))).toBeLessThan(50);
    await screenshot(page, "agent-groups-resized");
    const wideShape = await shape(page);
    await page.setViewportSize({ width: 700, height: 900 });
    await expect(areas(page)).toHaveCount(1);
    await expect(page.locator("[data-agent-area-switch]")).toBeVisible();
    await screenshot(page, "agent-groups-narrow");
    await page.setViewportSize({ width: 1920, height: 1080 });
    await expect.poll(() => shape(page)).toEqual(wideShape);
    const restoredShape = await shape(page);
    const ratio = await divider.getAttribute("aria-valuenow");
    await expect.poll(() => fs.existsSync(path.join(daemon!.stateDir, "workspace-views.json"))).toBe(true);
    daemon = await daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await expect.poll(() => shape(page)).toEqual(restoredShape);
    await expect(divider).toHaveAttribute("aria-valuenow", ratio!);
    await screenshot(page, "agent-groups-restarted");
    const destination = await box(tab(page, second!));
    await drag(page, tab(page, first!), { x: destination.x + destination.width / 4, y: destination.y + destination.height / 2 }, async () => {
      await expect(page.locator('[data-agent-drop="bar"]')).toBeVisible();
    });
    await expect(areas(page)).toHaveCount(1);
    expect((await shape(page))[0]?.tabs).toEqual([first, second, third]);
    expect(sent.get("reorder_tab") ?? 0).toBe(0);
    await screenshot(page, "agent-groups-collapsed");
    for (const edge of ["left", "up", "down"] as const) {
      const content = await box(page.locator("[data-agent-body]"));
      const target = {
        x: content.x + content.width * (edge === "left" ? .05 : .5),
        y: content.y + content.height * (edge === "up" ? .05 : edge === "down" ? .95 : .5),
      };
      await drag(page, tab(page, third!), target, async () => {
        await expect(page.locator(`[data-agent-drop="${edge}"]`)).toHaveText(`Split ${edge}`);
      });
      await expect(areas(page)).toHaveCount(2);
      const destination = await box(tab(page, first!));
      // The sidebar's centered resize grab owns the column's first 10px.
      // The first quarter of the tab is inside its bar, before its midpoint.
      await drag(page, tab(page, third!), { x: destination.x + destination.width / 4, y: destination.y + destination.height / 2 }, async () => {
        await expect(page.locator('[data-agent-drop="bar"]')).toBeVisible();
      });
      await expect(areas(page)).toHaveCount(1);
    }
    await page.locator('[data-panel-toggle="off"]').click();
    const panel = page.locator("[data-side-panel]");
    await expect(panel).toBeVisible();
    const floating = await box(panel);
    const beforePanelDrop = await shape(page);
    await drag(page, tab(page, first!), { x: floating.x + floating.width - 5, y: floating.y + floating.height / 2 }, async () => {
      await expect(page.locator("[data-agent-drop]")).toHaveCount(0);
      await expect(page.locator("html")).toHaveAttribute("data-agent-drag", "forbidden");
    });
    expect(await shape(page)).toEqual(beforePanelDrop);
  } finally { daemon?.stop(); herdr.stop(); }
});

test("New tab and Reopen use the requested area and Rename works in either bar", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const second = herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--no-focus"]) as { result: { tab: { tab_id: string } } };
    const secondId = second.result.tab.tab_id;
    daemon = await startHided(herdr, "agent-placement");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const body = await box(page.locator("[data-agent-body]"));
    await drag(page, tab(page, secondId), { x: body.x + body.width * .95, y: body.y + body.height / 2 });
    await expect(areas(page)).toHaveCount(2);
    const leftId = (await shape(page))[0]!.id;
    const left = page.locator(`[data-agent-area-id="${leftId}"]`);
    await left.getByRole("button", { name: /^New tab/ }).click();
    await expect(left.locator('[role="tab"]')).toHaveCount(2);
    const createdId = (await shape(page))[0]!.tabs.find((id) => id !== herdr.tab)!;
    await expect(left.locator("[data-canvas]")).toHaveAttribute("data-canvas", createdId);
    for (const id of [createdId, secondId]) {
      await tab(page, id).click({ button: "right" });
      await page.locator('[data-menu-item="rename_tab"]').click();
      const input = page.getByRole("textbox", { name: "Tab name", exact: true });
      await expect(input).toBeFocused();
      await input.fill(`영역 ${id}`);
      await input.press("Enter");
      await expect(input).toHaveCount(0);
      await expect(tab(page, id)).toContainText(`영역 ${id}`);
    }
    await tab(page, createdId).hover();
    await tab(page, createdId).getByRole("button", { name: /^Close tab/ }).click();
    await expect(tab(page, createdId)).toHaveCount(0);
    await tab(page, secondId).click();
    await page.keyboard.press(chord("reopen_closed_tab"));
    await expect(left.locator('[role="tab"]')).toHaveCount(2);
    await expect(left).toHaveAttribute("data-active-area", "true");
    await tab(page, secondId).hover();
    await tab(page, secondId).getByRole("button", { name: /^Close tab/ }).click();
    await expect(areas(page)).toHaveCount(1);
    await page.keyboard.press(chord("reopen_closed_tab"));
    await expect(left.locator('[role="tab"]')).toHaveCount(3);
    await expect(areas(page)).toHaveCount(1);
    await screenshot(page, "agent-groups-reopen-placement");
  } finally { daemon?.stop(); herdr.stop(); }
});

// Quarantined: runs in CI without blocking `verify` until #287 is fixed.
test("Delegated canvas returns to its normal tab and its tab menu keeps the Agent commands", { tag: "@flaky", annotation: { type: "issue", description: "https://github.com/modakbul-gongbang/hide/issues/287" } }, async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [parent, child] = herdr.panes;
    daemon = await startHided(herdr, "agent-delegated-return");
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    declareParent(herdr, child, parent);
    const chip = page.locator(`[data-child-chip="${child}"]`).first();
    await expect(chip).toBeVisible({ timeout: 20_000 });
    // The chip shows once the lineage is known; the delegated canvas exists
    // once the core has moved the child out of its parent's tab.
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveCount(0);
    await chip.click();
    await expect(page.locator(`[data-pane-view="${child}"]`)).toHaveAttribute("data-focused", "true");
    const before = sent.get("agent_layout.focus") ?? 0;
    await tab(page, herdr.tab).click();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toHaveAttribute("data-focused", "true");
    await expect(tab(page, herdr.tab)).toHaveAttribute("aria-selected", "true");
    expect(sent.get("agent_layout.focus")).toBe(before + 1);
    await tab(page, herdr.tab).click({ button: "right" });
    await expect(page.locator('[role="menu"] [data-menu-item="split_right"]')).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator('[role="menu"]')).toHaveCount(0);
    await screenshot(page, "agent-groups-delegated-return");
  } finally { daemon?.stop(); herdr.stop(); }
});

test("An external focused creation joins a bar without replacing either shown canvas", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const second = herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", "second", "--no-focus"]) as { result: { tab: { tab_id: string } } };
    daemon = await startHided(herdr, "agent-external-focus");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(tab(page, second.result.tab.tab_id)).toBeVisible();
    const body = await box(page.locator("[data-agent-body]"));
    await drag(page, tab(page, second.result.tab.tab_id), { x: body.x + body.width - 5, y: body.y + body.height / 2 });
    await expect(areas(page)).toHaveCount(2);
    const before = (await shape(page)).map(({ id, active, shown }) => ({ id, active, shown }));
    const external = herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", "외부 생성", "--focus"]) as { result: { tab: { tab_id: string } } };
    await expect(tab(page, external.result.tab.tab_id)).toBeVisible();
    // Wait through the actual separate creation/layout/focus stream, including
    // the next read-only projection, rather than asserting its first frame.
    await expect.poll(async () => (await shape(page)).map(({ id, active, shown }) => ({ id, active, shown }))).toEqual(before);
    await page.waitForTimeout(1500);
    expect((await shape(page)).map(({ id, active, shown }) => ({ id, active, shown }))).toEqual(before);
    // A genuinely later external focus still selects a known tab.
    // Pinned Herdr emits no event for a no-op focus on its current tab.
    // Exercise an observable external focus transition after creation instead.
    herdr.run(["tab", "focus", second.result.tab.tab_id]);
    await page.waitForTimeout(300);
    herdr.run(["tab", "focus", external.result.tab.tab_id]);
    await expect(tab(page, external.result.tab.tab_id)).toHaveAttribute("aria-selected", "true");
    await expect(page.locator(`[data-canvas="${external.result.tab.tab_id}"]`)).toBeVisible();
    await screenshot(page, "agent-groups-external-focus");
  } finally { daemon?.stop(); herdr.stop(); }
});
