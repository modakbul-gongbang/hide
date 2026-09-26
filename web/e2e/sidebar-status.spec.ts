// The sidebar agent status flow on an isolated pinned Herdr and hided (PRD
// sidebar-agent-status B1-B7): a root that finished its turn while its child
// works waits on it in Working with a ring and a badge; the child's question
// keeps it there and turns it unread; the badge opens the child list by
// keyboard and pointer, Enter opens the child, Esc hands focus back to the
// row, the last item unfolds the children in the list; the request line
// stays under the child; once both are quiet the root moves to Done; and a
// child that goes away while the list is open leaves it.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, keyboardFocus, rest, rowGeometry, screenshot, sidebarOverflow } from "./wire";

test.describe.configure({ timeout: 150_000 });

const LONG_TITLE = "사이드바 가독성 개선과 행 높이 고정을 확인하는 아주 긴 한글 작업 제목 with a long English tail for truncation";

/** Sets and clears the status tokens one pane reports, the way the label plugin does. */
function report(herdr: HerdrFixture, pane: string, set: Record<string, string>, clear: string[] = []): void {
  const args = ["pane", "report-metadata", pane, "--source", "e2e-status"];
  for (const [name, value] of Object.entries(set)) args.push("--token", `${name}=${value}`);
  for (const name of clear) args.push("--clear-token", name);
  execFileSync(herdr.bin, args, { env: herdr.env, timeout: 30_000 });
}

function declareChild(herdr: HerdrFixture, child: string, parent: string): void {
  execFileSync(herdr.bin, ["pane", "report-metadata", child, "--source", "e2e-lineage", "--token", `parent_pane=${parent}`], { env: herdr.env, timeout: 30_000 });
}

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
}

