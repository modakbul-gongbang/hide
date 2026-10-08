// Sessions and the quiet header on one exact native candidate. Herdr, HOME,
// daemon and profile belong to this fixture; input never reaches the operator.
import { expect, type ElectronApplication } from "@playwright/test";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { declareParent, labelAgent, setFixtureLifecycle, startHerdr } from "../../web/e2e/herdr-fixture";
import { enterWorkspace } from "../../web/e2e/wire";
import { fitWindow, isolate, launchShell, test } from "./fixture";
import { captureNativeWindow } from "./native-window";

test("Sessions and raised-child pane bands preserve native terminal geometry in both themes", async () => {
  const herdr = await startHerdr();
  const run = isolate(herdr, "session-workflow");
  let app: ElectronApplication | undefined;
  const failures: unknown[] = [];
  try {
    const [parent, child] = herdr.panes;
    labelAgent(herdr, parent, { task: "한국어 입력과 세션 복귀 경계를 검토하는 긴 작업 제목", progress: "자식 검토 결과를 기다리는 중" });
    labelAgent(herdr, child, { task: "자식의 긴 한국어 검증 요청과 회귀 결과 확인", progress: "검증 명령 실행 권한이 필요합니다" });
    declareParent(herdr, child, parent);
    await setFixtureLifecycle(herdr, child, "blocked");
    const opened = await launchShell(run.env);
    app = opened.app;
    const page = opened.page;
    await fitWindow(app, { width: 1440, height: 1000 });
    await enterWorkspace(page, "fixture");
    const pane = page.locator(`[data-pane-view="${parent}"]`);
    const host = pane.locator(`[data-terminal-host="${parent}"]`);
    await expect(pane.locator('[data-pane-header-band="raised_child"]')).toBeVisible({ timeout: 30_000 });
    await expect(pane.locator("[data-descendant-badge]")).toBeVisible();
    const terminal = await host.boundingBox();
    const facts = { head: execFileSync("git", ["rev-parse", "HEAD"], { cwd: path.resolve(__dirname, "../.."), encoding: "utf8" }).trim(), parent, child, home: run.env.HOME, socket: herdr.socket, state: run.env.HIDE_STATE_DIR, provider: "synthetic agent transcript and lifecycle" };
    for (const theme of ["light", "dark"] as const) {
      await page.getByRole("button", { name: /^Settings \(/ }).click();
      await page.locator('[data-settings-tab="general"]').click();
      await page.locator(`[data-theme-option="${theme}"]`).click();
      await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
      await page.keyboard.press("Escape");
      await captureNativeWindow(app, `session-header-${theme}`, { ...facts, terminal });
      await pane.locator("[data-descendant-badge]").click();
      await expect(page.locator(`[data-agent-child="${child}"]`)).toBeVisible();
      await captureNativeWindow(app, `session-children-${theme}`, facts);
      await page.keyboard.press("Escape");
      await expect(pane.locator("[data-descendant-badge]")).toBeFocused();
      await page.getByRole("button", { name: "Tools", exact: true }).click();
      await page.locator('[data-tool-tab="agent_sessions"]').click();
      await expect(page.locator(`[data-session-row="${child}"]`)).toContainText("Approve");
      await captureNativeWindow(app, `sessions-${theme}`, facts);
      await page.getByRole("button", { name: "Tools", exact: true }).click();
    }
    await setFixtureLifecycle(herdr, child, "working");
    await expect(pane.locator('[data-pane-header-band="raised_child"]')).toHaveCount(0);
    expect(await host.boundingBox()).toEqual(terminal);
    await expect(page.locator(`nav[data-sidebar] [data-pane="${child}"]`)).toHaveCount(0);
    await captureNativeWindow(app, "session-working-restored", { ...facts, terminal });
  } catch (error) { failures.push(error); }
  finally {
    try { await app?.close(); } catch (error) { failures.push(error); }
    try { run.cleanup(); } catch (error) { failures.push(error); }
    try { herdr.stop(); } catch (error) { failures.push(error); }
  }
  if (failures.length) throw new AggregateError(failures, "native Sessions fixture or teardown failed");
});
