// A turn that ended blocked or unfinished draws its own mark on a real daemon
// (PRD agent-blocked-state B1-B4): the triangle in Needs You until it is read,
// the half-disc beside the Done rows, each with its word leading the sentence.
import { expect, test } from "@playwright/test";
import { elsewhereTab, finishFixtureTurn, labelAgent, startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";

test.describe.configure({ timeout: 150_000 });

test("a blocked turn draws a triangle in Needs You and an unfinished turn a half-disc outside it", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [blocked, unfinished] = herdr.panes;
    labelAgent(herdr, blocked, { task: "디스크 정리 작업", progress: "디스크가 가득 차 멈췄어요", end: "blocked" });
    await finishFixtureTurn(herdr, blocked, elsewhereTab(herdr));
    labelAgent(herdr, unfinished, { task: "설정 화면 스위치 추가", progress: "테스트 3개 남음", end: "unfinished" });
    await finishFixtureTurn(herdr, unfinished, elsewhereTab(herdr));
    daemon = await startHided(herdr, "blocked-state");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    const row = (pane: string) => page.locator(`nav[data-sidebar] [data-pane="${pane}"]`).first();
    await expect(row(blocked).locator('[data-agent-status-mark="error"]')).toBeVisible({ timeout: 30_000 });
    await expect(page.locator(`[data-raised-group="needs_you"] [data-pane="${blocked}"]`)).toBeVisible();
    await expect(row(unfinished).locator('[data-agent-status-mark="stopped"]')).toBeVisible();
    await expect(page.locator(`[data-raised-group="needs_you"] [data-pane="${unfinished}"]`)).toHaveCount(0);
    await expect(row(unfinished)).toContainText("Stopped");
    await row(blocked).locator("[data-agent-open]").first().click();
    await expect(page.locator(`[data-pane-view="${blocked}"]`)).toHaveAttribute("data-focused", "true");
    await expect(row(blocked).locator('[data-agent-status-mark="error"]')).toBeVisible();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
