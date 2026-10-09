// Synthetic native-format writers on actual pinned Herdr and hided.
// Installed CLI measurements remain a separate acceptance boundary.
import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { GROK_ID, GROK_PLAN, GROK_TITLE, OMP_ID, OMP_TITLE, OMP_UPDATED_TITLE, PI_ID, PI_TITLE, appendOmpQuestion, prepareNativeWriter, reportNativeWriter, setGrokPlanApproval, updateOmpTitle } from "./session-reader-fixture";
import { screenshot } from "./wire";
import { afterCleanup } from "./worker-owned";
import type { AgentRow } from "../src/snapshot";

type QuestionRow = AgentRow & { user_turn?: { kind: string; content: { text: string; choices: string[]; truncated: boolean } | null } };

test.describe.configure({ timeout: 150_000 });

for (const [kind, id, initialTitle, resumeFlag] of [["pi", PI_ID, PI_TITLE, "--session"], ["omp", OMP_ID, OMP_TITLE, "--resume"], ["grok", GROK_ID, GROK_TITLE, "--resume"]] as const) {
test(`${kind} native title and durable sleep wake the exact conversation in a fresh pane`, async ({ page }) => {
  let agents: QuestionRow[] = [];
  page.on("websocket", (socket) => socket.on("framereceived", (frame) => {
    if (typeof frame.payload !== "string") return;
    const incoming = JSON.parse(frame.payload) as { type: string; payload?: { rest?: { navigator?: { agents?: QuestionRow[] } } } };
    if (["snapshot", "delta"].includes(incoming.type) && incoming.payload?.rest?.navigator?.agents) agents = incoming.payload.rest.navigator.agents;
  }));
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const sourcePane = herdr.panes[0];
    const session = prepareNativeWriter(herdr, kind);
    expect(fs.existsSync(session)).toBe(false);
    herdr.run(["agent", "start", `${kind}-reader`, "--kind", kind, "--pane", sourcePane]);
    await expect.poll(() => fs.existsSync(session)).toBe(true);
    reportNativeWriter(herdr, kind, sourcePane, session);
    daemon = await startHided(herdr, `${kind}-reader`, herdr.env.HOME);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await page.locator("[data-checkout]").first().click();
    const row = page.locator(`nav[data-sidebar] [data-checkout-agents-open] [data-pane="${sourcePane}"]`);
    await expect(row).toContainText(initialTitle, { timeout: 30_000 });
    await screenshot(page, `${kind}-native-title`);
    let title: string = initialTitle;
    if (kind === "omp") {
      const size = fs.statSync(session).size;
      updateOmpTitle(session, OMP_UPDATED_TITLE);
      expect(fs.statSync(session).size).toBe(size);
      title = OMP_UPDATED_TITLE;
      await expect(row).toContainText(title, { timeout: 30_000 });
      await expect(row).not.toContainText("Old audit title");
      const observed = () => agents.find((agent) => agent.pane_id === sourcePane);
      const question = { kind: "question", content: { text: "배포 대상을 골라주세요", choices: ["미리보기", "운영"], truncated: false } };
      appendOmpQuestion(session);
      await expect.poll(() => observed()?.user_turn, { timeout: 30_000 }).toEqual(question);
      await expect(row.locator('[data-agent-status-mark="question"]')).toBeVisible();
      await row.click();
      await expect(page.locator(`[data-pane-view="${sourcePane}"] [data-pane-header-band]`)).toHaveAttribute("data-pane-header-band", "answer");
      await screenshot(page, "omp-current-title-question");
      appendOmpQuestion(session, true);
      await expect.poll(() => observed()?.user_turn, { timeout: 30_000 }).toBeUndefined();
      await expect(row.locator('[data-agent-status-mark="question"]')).toHaveCount(0);
    }
    if (kind === "grok") {
      // Herdr reads Grok's plan approval as working; Grok's own state is the wait.
      const observed = () => agents.find((agent) => agent.pane_id === sourcePane);
      setGrokPlanApproval(session, true);
      await expect.poll(() => observed()?.user_turn, { timeout: 30_000 }).toEqual({ kind: "plan_approval", content: { text: GROK_PLAN, choices: [], truncated: false } });
      await expect(row.locator('[data-agent-status-mark="approval"]')).toBeVisible();
      await screenshot(page, "grok-plan-approval");
      setGrokPlanApproval(session, false);
      await expect.poll(() => observed()?.user_turn, { timeout: 30_000 }).toBeUndefined();
    }
    await row.click();
    await page.locator(`[data-pane-menu="${sourcePane}"]`).click();
    await expect(page.locator('[data-menu-item="sleep_agent"]')).toBeEnabled({ timeout: 20_000 });
    await page.locator('[data-menu-item="sleep_agent"]').click();
    const sleeping = page.locator('[data-sleeping-session]');
    await expect(sleeping).toContainText(title, { timeout: 30_000 });
    await expect(row).toHaveCount(0);
    await expect.poll(() => JSON.stringify(herdr.run(["pane", "list"]))).not.toContain(`"pane_id":"${sourcePane}"`);
    await screenshot(page, `${kind}-durable-sleep`);
    const prior = fs.readFileSync(session, "utf8");
    await sleeping.getByRole("button", { name: "Wake agent" }).click();
    type Listed = { result: { agents: { pane_id: string; agent: string }[] } };
    let fresh = "";
    await expect.poll(() => {
      const agents = (herdr.run(["agent", "list"]) as Listed).result.agents;
      fresh = agents.find(agent => agent.agent === kind && agent.pane_id !== sourcePane)?.pane_id ?? "";
      return fresh;
    }, { timeout: 30_000 }).not.toBe("");
    await expect.poll(() => fs.readFileSync(path.join(herdr.root, `${kind}-launches.jsonl`), "utf8").trim().split("\n").map(line => JSON.parse(line) as string[])).toContainEqual([resumeFlag, id]);
    reportNativeWriter(herdr, kind, fresh, session);
    await expect(sleeping).toHaveCount(0, { timeout: 30_000 });
    await expect(page.locator(`nav[data-sidebar] [data-pane="${fresh}"]`)).toContainText(title);
    expect(fs.readFileSync(session, "utf8")).toBe(prior);
    await screenshot(page, `${kind}-exact-wake`);
  } catch (error) {
    throw afterCleanup(afterCleanup(error, () => daemon?.stop()), () => herdr.stop());
  }
  try { daemon?.stop(); } catch (error) { throw afterCleanup(error, () => herdr.stop()); }
  herdr.stop();
});
}
