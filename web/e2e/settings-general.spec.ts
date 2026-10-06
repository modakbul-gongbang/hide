// Settings, the six tabs and General on an isolated pinned Herdr and hided
// (PRD settings-cleanup B1-B6, B23, B24): the tab list, General's Appearance,
// Connections and About with its closed Details, the real Herdr version and
// protocol of a healthy connection, and the settings that moved into Agents
// and Hide AI.

import { expect, test } from "@playwright/test";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { enterWorkspace, screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

test("Settings has six tabs, and General shows the real Herdr values behind a closed Details", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "settings-general");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await page.locator("[data-open-settings]").click();
    const sheet = page.locator('[data-settings="true"]');
    await expect(sheet).toBeVisible();

    // B1: six tabs in order, none of the old Appearance, Issues or Performance.
    await expect(sheet.locator("[data-settings-tab]")).toHaveText(["General", "Agents", "Hide AI", "Devices", "Mobile", "Shortcuts"]);

    // B2: language, theme, accent and interface text in one group, with no hex value beside the accent.
    const appearance = sheet.locator('[data-settings-group="appearance"]');
    await expect(appearance.locator("[data-interface-language]")).toBeVisible();
    await expect(appearance.locator("[data-theme-choice]")).toBeVisible();
    await expect(appearance.locator("[data-accent]")).toHaveCount(4);
    await expect(appearance.locator("[data-font-size]")).toBeVisible();
    await expect(appearance).not.toContainText(/#[0-9a-f]{6}/i);

    // B3: the GitHub connection is here, and there is no Local row.
    const connections = sheet.locator('[data-settings-group="connections"]');
    await expect(connections).toContainText("GitHub");
    await expect(connections).not.toContainText("Always available");

    // B6: no ownership sentence and no Authentication paragraph, and no device chip on this Mac.
    await expect(sheet).not.toContainText("kept by hided");
    await expect(sheet).not.toContainText("Authentication");
    await expect(sheet.locator("[data-settings-device]")).toHaveCount(0);

    // B4: Details is closed until opened, and then holds the runtime facts.
    const details = sheet.locator("[data-settings-details]");
    await expect(details).not.toHaveAttribute("open", "");
    await expect(details.getByText("Protocol")).toBeHidden();
    await details.locator("summary").click();
    await expect(details).toHaveAttribute("open", "");
    for (const label of ["Process", "State file", "Socket", "Binary", "Recent"]) await expect(details.getByText(label, { exact: true })).toBeVisible();

    // B5: a healthy Herdr shows its real version and protocol, never "unavailable".
    await expect(details.locator("[data-herdr-version]")).toHaveText(/^\d+\.\d+\.\d+/);
    await expect(details.locator("[data-herdr-protocol]")).toHaveAttribute("data-herdr-protocol", "matches");
    await expect(details.locator("[data-herdr-protocol]")).toHaveText(/^\d+$/);
    await expect(sheet.locator('[data-settings-group="about"] [data-herdr-state="connected"]')).toContainText(/Connected · \d+\.\d+\.\d+/);
    await screenshot(page, "settings-general-dark");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("Sleep after, the issue link and the worktree names live in Agents and Hide AI with their stored values", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "settings-moved");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await page.locator("[data-open-settings]").click();

    // B23: the idle choice and its sentence, the same value the Performance tab held.
    await page.locator('[data-settings-tab="agents"]').click();
    const idle = page.locator('[data-settings-group="idle-agents"]');
    await expect(idle.locator("[data-agent-sleep-after]")).toHaveAttribute("data-agent-sleep-after", "never");
    await expect(idle).toContainText("0 sleeping now. Working, unread and on-screen agents never sleep.");
    await idle.locator("[data-agent-sleep-after]").click();
    await page.locator('[data-agent-sleep-option="12"]').click();
    await expect(idle.locator("[data-agent-sleep-after]")).toHaveAttribute("data-agent-sleep-after", "12");

    // B24: the pull request link switch, on by default as the Issues tab had it.
    const starting = page.locator('[data-settings-group="issue-start"]');
    const link = starting.locator("[data-issue-closes-instruction]");
    await expect(link).toHaveAttribute("data-issue-closes-instruction", "true");
    await expect(starting).toContainText("Link the issue in pull requests");
    await link.click();
    await expect(link).toHaveAttribute("data-issue-closes-instruction", "false");

    // D-24: Let AI name worktrees is a Hide AI feature now.
    await page.locator('[data-settings-tab="hideAi"]').click();
    const features = page.locator('[data-settings-group="hide-ai-features"]');
    const names = features.locator("[data-issue-ai-worktree-name]");
    await expect(names).toHaveAttribute("data-issue-ai-worktree-name", "true");
    await names.click();
    await expect(names).toHaveAttribute("data-issue-ai-worktree-name", "false");
    await expect(features.locator("[data-ai-agent-summary]")).toBeVisible();
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("General reads without sideways scrolling in Korean and English, light and dark, on a narrow window", async ({ page }) => {
  // B65: the sheet shrinks with its window; every row has to wrap, not clip.
  await page.setViewportSize({ width: 560, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "settings-narrow");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    await page.locator("[data-open-settings]").click();
    const sheet = page.locator('[data-settings="true"]');
    await expect(sheet).toBeVisible();
    const panel = sheet.locator('[role="tabpanel"]');
    const overflow = () => panel.evaluate((node) => node.scrollWidth - node.clientWidth);
    const details = sheet.locator("[data-settings-details]");

    for (const [language, theme] of [
      ["en", "light"],
      ["ko", "light"],
      ["ko", "dark"],
    ] as const) {
      await sheet.locator("[data-interface-language]").click();
      await page.locator(`[data-language-option="${language}"]`).click();
      await expect(page.locator("html")).toHaveAttribute("lang", language);
      await sheet.locator(`[data-theme-option="${theme}"]`).click();
      await expect(sheet.locator("[data-theme-choice]")).toHaveAttribute("data-theme-choice", theme);
      await expect.poll(overflow).toBeLessThanOrEqual(0);
      await screenshot(page, `settings-general-${language}-${theme}-narrow`);
      if (!(await details.evaluate((node) => (node as HTMLDetailsElement).open))) await details.locator("summary").click();
      await expect.poll(overflow).toBeLessThanOrEqual(0);
      await screenshot(page, `settings-general-details-${language}-${theme}-narrow`);
    }
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
