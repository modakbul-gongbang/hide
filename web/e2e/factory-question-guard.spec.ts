// One real Factory worker and an independent native Claude pane on the pinned
// Herdr/hided stack (factory-ask-guard D-08). The provider is a compiled shim;
// the hook, Factory registration, transcript read and snapshot are production.
import { expect, test } from "@playwright/test";
import fs from "node:fs";
import { factoryQuestionFixture, startFactoryStack } from "./factory-fixture";
import { claudeProjects, sessionOf, writeFixtureTranscript } from "./herdr-fixture";
import type { FactorySection } from "../src/factory/model";
import type { AgentRow } from "../src/snapshot";
import { afterCleanup } from "./worker-owned";

type QuestionRow = AgentRow & { user_turn?: { kind: string; content: { text: string; choices: string[]; truncated: boolean } | null } };

// Same bounded stack-start/Factory-worker journey as factory.spec.ts.
test.describe.configure({ timeout: 150_000 });

test("Factory questions redirect to ask while a nonworker publishes and clears its native question", async ({ page }) => {
  let factory: FactorySection | undefined;
  let agents: QuestionRow[] = [];
  page.on("websocket", (socket) => socket.on("framereceived", (frame) => {
    if (typeof frame.payload !== "string") return;
    const incoming = JSON.parse(frame.payload) as { type: string; payload?: { factory?: FactorySection; rest?: { navigator?: { agents?: QuestionRow[] } } } };
    if (!["snapshot", "delta"].includes(incoming.type)) return;
    if (incoming.payload?.factory) factory = incoming.payload.factory;
    if (incoming.payload?.rest?.navigator?.agents) agents = incoming.payload.rest.navigator.agents;
  }));
  const stack = await startFactoryStack(page, "factory-question-guard");
  let failure: unknown;
  try {
    const fixture = factoryQuestionFixture(stack);
    expect((await stack.cli("init", stack.project, "--no-verification", "--merge", "manual", "--confirm")).ok).toBe(true);
    expect((await stack.cli("config", "--project", stack.project, "--set", "disk_floor_gb=0")).ok).toBe(true);
    expect((await stack.cli("config", "--project", stack.project, "--set", "default_runtime=claude")).ok).toBe(true);
    const added = await stack.cli("add", "--project", stack.project, "--title", "Guard the worker question", "--goal", "Check native question ownership", "--criterion", "Question hook redirects the worker");
    expect(added.result, JSON.stringify(added)).toBe("ready");
    const task = (added.task as { id: string }).id;
    const card = () => factory?.summary?.factories.flatMap((entry) => entry.columns.flatMap((column) => column.cards)).find((entry) => entry.task === task);
    await expect.poll(() => card()?.worker_pane, { message: "Factory accepted its real supported worker", timeout: 30_000 }).toBeTruthy();
    const worker = card()!.worker_pane!;
    expect(card()?.state).toBe("running");
    for (const output of Object.values(await fixture.hooks(worker))) {
      expect(JSON.parse(output).hookSpecificOutput).toMatchObject({ permissionDecision: "deny", permissionDecisionReason: expect.stringContaining("hide factory ask") });
    }

    const nonworker = stack.herdr.panes[1];
    stack.herdr.run(["agent", "start", "independent", "--kind", "claude", "--pane", nonworker]);
    const session = sessionOf(stack.herdr, nonworker);
    const row = () => agents.find((agent) => agent.pane_id === nonworker);
    await expect.poll(() => row()?.session_id, { message: "independent native session reached the core", timeout: 20_000 }).toBe(session);
    expect(await fixture.hooks(nonworker)).toEqual({ AskUserQuestion: "", ExitPlanMode: "" });
    const transcript = writeFixtureTranscript(claudeProjects(stack.herdr), session, { task: "Independent native question", end: "working" });
    await fixture.state(nonworker, "working");
    fs.appendFileSync(transcript, `${JSON.stringify({ type: "assistant", sessionId: session, timestamp: "2026-10-08T09:00:00Z", message: { role: "assistant", content: [{ type: "tool_use", id: "target-question", name: "AskUserQuestion", input: { questions: [{ question: "배포 대상을 골라주세요", header: "배포", options: [{ label: "미리보기", description: "검토용 배포" }, { label: "운영", description: "공개 배포" }], multiSelect: false }] } }] } })}\n`);
    await fixture.state(nonworker, "idle");
    await expect.poll(() => row()?.user_turn, { message: "native unanswered question reached the snapshot", timeout: 20_000 }).toEqual({ kind: "question", content: { text: "배포 대상을 골라주세요", choices: ["미리보기", "운영"], truncated: false } });
    expect(row()).toMatchObject({ group: "needs_you", status_code: "question" });
    await page.locator('[data-sidebar-mode="agents"]').click();
    await expect(page.locator(`[data-agent-group="needs_you"] [data-pane="${nonworker}"] [data-agent-status-mark="question"]`)).toBeVisible();

    await fixture.state(nonworker, "working");
    fs.appendFileSync(transcript, `${JSON.stringify({ type: "user", sessionId: session, timestamp: "2026-10-08T09:00:01Z", message: { role: "user", content: [{ type: "tool_result", tool_use_id: "target-question", content: "미리보기" }] } })}\n`);
    await fixture.state(nonworker, "idle");
    await expect.poll(() => row()?.user_turn, { message: "answer cleared the structured question", timeout: 20_000 }).toBeUndefined();
    expect(row()).toMatchObject({ pane_id: nonworker, session_id: session });
    await expect(page.locator(`[data-agent-group="needs_you"] [data-pane="${nonworker}"]`)).toHaveCount(0);
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    const stop = () => {
      try { stack.daemon.stop(); } finally { stack.herdr.stop(); }
    };
    if (failure !== undefined) afterCleanup(failure, stop);
    else stop();
  }
});
