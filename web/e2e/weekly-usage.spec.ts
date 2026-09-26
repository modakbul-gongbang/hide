// The weekly usage chips and popover on an isolated pinned Herdr and hided
// (#153). The daemon's PATH carries a `claude` that answers `/usage` with a
// fixed reading once the test releases it, so its row is first loading and
// then available with a Fable bucket; this HOME has no Codex login, so that
// row is unavailable. Along the way the page tells the core when the popover
// opens and closes and when the page is hidden.

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { screenshot } from "./wire";

test.describe.configure({ timeout: 120_000 });

type Hints = { usage_window_visible?: boolean; usage_popover_open?: boolean };

/** Every usage hint the page sent, in order, read off its `ui_state_update` events. */
function sentHints(page: Page): Hints[] {
  const hints: Hints[] = [];
  page.on("websocket", (ws) =>
    ws.on("framesent", (frame) => {
      try {
        const event = JSON.parse(String(frame.payload)) as { kind?: string; payload?: Hints };
        if (event.kind !== "ui_state_update" || !event.payload) return;
        const { usage_window_visible, usage_popover_open } = event.payload;
        if (usage_window_visible !== undefined || usage_popover_open !== undefined) hints.push({ usage_window_visible, usage_popover_open });
      } catch {
        /* the handshake is not an event */
      }
    }),
  );
  return hints;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** A reset the way the CLI prints it: `Oct 1 at 3pm (UTC)`, with the year only when it differs. */
function printedReset(at: Date, now: Date): string {
  const year = at.getUTCFullYear() === now.getUTCFullYear() ? "" : `, ${at.getUTCFullYear()}`;
  const hour = at.getUTCHours();
  return `${MONTHS[at.getUTCMonth()]} ${at.getUTCDate()}${year} at ${hour % 12 === 0 ? 12 : hour % 12}${hour < 12 ? "am" : "pm"} (UTC)`;
}

/**
 * A `claude` on its own PATH directory that waits for `release()` and then
 * prints a `/usage` result frame: 62% of the week with a 6% Fable bucket,
 * both resetting on the whole hour five days and five hours from now. The
 * usage child only receives HOME, PATH, USER, LOGNAME and TMPDIR, so the
 * script carries its paths itself.
 */
function usageShim() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hide-e2e-usage-"));
  const bin = path.join(root, "bin");
  fs.mkdirSync(bin);
  const gate = path.join(root, "released");
  const frame = path.join(root, "frame.json");
  const now = new Date();
  const hour = Math.floor(now.getTime() / 3_600_000) * 3_600_000;
  const reset = printedReset(new Date(hour + (5 * 24 + 5) * 3_600_000), now);
  const result = [
    "You are currently using your subscription to power your Claude Code usage",
    "",
    "Current session: 4% used",
    `Current week (all models): 62% used · resets ${reset}`,
    `Current week (Fable): 6% used · resets ${reset}`,
  ].join("\n");
  fs.writeFileSync(frame, JSON.stringify({ type: "result", subtype: "success", is_error: false, result }));
  fs.writeFileSync(path.join(bin, "claude"), `#!/bin/sh\nwhile [ ! -e '${gate}' ]; do /bin/sleep 0.1; done\n/bin/cat '${frame}'\n`, { mode: 0o755 });
  return {
    path: `${bin}:/usr/bin:/bin`,
    release: () => fs.writeFileSync(gate, ""),
    cleanup: () => fs.rmSync(root, { recursive: true, force: true }),
  };
}

test("weekly usage: a loading, an available and an unavailable row, and the hints the page sends", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  const usage = usageShim();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "usage", undefined, { PATH: usage.path, CODEX_HOME: undefined });
    const hints = sentHints(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);

    // Claude's read is held open, so its row is still loading; Codex has no login here.
    const trigger = page.locator("[data-usage-trigger]");
    const claudeChip = page.locator('[data-usage-chip="claude"]');
    const codexChip = page.locator('[data-usage-chip="codex"]');
    await expect(codexChip).toHaveAttribute("data-usage-state", "unavailable", { timeout: 20_000 });
    await expect(claudeChip).toHaveAttribute("data-usage-state", "loading");
    await expect(claudeChip).toHaveText("");
    await expect(codexChip).toHaveText("");
    await expect(trigger).toHaveAccessibleName("Weekly usage, Claude Code loading, Codex unavailable");
    await expect.poll(() => hints.some((hint) => hint.usage_window_visible === true)).toBe(true);

    await trigger.click();
    const popover = page.locator("[data-usage-popover]");
    await expect(popover.getByText("Weekly Usage", { exact: true })).toBeVisible();
    await expect(popover.getByText("7 days", { exact: true })).toBeVisible();
    const claudeRow = popover.locator('[data-usage-row="claude"]:not([data-usage-bucket])');
    const codexRow = popover.locator('[data-usage-row="codex"]');
    await expect(claudeRow.locator("[data-usage-value]")).toHaveText("…");
    await expect(claudeRow).toContainText("Checking usage…");
    await expect(codexRow.locator("[data-usage-value]")).toHaveText("Unavailable");
    await expect(codexRow).toContainText("Sign in with codex to see usage");
    await expect.poll(() => hints.at(-1)).toEqual({ usage_popover_open: true });
    await screenshot(page, "usage-loading-unavailable");

    // Claude answers: the open popover fills in the row, its reset, and the Fable bucket under it.
    usage.release();
    await expect(claudeRow.locator("[data-usage-value]")).toHaveText("62%", { timeout: 20_000 });
    await expect(claudeRow).toHaveAttribute("data-usage-state", "available");
    await expect(claudeRow.locator("[data-usage-reset]")).toHaveText(/^· in 5d [45]h$/);
    const fable = popover.locator('[data-usage-row="claude"][data-usage-bucket="Fable"]');
    await expect(fable.locator("[data-usage-value]")).toHaveText("6%");
    await expect(fable.locator("[data-usage-reset]")).toHaveText(/^· in 5d [45]h$/);
    await screenshot(page, "usage-popover-available");

    await page.keyboard.press("Escape");
    await expect(popover).toHaveCount(0);
    await expect.poll(() => hints.at(-1)).toEqual({ usage_popover_open: false });
    await expect(claudeChip).toHaveText("62%");
    await expect(trigger).toHaveAccessibleName("Weekly usage, Claude Code 62%, Codex unavailable");
    await screenshot(page, "usage-chips");

    // A hidden page tells the core it is no longer looking.
    await page.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect.poll(() => hints.at(-1)).toEqual({ usage_window_visible: false });
  } finally {
    daemon?.stop();
    herdr.stop();
    usage.cleanup();
  }
});
