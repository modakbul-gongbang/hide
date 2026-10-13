import { test, expect, type Page } from "@playwright/test";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { countSent, enterWorkspace, screenshot } from "./wire";

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
    const split = herdr.run(["pane", "split", herdr.panes[0], "--direction", "right", "--cwd", `${herdr.root}/fixture`, "--no-focus"]) as { result: { pane: { pane_id: string } } };
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
    const created = ["single", "paired"].map((name) => herdr.run(["tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", name, "--no-focus"]) as { result: { tab: { tab_id: string }; root_pane: { pane_id: string } } });
    const single = created[0]!.result;
    const paired = created[1]!.result;
    herdr.run(["agent", "start", "single", "--kind", "claude", "--pane", single.root_pane.pane_id]);
    herdr.run(["agent", "start", "paired", "--kind", "claude", "--pane", paired.root_pane.pane_id]);
    const split = herdr.run(["pane", "split", paired.root_pane.pane_id, "--direction", "right", "--cwd", path.join(herdr.root, "fixture"), "--no-focus"]) as { result: { pane: { pane_id: string } } };
    herdr.run(["agent", "start", "paired-two", "--kind", "claude", "--pane", split.result.pane.pane_id]);
    daemon = await startHided(herdr, "sidebar-tab-boundary");
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await expect(header(page, herdr.tab)).toBeVisible();
    await expect(header(page, single.tab.tab_id)).toHaveCount(0);
    await expect(header(page, paired.tab.tab_id)).toHaveAccessibleName("paired, 2 agents");
    const title = tree(page, herdr.tab).locator("[data-sidebar-tab-title]");
    const groupedTitle = tree(page, herdr.tab).locator("[data-agent-title]").first();
    const singleTitle = tree(page, single.tab.tab_id).locator("[data-agent-title]");
    const position = async (row: ReturnType<Page["locator"]>) => (await row.boundingBox())!;
    expect((await position(groupedTitle)).x - (await position(title)).x).toBe(24);
    expect((await position(singleTitle)).x).toBe((await position(title)).x);
    const first = await position(tree(page, herdr.tab));
    const middle = await position(tree(page, single.tab.tab_id));
    const last = await position(tree(page, paired.tab.tab_id));
    expect(middle.y - first.y - first.height).toBe(12);
    expect(last.y - middle.y - middle.height).toBe(12);
    await screenshot(page, "sidebar-tab-boundary-expanded");
    await fold(page, herdr.tab).click();
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
  } finally { daemon?.stop(); herdr.stop(); }
});
