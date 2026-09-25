// The S8 flow on an isolated pinned Herdr and hided, with Claude Code and
// Codex session files written under the daemon's private HOME: a Project's
// Sessions entered from its Overview, empty until an agent has run there and
// then newest first with provider, request, checkout, time and availability
// (B1, B2, B4); provider, search, no match and Clear filters (B4); a session
// read beside the list (B3); an unreadable row and a session whose file went
// away, each with its reason, Retry and actual source location (B5); a list
// read that fails and recovers (B4); the keyboard path (B9); a second window
// that moves the focus and then names another Project (B6, A7); and no
// Memory on any of these screens (B8).

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 180_000 });

async function open(page: Page, daemon: Daemon): Promise<void> {
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
}

/** The daemon spells its HOME as it was given (`/var/...`), the fixture by its real path (`/private/var/...`). */
function samePath(left: string, right: string): boolean {
  const plain = (value: string) => value.replace(/^\/private(?=\/)/, "");
  return plain(left) === plain(right);
}

const GONE = "The session file can no longer be found. It may have been moved or deleted.";

const LONG_REQUEST =
  "배포 스크립트 정리하고 release note 초안까지 작성해줘. 그리고 CI에서 flaky한 e2e 테스트가 왜 실패하는지 원인을 찾아서 한 문단으로 정리해줘";
const PATH_REQUEST = "Investigate why /opt/builds/very-long-directory-name-without-any-breaks-to-test-wrapping/output/artifact.tar.gz is empty";

function claudeLine(cwd: string, at: string, role: "user" | "assistant", text: string): string {
  return role === "assistant"
    ? JSON.stringify({ type: "assistant", cwd, timestamp: at, message: { role: "assistant", content: [{ type: "text", text }] } })
    : JSON.stringify({ type: "user", cwd, timestamp: at, userType: "external", promptId: `p-${at}`, message: { role: "user", content: text } });
}

function codexSession(id: string, cwd: string, at: string, request: string, answer: string): string {
  return [
    { type: "session_meta", payload: { id, cwd } },
    { type: "response_item", timestamp: at, payload: { type: "message", role: "user", content: [{ type: "input_text", text: request }] } },
    { type: "response_item", timestamp: at, payload: { type: "message", role: "assistant", content: [{ type: "output_text", text: answer }] } },
  ]
    .map((line) => JSON.stringify(line))
    .join("\n");
}

/** Session files as the providers write them, under the daemon's HOME. Returns each file by session id. */
function writeSessions(home: string, root: string, alpha: string): Record<string, string> {
  const claude = path.join(home, ".claude", "projects", "-fixture");
  const codex = path.join(home, ".codex", "sessions", "2026", "09", "22");
  fs.mkdirSync(claude, { recursive: true });
  fs.mkdirSync(codex, { recursive: true });
  const files = {
    "claude-release": path.join(claude, "claude-release.jsonl"),
    "claude-path": path.join(claude, "claude-path.jsonl"),
    "claude-broken": path.join(claude, "claude-broken.jsonl"),
    "claude-alpha": path.join(claude, "claude-alpha.jsonl"),
    "codex-login": path.join(codex, "rollout-2026-09-22T09-00-00-codex-login.jsonl"),
  };
  fs.writeFileSync(
    files["claude-release"],
    [
      claudeLine(root, "2026-09-21T01:00:00Z", "user", LONG_REQUEST),
      claudeLine(root, "2026-09-21T01:02:00Z", "assistant", "Release note draft:\n\n- Deploy script now checks the tag first.\n- 실패한 e2e는 타이밍 문제였습니다."),
    ].join("\n") + "\n",
  );
  fs.writeFileSync(files["claude-path"], claudeLine(root, "2026-09-20T08:00:00Z", "user", PATH_REQUEST) + "\n");
  // Its only record cannot be dated, so the reader cannot parse the session.
  fs.writeFileSync(files["claude-broken"], JSON.stringify({ type: "user", cwd: root, timestamp: "not a time", message: { content: "lost" } }) + "\n");
  // Another Project's session: it must never appear in the fixture's list.
  fs.writeFileSync(files["claude-alpha"], claudeLine(alpha, "2026-09-23T10:00:00Z", "user", "alpha only request") + "\n");
  fs.writeFileSync(files["codex-login"], codexSession("codex-login", root, "2026-09-22T09:00:00Z", "Fix the flaky login test in the auth module", "The retry wrapper hid a real race; fixed.") + "\n");
  return files;
}

