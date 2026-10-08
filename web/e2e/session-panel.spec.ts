// Sessions on a real isolated Herdr and daemon.
import { expect, test, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { declareParent, labelAgent, setFixtureLifecycle, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { chord } from "./chords";
import { chooseTheme, countSent, screenshot } from "./wire";
import { quietFor, unchangedForFrames } from "./wait";

test.describe.configure({ timeout: 240_000 });
test.use({ actionTimeout: 15_000 });

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false", ...args], { cwd, stdio: "ignore" });
}

async function prompt(herdr: HerdrFixture, pane: string): Promise<void> {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const read = spawnSync(herdr.bin, ["pane", "read", pane, "--source", "visible", "--format", "text"], { env: herdr.env, encoding: "utf8", timeout: 10_000 });
    if (read.status === 0 && read.stdout.includes("fixture %")) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`no prompt in pane ${pane}`);
}

/** A Herdr workspace at `cwd` running the fake `claude`. */
async function agentAt(herdr: HerdrFixture, cwd: string): Promise<string> {
  const created = herdr.run(["workspace", "create", "--cwd", cwd, "--label", path.basename(cwd), "--env", `PATH=${herdr.fixturePath}`, "--no-focus"]) as {
    result: { root_pane: { pane_id: string } };
  };
  const pane = created.result.root_pane.pane_id;
  await prompt(herdr, pane);
  herdr.run(["agent", "start", `agent-${path.basename(cwd)}`, "--kind", "claude", "--pane", pane]);
  return pane;
}

const LONG_TITLE = "세션 패널에서 긴 한국어 제목과 브랜치를 한 줄로 안전하게 표시하는 작업";
const QUESTION = "긴 한국어 요청을 읽은 뒤 질문 표시가 흐려지는지 확인해 주세요";

const sentTotal = (sent: Map<string, number>) => [...sent.values()].reduce((sum, count) => sum + count, 0);

async function ensureSessions(page: Page) {
  if (!await page.locator("[data-session-panel]").isVisible()) {
    if (!await page.locator("[data-tool-tabs]").isVisible()) await page.getByRole("button", { name: "Tools", exact: true }).click();
    await page.locator('[data-tool-tab="agent_sessions"]').click();
  }
  await expect(page.locator("[data-session-panel]")).toBeVisible();
}

