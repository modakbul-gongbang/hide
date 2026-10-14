import { test, expect, type Page } from "@playwright/test";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { chooseTheme, countSent, enterWorkspace, rest, screenshot, sendEvent } from "./wire";

const glyph = (page: Page, tab: string) => page.locator(`[data-project-list] [data-sidebar-tab="${tab}"]`);
const guide = (page: Page, tab: string) => page.locator(`[data-project-list] [data-sidebar-tab-guide="${tab}"]`);
const segment = (page: Page, tab: string) => page.locator(`[data-project-list] [data-sidebar-tab-group="${tab}"]`);
const position = async (row: ReturnType<Page["locator"]>) => (await row.boundingBox())!;

test("a tab with two agents wears a glyph that opens it, names itself once renamed, and leaves with its second agent", { tag: "@platform" }, async ({ page }) => {
  // The platform's Herdr owns pane/process existence and the tab names.
  const herdr = await startHerdr();
  const daemon = await startHided(herdr, "sidebar-tab-segments");
  const sent = countSent(page);
  try {
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(glyph(page, herdr.tab)).toHaveAccessibleName("Tab 1, 2 agents");
    await expect(segment(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(2);
    // Unnamed: a glyph on the guide and no line of its own, so no `Tab N` text is drawn.
    await expect(page.locator("[data-sidebar-tab-name]")).toHaveCount(0);
    await screenshot(page, "sidebar-tab-glyph");

    const tabs = page.locator(`[data-tab="${herdr.tab}"]`).first();
    await tabs.click({ button: "right" });
    await page.locator('[data-menu-item="rename_tab"]').click();
    const input = page.getByRole("textbox", { name: "Tab name", exact: true });
    await input.fill("아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인");
    await input.press("Enter");
    await expect(glyph(page, herdr.tab)).toHaveAccessibleName("아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인, 2 agents");
    await expect(page.locator("[data-sidebar-tab-name]")).toHaveText("아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인");
    await screenshot(page, "sidebar-tab-named");

    // The glyph, like the old header, opens the real tab; it folds nothing and persists nothing.
    const before = sent.get("focus_pane") ?? 0;
    await glyph(page, herdr.tab).click();
    await expect(tabs).toHaveAttribute("aria-selected", "true");
    expect(sent.get("focus_pane") ?? 0).toBe(before);
    await expect(segment(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(2);

    // One agent left: a short segment with no glyph and no name line, even though the tab is still named.
    herdr.run(["pane", "close", herdr.panes[1]]);
    await expect(glyph(page, herdr.tab)).toHaveCount(0);
    await expect(page.locator("[data-sidebar-tab-name]")).toHaveCount(0);
    await expect(guide(page, herdr.tab)).toHaveCount(1);
    await expect(segment(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(1);

    // A second agent brings the glyph back with the retained name.
    const split = herdr.run(["pane", "split", herdr.panes[0], "--direction", "right", "--cwd", path.join(herdr.root, "fixture"), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as { result: { pane: { pane_id: string } } };
    herdr.run(["agent", "start", "returned", "--kind", "claude", "--pane", split.result.pane.pane_id]);
    await expect(glyph(page, herdr.tab)).toHaveAccessibleName("아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인, 2 agents");
    expect(sent.get("sidebar_tab_expanded") ?? 0).toBe(0);
  } finally { daemon.stop(); herdr.stop(); }
});

test("2-1-2 tab segments share one root column right of the kind glyph, with guides under it", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Awaited<ReturnType<typeof startHided>> | undefined;
  try {
    herdr.run(["tab", "rename", herdr.tab, "작업 묶음 A"]);
    const created = ["single", "작업 묶음 B"].map((name) => herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--env", `PATH=${herdr.fixturePath}`, "--label", name, "--no-focus"]) as { result: { tab: { tab_id: string }; root_pane: { pane_id: string } } });
    const single = created[0]!.result;
    const paired = created[1]!.result;
    herdr.run(["agent", "start", "single", "--kind", "claude", "--pane", single.root_pane.pane_id]);
    herdr.run(["agent", "start", "paired", "--kind", "claude", "--pane", paired.root_pane.pane_id]);
    const split = herdr.run(["pane", "split", paired.root_pane.pane_id, "--direction", "right", "--cwd", path.join(herdr.root, "fixture"), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as { result: { pane: { pane_id: string } } };
    herdr.run(["agent", "start", "paired-two", "--kind", "claude", "--pane", split.result.pane.pane_id]);
    daemon = await startHided(herdr, "sidebar-tab-boundary");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(glyph(page, herdr.tab)).toBeVisible();
    await expect(glyph(page, single.tab.tab_id)).toHaveCount(0);
    await expect(glyph(page, paired.tab.tab_id)).toHaveAccessibleName("작업 묶음 B, 2 agents");
    await page.evaluate(() => document.fonts.ready);

    // Every root's status mark stands on one column, right of the folder glyph, whichever tab holds it.
    const kind = await position(page.locator("[data-project-list] [data-project-menu] svg").first());
    const marks = await Promise.all((await page.locator("[data-project-list] li[data-pane][data-depth='0'] [data-agent-status-mark]").all()).map(position));
    expect(marks).toHaveLength(5);
    expect(new Set(marks.map((mark) => mark.x)).size).toBe(1);
    expect(marks[0]!.x).toBeGreaterThanOrEqual(kind.x + kind.width);

    // Each segment's guide is 1px, centered under the kind glyph, and the gap between segments is 8px.
    const center = kind.x + kind.width / 2;
    for (const tab of [herdr.tab, single.tab.tab_id, paired.tab.tab_id]) {
      const line = await position(guide(page, tab));
      expect(line.width).toBe(1);
      expect(Math.abs(line.x + line.width / 2 - center)).toBeLessThanOrEqual(0.5);
    }
    // A tab glyph stands on the guide's column too, and the head adds no row: the first row keeps the 28px a singleton row has.
    for (const tab of [herdr.tab, paired.tab.tab_id]) {
      const head = await position(segment(page, tab).locator("svg").first());
      expect(Math.abs(head.x + head.width / 2 - center)).toBeLessThanOrEqual(0.5);
    }
    const first = await position(segment(page, herdr.tab));
    const middle = await position(segment(page, single.tab.tab_id));
    const last = await position(segment(page, paired.tab.tab_id));
    expect(middle.y - first.y - first.height).toBe(8);
    expect(last.y - middle.y - middle.height).toBe(8);
    expect((await position(segment(page, single.tab.tab_id).locator("li[data-pane]"))).height).toBe(28);
    await rest(page);
    await screenshot(page, "sidebar-tab-segments");

    // Arrow keys visit the glyph at a segment's head and then its rows; Home and End reach the ends.
    await glyph(page, herdr.tab).focus();
    await glyph(page, herdr.tab).press("ArrowDown");
    await expect(segment(page, herdr.tab).locator("[data-agent-open]").first()).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(segment(page, herdr.tab).locator("[data-agent-open]").nth(1)).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(segment(page, single.tab.tab_id).locator("[data-agent-open]")).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(glyph(page, paired.tab.tab_id)).toBeFocused();
    await page.keyboard.press("Home");
    await expect(glyph(page, herdr.tab)).toBeFocused();
    await page.keyboard.press("End");
    await expect(segment(page, paired.tab.tab_id).locator("[data-agent-open]").last()).toBeFocused();
    await glyph(page, paired.tab.tab_id).focus();
    await glyph(page, paired.tab.tab_id).press("Enter");
    await expect(page.locator(`[data-tab="${paired.tab.tab_id}"]`).first()).toHaveAttribute("aria-selected", "true");
    await glyph(page, herdr.tab).focus();
    await glyph(page, herdr.tab).press("Space");
    await expect(page.locator(`[data-tab="${herdr.tab}"]`).first()).toHaveAttribute("aria-selected", "true");

    // Focus brings the glyph's hint, the name and the agent count.
    await glyph(page, herdr.tab).focus();
    await expect(page.getByRole("tooltip")).toHaveText("작업 묶음 A, 2 agents");
  } finally { daemon?.stop(); herdr.stop(); }
});

test("long manual tab names keep their hint and line inside the sidebar at 240 and 292 in both themes", async ({ page }) => {
  const herdr = await startHerdr();
  const daemon = await startHided(herdr, "sidebar-tab-narrow");
  const name = "아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인 · Sidebar tab segments and long names";
  try {
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await page.locator(`[data-tab="${herdr.tab}"]`).first().click({ button: "right" });
    await page.locator('[data-menu-item="rename_tab"]').click();
    const input = page.getByRole("textbox", { name: "Tab name", exact: true });
    await input.fill(name);
    await input.press("Enter");
    for (const theme of ["light", "dark"] as const) {
      await chooseTheme(page, theme);
      for (const width of [240, 292]) {
        await sendEvent(page, daemon, "ui_state_update", { sidebar_width: width, expanded_paths: [], selected_path: null });
        await expect.poll(async () => Math.round((await page.locator("nav[data-sidebar]").boundingBox())!.width)).toBe(width + 48);
        await expect(glyph(page, herdr.tab)).toHaveAccessibleName(`${name}, 2 agents`);
        expect(await segment(page, herdr.tab).evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
        const label = page.locator("[data-sidebar-tab-name]");
        await page.evaluate(() => document.fonts.ready);
        expect(await label.evaluate((node) => node.scrollWidth > node.clientWidth)).toBe(true);
        await glyph(page, herdr.tab).hover();
        await expect(page.getByRole("tooltip")).toHaveText(`${name}, 2 agents`);
        await screenshot(page, `sidebar-tab-tooltip-${theme}-${width}`);
        await rest(page);
        await screenshot(page, `sidebar-tab-${theme}-${width}`);
      }
    }
  } finally { daemon.stop(); herdr.stop(); }
});
