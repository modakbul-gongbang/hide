// The request view on an isolated pinned Herdr and hided (PRD
// overview-request-view): a Git project whose main agent was asked a long,
// many-line request with an image and delegated a working child, a
// worktree agent asking for approval, one that finished unseen and one
// resting. Every way in opens the view (B1); the `요청` tile counts what is
// the operator's (B2); the groups stand in the PRD's order with the resting
// one folded (B3, B40); a row shows who asked, the request on one line, the
// result and its open chips, the descendants (B4, B13, B42, B49, B52);
// clicking expands it and a finished row is read (B6, B15); the keyboard
// walks it and ⌘Enter opens the pane (B7, B8); nothing is sent while
// nothing changes (B30); a turn the label read as unfinished stops its row,
// and the `에이전트 요약` switch takes that and every AI title away and
// brings them back (B14, B21). Light and Dark captures land in
// HIDE_E2E_SCREENSHOT_DIR.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { claudeProjects, declareParent, elsewhereTab, finishFixtureTurn, labelAgent, labelMarker, setFixtureLifecycle, setFixtureSession, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { chord, field } from "./chords";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 240_000 });

function git(cwd: string, args: string[]): void {
  execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: "ignore" });
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

/** The operator's long request: four lines, a blank one, a home-folder path, an address, and a pasted image. */
const LONG_REQUEST = [
  "Overview 요청 보기에서 긴 요청을 한 줄로 보여 주세요",
  "",
  "/Users/example/projects/app/docs/request-row-expanded-detail.md 를 참고하고",
  "https://github.com/acme/repo/pull/336 리뷰도   함께 [Image #1]",
  "끝쪽 단어는 남겨 둘 것",
].join("\n");

/** A session whose request is `LONG_REQUEST` and whose answer names a report address. */
function longSession(herdr: HerdrFixture, pane: string): void {
  const sessionId = `requests-${process.pid}-long`;
  const marker = labelMarker({ task: "요청 보기 웹 화면 구현", progress: "요청 보기 행을 그리는 중" });
  const records = [
    {
      type: "user",
      sessionId,
      timestamp: "2026-10-01T09:00:00Z",
      origin: { kind: "human" },
      message: { role: "user", content: [{ type: "text", text: LONG_REQUEST }, { type: "image", source: { type: "base64", media_type: "image/png", data: "iVBORw0KGgo=" } }] },
    },
    { type: "assistant", sessionId, timestamp: "2026-10-01T09:00:01Z", message: { role: "assistant", content: [{ type: "text", text: `보고서는 https://example.com/report 에 있습니다.\n${marker}` }] } },
  ];
  const dir = path.join(claudeProjects(herdr), "e2e");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, `${sessionId}.jsonl`), records.map((record) => `${JSON.stringify(record)}\n`).join(""));
  setFixtureSession(herdr, pane, sessionId);
}

