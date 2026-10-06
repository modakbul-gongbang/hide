// Settings > Agents on an isolated pinned Herdr and a hided that runs the install kit (PRD settings-cleanup
// B8 to B20): the seven supported agents of this Mac, the ones whose program the private HOME holds under
// Installed and the rest folded under Not installed, a switch that writes and removes Hide's own entries,
// the Partial popover from its chip by keyboard, and a hook the operator removed shown on its row with
// Reinstall. How a status reads for sessions that run without Hide is the component test's, on invented
// snapshots; a real session of each kind needs an agent the machine does not have.

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

// The kit writes POSIX hook files; the guidance hooks are not written on Windows.
test.skip(process.platform === "win32", "the kit installs POSIX hooks");
test.describe.configure({ timeout: 120_000 });

const read = (file: string) => (fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "");

/**
 * A private HOME with four agents installed (a program in `~/.local/bin`, which the kit searches; Pi,
 * OpenCode and Cursor have none) and a kit record, so Claude Code and Codex are on and the rest wait.
 */
async function start(label: string) {
  const herdr = await startHerdr();
  const home = path.join(fs.mkdtempSync(path.join(herdr.root, "ag-")), "home");
  for (const folder of [".claude", ".codex", ".gemini", ".grok"]) fs.mkdirSync(path.join(home, folder), { recursive: true });
  fs.mkdirSync(path.join(home, ".local", "bin"), { recursive: true });
  for (const program of ["claude", "codex", "gemini", "grok"]) fs.writeFileSync(path.join(home, ".local", "bin", program), "#!/bin/sh\n", { mode: 0o755 });
  fs.mkdirSync(path.join(home, ".hide", "kit"), { recursive: true });
  fs.writeFileSync(path.join(home, ".hide", "kit", "installed.json"), JSON.stringify({ format: 1, installed: [] }));
  const daemon = await startHided(herdr, label, home, {}, true);
  return { herdr, daemon, home };
}

async function openAgents(page: Page, daemon: Daemon) {
  await page.setViewportSize({ width: 1200, height: 1000 });
  await page.goto(`${daemon.origin}/#token=${daemon.token}`);
  await enterWorkspace(page, "fixture");
  await page.locator("[data-open-settings]").click();
  await page.locator('[data-settings-tab="agents"]').click();
  return page.locator('[data-agents-machine-list="local"]');
}

test("lists the installed agents with a status and a switch, and folds the others under Not installed with an install link", async ({ page }) => {
  const { herdr, daemon } = await start("agents-list");
  try {
    const list = await openAgents(page, daemon);
    // B8, B9: the programs the home holds are Installed, in the fixed order; the rest are folded.
    await expect(list.locator("[data-agent-row]")).toHaveCount(7, { timeout: 60_000 });
    await expect(list).toContainText("Installed 4");
    await expect(list).toContainText("Not installed 3");
    const rows = await list.locator("[data-agent-row]").evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-agent-row")));
    expect(rows).toEqual(["local:claude-code:on", "local:codex:on", "local:gemini-cli:off", "local:grok:off", "opencode:not-installed", "pi:not-installed", "cursor:not-installed"]);
    // One machine: no switch at the top, and no 'Skill and session hook' line on any row (B11, B12).
    await expect(page.locator("[data-agents-machines]")).toHaveCount(0);
    await expect(list).not.toContainText("Skill and session hook");

    // B16: an agent that is on and has no session is Ready; one that is off has no status.
    await expect(list.locator('[data-agent-status="local:codex:ready"]')).toHaveText(/Ready/);
    await expect(list.locator('[data-agent-row="local:gemini-cli:off"] [data-agent-status]')).toHaveCount(0);

    // B17: the fixture's two Claude Code panes run without Hide; the row counts them and opens their list.
    const notConnected = list.locator('[data-agent-status="local:claude-code:not-connected"]');
    await expect(notConnected).toContainText("2 not connected");
    await expect(notConnected).toHaveAttribute("aria-expanded", "false");
    await notConnected.click();
    await expect(notConnected).toHaveAttribute("aria-expanded", "true");
    await expect(list.locator("[data-agent-go-to-pane]")).toHaveCount(2);
    await screenshot(page, "agents-not-connected");

    // B8: Not installed is closed until opened and each row's Install opens the vendor's guide.
    const fold = list.locator("[data-agents-not-installed]");
    await expect(fold).not.toHaveAttribute("open", "");
    await fold.locator("summary").click();
    await expect(fold.locator("[data-agent-install]")).toHaveCount(3);
    await expect(fold.locator('[data-agent-install="cursor"]')).toHaveAttribute("href", "https://cursor.com/docs/cli/installation");
    await screenshot(page, "agents-list");

    // B10: Check again reads the machine again and the list stays as it was; the in-progress
    // and failed states come from the kit snapshot's `checking` and `check_failed` (unit-tested).
    await list.locator("[data-agents-check]").click();
    await expect(list.locator("[data-agents-check]")).toHaveAttribute("data-agents-check", "idle");
    await expect(list.locator("[data-agent-row]")).toHaveCount(7);
  } finally {
    daemon.stop();
    herdr.stop();
  }
});

