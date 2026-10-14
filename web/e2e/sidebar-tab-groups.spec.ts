import { test, expect, type Page } from "@playwright/test";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { chooseTheme, countSent, enterWorkspace, rest, screenshot, sendEvent } from "./wire";

const header = (page: Page, tab: string) => page.locator(`[data-project-list] [data-sidebar-tab="${tab}"]`);
const fold = (page: Page, tab: string) => page.locator(`[data-project-list] [data-sidebar-tab-toggle="${tab}"]`);
const tree = (page: Page, tab: string) => page.locator(`[data-project-list] [data-sidebar-tab-group="${tab}"]`);

test("sidebar tab disclosure restores through singleton, agent return and daemon restart", { tag: "@platform" }, async ({ page }) => {
  // The platform's Herdr owns pane/process existence and the saved UI file.
  const herdr = await startHerdr();
  let daemon = await startHided(herdr, "sidebar-tab-disclosure");
  const sent = countSent(page);
  try {
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(header(page, herdr.tab)).toHaveAccessibleName("Tab 1, 2 agents");
    await expect(tree(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(2);
    const tabs = page.locator(`[data-tab="${herdr.tab}"]`).first();
    await tabs.click({ button: "right" });
    await page.locator('[data-menu-item="rename_tab"]').click();
    const input = page.getByRole("textbox", { name: "Tab name", exact: true });
    await input.fill("아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인");
    await input.press("Enter");
    await expect(header(page, herdr.tab)).toHaveAccessibleName("아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인, 2 agents");
    const beforeFocus = sent.get("focus_pane") ?? 0;
    await fold(page, herdr.tab).click();
    await expect(header(page, herdr.tab)).toHaveAttribute("aria-expanded", "false");
    await expect(tree(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(0);
    expect(sent.get("sidebar_tab_expanded")).toBe(1);
    expect(sent.get("focus_pane") ?? 0).toBe(beforeFocus);
    await expect(header(page, herdr.tab)).toHaveAttribute("aria-current", "true");
    await screenshot(page, "sidebar-tab-folded");
    herdr.run(["pane", "close", herdr.panes[1]]);
    await expect(header(page, herdr.tab)).toHaveCount(0);
    await expect(tree(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(1);
    await screenshot(page, "sidebar-tab-singleton");
    const split = herdr.run(["pane", "split", herdr.panes[0], "--direction", "right", "--cwd", path.join(herdr.root, "fixture"), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as { result: { pane: { pane_id: string } } };
    const replacement = herdr.run(["api", "snapshot"]) as { result: { snapshot: { panes: { pane_id: string; terminal_id: string; agent_status: string }[] } } };
    console.info("sidebar returned pane", JSON.stringify({ closed: herdr.panes[1], replacement: split.result.pane.pane_id, occupants: replacement.result.snapshot.panes.map(({ pane_id, terminal_id, agent_status }) => ({ pane_id, terminal_id, agent_status })) }));
    herdr.run(["agent", "start", "returned", "--kind", "claude", "--pane", split.result.pane.pane_id]);
    await expect(header(page, herdr.tab)).toHaveAttribute("aria-expanded", "false");
    await expect(header(page, herdr.tab)).toHaveAccessibleName("아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인, 2 agents");
    daemon = await daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(header(page, herdr.tab)).toHaveAttribute("aria-expanded", "false");
    await fold(page, herdr.tab).click();
    await expect(tree(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(2);
    await tabs.click({ button: "right" });
    await page.locator('[data-menu-item="rename_tab"]').click();
    await input.fill("");
    await input.press("Enter");
    await expect(header(page, herdr.tab)).toHaveAccessibleName("Tab 1, 2 agents");
    await screenshot(page, "sidebar-tab-restored");
  } finally { daemon.stop(); herdr.stop(); }
});

test("2-1-2 sidebar groups align titles and navigate only visible rows", async ({ page }) => {
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
    await expect(header(page, herdr.tab)).toBeVisible();
    await expect(header(page, single.tab.tab_id)).toHaveCount(0);
    await expect(header(page, paired.tab.tab_id)).toHaveAccessibleName("작업 묶음 B, 2 agents");
    const title = tree(page, herdr.tab).locator("[data-sidebar-tab-title]");
    const groupedTitle = tree(page, herdr.tab).locator("[data-agent-title]").first();
    const singleTitle = tree(page, single.tab.tab_id).locator("[data-agent-title]");
    const position = async (row: ReturnType<Page["locator"]>) => (await row.boundingBox())!;
    await page.evaluate(() => document.fonts.ready);
    expect((await position(groupedTitle)).x - (await position(title)).x).toBe(18);
    expect((await position(singleTitle)).x).toBe((await position(title)).x);
    const row = tree(page, herdr.tab).locator("[data-pane]").first();
    const sidebarBounds = await position(page.locator("nav[data-sidebar] > .sidebar-header-container"));
    const status = await position(row.locator("[data-agent-status-mark]"));
    const icon = await position(tree(page, herdr.tab).locator("[data-sidebar-tab-icon]"));
    const elapsed = await position(row.locator("[data-agent-elapsed]"));
    const count = await position(tree(page, herdr.tab).locator("[data-sidebar-tab-count]"));
    expect(status.x + status.width / 2).toBe(icon.x + icon.width / 2);
    expect((await position(title)).x - sidebarBounds.x).toBe(48);
    expect((await position(row)).height).toBe(28);
    expect(count.x + count.width - sidebarBounds.x).toBe(sidebarBounds.width - 32);
    expect(elapsed.x + elapsed.width).toBe(count.x + count.width);
    const first = await position(tree(page, herdr.tab));
    const middle = await position(tree(page, single.tab.tab_id));
    const last = await position(tree(page, paired.tab.tab_id));
    expect(middle.y - first.y - first.height).toBe(8);
    expect(last.y - middle.y - middle.height).toBe(8);
    await rest(page);
    await screenshot(page, "sidebar-tab-boundary-expanded");
    await fold(page, herdr.tab).click();
    await expect(header(page, herdr.tab)).toHaveAttribute("aria-expanded", "false");
    await expect(tree(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(0);
    await header(page, herdr.tab).focus();
    await header(page, herdr.tab).press("ArrowDown");
    await expect(tree(page, single.tab.tab_id).locator("[data-agent-open]")).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(header(page, paired.tab.tab_id)).toBeFocused();
    await page.keyboard.press("Home");
    await expect(header(page, herdr.tab)).toBeFocused();
    await page.keyboard.press("End");
    await expect(tree(page, paired.tab.tab_id).locator("[data-agent-open]").last()).toBeFocused();
    await header(page, paired.tab.tab_id).focus();
    await header(page, paired.tab.tab_id).press("Enter");
    await expect(page.locator(`[data-tab="${paired.tab.tab_id}"]`).first()).toHaveAttribute("aria-selected", "true");
    await header(page, herdr.tab).focus();
    await header(page, herdr.tab).press("Space");
    await expect(page.locator(`[data-tab="${herdr.tab}"]`).first()).toHaveAttribute("aria-selected", "true");
    await expect(header(page, herdr.tab)).toHaveAttribute("aria-expanded", "false");
    await screenshot(page, "sidebar-tab-boundary");
    await fold(page, paired.tab.tab_id).click();
    await expect(header(page, paired.tab.tab_id)).toHaveAttribute("aria-expanded", "false");
    herdr.run(["tab", "focus", single.tab.tab_id]);
    await expect(page.locator(`[data-tab="${single.tab.tab_id}"]`).first()).toHaveAttribute("aria-selected", "true");
    const list = page.locator("[data-project-list]");
    await list.evaluate((node) => { (node as HTMLElement).style.height = "72px"; (node as HTMLElement).style.flex = "none"; node.scrollTop = 0; });
    herdr.run(["tab", "focus", paired.tab.tab_id]);
    await expect(header(page, paired.tab.tab_id)).toHaveAttribute("aria-current", "true");
    await expect.poll(async () => {
      const view = await position(list);
      const row = await position(header(page, paired.tab.tab_id));
      return row.y >= view.y && row.y + row.height <= view.y + view.height;
    }).toBe(true);
    await expect(header(page, paired.tab.tab_id)).toHaveAttribute("aria-expanded", "false");
    await rest(page);
    await screenshot(page, "sidebar-tab-scroll-representative");
  } finally { daemon?.stop(); herdr.stop(); }
});

test("long manual tab names keep their tooltip and controls at 240 and 292 in both themes", async ({ page }) => {
  const herdr = await startHerdr();
  const daemon = await startHided(herdr, "sidebar-tab-narrow");
  const name = "아주 긴 한국어 탭 이름과 작업 묶음의 경계 확인 · Sidebar tab groups and disclosure restoration";
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
        for (const expanded of [true, false]) {
          if ((await header(page, herdr.tab).getAttribute("aria-expanded")) !== String(expanded)) await fold(page, herdr.tab).click();
          await expect(header(page, herdr.tab)).toHaveAttribute("aria-expanded", String(expanded));
          await expect(header(page, herdr.tab)).toHaveAccessibleName(`${name}, 2 agents`);
          await expect(tree(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(expanded ? 2 : 0);
          expect(await tree(page, herdr.tab).evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
          const label = tree(page, herdr.tab).locator("[data-sidebar-tab-title]");
          await page.evaluate(() => document.fonts.ready);
          console.info("long tab geometry", JSON.stringify(await label.evaluate((node) => ({ font: getComputedStyle(node).font, fonts: document.fonts.status, width: node.clientWidth, content: node.scrollWidth }))));
          expect(await label.evaluate((node) => node.scrollWidth > node.clientWidth)).toBe(true);
          await header(page, herdr.tab).hover();
          await expect(page.getByRole("tooltip")).toHaveText(`${name}, 2 agents`);
          await screenshot(page, `sidebar-tab-tooltip-${theme}-${width}-${expanded ? "open" : "folded"}`);
          await rest(page);
          await screenshot(page, `sidebar-tab-${theme}-${width}-${expanded ? "open" : "folded"}`);
        }
      }
    }
    herdr.run(["pane", "close", herdr.panes[1]]);
    await expect(header(page, herdr.tab)).toHaveCount(0);
    for (const theme of ["light", "dark"] as const) {
      await chooseTheme(page, theme);
      for (const width of [240, 292]) {
        await sendEvent(page, daemon, "ui_state_update", { sidebar_width: width, expanded_paths: [], selected_path: null });
        await expect.poll(async () => Math.round((await page.locator("nav[data-sidebar]").boundingBox())!.width)).toBe(width + 48);
        await expect(tree(page, herdr.tab).locator("[data-agent-open]")).toHaveCount(1);
        expect(await tree(page, herdr.tab).evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
        await rest(page);
        await screenshot(page, `sidebar-tab-${theme}-${width}-singleton`);
      }
    }
  } finally { daemon.stop(); herdr.stop(); }
});
