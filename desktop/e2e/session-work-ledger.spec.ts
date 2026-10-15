// One background candidate, with production record workers and private native
// conversations. GitHub and drafting stay controlled in this visual fixture.
import { expect, type ElectronApplication } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { continueFixtureTranscript, setFixtureLifecycle, startHerdr } from "../../web/e2e/herdr-fixture";
import { FIRST_GOAL, MISSED, NEXT_GOAL, nextWork, seedFirstWork, workFixture } from "../../web/e2e/session-work-fixture";
import { chord } from "../../web/e2e/chords";
import { enterWorkspace, showTool } from "../../web/e2e/wire";
import { fitWindow, isolate, launchShell, REPO, test } from "./fixture";
import { captureElectronWindow } from "./electron-window";

test("@platform session work records, drafts, ended work and search render in both themes in a narrow native lane", async () => {
  test.setTimeout(240_000);
  const herdr = await startHerdr();
  const github = workFixture(herdr);
  const run = isolate(herdr, "session-record");
  Object.assign(run.env, github.env);
  let app: ElectronApplication | undefined;
  const failures: unknown[] = [];
  try {
    const opened = await launchShell(run.env);
    app = opened.app;
    const page = opened.page;
    await fitWindow(app, { width: 1440, height: 1000 });
    await enterWorkspace(page, "fixture");
    // Sessions pinned, so the lane stays docked through the sheets below (PRD right-lane B5).
    await showTool(page, "agent_sessions");
    await page.locator('[data-lane-pin="off"]').click();
    const first = Date.now();
    github.clock(first, first);
    const { session, child } = seedFirstWork(herdr, first);
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL, { timeout: 45_000 });
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const record = page.locator('[data-session-record]');
    await expect(record.locator('[data-record-work]').first().locator('[data-record-task]')).toHaveCount(1, { timeout: 45_000 });
    await expect(record.locator('[data-record-output="pr"]')).toHaveCount(2, { timeout: 45_000 });
    await page.locator(`[data-pane-view="${herdr.panes[0]}"] .xterm-helper-textarea`).focus();
    await page.keyboard.type(NEXT_GOAL);
    await page.keyboard.press("Enter");
    const second = Date.now();
    github.clock(first, second);
    continueFixtureTranscript(herdr, session, nextWork(second));
    await setFixtureLifecycle(herdr, herdr.panes[0], "working");
    await expect(root).toContainText(NEXT_GOAL, { timeout: 45_000 });
    await setFixtureLifecycle(herdr, herdr.panes[0], "idle");
    await expect(record.locator('[data-record-task]')).toHaveCount(3, { timeout: 45_000 });
    await expect(record.locator('[data-record-output="pr"]')).toHaveCount(3, { timeout: 45_000 });
    await record.locator(`[data-record-child="${child}"] > summary`).click();
    const fixtureIdentity = { head: execFileSync("git", ["rev-parse", "HEAD"], { cwd: REPO, encoding: "utf8" }).trim(), home: run.env.HOME, state: run.env.HIDE_STATE_DIR, socket: herdr.socket, daemon: run.daemonPid(), parent: herdr.panes[0], child, provider: "scripted", github: "scripted" };
    for (const theme of ["light", "dark"] as const) {
      await page.locator('[data-open-settings]').click();
      await page.locator('[data-settings-tab="general"]').click();
      await page.locator(`[data-theme-option="${theme}"]`).click();
      await page.locator('[data-interface-language]').click();
      await page.locator('[data-language-option="ko"]').click();
      await page.keyboard.press("Escape");
      const facts = { ...fixtureIdentity, theme, tools: await page.locator("[data-lane-frame]").boundingBox() };
      await captureElectronWindow(app, `record-chapters-children-${theme}`, facts);
      const missed = record.locator('[data-missed-work]', { hasText: MISSED });
      await missed.getByRole("button", { name: "무시", exact: true }).click();
      await record.getByText("닫힘 1", { exact: true }).click();
      await captureElectronWindow(app, `record-closed-${theme}`, facts);
      await missed.getByRole("button", { name: "되돌리기", exact: true }).click();
      await missed.locator('[data-missed-issue]').click();
      await expect(page.locator('[data-new-issue-title]')).toHaveValue("한국어 요청 줄바꿈 확인", { timeout: 30_000 });
      await captureElectronWindow(app, `record-issue-draft-${theme}`, facts);
      await page.locator('[data-new-issue]').getByRole("button", { name: "취소", exact: true }).click();
      await page.keyboard.press(chord("search", "electron"));
      await page.locator('[data-palette-input]').fill(NEXT_GOAL);
      await expect(page.locator('[data-palette-group="sessions"]')).toContainText(NEXT_GOAL, { timeout: 30_000 });
      await captureElectronWindow(app, `record-search-${theme}`, facts);
      await page.keyboard.press("Escape");
      const divider = page.locator('[data-lane-divider="lane"]');
      for (let step = 0; step < 6; step += 1) await divider.press("ArrowRight");
      await captureElectronWindow(app, `record-narrow-${theme}`, { ...facts, tools: await page.locator("[data-lane-frame]").boundingBox() });
      await page.locator('[data-open-settings]').click();
      await page.locator('[data-settings-tab="hideAi"]').click();
      await page.locator('[data-ai-agent-summary="true"]').click();
      await page.keyboard.press("Escape");
      await expect(record.locator('[data-record-task]')).toHaveCount(0);
      await expect(record.locator('[data-record-request]')).toHaveCount(3);
      await expect(record.getByRole('heading', { level: 2 })).toHaveText('Claude');
      await expect(record.locator('[data-record-child] > summary')).toContainText('Claude');
      await expect(record.locator('[data-output-source]')).toHaveText(Array(4).fill('Claude'));
      await captureElectronWindow(app, `record-summaries-off-${theme}`, facts);
      await page.locator('[data-open-settings]').click();
      await page.locator('[data-settings-tab="hideAi"]').click();
      await page.locator('[data-ai-agent-summary="false"]').click();
      await page.keyboard.press("Escape");
    }
    await record.getByRole("button", { name: "Sessions로 돌아가기" }).click();
    await root.hover();
    await root.locator('[data-session-resolve]').click();
    const ended = page.locator('[data-ended-missed]');
    await expect(ended).toBeVisible({ timeout: 30_000 });
    await ended.locator('summary').click();
    for (const theme of ["light", "dark"] as const) {
      await page.locator('[data-open-settings]').click();
      await page.locator('[data-settings-tab="general"]').click();
      await page.locator(`[data-theme-option="${theme}"]`).click();
      await page.keyboard.press("Escape");
      await captureElectronWindow(app, `record-ended-${theme}`, { ...fixtureIdentity, theme, tools: await page.locator("[data-lane-frame]").boundingBox() });
    }
  } catch (error) { failures.push(error); }
  finally {
    try { await app?.close(); } catch (error) { failures.push(error); }
    try { run.cleanup(); } catch (error) { failures.push(error); }
    try { herdr.stop(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "native session record or teardown failed");
});
