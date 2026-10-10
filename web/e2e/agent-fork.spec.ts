// Fork agent from the pane menu on an isolated pinned Herdr and hided (issue
// 916): the item is offered on an agent pane whose conversation Herdr
// reported and starts the same conversation again as a new session in a pane
// of its own, which then wears the fork mark. The disabled item of an agent with no conversation yet is the
// pane menu unit test's and the core's.

import { expect, test } from "@playwright/test";
import { claudeProjects, setFixtureSession, startHerdr, writeFixtureTranscript } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 150_000 });

const SESSION = "11111111-2222-3333-4444-555555555555";
const FORKED_SESSION = "66666666-7777-8888-9999-000000000000";

type Listed = { result: { agents: { pane_id: string; name?: string | null; agent: string }[] } };
type ProcessInfo = { result: { process_info: { foreground_processes: { argv?: string[] | null }[] } } };

test("Fork agent from the pane menu starts the pane's conversation again in a new pane", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [, forker] = herdr.panes;
    // The official Claude integration's report: the conversation id, under
    // Herdr's own source.
    writeFixtureTranscript(claudeProjects(herdr), SESSION, { task: "Agent two" });
    setFixtureSession(herdr, forker, SESSION);
    const agentPanes = () => (herdr.run(["agent", "list"]) as Listed).result.agents.map((agent) => agent.pane_id);

    daemon = await startHided(herdr, "agent-fork");
    const last = new Map<string, Record<string, unknown>>();
    countSent(page, last);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await page.locator("[data-checkout]").first().click();
    await expect(page.locator(`[data-pane-view="${forker}"]`)).toBeVisible({ timeout: 20_000 });
    const before = agentPanes();

    // Fork agent is offered on an agent pane that reported its conversation and
    // sends one event naming the pane.
    await page.locator(`[data-pane-menu="${forker}"]`).click();
    const item = page.locator('[data-menu-item="fork_agent"]');
    await expect(item).toBeEnabled({ timeout: 20_000 });
    await screenshot(page, "agent-fork-menu");
    await item.click();
    await expect.poll(() => last.get("fork_pane")?.pane_id).toBe(forker);

    // The new execution is a pane of its own that resumes the conversation as
    // a new session.
    let forked = "";
    await expect.poll(() => {
      forked = agentPanes().find((pane) => !before.includes(pane)) ?? "";
      return forked;
    }, { timeout: 60_000 }).not.toBe("");
    // The official Claude integration reports the new execution's own
    // conversation id; the fork waits for it to register the lineage.
    setFixtureSession(herdr, forked, FORKED_SESSION);
    await expect.poll(
      () => (herdr.run(["pane", "process-info", "--pane", forked]) as ProcessInfo).result.process_info.foreground_processes.map((process) => process.argv?.join(" ")),
      { timeout: 30_000 },
    ).toContain(`claude --resume ${SESSION} --fork-session`);

    // The forked pane says so, and names its source; the source does not.
    await page.locator(`nav[data-sidebar] [data-pane="${forked}"]`).click();
    const mark = page.locator(`[data-pane-view="${forked}"] [data-pane-fork]`);
    await expect(mark).toHaveAttribute("data-pane-fork", forker, { timeout: 30_000 });
    await expect(mark).toHaveAccessibleName("Forked from Agent two");
    await expect(page.locator(`[data-pane-view="${forker}"] [data-pane-fork]`)).toHaveCount(0);
    await screenshot(page, "agent-fork-forked");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
