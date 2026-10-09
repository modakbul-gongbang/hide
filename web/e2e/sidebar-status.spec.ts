// Session-first sidebar and the agent tree on an isolated real daemon
// (PRD agent-hierarchy-screens B10 to B21). Ordinary questions remain the
// parent's responsibility; children unfold under their root only when the
// operator opens it, and the pane header's tree button opens the same tree.
import { expect, test } from "@playwright/test";
import { continueFixtureTranscript, declareParent, elsewhereTab, finishFixtureTurn, labelAgent, setFixtureLifecycle, startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot, sidebarOverflow } from "./wire";

test.describe.configure({ timeout: 150_000 });
const LONG_TITLE = "행 높이 WIDE TITLE MUST CUT HERE";
const QUESTION = "PR 병합 전 검증을 다시 돌려도 될까요?";

test("roots stay in one sidebar, open their children in place, and the header's tree button opens the tree", async ({ page }) => {
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
    const childRow = page.locator(`nav[data-sidebar] [data-pane="${child}"]`);
    await expect(parentRow).toHaveAttribute("data-waiting", "true", { timeout: 20_000 });
    await expect(page.locator("[data-sidebar-mode]")).toHaveCount(0);
    // Folded, the root wears one mark for its working descendants (B10, B17).
    await expect(childRow).toHaveCount(0);
    await expect(parentRow.locator('[data-descendant-mark="working"]')).toContainText("1");
    continueFixtureTranscript(herdr, childSession, { task: LONG_TITLE, reply: QUESTION, question: true });
    await setFixtureLifecycle(herdr, child, "idle");
    // An ordinary question keeps the root waiting, never raised, and a quiet child adds no mark.
    await expect(parentRow.locator("[data-descendant-mark]")).toHaveCount(0, { timeout: 20_000 });
    await expect(parentRow).toHaveAttribute("data-waiting", "true");
    await expect(page.locator('[data-raised-group="needs_you"]')).toHaveCount(0);

    // Right opens the root in place and Left folds it (B15).
    const open = parentRow.locator(`[data-agent-open="${parent}"]`);
    await open.focus();
    await page.keyboard.press("ArrowRight");
    await expect.poll(() => sent.get("agent_tree_toggle") ?? 0).toBe(1);
    await expect(childRow).toHaveAttribute("data-depth", "1", { timeout: 20_000 });
    await expect(childRow).toContainText(LONG_TITLE);
    await expect(open).toHaveAttribute("aria-expanded", "true");
    await screenshot(page, "sessions-sidebar-child-question");
    for (const width of ["calc(240px + var(--size-rail))", "calc(var(--size-sidebar-min) + var(--size-rail))"]) expect(await sidebarOverflow(page, width)).toEqual([]);
    await sidebarOverflow(page, "");

    // A child row opens the child's pane.
    const before = sent.get("focus_pane") ?? 0;
    await childRow.locator(`[data-agent-open="${child}"]`).click();
    await expect.poll(() => sent.get("focus_pane") ?? 0).toBe(before + 1);
    expect(last.get("focus_pane")?.pane_id).toBe(child);
    await expect(page.locator(`[data-pane-view="${child}"]`)).toBeVisible();
    // The child's header walks back to its parent (B20).
    const back = page.locator(`[data-pane-return="${parent}"]`);
    await expect(back).toBeVisible();
    await back.click();
    await expect(page.locator(`[data-pane-view="${parent}"]`)).toBeVisible();

    // The parent's tree button opens the tree popover; Escape returns to it (B18, B19, B21).
    const treeButton = page.locator(`[data-pane-view="${parent}"] [data-pane-tree="${parent}"]`);
    const tree = page.locator(`[data-agent-tree="${parent}"]`);
    await treeButton.focus();
    await page.keyboard.press("Space");
    await expect(tree.locator(`[data-agent-child="${child}"]`)).toContainText(LONG_TITLE);
    await page.keyboard.press("Escape");
    await expect(tree).toHaveCount(0);
    await expect(treeButton).toBeFocused();
    await treeButton.click();
    await screenshot(page, "sessions-pane-child-popover");
    await tree.locator("[data-agent-tree-graph]").click();
    await expect(page.getByRole("dialog", { name: "Overview", exact: true })).toBeVisible();
    await page.keyboard.press("Escape");

    // B16: when the child goes, the tree and its button go with it.
    await page.locator(`nav[data-sidebar] [data-pane="${parent}"] [data-agent-open="${parent}"]`).first().click();
    await treeButton.click();
    herdr.run(["pane", "close", child]);
    await expect(tree).toHaveCount(0, { timeout: 20_000 });
    await expect(treeButton).toHaveCount(0);
    await expect(parentRow.locator(`[data-tree-chevron="${parent}"]`)).toHaveCount(0);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