test("a Project's Sessions: history, filters, a read-only session, failures and two windows", async ({ page, context }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const root = path.join(fs.realpathSync(herdr.root), "fixture");
    daemon = await startHided(herdr, "s8");
    const alpha = path.join(daemon.home, "projects", "alpha");
    await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: daemon.origin });
    const last = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, last);
    await open(page, daemon);

    // The Overview offers the Project's Sessions (B1); before any agent has
    // written a session here the list says so.
    await expect(page.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await page.locator("[data-main-project]", { hasText: "fixture" }).click();
    await expect(page.locator("[data-overview-screen]")).toBeVisible();
    await expect(page.locator("[data-overview-screen]")).not.toContainText(/memory/i);
    await page.locator("[data-overview-sessions]").click();
    const screen = page.locator("[data-sessions-screen]");
    await expect(screen).toBeVisible();
    await expect(page.locator("[data-sessions-state]")).toHaveAttribute("data-sessions-state", "empty", { timeout: 20_000 });
    await expect(screen).toContainText("No sessions yet");
    await expect(page.locator("[data-session-detail]")).toHaveAttribute("data-session-detail", "none");
    const request = last.get("sessions_refresh");
    expect(request?.workspace_id).toEqual(expect.stringMatching(/^workspace:/));
    await screenshot(page, "s8-empty");

    // Arriving again reads the history afresh: newest first, the Project's own
    // sessions only, the unreadable one dated by its file (B1, B2).
    const files = writeSessions(daemon.home, root, alpha);
    await page.locator("[data-go-overview]").click();
    await page.locator("[data-overview-sessions]").click();
    const rows = page.locator("[data-session-row]");
    await expect(rows).toHaveCount(4, { timeout: 20_000 });
    expect(await rows.evaluateAll((items) => items.map((item) => item.getAttribute("data-session-row")))).toEqual([
      "claude-broken",
      "codex-login",
      "claude-release",
      "claude-path",
    ]);
    await expect(screen).not.toContainText("alpha only request");
    await expect(page.locator("[data-sessions-count]")).toHaveText("4 sessions");
    const release = page.locator('[data-session="claude-release"]');
    await expect(release).toContainText("Claude Code");
    await expect(release).toContainText(LONG_REQUEST);
    await expect(release).toContainText("fixture");
    await expect(page.locator('[data-session="codex-login"]')).toContainText("Codex");
    await expect(page.locator('[data-session="codex-login"]')).toContainText("Fix the flaky login test");
    await expect(page.locator('[data-session-row="claude-release"]')).toHaveAttribute("aria-label", /^Claude Code, 배포 스크립트 정리하고 .*, fixture, .*, available$/);
    await expect(screen).not.toContainText(/memory/i);
    await screenshot(page, "s8-history");

    // An unreadable session keeps its row, dimmed, with the reason, Retry and
    // its actual file path to copy (B5).
    const broken = page.locator('[data-session="claude-broken"]');
    await expect(broken).toHaveAttribute("data-session-available", "false");
    await expect(page.locator('[data-session-reason="claude-broken"]')).toHaveText("The session file could not be parsed.");
    await expect(page.locator('[data-session-row="claude-broken"]')).toHaveAttribute("aria-label", /unavailable$/);
    await page.locator('[data-session-copy="claude-broken"]').click();
    await expect(broken).toContainText("Copied");
    // The daemon spells its HOME as it was given, so compare the files, not the spelling.
    expect(fs.realpathSync(await page.evaluate(() => navigator.clipboard.readText()))).toBe(files["claude-broken"]);
    await screenshot(page, "s8-unavailable-row");
    // Opening it answers with the same reason in the detail place.
    await page.locator('[data-session-row="claude-broken"]').click();
    await expect(page.locator("[data-session-detail]")).toHaveAttribute("data-session-detail", "failed");
    await expect(page.locator('[data-session-failure="claude-broken"]')).toContainText("The session file could not be parsed.");

    // Provider and search narrow the list without a round trip; a search that
    // matches nothing says so and offers to clear both (B2, B4).
    const refreshes = sent.get("sessions_refresh") ?? 0;
    await page.locator('[data-provider-choice="codex"]').click();
    await expect(rows).toHaveCount(1);
    await expect(page.locator("[data-sessions-count]")).toHaveText("1 of 4 sessions");
    await page.locator("[data-sessions-search]").fill("배포");
    await expect(page.locator("[data-sessions-state]")).toHaveAttribute("data-sessions-state", "no_match");
    await expect(screen).toContainText("No matching sessions");
    await screenshot(page, "s8-no-match");
    await page.locator("[data-sessions-clear]").click();
    await expect(rows).toHaveCount(4);
    await expect(page.locator("[data-sessions-search]")).toHaveValue("");
    await expect(page.locator("[data-sessions-provider]")).toHaveAttribute("data-sessions-provider", "all");
    expect(sent.get("sessions_refresh") ?? 0).toBe(refreshes);

    // The keyboard reaches a session from the search and opens it beside the
    // list, read-only: the request and the answer, nothing to run (B3, B9).
    await page.locator("[data-sessions-search]").fill("release");
    await expect(rows).toHaveCount(1);
    await page.locator("[data-sessions-search]").press("ArrowDown");
    await expect(page.locator('[data-session-row="claude-release"]')).toBeFocused();
    await page.keyboard.press("Enter");
    await expect.poll(() => last.get("archive_open")).toEqual({ kind: "session", id: "claude-release", workspace_id: request?.workspace_id });
    await expect(page.locator("[data-session-detail]")).toHaveAttribute("data-session-detail", "open", { timeout: 15_000 });
    await expect(page.locator("[data-turn]")).toHaveCount(2);
    await expect(page.locator('[data-turn="user"]')).toContainText(LONG_REQUEST);
    await expect(page.locator('[data-turn="assistant"]')).toContainText("실패한 e2e는 타이밍 문제였습니다.");
    await expect(page.locator('[data-session-row="claude-release"]')).toHaveAttribute("aria-current", "true");
    await expect(page.locator("[data-session-detail] button", { hasText: /resume|run|send|open in/i })).toHaveCount(0);
    await page.locator("[data-sessions-search]").focus();
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-sessions-search]")).toHaveValue("");
    await expect(rows).toHaveCount(4);
    // Clear filters left the pointer over the first row; hover would raise it.
    await page.mouse.move(1000, 700);
    await screenshot(page, "s8-detail");
    // The detail copies the file it read.
    await page.locator('[data-session-copy="detail"]').click();
    expect(fs.realpathSync(await page.evaluate(() => navigator.clipboard.readText()))).toBe(files["claude-release"]);

    // Arrow keys walk the rows and the provider choice.
    await page.locator('[data-session-row="claude-release"]').focus();
    await page.keyboard.press("ArrowDown");
    await expect(page.locator('[data-session-row="claude-path"]')).toBeFocused();
    await page.keyboard.press("Home");
    await expect(page.locator('[data-session-row="claude-broken"]')).toBeFocused();
    await screenshot(page, "s8-keyboard-focus");
    await page.locator('[data-provider-choice="all"]').focus();
    await page.keyboard.press("ArrowRight");
    await expect(page.locator("[data-sessions-provider]")).toHaveAttribute("data-sessions-provider", "codex");
    await expect(page.locator('[data-provider-choice="codex"]')).toBeFocused();
    await page.keyboard.press("ArrowLeft");
    await expect(page.locator("[data-sessions-provider]")).toHaveAttribute("data-sessions-provider", "all");

    // A session whose file went away after the list was read fails where it
    // opens; Retry reads the history again, and the session stays listed as
    // unavailable with its last location instead of vanishing (B5, D-04).
    fs.rmSync(files["codex-login"]);
    // The copy note belongs to the session that was copied, not to the next
    // one opened: copy, open another at once, and look once.
    await page.locator('[data-session-copy="detail"]').click();
    await expect(page.locator('[data-session-header="claude-release"]')).toContainText("Copied");
    await page.locator('[data-session-row="codex-login"]').click();
    const header = page.locator('[data-session-header="codex-login"]');
    await expect(header).toBeVisible();
    expect(await header.textContent()).not.toContain("Copied");
    await expect(page.locator('[data-session-failure="codex-login"]')).toContainText("The session file could not be read: No such file or directory (os error 2)", {
      timeout: 15_000,
    });
    await screenshot(page, "s8-detail-failed");
    await page.locator("[data-session-detail-retry]").click();
    await expect(page.locator('[data-session-failure="codex-login"]')).toContainText(GONE, { timeout: 15_000 });
    await expect(rows).toHaveCount(4);
    const moved = page.locator('[data-session="codex-login"]');
    await expect(moved).toHaveAttribute("data-session-available", "false");
    await expect(page.locator('[data-session-reason="codex-login"]')).toHaveText(GONE);
    await expect(moved).toContainText("Fix the flaky login test");
    await page.locator('[data-session-copy="codex-login"]').click();
    await expect(moved).toContainText("Copied");
    expect(samePath(await page.evaluate(() => navigator.clipboard.readText()), files["codex-login"])).toBe(true);
    await screenshot(page, "s8-detail-gone");

    // A history that cannot be read says why, in the list, with Retry (B4).
    const claudeProjects = path.join(daemon.home, ".claude", "projects");
    fs.chmodSync(claudeProjects, 0o000);
    try {
      await page.locator('[data-session-retry="claude-broken"]').click();
      await expect(page.locator("[data-sessions-state]")).toHaveAttribute("data-sessions-state", "failed", { timeout: 15_000 });
      // The daemon names the folder by its HOME's own spelling (/var, not /private/var).
      await expect(screen).toContainText(/The session folder \S+\/home\/\.claude\/projects could not be read: Permission denied/);
      await screenshot(page, "s8-list-failed");
    } finally {
      fs.chmodSync(claudeProjects, 0o755);
    }
    await page.locator('[data-sessions-retry="list"]').click();
    await expect(rows).toHaveCount(4, { timeout: 15_000 });
    await expect(page.locator('[data-session="codex-login"]')).toHaveAttribute("data-session-available", "false");

    // Another window moves the agent focus: this window keeps its Project and
    // its list (B6). Then it names another Project: this window says so and
    // takes its Project back only when asked (A7).
    const other = await context.newPage();
    await other.setViewportSize({ width: 1280, height: 800 });
    await open(other, daemon);
    await expect(other.locator("[data-main-screen]")).toBeVisible({ timeout: 20_000 });
    await other.locator("[data-main-project]", { hasText: "fixture" }).click();
    const [, second] = herdr.panes;
    await other.locator(`[data-overview-screen] [data-agent-open="${second}"]`).click();
    await expect(other.locator("[data-workspace-screen]")).toBeVisible();
    await expect(other.locator(`[data-pane-view="${second}"]`)).toHaveAttribute("data-focused", "true", { timeout: 15_000 });
    // This window has seen the focus move, and its Project and list stayed.
    // `bg-secondary` alone marks the selected row; `focus-visible:bg-accent` is on every row.
    await expect(page.locator(`[data-agent-list] [data-pane="${second}"]`)).toHaveClass(/(^|\s)bg-secondary(\s|$)/, { timeout: 15_000 });
    await expect(page.locator("[data-sessions-screen]")).toHaveAttribute("data-sessions-screen", request?.workspace_id as string);
    await expect(rows).toHaveCount(4);
    await expect(page.locator("[data-session-detail]")).toHaveAttribute("data-session-detail", "failed");

    await other.keyboard.press("Alt+Shift+KeyN");
    const input = other.getByLabel("Workspace path");
    // The field checks a path against the home it lists first; until then Enter does nothing.
    await expect(input).toHaveValue(`${daemon.home}/`);
    await input.fill(alpha);
    await other.keyboard.press("Enter");
    // This machine's sheet stays open for another path; the Project shows on Main.
    await other.getByLabel("Close new workspace").first().click();
    await other.locator("[data-go-main]").first().click();
    await expect(other.locator("[data-main-project]", { hasText: "alpha" })).toBeVisible({ timeout: 20_000 });
    await other.locator("[data-main-project]", { hasText: "alpha" }).click();
    await other.locator("[data-overview-sessions]").click();
    await expect(other.locator("[data-session-row]")).toHaveCount(1, { timeout: 20_000 });
    await expect(other.locator('[data-session="claude-alpha"]')).toContainText("alpha only request");
    await screenshot(other, "s8-second-window");

    await expect(page.locator("[data-sessions-state]")).toHaveAttribute("data-sessions-state", "replaced", { timeout: 15_000 });
    await expect(page.locator("[data-session-detail]")).toHaveAttribute("data-session-detail", "none");
    await screenshot(page, "s8-replaced");
    const beforeShowHere = sent.get("sessions_refresh") ?? 0;
    await page.locator("[data-sessions-show-here]").click();
    await expect(rows).toHaveCount(4, { timeout: 15_000 });
    // Naming it again keeps the session whose file went away listed.
    await expect(page.locator('[data-session="codex-login"]')).toHaveAttribute("data-session-available", "false");
    expect(sent.get("sessions_refresh")).toBe(beforeShowHere + 1);
    await expect(other.locator("[data-sessions-state]")).toHaveAttribute("data-sessions-state", "replaced", { timeout: 15_000 });
    await other.close();

    // At the narrowest supported window the list and the detail stay
    // readable: Korean and English wrap, long paths do not push the layout (B9).
    await page.setViewportSize({ width: 1024, height: 700 });
    await page.locator('[data-session-row="claude-release"]').click();
    await expect(page.locator("[data-session-detail]")).toHaveAttribute("data-session-detail", "open", { timeout: 15_000 });
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
    expect(overflow).toBe(false);
    await screenshot(page, "s8-narrow");
    await page.locator('[data-session-row="claude-path"]').click();
    await expect(page.locator("[data-session-detail]")).toHaveAttribute("data-session-detail", "open", { timeout: 15_000 });
    await expect(page.locator('[data-turn="user"]')).toContainText(PATH_REQUEST);
    await screenshot(page, "s8-narrow-long-path");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
