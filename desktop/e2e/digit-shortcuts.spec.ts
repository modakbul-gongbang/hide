// The desktop app's numbered chords and hold hints end to end (PRD
// electron-digit-shortcuts-hints D-09): ⌘2 brings the second tab forward as
// one focus_tab and a number with no tab does nothing; holding ⌘ alone shows
// each tab's digit at its top right after the delay and moves nothing,
// releasing hides it, a release before the delay shows nothing, a key
// pressed during the hold ends it, losing the window ends it, and a menu or
// a sheet opening during it ends it; holding
// ⌥ numbers the Agents rows without moving their time or fold slot, and ⌥2
// opens the second row. A private Herdr server, hided and Electron app.

import { expect, test, type ElectronApplication, type Locator, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import "../../web/src/host";
import { countSent, enterWorkspace } from "../../web/e2e/wire";
import { hostLog, isolate, launch, type Isolated } from "./fixture";

let herdr: HerdrFixture;
let run: Isolated;
let app: ElectronApplication | null = null;

test.beforeAll(async () => {
  herdr = await startHerdr();
});

test.afterAll(() => {
  herdr?.stop();
});

test.beforeEach(() => {
  run = isolate(herdr, "digits");
});

test.afterEach(async () => {
  const info = test.info();
  if (info.status !== info.expectedStatus) console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
});

/** After the one expected event, a quiet moment in which no second one arrives. */
async function exactlyOnce(sent: Map<string, number>, kind: string, expected: number, page: Page): Promise<void> {
  await expect.poll(() => sent.get(kind) ?? 0).toBe(expected);
  await page.waitForTimeout(600);
  expect(sent.get(kind) ?? 0).toBe(expected);
}

/** The boxes of what must not move while a keycap shows, rounded to the device pixel. */
async function boxes(locators: Locator[]): Promise<(string | null)[]> {
  return Promise.all(
    locators.map(async (locator) => {
      const box = await locator.boundingBox();
      return box ? [box.x, box.y, box.width, box.height].map(Math.round).join(",") : null;
    }),
  );
}

/** A quiet moment in which no keycap appears. */
async function noKeycap(page: Page): Promise<void> {
  await page.waitForTimeout(400);
  await expect(page.locator("[data-keycap]")).toHaveCount(0);
}

/**
 * A capture of the app's one window when a run directory is named: the
 * native window image by id where the runner may read the screen, else the
 * renderer's own image, with a sidecar saying which one was taken.
 */
async function capture(page: Page, name: string, source: string): Promise<void> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!dir) return;
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${name}.png`);
  let method = "screencapture -l <window id>";
  try {
    execFileSync("/usr/sbin/screencapture", ["-x", "-o", "-l", source.split(":")[1]!, file], { stdio: "pipe" });
  } catch (error) {
    method = `page.screenshot (screencapture refused: ${error instanceof Error ? error.message.split("\n").at(-1) : String(error)})`;
    await page.screenshot({ path: file });
  }
  fs.writeFileSync(path.join(dir, `${name}.json`), JSON.stringify({ source, method, build: "worktree desktop/dist", isolated: true }));
}

test("⌘n selects a tab, ⌥n an agent, and holding ⌘ or ⌥ shows the numbers without moving anything", async () => {
  const tabs = [herdr.tab];
  const made = herdr.run([
    "tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", "second", "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
  ]) as { result: { tab: { tab_id: string } } };
  tabs.push(made.result.tab.tab_id);
  try {
    ({ app } = await launch(run.env, { switches: ["--disable-backgrounding-occluded-windows"] }));
    const page = await app.firstWindow();
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true });
    const sent = countSent(page);
    await enterWorkspace(page, "fixture");
    const canvas = page.locator("[data-canvas]").first();
    const tab = (id: string) => page.locator(`[data-tab="${id}"]`);
    await expect(tab(tabs[1]!)).toBeVisible();
    await expect(canvas).toHaveAttribute("data-canvas", tabs[0]!);
    await page.locator("[data-pane-view] .xterm-helper-textarea").first().focus();

    // B1: ⌘2 is one focus_tab; ⌘3 has no tab and sends nothing; ⌘1 comes back.
    let focused = sent.get("focus_tab") ?? 0;
    await page.keyboard.press("Meta+Digit2");
    await expect(canvas).toHaveAttribute("data-canvas", tabs[1]!);
    await exactlyOnce(sent, "focus_tab", focused + 1, page);
    await page.keyboard.press("Meta+Digit3");
    await page.waitForTimeout(400);
    expect(sent.get("focus_tab") ?? 0).toBe(focused + 1);
    await page.keyboard.press("Meta+Digit1");
    await expect(canvas).toHaveAttribute("data-canvas", tabs[0]!);
    await exactlyOnce(sent, "focus_tab", focused + 2, page);

    // B8: a hold released before the delay shows nothing.
    await page.keyboard.down("Meta");
    await page.keyboard.up("Meta");
    await noKeycap(page);

    // B5, B7, B9: holding ⌘ alone floats each tab's digit; the tab's title,
    // its Rename field and the tab bar keep their boxes.
    await tab(tabs[1]!).click({ button: "right" });
    await page.getByRole("menuitem", { name: "Rename…", exact: true }).click();
    const rename = page.getByRole("textbox", { name: "Tab name", exact: true });
    await expect(rename).toBeFocused();
    const still = [tab(tabs[0]!), tab(tabs[1]!), rename, page.locator("[data-new-agent-tab]")];
    const before = await boxes(still);
    await page.keyboard.down("Meta");
    const tabCaps = page.locator("[data-agent-tab-bar] [data-keycap]");
    await expect(tabCaps).toHaveCount(2);
    await expect(tab(tabs[0]!).locator("[data-keycap]")).toHaveText("1");
    await expect(tab(tabs[1]!).locator("[data-keycap]")).toHaveText("2");
    expect(await boxes(still)).toEqual(before);
    const cap = await tab(tabs[0]!).locator("[data-keycap]").boundingBox();
    const owner = await tab(tabs[0]!).boundingBox();
    expect(cap!.x + cap!.width).toBeLessThanOrEqual(owner!.x + owner!.width + 1);
    expect(cap!.y).toBeGreaterThanOrEqual(owner!.y - 1);
    const source = await app.evaluate(({ BrowserWindow }) => {
      const windows = BrowserWindow.getAllWindows();
      if (windows.length !== 1) throw new Error("Expected exactly one candidate window");
      return windows[0]!.getMediaSourceId();
    });
    await capture(page, "digit-hints-tabs-dark", source);
    // B6: releasing ⌘ hides the numbers at once.
    await page.keyboard.up("Meta");
    await expect(page.locator("[data-keycap]")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(rename).toHaveCount(0);

    // B6: a key during the hold is the chord itself; the numbers go with it
    // and stay gone while ⌘ is still down.
    focused = sent.get("focus_tab") ?? 0;
    await page.keyboard.down("Meta");
    await expect(tabCaps).toHaveCount(2);
    await page.keyboard.press("Digit2");
    await expect(page.locator("[data-keycap]")).toHaveCount(0);
    await expect(canvas).toHaveAttribute("data-canvas", tabs[1]!);
    await exactlyOnce(sent, "focus_tab", focused + 1, page);
    await noKeycap(page);
    await page.keyboard.up("Meta");

    // B6: losing the window ends the hold.
    await page.keyboard.down("Meta");
    await expect(tabCaps).toHaveCount(2);
    await page.evaluate(() => window.dispatchEvent(new Event("blur")));
    await expect(page.locator("[data-keycap]")).toHaveCount(0);
    await page.keyboard.up("Meta");

    // B6: a layer opening during the hold ends it: a tab's context menu,
    // then the Settings sheet, each with ⌘ still down.
    await page.keyboard.down("Meta");
    await expect(tabCaps).toHaveCount(2);
    await tab(tabs[0]!).click({ button: "right" });
    await expect(page.getByRole("menuitem", { name: "Rename…", exact: true })).toBeVisible();
    await expect(page.locator("[data-keycap]")).toHaveCount(0);
    await page.keyboard.up("Meta");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("menuitem", { name: "Rename…", exact: true })).toHaveCount(0);
    await page.keyboard.down("Meta");
    await expect(tabCaps).toHaveCount(2);
    await page.locator("[data-open-settings]").click();
    await expect(page.locator('[data-settings="true"]')).toBeVisible();
    await expect(page.locator("[data-keycap]")).toHaveCount(0);
    await page.keyboard.up("Meta");
    await page.keyboard.press("Escape");
    await expect(page.locator('[data-settings="true"]')).toHaveCount(0);

    // B5, B2: holding ⌥ alone numbers the Agents rows top to bottom; the
    // time and the fold slot stay put; ⌥2 opens the second row.
    await page.locator('[data-sidebar-mode="agents"]').click();
    const rows = page.locator("[data-agent-list] [data-pane]");
    await expect(rows).toHaveCount(2);
    const firstRow = rows.first();
    const kept = [firstRow, firstRow.locator("[data-agent-title]"), firstRow.locator("[data-fold-slot]"), rows.nth(1)];
    const rowsBefore = await boxes(kept);
    await page.keyboard.down("Alt");
    const rowCaps = page.locator("[data-agent-list] [data-keycap]");
    await expect(rowCaps).toHaveCount(2);
    await expect(rowCaps).toHaveText(["1", "2"]);
    expect(await boxes(kept)).toEqual(rowsBefore);
    await capture(page, "digit-hints-agents-dark", source);
    await page.keyboard.up("Alt");
    await expect(page.locator("[data-keycap]")).toHaveCount(0);
    const secondPane = await rows.nth(1).getAttribute("data-pane");
    const opened = sent.get("focus_pane") ?? 0;
    await page.keyboard.press("Alt+Digit2");
    await exactlyOnce(sent, "focus_pane", opened + 1, page);
    await expect(page.locator(`[data-agent-list] [data-pane="${secondPane}"] [aria-current="true"]`)).toHaveCount(1);
    await page.keyboard.press("Alt+Digit3");
    await page.waitForTimeout(400);
    expect(sent.get("focus_pane") ?? 0).toBe(opened + 1);

    // B7: the keycap draws from the Light tokens too.
    if (process.env.HIDE_E2E_SCREENSHOT_DIR) {
      await page.locator("[data-open-settings]").click();
      await page.locator('[data-settings-tab="appearance"]').click();
      await page.locator('[data-theme-option="light"]').click();
      await expect(page.locator("html")).toHaveClass(/\blight\b/);
      await page.keyboard.press("Escape");
      await expect(page.locator('[data-settings="true"]')).toHaveCount(0);
      await page.waitForTimeout(400);
      await page.keyboard.down("Meta");
      await expect(tabCaps).toHaveCount(2);
      await capture(page, "digit-hints-tabs-light", source);
      await page.keyboard.up("Meta");
    }
  } finally {
    herdr.run(["tab", "close", tabs[1]!]);
  }
});
