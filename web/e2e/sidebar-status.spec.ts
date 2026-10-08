// Session-first sidebar and shared child badge on an isolated real daemon.
// Ordinary questions remain the parent's responsibility; children never
// unfold into the sidebar. The same badge drives pane-header navigation.
import { expect, test } from "@playwright/test";
import { continueFixtureTranscript, declareParent, elsewhereTab, finishFixtureTurn, labelAgent, setFixtureLifecycle, startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot, sidebarOverflow } from "./wire";

test.describe.configure({ timeout: 150_000 });
const LONG_TITLE = "행 높이 WIDE TITLE MUST CUT HERE";
const QUESTION = "PR 병합 전 검증을 다시 돌려도 될까요?";

test("roots stay in one sidebar and both badges open direct children by pointer and keyboard", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [parent, child] = herdr.panes;
    labelAgent(herdr, parent, { task: "Agent one", progress: "하위 작업 위임 후 대기" });
    await finishFixtureTurn(herdr, parent, elsewhereTab(herdr));
    const childSession = labelAgent(herdr, child, { task: LONG_TITLE, progress: "계보 투영 구현 중" });
    await setFixtureLifecycle(herdr, child, "working");
    declareParent(herdr, child, parent);
    daemon = await startHided(herdr, "sidebar-status");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    const parentRow = page.locator(`nav[data-sidebar] [data-pane="${parent}"]`).first();
    await expect(parentRow).toHaveAttribute("data-waiting", "true", { timeout: 20_000 });
    await expect(page.locator("[data-sidebar-mode]")).toHaveCount(0);
    await expect(page.locator(`nav[data-sidebar] [data-pane="${child}"]`)).toHaveCount(0);
    await expect(parentRow.locator('[data-badge-part="working"]')).toHaveText("1");
    continueFixtureTranscript(herdr, childSession, { task: LONG_TITLE, reply: QUESTION, question: true });
    await setFixtureLifecycle(herdr, child, "idle");
    await expect(parentRow.locator('[data-badge-part="question"]')).toHaveText("?1", { timeout: 20_000 });
    await expect(parentRow).toHaveAttribute("data-waiting", "true");
    await expect(page.locator('[data-raised-group="needs_you"]')).toHaveCount(0);
    await expect(page.locator("[data-agent-tree-toggle]")).toHaveCount(0);
    const badge = parentRow.locator("[data-descendant-badge]");
    await badge.focus();
    await page.keyboard.press("Enter");
    const list = page.locator(`[data-agent-children="${parent}"]`);
    const item = list.locator(`[data-agent-child="${child}"]`);
    await expect(item).toContainText(LONG_TITLE);
    await expect(item).toContainText(QUESTION);
    await expect(item).toHaveAttribute("data-selected", "true");
    await page.keyboard.press("ArrowDown");
    await expect(list.locator("[data-agent-children-unfold]")).toHaveAttribute("data-selected", "true");
    await page.keyboard.press("Escape");
    await expect(list).toHaveCount(0);
    await expect(badge).toBeFocused();
    await screenshot(page, "sessions-sidebar-child-question");
    for (const width of ["calc(240px + var(--size-rail))", "calc(var(--size-sidebar-min) + var(--size-rail))"]) expect(await sidebarOverflow(page, width)).toEqual([]);
    await sidebarOverflow(page, "");

    await badge.click();
    const before = sent.get("focus_pane") ?? 0;
    await page.keyboard.press("Enter");
    await expect.poll(() => sent.get("focus_pane") ?? 0).toBe(before + 1);
    expect(last.get("focus_pane")?.pane_id).toBe(child);
    await expect(page.locator(`[data-pane-view="${child}"]`)).toBeVisible();
    await expect(list).toHaveCount(0);
    const back = page.locator(`[data-pane-return="${parent}"]`);
    await expect(back).toBeVisible();
    await back.click();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toBeVisible();
    const headerBadge = page.locator(`[data-pane-view="${parent}"] [data-descendant-badge]`);
    await headerBadge.focus();
    await page.keyboard.press("Space");
    await expect(list).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(headerBadge).toBeFocused();
    await headerBadge.click();
    await screenshot(page, "sessions-pane-child-popover");
    await list.locator("[data-agent-children-unfold]").click();
    await expect(page.getByRole("dialog", { name: "Overview", exact: true })).toBeVisible();
    expect(sent.get("agent_tree_toggle") ?? 0).toBe(0);
    await page.keyboard.press("Escape");
    await headerBadge.click();
    herdr.run(["pane", "close", child]);
    await expect(list).toHaveCount(0, { timeout: 20_000 });
    await expect(headerBadge).toHaveCount(0);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
