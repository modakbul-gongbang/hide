import { expect, test } from "@playwright/test";
import fs from "node:fs";
import { continueFixtureTranscript, setFixtureLifecycle, startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { FIRST_GOAL, MISSED, NEXT_GOAL, nextWork, seedFirstWork, workFixture } from "./session-work-fixture";
import { enterWorkspace, showTool } from "./wire";
import { chord } from "./chords";

test.describe.configure({ timeout: 180_000 });

test("@platform a native session keeps its chapters, child outputs and reversible missed work through search and summaries off", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr);
    daemon = await startHided(herdr, "session-work-ledger", undefined, github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await showTool(page, "work");
    const first = Date.now();
    github.clock(first, first);
    const { session, child } = seedFirstWork(herdr, first);
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL, { timeout: 45_000 });
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const record = page.locator("[data-session-record]");
    const tasks = record.locator('[data-record-work]').first().locator('[data-record-task]');
    await expect(tasks).toHaveCount(1, { timeout: 45_000 });
    await expect(tasks.first().locator('[data-task-state="done"]')).toBeVisible();
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
    // Typing in the terminal chose its agent, which put the unpinned lane away (PRD right-lane B15).
    await showTool(page, "work");
    await record.getByRole("button", { name: "Back to Sessions" }).click();
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    await expect(tasks).toHaveCount(2, { timeout: 45_000 });
    await expect(record).toContainText(FIRST_GOAL);
    await expect(record).toContainText(NEXT_GOAL);
    await expect(record.locator('[data-record-output="pr"]')).toHaveCount(3, { timeout: 45_000 });
    // First asked first: the finished task, then the one whose PR failed CI.
    expect(await tasks.evaluateAll((rows) => rows.map((row) => row.querySelector("[data-task-state]")?.getAttribute("data-task-state")))).toEqual(["done", "ci_failed"]);
    await record.locator(`[data-record-child="${child}"] > summary`).click();
    await expect(record.locator(`[data-record-child="${child}"] [data-record-task]`)).toContainText("자식 연결 검증");
    const missed = record.locator("[data-missed-work]", { hasText: MISSED });
    await missed.getByRole("button", { name: "Dismiss", exact: true }).click();
    // A closed one is a muted line that says how it closed; Undo shows under the pointer.
    await expect(missed).toHaveAttribute("data-missed-status", "dismissed");
    await missed.hover();
    await missed.getByRole("button", { name: "Undo", exact: true }).click();
    await expect(missed).toHaveAttribute("data-missed-status", "open");
    await page.keyboard.press(chord("search"));
    await page.locator("[data-palette-input]").fill(NEXT_GOAL);
    const hits = page.locator('[data-palette-group="sessions"]');
    await expect(hits).toContainText(NEXT_GOAL, { timeout: 30_000 });
    await hits.locator('[data-palette-row]').first().click();
    await expect(record).toContainText(FIRST_GOAL);
    await page.keyboard.press(chord("settings"));
    await page.locator('[data-settings-tab="hideAi"]').click();
    await page.locator('[data-ai-agent-summary="true"]').click();
    await page.keyboard.press("Escape");
    await expect(record.locator('[data-record-words]')).toHaveCount(0);
    await expect(record.locator('[data-record-task]')).toHaveCount(0);
    await expect(record.locator('[data-missed-work]')).toHaveCount(0);
    await expect(record.locator('[data-record-request]')).toHaveCount(3);
    await expect(record.locator('[data-record-output="pr"]')).toHaveCount(3);
    await expect(record.getByRole('heading', { level: 2 })).toHaveText('Claude');
    await expect(record.locator('[data-record-child] > summary')).toContainText('Claude');
    // Only the child's pull request names its maker; the session's own say nothing.
    await expect(record.locator('[data-output-source]')).toHaveText(['Claude']);
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform 백로그로 makes one title-only issue, a lost create response converges on retry, and 되돌리기 closes it", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr, true);
    daemon = await startHided(herdr, "session-backlog-retry", undefined, github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await showTool(page, "work");
    const first = Date.now();
    github.clock(first, first);
    seedFirstWork(herdr, first);
    const root = page.locator(`[data-session-row="${herdr.panes[0]}"]`);
    await expect(root).toContainText(FIRST_GOAL, { timeout: 45_000 });
    await root.hover();
    await page.locator(`[data-session-record-button="${herdr.panes[0]}"]`).click();
    const missed = page.locator("[data-session-record] [data-missed-work]", { hasText: MISSED });
    // PRD overview-device-tasks B43-B45: one press, no form; GitHub made the
    // issue but its answer was lost, so the line fails, and the retry joins
    // the issue GitHub already has instead of making a second one.
    await missed.locator("[data-missed-backlog-add]").click({ timeout: 45_000 });
    await expect(missed.locator("[data-missed-failed]")).toBeVisible({ timeout: 30_000 });
    await missed.locator("[data-missed-failed]").getByRole("button", { name: "Retry", exact: true }).click();
    await expect(missed.locator('[data-missed-backlog="#77"]')).toBeVisible({ timeout: 30_000 });
    await expect(missed).toHaveAttribute("data-missed-status", "backlog");
    const calls = () => fs.readFileSync(github.calls, "utf8").trim().split("\n").map((line) => JSON.parse(line) as string[]);
    const creates = calls().filter((args) => args[0] === "issue" && args[1] === "create");
    expect(creates).toHaveLength(1);
    // Title only: nothing of the conversation goes to GitHub (D-10).
    expect(creates[0]![creates[0]!.indexOf("--title") + 1]).toBe(MISSED);
    expect(creates[0]![creates[0]!.indexOf("--body") + 1]).toBe("");

    // `· 백로그 #77` opens the item in the Overview's backlog, whose 되돌리기
    // closes the issue and opens the missed line again (B32).
    await missed.locator('[data-missed-backlog="#77"]').click();
    const item = page.locator("[data-overview-panel] [data-backlog-item]");
    await expect(item).toBeVisible();
    await item.locator("[data-item-undo]").click();
    await expect.poll(() => calls().some((args) => args[0] === "issue" && args[1] === "close" && args[2] === "77"), { timeout: 30_000 }).toBe(true);
    await page.keyboard.press("Escape");
    await page.keyboard.press("Escape");
    await expect(missed).toHaveAttribute("data-missed-status", "open", { timeout: 30_000 });
  } finally { daemon?.stop(); herdr.stop(); }
});

