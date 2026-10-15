import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fixtureExecutable } from "./platform-fixture";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { copyFixtureShim } from "./shims/build";
import { enterWorkspace, screenshot, showTool } from "./wire";

// View openers and the operator's comments (PRD view-opener-feedback): an
// agent's `hide file open` marks the View it opened, the operator comments on
// a line of it, and Send delivers one letter from the operator to that agent.

test.describe.configure({ timeout: 180_000 });
test.use({ actionTimeout: 15_000 });

/**
 * Makes `pane` a Claude Code agent Herdr lists, whose `!` runs one `hide`
 * command from inside the agent's own process, as the agent would: the
 * fixture `claude` execs the spawn-provider shim, which reports the native
 * session and runs a small driver that reads the next command from a file.
 */
async function agentPane(herdr: HerdrFixture, daemon: Daemon, pane: string): Promise<(args: string[]) => Promise<string>> {
  const root = herdr.root;
  const runner = path.join(root, "worker-bin", fixtureExecutable("claude"));
  fs.mkdirSync(path.dirname(runner));
  copyFixtureShim("spawn-provider", runner);
  fs.writeFileSync(path.join(root, "factory-question-runner.config"), runner);
  const command = path.join(root, "view-feedback-command.json");
  const output = path.join(root, "view-feedback-output.txt");
  const completed = path.join(root, "view-feedback-command.status");
  const driver = path.join(root, "view-feedback-driver.cjs");
  const hide = path.resolve("..", "target", "debug", fixtureExecutable("hide"));
  fs.writeFileSync(driver, `
const fs = require("node:fs");
const { execFileSync } = require("node:child_process");
const args = JSON.parse(fs.readFileSync(${JSON.stringify(command)}, "utf8"));
const stdout = execFileSync(${JSON.stringify(hide)}, args, {
  env: { ...process.env, HIDE_STATE_DIR: ${JSON.stringify(daemon.stateDir)} },
  encoding: "utf8", timeout: 15000, maxBuffer: 1 << 20,
});
fs.writeFileSync(${JSON.stringify(output)}, stdout);
`);
  fs.writeFileSync(path.join(root, "provider.config"), [herdr.bin, completed, process.execPath, driver, ""].join("\n"));
  herdr.run(["agent", "start", "viewer", "--kind", "claude", "--pane", pane]);
  await expect.poll(() => {
    const listed = herdr.run(["agent", "list"]) as { result: { agents: { pane_id: string; agent_session?: { value: string } }[] } };
    return listed.result.agents.find((agent) => agent.pane_id === pane)?.agent_session?.value ?? null;
  }, { message: `${pane} runs a listed Claude Code agent`, timeout: 20_000 }).toBeTruthy();
  return async (args) => {
    fs.writeFileSync(command, JSON.stringify(args));
    fs.rmSync(completed, { force: true });
    fs.rmSync(output, { force: true });
    execFileSync(herdr.bin, ["pane", "send-text", pane, "!"], { env: herdr.env, timeout: 10_000 });
    await expect.poll(() => (fs.existsSync(completed) ? fs.readFileSync(completed, "utf8").trim() : null),
      { message: `hide ${args.join(" ")} ran in ${pane}`, timeout: 20_000 }).toBe("0");
    return fs.readFileSync(output, "utf8");
  };
}

const front = (page: Page) => page.locator('[data-view-tab-bar] [role="tab"][aria-selected="true"]');

/** Opens the note under line `line` of the View in front through the `+` the gutter shows on the pointer's row (B12). */
async function noteOnLine(page: Page, line: number, note: string): Promise<void> {
  await page.locator(".cm-lineNumbers .cm-gutterElement", { hasText: new RegExp(`^${line}$`) }).hover();
  await page.locator(`[data-view-comment-add="${line}"]`).click();
  const field = page.locator("[data-view-note-field]");
  await expect(field).toBeFocused();
  await field.fill(note);
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.locator("[data-view-note-form]")).toHaveCount(0);
}

test("an agent's View wears its mark, and the operator's line comment reaches that agent as one letter", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const herdr = await startHerdr({ agents: false });
  let daemon: Daemon | null = null;
  try {
    const fixture = path.join(herdr.root, "fixture");
    fs.writeFileSync(path.join(fixture, "notes.md"), ["# Notes", "", "The first line.", "The second line.", "The end."].join("\n") + "\n");
    const pane = herdr.panes[0]!;
    daemon = await startHided(herdr, "view-feedback");
    const hide = await agentPane(herdr, daemon, pane);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await showTool(page, "files");
    await page.locator("[data-lane-pin]").click();

    // B1, B5: the agent's open marks the View with its logo; the mark says who and leads to its pane.
    await hide(["file", "open", "notes.md"]);
    await expect(front(page)).toHaveAttribute("aria-label", /\/notes\.md/);
    const mark = front(page).locator("[data-view-opener]");
    await expect(mark).toHaveAttribute("data-view-opener", pane);
    await expect(mark).toHaveAttribute("aria-label", /Claude Code/);
    // B8: the agent's pane counts the View it opened.
    await expect(page.locator(`[data-pane-views="${pane}"]`)).toHaveText(/1 view/);

    // B12, B14: a comment on line 3 shows under it, on the tab and on the line under the View.
    await expect(page.locator(".cm-content")).toContainText("The first line.");
    await noteOnLine(page, 3, "Say what the first line is for.");
    await expect(page.locator('[data-view-comment-number="1"]')).toContainText("Say what the first line is for.");
    await expect(front(page).locator("[data-view-comments]")).toHaveText("1");
    const row = page.locator("[data-view-feedback-row]");
    await expect(row).toHaveAttribute("data-view-feedback-state", "unsent");
    await expect(row.locator("[data-view-feedback-summary]")).toHaveText(/1 comment · 1 view/);

    // B43: a deleted comment comes back with Undo, at the same number.
    await noteOnLine(page, 5, "Throwaway.");
    const second = page.locator('[data-view-comment-number="2"]');
    await second.locator("[data-view-comment-delete]").click();
    await expect(second).toHaveCount(0);
    await expect(row).toHaveAttribute("data-view-feedback-state", "deleted");
    await row.locator("[data-view-feedback-undo]").click();
    await expect(page.locator('[data-view-comment-number="2"]')).toContainText("Throwaway.");
    await page.locator('[data-view-comment-number="2"] [data-view-comment-delete]').click();
    await expect(page.locator("[data-view-comment]")).toHaveCount(1);

    // B16, B21: once Undo's moment passes, Send carries the comment to the opener as one letter from the operator, and the View lets it go.
    await expect(row).toHaveAttribute("data-view-feedback-state", "unsent", { timeout: 15_000 });
    await screenshot(page, "view-feedback-unsent");
    await row.locator('[data-view-feedback-send-main="default"]').click();
    await expect(row).toHaveAttribute("data-view-feedback-state", "sent");
    await expect(row).toContainText("delivered when it rests");
    await expect(page.locator("[data-view-comment]")).toHaveCount(0);
    await expect(front(page).locator("[data-view-comments]")).toHaveCount(0);
    await screenshot(page, "view-feedback-sent");

    // B22, B24: the agent's inbox holds the letter, with the path, the line and the quoted text.
    const inbox = await hide(["inbox"]);
    expect(inbox).toContain("notes.md");
    expect(inbox).toContain("The first line.");
    expect(inbox).toContain("Say what the first line is for.");
    expect(inbox).not.toContain("Throwaway.");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
