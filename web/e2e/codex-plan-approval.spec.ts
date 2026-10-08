// A Codex plan waiting for approval on an isolated pinned Herdr and hided
// (PRD codex-plan-approval-hold B1, B4, B12): Herdr reads Codex's
// "Implement this plan?" menu as an ordinary stop, so the wait is read from
// the session file. A rollout whose last plan-mode turn proposed a plan puts
// the Codex row in Needs You with the approval mark, and the next turn's
// record takes it out. That the row stays after it is read (B2) is the
// core's rule, tested in `herdr-core/src/runtime/tests/labels.rs`.
//
// The fixture Codex is the shim copied as `codex`. Pinned Herdr reads its
// screen as working while the title carries Codex's spinner and as unknown
// otherwise (`codex_state_ambiguous`); only a real Codex screen reads idle.
// Each change moves Herdr's state sequence, which is what makes the core read
// the session again, so the wait is read for the state the row is in.

import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { sessionOf, startHerdr, type HerdrFixture } from "./herdr-fixture";
import { fixtureExecutable } from "./platform-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

const SESSION = "0199a000-0000-7000-8000-0000000000e2";

/** One Codex 0.160.1 rollout record. */
function record(type: string, payload: Record<string, unknown>): string {
  return `${JSON.stringify({ timestamp: "2026-10-07T01:00:00.000Z", type, payload })}\n`;
}

function event(kind: string, turn: string, extra: Record<string, unknown> = {}): string {
  return record("event_msg", { type: kind, turn_id: turn, ...extra });
}

function person(text: string): string {
  return record("response_item", { type: "message", role: "user", content: [{ type: "input_text", text }] });
}

/** A session whose last turn ran in plan mode and proposed a plan. */
function planWaiting(cwd: string): string {
  return record("session_meta", { id: SESSION, cwd, cli_version: "0.160.1" })
    + event("task_started", "turn-1", { collaboration_mode_kind: "plan" })
    + person("로그인 화면을 고칠 계획을 세워줘")
    + event("item_completed", "turn-1", { item: { type: "Plan", id: "item-1", text: "1. 입력 검증을 고친다" } })
    + record("response_item", { type: "message", role: "assistant", content: [{ type: "output_text", text: "<proposed_plan>\n1. 입력 검증을 고친다\n</proposed_plan>" }] })
    + event("task_complete", "turn-1", { last_agent_message: null });
}

/** The session read the daemon keeps for the pane: its Herdr state and the last turn it folded. */
function turnRead(daemon: Daemon, pane: string): { seq?: number; mode?: string } {
  type Stored = { turns_seq?: number; turns?: { last?: { mode?: string } } };
  try {
    const file = JSON.parse(fs.readFileSync(path.join(daemon.stateDir, "labels.json"), "utf8")) as { targets?: Record<string, Record<string, Stored>> };
    const record = Object.values(file.targets ?? {}).map((panes) => panes[pane]).find(Boolean);
    return { seq: record?.turns_seq, mode: record?.turns?.last?.mode };
  } catch {
    return {};
  }
}

function seqOf(herdr: HerdrFixture, pane: string): number {
  type Agent = { pane_id: string; state_change_seq: number };
  return (herdr.run(["agent", "list"]) as { result: { agents: Agent[] } }).result.agents.find((agent) => agent.pane_id === pane)!.state_change_seq;
}

/** Draws a screen Herdr reads as Codex working, or, with no spinner, unknown. */
async function moveState(herdr: HerdrFixture, pane: string, state: "working" | "unknown"): Promise<void> {
  type Agent = { pane_id: string; agent_status: string; state_change_seq: number };
  const current = () => (herdr.run(["agent", "list"]) as { result: { agents: Agent[] } }).result.agents.find((agent) => agent.pane_id === pane)!;
  const before = current().state_change_seq;
  const title = state === "working" ? "⠋ Working" : "plan";
  execFileSync(herdr.bin, ["pane", "send-text", pane, `\x1b]0;${title}\x07`], { env: herdr.env, timeout: 30_000 });
  await expect.poll(() => {
    const agent = current();
    return agent.agent_status === state && agent.state_change_seq > before;
  }, { message: `Herdr reads the Codex pane ${state}`, timeout: 10_000 }).toBe(true);
}

test("a Codex plan waiting for approval holds its row in Needs You until the next turn", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const cwd = path.join(herdr.root, "plan");
    fs.mkdirSync(cwd, { recursive: true });
    const created = herdr.run([
      "workspace", "create", "--cwd", cwd, "--label", "plan", "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]) as { result: { root_pane: { pane_id: string } } };
    const pane = created.result.root_pane.pane_id;
    fs.copyFileSync(path.join(herdr.root, "bin", fixtureExecutable("claude")), path.join(herdr.root, "bin", fixtureExecutable("codex")));
    await expect.poll(() => execFileSync(herdr.bin, ["pane", "read", pane, "--source", "recent", "--lines", "5"], { env: herdr.env, encoding: "utf8" }), { timeout: 20_000 }).toContain("fixture %");
    herdr.run(["agent", "start", "planner", "--kind", "codex", "--pane", pane]);
    // Herdr's Codex integration reports the session at SessionStart; this
    // replaces the session the fixture declared for the started agent.
    execFileSync(herdr.bin, ["pane", "report-agent-session", pane, "--source", "herdr:codex", "--agent", "codex", "--agent-session-id", SESSION, "--seq", "2", "--session-start-source", "clear"], { env: herdr.env, timeout: 30_000 });
    await expect.poll(() => sessionOf(herdr, pane)).toBe(SESSION);

    daemon = await startHided(herdr, "codex-plan-approval");
    const sessions = path.join(daemon.home, ".codex", "sessions", "2026", "10", "07");
    fs.mkdirSync(sessions, { recursive: true });
    const rollout = path.join(sessions, `rollout-2026-10-07T01-00-00-${SESSION}.jsonl`);
    fs.writeFileSync(rollout, planWaiting(cwd));

    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "plan");
    const row = page.locator(`nav[data-sidebar] [data-project] [data-pane="${pane}"]`);
    await expect(row).toBeVisible({ timeout: 20_000 });

    // B1: the turn that proposed the plan has ended, as Herdr's next state
    // reads it; the row is the operator's to answer, with the approval mark.
    await moveState(herdr, pane, "working");
    await moveState(herdr, pane, "unknown");
    const waiting = page.locator(`[data-raised-group="needs_you"] [data-pane="${pane}"]`);
    await expect(waiting).toBeVisible({ timeout: 20_000 });
    await expect(waiting.locator('[data-mark="!"]')).toBeVisible();
    await screenshot(page, "codex-plan-approval-waiting");

    // B4: approving starts the next turn; once Herdr's state moves, the row
    // leaves Needs You.
    fs.appendFileSync(rollout, event("task_started", "turn-2", { collaboration_mode_kind: "default" }) + person("Implement the plan."));
    await moveState(herdr, pane, "working");
    await expect(row.locator('[data-mark="●"]')).toBeVisible({ timeout: 20_000 });
    await moveState(herdr, pane, "unknown");
    // A state not read yet shows no wait either, so the row's absence counts
    // only once the read for this state has folded the approving turn.
    const current = seqOf(herdr, pane);
    await expect.poll(() => turnRead(daemon!, pane), { message: "the session is read for the current state", timeout: 20_000 }).toEqual({ seq: current, mode: "other" });
    await expect(row).toBeVisible();
    await expect(page.locator(`[data-raised-group="needs_you"] [data-pane="${pane}"]`)).toHaveCount(0);
    await screenshot(page, "codex-plan-approval-approved");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