test("@platform ⌘⇧M adds a backlog issue from the terminal and hands the keyboard back", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | undefined;
  try {
    const github = workFixture(herdr);
    daemon = await startHided(herdr, "quick-add", undefined, github.env);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const terminal = page.locator(`[data-pane-view="${herdr.panes[0]}"] .xterm-helper-textarea`);
    await terminal.focus();
    // B37-B39: the input opens over the Workspace on the front checkout's
    // project, says why, and says where the item goes.
    await page.keyboard.press(chord("add_backlog"));
    const quick = page.locator("[data-quick-add]");
    await expect(quick).toBeVisible();
    await expect(quick.locator("[data-quick-add-input]")).toBeFocused();
    await expect(quick.locator("[data-quick-add-reason]")).toHaveAttribute("data-quick-add-reason", "front");
    await expect(quick.locator("[data-quick-add-destination]")).toBeVisible({ timeout: 30_000 });
    // B42: Esc makes nothing and the terminal holds the keyboard again.
    await page.keyboard.press("Escape");
    await expect(quick).toHaveCount(0);
    await expect(terminal).toBeFocused();
    // B42: a pasted multi-line text is one line.
    await page.keyboard.press(chord("add_backlog"));
    await quick.locator("[data-quick-add-input]").fill("빠른 추가\n두 번째 줄");
    await expect(quick.locator("[data-quick-add-input]")).toHaveValue("빠른 추가 두 번째 줄");
    // B40: Enter makes it, the made line names it, and the input closes on its own.
    await page.keyboard.press("Enter");
    await expect(quick.locator('[data-quick-add-made="github:acme/repo#77"]')).toBeVisible({ timeout: 30_000 });
    await expect(quick).toHaveCount(0, { timeout: 10_000 });
    await expect(terminal).toBeFocused();
    const creates = fs.readFileSync(github.calls, "utf8").trim().split("\n").map((line) => JSON.parse(line) as string[]).filter((args) => args[0] === "issue" && args[1] === "create");
    expect(creates).toHaveLength(1);
    expect(creates[0]![creates[0]!.indexOf("--title") + 1]).toBe("빠른 추가 두 번째 줄");
  } finally { daemon?.stop(); herdr.stop(); }
});