async function chooseTheme(page: Page, theme: "light" | "dark"): Promise<void> {
  await page.keyboard.press(chord("settings"));
  await expect(page.locator('[data-settings="true"]')).toBeVisible();
  await page.locator('[data-settings-tab="appearance"]').click();
  await page.locator(`[data-theme-option="${theme}"]`).click();
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${theme}\\b`));
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
  await page.waitForTimeout(400);
}

async function atRest(page: Page): Promise<void> {
  await page.mouse.move(2, 998);
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

test("the request view: what each agent was asked, what came of it, and what is the operator's", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const repo = path.join(herdr.root, "repo");
    fs.mkdirSync(repo);
    git(repo, ["init"]);
    fs.writeFileSync(path.join(repo, "README.md"), "# repo\n");
    git(repo, ["add", "README.md"]);
    git(repo, ["commit", "-m", "initial"]);
    const tree = (name: string) => path.join(herdr.root, `repo-${name}`);
    for (const name of ["child", "asking", "done", "resting", "stopped"]) {
      git(repo, ["worktree", "add", "-b", `prd/${name}`, tree(name)]);
      git(tree(name), ["commit", "--allow-empty", "-m", `${name} work`]);
    }

    const mainPane = await agentAt(herdr, repo);
    longSession(herdr, mainPane);
    const childPane = await agentAt(herdr, tree("child"));
    labelAgent(herdr, childPane, { task: "요청 행 컴포넌트 작성" });
    declareParent(herdr, childPane, mainPane);
    await setFixtureLifecycle(herdr, childPane, "working");
    const askingPane = await agentAt(herdr, tree("asking"));
    labelAgent(herdr, askingPane, { task: "사이드바 상태 규칙 구현", progress: "Done 그룹 회색 링을 바꿔도 될까요?" });
    await setFixtureLifecycle(herdr, askingPane, "blocked");
    const donePane = await agentAt(herdr, tree("done"));
    labelAgent(herdr, donePane, { task: "설치 키트 항목 추가", progress: "키트 항목을 추가했습니다" });
    await finishFixtureTurn(herdr, donePane, elsewhereTab(herdr));
    const restingPane = await agentAt(herdr, tree("resting"));
    labelAgent(herdr, restingPane, { task: "문서 정리 작업 마무리" });
    const stoppedPane = await agentAt(herdr, tree("stopped"));
    labelAgent(herdr, stoppedPane, { task: "설정 화면 스위치 추가", progress: "테스트 환경이 없어 멈췄어요", end: "unfinished" });

    daemon = await startHided(herdr, "requests");
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await expect(page.locator("[data-main-screen]").or(page.locator("[data-workspace-screen]"))).toBeVisible({ timeout: 20_000 });

    // The sidebar's project row opens the request view, the first tile (B1).
    await page.locator('[data-sidebar-mode="projects"]').click();
    await page.locator("[data-project-row]", { hasText: /^repo/ }).click();
    const overview = page.locator("[data-overview-screen]");
    await expect(overview).toHaveAttribute("data-overview-view", "requests");
    expect(await overview.locator("[data-lens-tile]").evaluateAll((tiles) => tiles.map((tile) => tile.getAttribute("data-lens-tile")))).toEqual(["requests", "agents", "issues", "prs", "sessions"]);
    await expect(overview.locator('[data-lens-tile="requests"]')).toHaveAttribute("data-selected", "true");
    await expect.poll(() => last.get("request_view")?.observing).toBe(true);

    const row = (pane: string) => overview.locator(`[data-request-row="${pane}"]`);
    const groups = () => overview.locator("[data-request-group]").evaluateAll((sections) => sections.map((section) => section.getAttribute("data-request-group")));
    // The groups in D-06's order; the delegated child is its parent's, not a row (B3, B13).
    await expect.poll(groups, { timeout: 30_000 }).toEqual(["answer", "stopped", "result", "waiting", "idle"]);
    await expect(row(stoppedPane)).toHaveAttribute("data-request-verb", "stopped");
    await expect(row(stoppedPane)).toContainText("테스트 환경이 없어 멈췄어요");
    await expect(row(childPane)).toHaveCount(0);
    await expect(row(askingPane)).toHaveAttribute("data-request-verb", "answer");
    await expect(row(donePane)).toHaveAttribute("data-request-verb", "result");
    await expect(row(mainPane)).toHaveAttribute("data-request-verb", "waiting");

    // The tile: three to do, one to answer, the bar by verb, no sentence (B2).
    const tile = overview.locator('[data-lens-tile="requests"]');
    await expect(tile.locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "3");
    await expect(tile.locator("[data-lens-tile-badge]")).toHaveText("1");
    await expect(tile.locator("[data-lens-tile-bar]")).toHaveAttribute("data-lens-tile-bar", "answer:1 fix:0 review:0 stopped:1 result:1");

    // The resting group is one folded line until it is opened (B3).
    const restingHead = overview.locator('[data-request-group-head="idle"]');
    await expect(restingHead).toHaveText(/쉬는 중 1 · 펼치기/);
    await expect(row(restingPane)).toHaveCount(0);
    await restingHead.click();
    await expect(row(restingPane)).toBeVisible();

    // The main agent's row (B4, B13, B52): who asked, the request on one
    // line with its paths and address by their last names and the image
    // counted, the descendants, and the address its answer names as a chip.
    const mainRow = row(mainPane);
    await expect(mainRow.locator("[data-request-sender]")).toHaveText("나 ›");
    await expect(mainRow.locator("[data-request-text]")).toHaveAttribute(
      "data-request-text",
      "Overview 요청 보기에서 긴 요청을 한 줄로 보여 주세요 · request-row-….md 를 참고하고 · #336 리뷰도 함께 · 끝쪽 단어는 남겨 둘 것 · 이미지 1",
    );
    await expect(mainRow.locator("[data-request-line]")).not.toContainText("/Users/example");
    await expect(mainRow.locator("[data-request-children]")).toHaveText("자식 1 · 일하는 중 1");
    await expect(mainRow.locator('[data-request-open="https://example.com/report"]')).toBeVisible();
    // One line at the default width and at a narrow one, and no sideways scroll (B42, B52).
    for (const width of [1600, 900]) {
      await page.setViewportSize({ width, height: 1000 });
      const line = mainRow.locator("[data-request-text]");
      const box = (await line.boundingBox())!;
      const lineHeight = await line.evaluate((element) => parseFloat(getComputedStyle(element).lineHeight));
      expect(box.height).toBeLessThanOrEqual(lineHeight + 1);
      expect(await page.evaluate(() => document.scrollingElement!.scrollWidth <= document.scrollingElement!.clientWidth)).toBe(true);
      await screenshot(page, `requests-${width}-light`);
    }
    await page.setViewportSize({ width: 1600, height: 1000 });

    // A click expands the row in place with the request as written; again folds it (B6).
    await mainRow.locator(`[data-request-toggle="${mainPane}"]`).click();
    const detail = mainRow.locator(`[data-request-detail="${mainPane}"]`);
    await expect(detail).toBeVisible();
    expect(await detail.locator("[data-request-full]").innerText()).toContain("/Users/example/projects/app/docs/request-row-expanded-detail.md 를 참고하고\nhttps://github.com/acme/repo/pull/336");
    await expect(detail.locator(`[data-request-child="${childPane}"]`)).toContainText("일하는 중");
    // The label's verdict sits with the rest while summaries are on (B6, D-28).
    await expect(detail.locator("[data-request-verdict]")).toContainText("AI 판정 · 끝남 · ");
    await mainRow.locator(`[data-request-toggle="${mainPane}"]`).click();
    await expect(detail).toHaveCount(0);

    // An open chip opens its address as a terminal link does (B49).
    const popup = page.waitForEvent("popup");
    await mainRow.locator('[data-request-open="https://example.com/report"]').click();
    expect((await popup).url()).toBe("https://example.com/report");
    await (await popup).close();

    // Expanding the finished row reads it: it leaves 결과 볼 것 (B15, D-29).
    await restingHead.click();
    await expect(row(restingPane)).toHaveCount(0);
    const before = sent.get("overview_open_result") ?? 0;
    await row(donePane).locator(`[data-request-toggle="${donePane}"]`).click();
    await expect.poll(() => (sent.get("overview_open_result") ?? 0) - before).toBe(1);
    expect(last.get("overview_open_result")?.pane_id).toBe(donePane);
    await expect(row(donePane)).toHaveAttribute("data-request-verb", "idle", { timeout: 15_000 });
    await expect(row(donePane).locator("[data-request-detail]")).toBeVisible();
    await expect(tile.locator("[data-lens-tile-value]")).toHaveAttribute("data-lens-tile-value", "2");
    // A row to answer stays one to answer once expanded (D-29).
    await row(askingPane).locator(`[data-request-toggle="${askingPane}"]`).click();
    await expect(row(askingPane)).toHaveAttribute("data-request-verb", "answer");

    for (const theme of ["dark", "light"] as const) {
      await chooseTheme(page, theme);
      await atRest(page);
      await screenshot(page, `requests-expanded-${theme}`);
    }

    // Nothing changes, nothing is sent (B30).
    await atRest(page);
    await page.waitForTimeout(500);
    const quiet = [...sent.values()].reduce((sum, count) => sum + count, 0);
    await page.waitForTimeout(3_000);
    expect([...sent.values()].reduce((sum, count) => sum + count, 0)).toBe(quiet);

    // The keyboard (B8): the arrows step through heads, rows and chips in
    // order, Home and End go to the ends, and ⌘Enter opens the pane (B7).
    const focusables = overview.locator("[data-requests] [data-request-focus]");
    await overview.locator(`[data-request-toggle="${askingPane}"]`).focus();
    await page.keyboard.press("Home");
    await expect(focusables.first()).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(overview.locator(`[data-request-toggle="${askingPane}"]`)).toBeFocused();
    await page.keyboard.press("End");
    await expect(focusables.last()).toBeFocused();
    await overview.locator(`[data-request-toggle="${askingPane}"]`).focus();
    await page.keyboard.press(field("Enter"));
    await expect(page.locator("[data-workspace-screen]")).toBeVisible();
    await expect(page.locator(`[data-pane-view="${askingPane}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    await expect.poll(() => last.get("request_view")?.observing).toBe(false);

    // ⌘⇧H comes back to the request view; Home opens All projects on its 요청 tab (B1).
    await page.locator("body").click({ position: { x: 1, y: 1 } });
    await page.keyboard.press(chord("project_home"));
    await expect(overview).toHaveAttribute("data-overview-view", "requests");
    await page.locator("[data-home-destination]").click();
    const main = page.locator("[data-main-screen]");
    await expect(main).toHaveAttribute("data-main-view", "requests");
    expect(await main.locator("[data-main-tab]").evaluateAll((tabs) => tabs.map((tab) => tab.getAttribute("data-main-tab")))).toEqual(["requests", "tasks", "agents", "projects"]);
    await expect(main.locator('[data-main-tab="requests"] [data-requests-answer]')).toHaveText("1");
    await expect(main.locator(`[data-request-row="${askingPane}"]`)).toContainText("repo");
    await atRest(page);
    await screenshot(page, "requests-all-projects-light");

    // Turning agent summaries off (B21): no row stops, every title is the
    // session's own or the provider's, and the request stays as written.
    const summary = async (on: boolean) => {
      await page.keyboard.press(chord("settings"));
      await page.locator('[data-settings-tab="agents"]').click();
      const toggle = page.locator("[data-ai-agent-summary]");
      await expect(toggle).toHaveAttribute("data-ai-agent-summary", String(!on));
      await toggle.click();
      await expect(toggle).toHaveAttribute("data-ai-agent-summary", String(on));
      await expect.poll(() => last.get("ai_settings")?.agent_summary).toBe(on);
      if (!on) await screenshot(page, "settings-agent-summary-off-light");
      await page.keyboard.press("Escape");
    };
    const stoppedRow = main.locator(`[data-request-row="${stoppedPane}"]`);
    await expect(stoppedRow).toHaveAttribute("data-request-verb", "stopped");
    await summary(false);
    await expect(main.locator('[data-request-group="stopped"]')).toHaveCount(0, { timeout: 15_000 });
    const mainResting = main.locator('[data-request-group-head="idle"]');
    if ((await mainResting.innerText()).includes("펼치기")) await mainResting.click();
    await expect(stoppedRow).toHaveAttribute("data-request-verb", "idle");
    await expect(stoppedRow.locator("[data-request-title]")).toHaveText("Claude");
    await expect(stoppedRow.locator("[data-request-result]")).not.toHaveText("테스트 환경이 없어 멈췄어요");
    await expect(main.locator(`[data-request-row="${askingPane}"]`)).toHaveAttribute("data-request-verb", "answer");
    await summary(true);
    await expect(stoppedRow).toHaveAttribute("data-request-verb", "stopped", { timeout: 15_000 });
    await expect(stoppedRow.locator("[data-request-title]")).toHaveText("설정 화면 스위치 추가");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