test("the Partial chip opens by keyboard with a mark and a word per feature, and Escape returns to the chip", async ({ page }) => {
  const { herdr, daemon } = await start("agents-partial");
  try {
    const list = await openAgents(page, daemon);
    const chip = list.locator('[data-agent-partial="gemini-cli"]');
    await expect(chip).toBeVisible({ timeout: 60_000 });
    // B18: Partial is on every agent Hide does only some things for, on or off; a Full agent has none.
    await expect(list.locator('[data-agent-partial="grok"]')).toBeVisible();
    await expect(list.locator('[data-agent-partial="claude-code"]')).toHaveCount(0);
    await chip.focus();
    await page.keyboard.press("Enter");
    const popover = page.locator('[data-agent-partial-popover="gemini-cli"]');
    await expect(popover).toBeVisible();
    await expect(popover.locator("[data-agent-feature]").first()).toBeVisible();
    // A mark and a word for each feature; the table is the kit's, so a '–' is something Hide does not do.
    await expect(popover.locator("[data-agent-feature$=':no']").first()).toContainText("–");
    await expect(popover.locator("[data-agent-feature$=':yes']").first()).toContainText("✓");
    // B15: Gemini CLI has no Herdr integration, and the popover says its status is judged from the screen.
    await expect(popover).toContainText("Hide judges its status from the screen");
    await screenshot(page, "agents-partial-popover");
    await page.keyboard.press("Escape");
    await expect(popover).toHaveCount(0);
    await expect(chip).toBeFocused();
  } finally {
    daemon.stop();
    herdr.stop();
  }
});

test("a switch installs and takes out Hide's own entries, and a hook the operator removed shows on its row until Reinstall", async ({ page }) => {
  const { herdr, daemon, home } = await start("agents-switch");
  try {
    const list = await openAgents(page, daemon);
    const gemini = path.join(home, ".gemini", "settings.json");
    const claude = path.join(home, ".claude", "settings.json");
    await expect(list.locator('[data-agent-switch="local:gemini-cli:off"]')).toBeVisible({ timeout: 60_000 });
    // B13: turning an agent on writes its hook on this machine.
    await list.locator('[data-agent-switch="local:gemini-cli:off"]').click();
    await expect(list.locator('[data-agent-switch="local:gemini-cli:on"]')).toBeVisible();
    await expect.poll(() => read(gemini), { timeout: 60_000 }).toContain("hide-guidance@");
    // B14: turning it off takes out what Hide wrote.
    await list.locator('[data-agent-switch="local:gemini-cli:on"]').click();
    await expect(list.locator('[data-agent-switch="local:gemini-cli:off"]')).toBeVisible();
    await expect.poll(() => read(gemini), { timeout: 60_000 }).not.toContain("hide-guidance@");

    // B20: a hook the operator took out is not put back; its row says so with Reinstall, on that row only.
    await expect.poll(() => read(claude), { timeout: 60_000 }).toContain("hide-subagents@");
    fs.writeFileSync(claude, "{}\n");
    await list.locator("[data-agents-check]").click();
    const problem = list.locator('[data-agent-problem="local:claude-code"]');
    await expect(problem).toContainText("Hook: Removed", { timeout: 60_000 });
    await expect(list.locator("[data-agent-problem]")).toHaveCount(1);
    await screenshot(page, "agents-problem");
    expect(read(claude)).not.toContain("hide-subagents@");
    await problem.locator("[data-hook-reinstall]").click();
    await expect.poll(() => read(claude), { timeout: 60_000 }).toContain("hide-subagents@");
    await expect(list.locator("[data-agent-problem]")).toHaveCount(0);
  } finally {
    daemon.stop();
    herdr.stop();
  }
});

test("reads without sideways scrolling in Korean and English, light and dark, on a narrow window", async ({ page }) => {
  // B65: the sheet shrinks with its window; the rows' status, the Partial chip and the Not installed list wrap, not clip.
  const { herdr, daemon } = await start("agents-narrow");
  try {
    const list = await openAgents(page, daemon);
    await expect(list.locator("[data-agent-row]")).toHaveCount(7, { timeout: 60_000 });
    await page.setViewportSize({ width: 560, height: 1000 });
    await list.locator("[data-agents-not-installed] summary").click();
    const panel = page.locator('[data-settings="true"] [role="tabpanel"]');
    const overflow = () => panel.evaluate((node) => node.scrollWidth - node.clientWidth);
    for (const [language, theme] of [
      ["en", "light"],
      ["en", "dark"],
      ["ko", "light"],
      ["ko", "dark"],
    ] as const) {
      await page.locator('[data-settings-tab="general"]').click();
      await page.locator("[data-interface-language]").click();
      await page.locator(`[data-language-option="${language}"]`).click();
      await expect(page.locator("html")).toHaveAttribute("lang", language);
      await page.locator(`[data-theme-option="${theme}"]`).click();
      await expect(page.locator("[data-theme-choice]")).toHaveAttribute("data-theme-choice", theme);
      await page.locator('[data-settings-tab="agents"]').click();
      await expect(list).toBeVisible();
      // The tab strip fades the left tab out; the picture waits for the fade so it shows the resting sheet.
      await expect(page.locator('[data-settings-tab="general"]')).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
      await expect.poll(overflow).toBeLessThanOrEqual(0);
      await screenshot(page, `agents-${language}-${theme}-narrow`);
    }
  } finally {
    daemon.stop();
    herdr.stop();
  }
});