test("Sessions groups, read rules, child navigation and durable resolution share one project scope", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    git(repo, ["commit", "--allow-empty", "-m", "initial"]);
    const tree = (name: string) => path.join(herdr.root, `repo-${name}`);
    for (const name of ["child", "asking", "question", "stopped"]) {
      git(repo, ["worktree", "add", "-b", `prd/${name}`, tree(name)]);
    }
    const parent = await agentAt(herdr, repo);
    labelAgent(herdr, parent, { task: LONG_TITLE, progress: "자식의 결과를 기다리는 중" });
    const child = await agentAt(herdr, tree("child"));
    labelAgent(herdr, child, { task: "하위 작업 검증" });
    declareParent(herdr, child, parent);
    await setFixtureLifecycle(herdr, child, "working");
    const asking = await agentAt(herdr, tree("asking"));
    labelAgent(herdr, asking, { task: "권한 승인 요청", progress: "검증 명령을 승인해 주세요" });
    await setFixtureLifecycle(herdr, asking, "blocked");
    const question = await agentAt(herdr, tree("question"));
    labelAgent(herdr, question, { task: "AI 질문 읽음 확인", reply: QUESTION, question: true });
    const stopped = await agentAt(herdr, tree("stopped"));
    labelAgent(herdr, stopped, { task: "설정 화면 스위치 추가", progress: "테스트 환경이 없어 멈췄어요", end: "unfinished" });
    daemon = await startHided(herdr, "session-panel");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await page.locator(`nav[data-sidebar] [data-agent-open="${parent}"]`).first().click();
    await page.getByRole("button", { name: "Tools", exact: true }).click();
    await expect(page.locator('[data-tool-tabs] [data-tool-tab]')).toHaveText(["Sessions", "Explorer", "History"]);
    await page.locator('[data-tool-tab="agent_sessions"]').click();
    const panel = page.locator("[data-session-panel]");
    const showSessions = () => ensureSessions(page);
    const row = (pane: string) => panel.locator(`[data-session-row="${pane}"]`);
    const group = (name: string) => panel.locator(`[data-session-group="${name}"]`);
    await expect(row(asking)).toContainText("Approve", { timeout: 30_000 });
    await expect(row(question)).toContainText("Answer");
    await expect(row(stopped)).toContainText("Stopped");
    await expect(group("in_progress").locator(`[data-session-row="${parent}"]`)).toBeVisible();
    await expect(row(child)).toHaveCount(0);
    await expect.poll(() => last.get("request_view")?.observing).toBe(true);
    expect(last.get("ui_state_update")?.right_panel_section).not.toBe("sessions");
    await row(parent).locator("[data-descendant-badge]").click();
    await expect(page.locator(`[data-agent-children="${parent}"] [data-agent-child="${child}"]`)).toContainText("하위 작업 검증");
    await page.keyboard.press("Escape");
    await expect(row(parent).locator("[data-descendant-badge]")).toBeFocused();
    await row(question).locator('[data-session-focus="row"]').focus();
    await page.keyboard.press("Enter");
    await expect(page.locator(`[data-pane-view="${question}"]`)).toHaveAttribute("data-focused", "true");
    await showSessions();
    await expect(group("my_turn").locator(`[data-session-row="${question}"]`)).toHaveCount(0);
    await group("resting").locator('[data-session-focus="group"]').click();
    await expect(group("resting").locator(`[data-session-row="${question}"]`)).toBeVisible();
    await expect(row(question)).not.toContainText("Answer");
    await row(asking).locator('[data-session-focus="row"]').click();
    await expect(page.locator(`[data-pane-view="${asking}"]`)).toHaveAttribute("data-focused", "true");
    await showSessions();
    await expect(group("my_turn").locator(`[data-session-row="${asking}"]`)).toContainText("Approve");
    // Keyboard arrows reach the next row; Resolve is a named keyboard action.
    await row(stopped).locator('[data-session-focus="row"]').focus();
    await page.keyboard.press("Home");
    await expect(panel.locator('[data-session-focus]').first()).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(row(asking).locator('[data-session-focus="row"]')).toBeFocused();
    await row(stopped).locator("[data-session-resolve]").focus();
    await page.keyboard.press("Enter");
    await expect(row(stopped)).toHaveCount(0);
    await expect(page.locator(`nav[data-sidebar] [data-pane="${stopped}"]`)).toHaveCount(0);
    await expect(panel.locator('[data-session-count="resolved_today"]')).toHaveText("1");
    await group("resolved_today").locator('[data-session-focus="group"]').click();
    await expect(row(stopped)).toBeVisible();
    expect(JSON.parse(fs.readFileSync(path.join(daemon.stateDir, "core-state.json"), "utf8")).resolved_sessions[stopped]).toBeTruthy();
    // Closing/reopening the document retains both the chosen tool and resolution.
    daemon = await daemon.restart();
    await page.goto("about:blank");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator('[data-tool-tab="agent_sessions"]')).toHaveAttribute("aria-selected", "true");
    await expect(panel.locator('[data-session-count="resolved_today"]')).toHaveText("1");
    await expect(page.locator(`nav[data-sidebar] [data-pane="${stopped}"]`)).toHaveCount(0);
    await group("resolved_today").locator('[data-session-focus="group"]').click();
    await row(stopped).locator('[data-session-focus="row"]').click();
    await expect(page.locator(`[data-pane-view="${stopped}"]`)).toHaveAttribute("data-focused", "true");
    await showSessions();
    await page.locator(`[data-pane-view="${stopped}"] .xterm-helper-textarea`).pressSequentially("continue");
    await expect(panel.locator('[data-session-count="resolved_today"]')).toHaveText("0");
    await expect(page.locator(`nav[data-sidebar] [data-pane="${stopped}"]`).first()).toBeVisible();
    for (const theme of ["light", "dark"] as const) {
      await chooseTheme(page, theme);
      await screenshot(page, `sessions-panel-${theme}`);
    }
    // All projects has three tabs; project Overview has conversation history, no Requests.
    await page.keyboard.press(chord("sidebar_agents"));
    const modal = page.getByRole("dialog", { name: "Overview", exact: true });
    await modal.getByRole("tab", { name: "repo", exact: true }).click();
    expect(await modal.locator("[data-lens-tile]").evaluateAll((tabs) => tabs.map((tab) => tab.getAttribute("data-lens-tile")))).toEqual(["agents", "issues", "prs", "sessions"]);
    await expect(modal.locator('[data-lens-tile="sessions"]')).toContainText("Conversation history");
    await page.keyboard.press("Escape");
    await page.locator("[data-home-destination]").click();
    expect(await page.locator("[data-main-tab]").evaluateAll((tabs) => tabs.map((tab) => tab.getAttribute("data-main-tab")))).toEqual(["agents", "tasks", "projects"]);
    await row(stopped).locator('[data-session-focus="row"]').click();
    await showSessions();
    await page.mouse.move(2, 998);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await unchangedForFrames(page, () => sentTotal(sent));
    const quiet = sentTotal(sent);
    await quietFor(page, 3_000, "idle Sessions sends no events");
    expect(sentTotal(sent)).toBe(quiet);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
