// Korean typed through the browser's input-method path, on an isolated pinned
// Herdr and hided: a pane's program reads the syllables before a ⇧↩ pressed
// right after them, and an Escape the input method answers while a dialog's
// field is composing leaves that dialog open.

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import { startHerdr } from "./herdr-fixture";
import { startHided, type Daemon } from "./hided-fixture";
import { composeKorean } from "./ime";
import { enterWorkspace } from "./wire";
import { chord } from "./chords";

test.describe.configure({ timeout: 120_000 });

/** What the pane's program has read since the log held `from` bytes, as text. */
function heard(log: string, from: number): string {
  return fs.readFileSync(log).subarray(from).toString("utf8");
}

/** Two animation frames, so an effect of the last event has run before the test asserts it did not happen. */
async function twoFrames(page: Page): Promise<void> {
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
}

// @platform: Hangul bytes reach the agent's PTY through the platform's Herdr.
test("⇧↩ right after a Hangul syllable reaches the program after that syllable", { tag: "@platform" }, async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    const [agent] = herdr.panes;
    daemon = await startHided(herdr, "korean-terminal");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    // Herdr focused this pane when the fixture made it; a click would send
    // the agent a mouse report, so the pane is not clicked.
    const view = page.locator(`[data-pane-view="${agent}"]`);
    await expect(view).toHaveAttribute("data-focused", "true", { timeout: 20_000 });
    const textarea = view.locator(".xterm-helper-textarea");
    await textarea.focus();
    const before = fs.readFileSync(herdr.inputLogs[0]).length;

    await composeKorean(page, [["ㅇ", "아", "안"]]);
    // JSON keeps a control byte readable in a failure.
    await expect.poll(() => JSON.stringify(heard(herdr.inputLogs[0], before)), { timeout: 10_000 }).toBe(JSON.stringify("안"));

    // macOS Chromium ends the last syllable with `compositionend` and then
    // delivers the ⇧↩ that ended it as a second keydown, before xterm sends
    // the syllable a task later. The page dispatches that order itself in
    // one task, so the test decides it rather than the scheduler.
    await textarea.evaluate((node: HTMLTextAreaElement) => {
      node.dispatchEvent(new CompositionEvent("compositionstart", { data: "" }));
      node.value += "녕";
      node.dispatchEvent(new CompositionEvent("compositionupdate", { data: "녕" }));
      node.dispatchEvent(new CompositionEvent("compositionend", { data: "녕" }));
      node.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", shiftKey: true, bubbles: true, cancelable: true }));
    });
    // ⇧↩ is ESC CR, the newline Claude Code and Codex read without sending;
    // a Unix tty hands the program the CR as LF, the Windows console as CR.
    await expect.poll(() => JSON.stringify(heard(herdr.inputLogs[0], before)), { timeout: 10_000 }).toMatch(/^"안녕\\u001b\\[rn]"$/);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("Escape while Hangul is composing in the search field keeps ⌘K open, and closes it once the syllable is in", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr, "korean-escape");
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await enterWorkspace(page, "fixture");
    const palette = page.locator('[data-palette="Search"]');
    const input = palette.locator("[data-palette-input]");
    await page.keyboard.press(chord("search"));
    await expect(input).toBeFocused();

    // An Escape pressed while the input method holds "하" is the input
    // method's, which Blink marks `isComposing`.
    const cdp = await page.context().newCDPSession(page);
    try {
      await cdp.send("Input.imeSetComposition", { text: "하", selectionStart: 1, selectionEnd: 1 });
      await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
      await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
      await twoFrames(page);
      await expect(palette).toHaveCount(1);
      await cdp.send("Input.insertText", { text: "하" });
    } finally {
      await cdp.detach();
    }
    await expect(input).toHaveValue("하");

    // With nothing composing, Escape is the operator's and closes it.
    await page.keyboard.press("Escape");
    await expect(palette).toHaveCount(0);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