test("a root waiting on its child, the badge's child list, and the progress line rules", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [parent, child] = herdr.panes;
    daemon = await startHided(herdr, "sidebar-status");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await open(page, daemon);
    await expect(page.locator('[data-sidebar="agents"]')).toBeVisible({ timeout: 20_000 });

    // B1: the parent finished its own turn and its child is working.
    report(herdr, parent, { status_done: "✓", progress: "하위 작업 위임 후 대기" });
    report(herdr, child, { status_working: "●", progress: "계보 투영 구현 중" });
    declareChild(herdr, child, parent);
    const parentRow = page.locator(`[data-agent-list] [data-pane="${parent}"]`);
    await expect(parentRow).toHaveAttribute("data-waiting", "true", { timeout: 20_000 });
    await expect(page.locator(`[data-agent-group="working"] [data-pane="${parent}"]`)).toBeVisible();
    const workingPart = parentRow.locator('[data-badge-part="working"]');
    await expect(workingPart).toHaveText("1");
    // One mark size: the waiting root's ring and the badge's working dot are
    // drawn shapes of one diameter, not two glyphs that render apart.
    const ring = await parentRow.locator('[data-agent-status-mark="waiting"][data-mark="○"] > span').boundingBox();
    const dot = await workingPart.locator('[data-mark="●"] > span').boundingBox();
    expect(ring && [ring.width, ring.height]).toEqual(dot && [dot.width, dot.height]);
    // The heading counts the folded child with its parent.
    await expect(page.locator("#agent-group-working")).toHaveText(/· 2$/);
    // Folded by default: the delegated child is drawn under its parent only once unfolded.
    await expect(page.locator(`[data-agent-list] [data-pane="${child}"]`)).toHaveCount(0);
    await screenshot(page, "sidebar-status-waiting");

    // B2: the child asks. The parent keeps waiting in Working, the badge
    // carries the question, and the row turns unread - never Needs You.
    report(herdr, child, { status_question_new: "?", expected_reply: "PR 병합 전 검증을 다시 돌려도 될까요?" }, ["status_working"]);
    await expect(parentRow.locator('[data-badge-part="question"]')).toHaveText("?1", { timeout: 20_000 });
    await expect(parentRow).toHaveAttribute("data-waiting", "true");
    await expect(page.locator(`[data-agent-group="working"] [data-pane="${parent}"]`)).toBeVisible();
    await expect(page.locator('[data-agent-group="needs_you"]')).toHaveCount(0);
    await expect(parentRow).toHaveAttribute("data-attention", "true");
    await screenshot(page, "sidebar-status-child-asks");

    // B5: the badge opens by keyboard; Esc closes and focus is back on the row.
    const badge = parentRow.locator("[data-descendant-badge]");
    await badge.focus();
    await page.keyboard.press("Enter");
    const list = page.locator(`[data-agent-children="${parent}"]`);
    await expect(list).toBeVisible();
    const item = list.locator(`[data-agent-child="${child}"]`);
    await expect(item).toContainText("Agent two");
    await expect(item).toContainText("Question");
    await expect(item).toHaveAttribute("data-selected", "true");
    await expect(list.getByText("Stop")).toHaveCount(0);
    await screenshot(page, "sidebar-status-popover");
    await page.keyboard.press("ArrowDown");
    await expect(list.locator("[data-agent-children-unfold]")).toHaveAttribute("data-selected", "true");
    await page.keyboard.press("ArrowUp");
    await expect(item).toHaveAttribute("data-selected", "true");
    await page.keyboard.press("Escape");
    await expect(list).toHaveCount(0);
    await expect(parentRow.locator(`[data-agent-open="${parent}"]`)).toBeFocused();

    // Enter opens the highlighted child's pane.
    await badge.click();
    await expect(list).toBeVisible();
    const focusesBefore = sent.get("focus_pane") ?? 0;
    await page.keyboard.press("Enter");
    await expect.poll(() => sent.get("focus_pane") ?? 0).toBe(focusesBefore + 1);
    expect(last.get("focus_pane")?.pane_id).toBe(child);
    await expect(list).toHaveCount(0);

    // B6: the last item unfolds the children in the list; the child's
    // request stays on its own line in the warning colour (B7).
    await badge.click();
    await list.locator("[data-agent-children-unfold]").click();
    await expect.poll(() => last.get("agent_tree_toggle")?.pane_id).toBe(parent);
    const childRow = page.locator(`[data-agent-list] [data-pane="${child}"]`);
    await expect(childRow).toHaveAttribute("data-depth", "1", { timeout: 15_000 });
    await expect(childRow.locator('[data-agent-line="request"]')).toHaveText("PR 병합 전 검증을 다시 돌려도 될까요?");
    await expect(parentRow.locator("[data-descendant-badge]")).toHaveCount(0);
    await screenshot(page, "sidebar-status-unfolded");

    // sidebar-readability B2, B4, B14, B25: a long Korean and English title is
    // cut on one line; the pointer and the keyboard move neither row, the
    // child's request keeps its one line, and the unfolded chevron waits in
    // a slot kept at rest until the pointer or the keyboard reaches the row.
    report(herdr, child, { task: LONG_TITLE });
    const childTitle = childRow.locator("[data-agent-title]");
    await expect(childTitle).toHaveText(LONG_TITLE, { timeout: 20_000 });
    const parts = [parentRow.locator("[data-agent-title]"), parentRow.locator("[data-agent-elapsed]"), childTitle, childRow.locator("[data-agent-elapsed]")];
    const geometry = async () => [...(await rowGeometry(parentRow, childRow, parts)), Math.round((await childRow.boundingBox())!.height)];
    const toggle = parentRow.locator(`[data-agent-tree-toggle="${parent}"]`);
    await rest(page);
    const atRest = await geometry();
    await expect(toggle).toHaveCSS("opacity", "0");
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await parentRow.hover();
    await expect(toggle).toHaveCSS("opacity", "1");
    expect(await geometry()).toEqual(atRest);
    await childRow.hover();
    expect(await geometry()).toEqual(atRest);
    await rest(page);
    await keyboardFocus(page, toggle);
    await expect(toggle).toHaveCSS("opacity", "1");
    expect(await geometry()).toEqual(atRest);
    await keyboardFocus(page, childRow.locator(`[data-agent-open="${child}"]`));
    expect(await geometry()).toEqual(atRest);
    await rest(page);
    expect(await childTitle.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);
    await expect(childRow.locator('[data-agent-line="request"]')).toHaveCSS("white-space", "nowrap");
    for (const width of ["240px", "var(--size-sidebar-min)"]) expect(await sidebarOverflow(page, width)).toEqual([]);
    await screenshot(page, "sidebar-status-narrow");
    await sidebarOverflow(page, "");
    // The chevron folds them away again.
    await parentRow.locator(`[data-agent-tree-toggle="${parent}"]`).click();
    await expect(childRow).toHaveCount(0, { timeout: 15_000 });

    // B3: the child answers and finishes; with both quiet the root is Done
    // and the ring is gone.
    report(herdr, child, { status_done: "✓" }, ["status_question_new", "expected_reply"]);
    await expect(page.locator(`[data-agent-group="done"] [data-pane="${parent}"]`)).toBeVisible({ timeout: 20_000 });
    await expect(parentRow).toHaveAttribute("data-waiting", "false");
    await screenshot(page, "sidebar-status-done");

    // B6: a child that goes away while the list is open leaves it, and with
    // none left the list closes.
    await parentRow.locator("[data-descendant-badge]").click();
    await expect(list.locator(`[data-agent-child="${child}"]`)).toBeVisible();
    herdr.run(["pane", "close", child]);
    await expect(list).toHaveCount(0, { timeout: 20_000 });
    await expect(parentRow.locator("[data-descendant-badge]")).toHaveCount(0);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
