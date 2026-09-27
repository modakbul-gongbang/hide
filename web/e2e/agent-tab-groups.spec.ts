import { expect, test, type Locator, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

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

test("Agent pointer drags split live canvases, reorder, move, cancel, resize, collapse and restore", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const created = ["second", "third"].map((label) => herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", label, "--no-focus"]) as { result: { tab: { tab_id: string }; root_pane: { pane_id: string } } });
    const [first, second, third] = [herdr.tab, ...created.map((row) => row.result.tab.tab_id)];
    daemon = await startHided(herdr, "agent-groups");
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
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
    await screenshot(page, "agent-groups-two-live-areas");
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
    const restoredShape = await shape(page);
    const ratio = await divider.getAttribute("aria-valuenow");
    await expect.poll(() => fs.existsSync(path.join(daemon!.stateDir, "workspace-views.json"))).toBe(true);
    daemon = await daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect.poll(() => shape(page)).toEqual(restoredShape);
    await expect(divider).toHaveAttribute("aria-valuenow", ratio!);
    await screenshot(page, "agent-groups-restarted");
    const destination = await box(tab(page, second!));
    await drag(page, tab(page, first!), { x: destination.x + 5, y: destination.y + destination.height / 2 });
    await expect(areas(page)).toHaveCount(1);
    expect((await shape(page))[0]?.tabs).toEqual([first, second, third]);
    expect(sent.get("reorder_tab") ?? 0).toBe(0);
    await screenshot(page, "agent-groups-collapsed");
  } finally { daemon?.stop(); herdr.stop(); }
});
