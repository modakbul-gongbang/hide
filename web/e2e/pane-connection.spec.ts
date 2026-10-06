// The Not connected chip on an isolated pinned Herdr and hided (PRD settings-cleanup B26, B28, B31): a
// Claude Code pane that was running when Hide's hook was installed wears the chip, its popover offers
// Reopen and Not now, the pane's own SessionStart hook (the real helper, run the way the agent runs it)
// turns the chip off, and a refused Reopen leaves the pane as it was and says why in the popover.
// A Reopen that succeeds is not here: the core starts the agent straight after ending it, and against a
// named agent the pinned Herdr still holds the name for a moment, so `agent.start` is refused as
// `agent_name_taken` and the pane is left on its shell. That is the core's to fix before this spec can
// follow a Reopen through to a connected pane.
// The Codex shared-server reasons need a Codex pane Herdr detects and a machine kit that read it, which
// this fixture has no program for: `PaneConnection.test.tsx` and the core's `runtime::tests::agent_connection`
// own them.

import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { claudeProjects, setFixtureLifecycle, setFixtureSession, startHerdr, writeFixtureTranscript, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

test.describe.configure({ timeout: 150_000 });

const SESSION = "22222222-3333-4444-5555-666666666666";
const HELPER = path.resolve("..", "target", "debug", "hide-agent-hooks");

type ProcessInfo = { result: { process_info: { foreground_processes: { argv?: string[] | null }[] } } };

/** Hide's hook entries as the kit writes them (`hide-agent-hooks/src/install.rs`), so this machine's files say the hook is installed. */
function installHook(home: string): void {
  const entry = (event: string) => [
    {
      hooks: [
        {
          type: "command",
          command: `if [ -x '${HELPER}' ]; then exec '${HELPER}' hook --runtime claude-code --event ${event} --memory-injection --source hide-subagents@6; fi`,
          timeout: 8,
        },
      ],
    },
  ];
  const hooks = Object.fromEntries(["SessionStart", "UserPromptSubmit", "SubagentStart", "SubagentStop", "Stop"].map((event) => [event, entry(event)]));
  fs.mkdirSync(path.join(home, ".claude"), { recursive: true });
  fs.writeFileSync(path.join(home, ".claude", "settings.json"), JSON.stringify({ hooks }));
}

async function start(page: Page, label: string): Promise<{ herdr: HerdrFixture; daemon: Daemon; pane: string; sent: Map<string, number> }> {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  const [, pane] = herdr.panes;
  writeFixtureTranscript(claudeProjects(herdr), SESSION, { task: "Agent two" });
  setFixtureSession(herdr, pane, SESSION);
  const home = path.join(herdr.root, "hided-home");
  installHook(home);
  const daemon = await startHided(herdr, label, home);
  const sent = countSent(page);
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await page.locator('[data-sidebar-mode="projects"]').click();
  await page.locator("[data-checkout]").first().click();
  await expect(page.locator(`[data-pane-view="${pane}"]`)).toBeVisible({ timeout: 20_000 });
  return { herdr, daemon, pane, sent };
}

const processArgv = (herdr: HerdrFixture, pane: string) =>
  (herdr.run(["pane", "process-info", "--pane", pane]) as ProcessInfo).result.process_info.foreground_processes.map((process) => process.argv?.join(" "));

test("a session started before the hook wears the chip, Not now keeps it, and the session's own hook turns it off", async ({ page }) => {
  const { herdr, daemon, pane, sent } = await start(page, "pane-connection");
  try {
    const view = page.locator(`[data-pane-view="${pane}"]`);
    const chip = view.locator("[data-pane-connection]");
    // B26: both fixture agents were running before the hook was installed, so both panes wear the chip.
    const other = page.locator(`[data-pane-view="${herdr.panes[0]}"] [data-pane-connection]`);
    await expect(chip).toHaveAttribute("data-pane-connection", "started_before_hide", { timeout: 30_000 });
    await expect(other).toHaveAttribute("data-pane-connection", "started_before_hide");

    // B28, B31: the popover says why and offers Reopen and Not now; Not now closes it and the chip stays.
    await chip.click();
    const popover = page.locator("[data-pane-connection-popover]");
    await expect(popover).toContainText("Started before Hide was set up");
    await screenshot(page, "pane-connection-popover");
    await popover.locator("[data-pane-connection-dismiss]").click();
    await expect(popover).toHaveCount(0);
    await expect(chip).toBeVisible();

    // B26: a session whose own SessionStart hook reaches Hide (the real helper, run the way the agent runs
    // it) is connected: its chip goes, with the popover that was open, and the other pane keeps its chip.
    await chip.click();
    await expect(popover).toBeVisible();
    execFileSync(HELPER, ["hook", "--runtime", "claude-code", "--event", "SessionStart", "--memory-injection", "--source", "hide-subagents@6"], {
      input: "{}",
      env: { ...process.env, HOME: daemon.home, HERDR_PANE_ID: pane, HERDR_SOCKET_PATH: herdr.socket, HERDR_ENV: "1" },
      timeout: 20_000,
    });
    await expect(chip).toHaveCount(0, { timeout: 30_000 });
    await expect(page.locator("[data-pane-connection-popover]")).toHaveCount(0);
    // Only the pane that connected lost its chip.
    await expect(other).toBeVisible();
    expect(sent.get("pane_reopen")).toBeUndefined();
  } finally {
    daemon.stop();
    herdr.stop();
  }
});

test("a Reopen the core refuses leaves the pane as it was and the popover says why", async ({ page }) => {
  const { herdr, daemon, pane, sent } = await start(page, "pane-connection-busy");
  try {
    await setFixtureLifecycle(herdr, pane, "working");
    const view = page.locator(`[data-pane-view="${pane}"]`);
    await expect(view.locator("[data-pane-connection]")).toBeVisible({ timeout: 30_000 });
    const before = processArgv(herdr, pane);

    await view.locator("[data-pane-connection]").click();
    await page.locator(`[data-pane-reopen="${pane}"]`).click();
    await expect.poll(() => sent.get("pane_reopen")).toBe(1);
    const failure = page.locator("[data-pane-reopen-failed]");
    // The agent's state in the core's rows lags the screen Herdr reads, so a busy agent is refused up front
    // (`agent_busy`) or by the end itself (`end_refused`); either way nothing was touched.
    await expect(failure).toHaveAttribute("data-pane-reopen-failed", /^(agent_busy|end_refused)$/, { timeout: 20_000 });
    await screenshot(page, "pane-connection-refused");
    // Everything knowable first was refused first: the agent was not touched.
    expect(processArgv(herdr, pane)).toEqual(before);
    await expect(page.locator(`[data-pane-reopen="${pane}"]`)).toBeEnabled();
  } finally {
    daemon.stop();
    herdr.stop();
  }
});
