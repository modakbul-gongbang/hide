// The desktop app's keys end to end: the operator's macOS pane chords, which
// hided brings across from the Swift app's state once, and the held-modifier
// cycles, whose commit waits for the modifier's release. A private Herdr
// server for this file, a private hided and Electron app per test.

import { expect, test, type ElectronApplication, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "../../web/e2e/herdr-fixture";
import "../../web/src/host";
import { countSent, enterWorkspace } from "../../web/e2e/wire";
import { hostLog, isolate, launch, screenshot, type Isolated } from "./fixture";

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
  run = isolate(herdr, test.info().title.split(":")[0]!);
});

test.afterEach(async () => {
  const info = test.info();
  if (info.status !== info.expectedStatus) console.log(hostLog(run.env).map((line) => JSON.stringify(line)).join("\n"));
  await app?.close().catch(() => undefined);
  app = null;
  run.cleanup();
});

/** A menu item's accelerator as the app menu holds it now. */
function accelerator(id: string): Promise<string | null> {
  return app!.evaluate(({ Menu }, itemId) => Menu.getApplicationMenu()?.getMenuItemById(itemId)?.accelerator ?? null, id);
}

/** After the one expected event, a quiet moment in which no second one arrives. */
async function exactlyOnce(sent: Map<string, number>, kind: string, expected: number, page: Page): Promise<void> {
  await expect.poll(() => sent.get(kind) ?? 0).toBe(expected);
  await page.waitForTimeout(600);
  expect(sent.get(kind) ?? 0).toBe(expected);
}

test("pane chords: the macOS app's set comes across, runs once, and Settings edits it", async () => {
  // The operator's Swift state on the day of the decision, with the fields
  // the Swift shell keeps around the one hided reads.
  const swiftState = path.join(run.env.HOME!, "Library", "Application Support", "hide", "state.json");
  fs.mkdirSync(path.dirname(swiftState), { recursive: true });
  fs.writeFileSync(
    swiftState,
    JSON.stringify({
      schema_version: 1,
      expanded_paths: [],
      selected_path: null,
      selected_pane_id: null,
      pet_visible: false,
      shortcut_bindings: {
        close_pane: "command+shift+w",
        split_down: "command+shift+d",
        split_right: "command+d",
        toggle_zoom: "command+shift+return",
        increase_text_size: "command+=",
        decrease_text_size: "command+-",
        reset_text_size: "command+0",
      },
    }),
  );
  ({ app } = await launch(run.env));
  const page = await app.firstWindow();
  const lastSent = new Map<string, Record<string, unknown>>();
  const sent = countSent(page, lastSent);
  await enterWorkspace(page);
  await expect(page.locator("[data-pane-view]")).toHaveCount(2);

  // The menu shows the imported chord once the shell reports the set.
  await expect.poll(() => accelerator("toggle_zoom")).toBe("Shift+Command+Return");
  expect(await accelerator("split_right")).toBe("Command+D");

  // ⇧⌘↩ zooms exactly once; the default ⌥⌘↩ is no longer a chord here.
  await page.locator("[data-pane-view] .xterm-helper-textarea").first().focus();
  await page.keyboard.press("Meta+Alt+Enter");
  await page.keyboard.press("Meta+Shift+Enter");
  await expect(page.locator("[data-canvas]").first()).toHaveAttribute("data-zoomed", "true");
  await exactlyOnce(sent, "toggle_zoom", 1, page);

  // The ⌘/ sheet lists it.
  await page.keyboard.press("Meta+Slash");
  const sheet = page.locator("[data-shortcut-sheet]");
  await expect(sheet.locator('[data-shortcut="toggle_zoom"] kbd')).toHaveText("⇧⌘↩");
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);

  // Settings > Shortcuts edits the same set, in the Swift app's form.
  await page.locator("[data-open-settings]").click();
  await page.locator('[data-settings-tab="shortcuts"]').click();
  await expect(page.locator('[data-shortcut-effective="toggle_zoom"]')).toHaveText("⇧⌘↩");
  await page.locator('[data-shortcut-record="split_right"]').click();
  await page.keyboard.press("Meta+Alt+KeyR");
  await expect(page.locator('[data-shortcut-draft="split_right"]')).toHaveText("⌥⌘R");
  await page.locator('[data-shortcut-apply="split_right"]').click();
  await expect(page.locator('[data-shortcut-effective="split_right"]')).toHaveText("⌥⌘R");
  const saved = lastSent.get("ui_state_update")?.shortcut_bindings as Record<string, string> | undefined;
  expect(saved).toMatchObject({ split_right: "command+option+r", toggle_zoom: "command+shift+return" });
  expect(lastSent.get("ui_state_update")?.browser_shortcut_bindings ?? {}).toEqual({});
  await expect.poll(() => accelerator("split_right")).toBe("Alt+Command+R");
  await screenshot(page, "desktop-shortcut-settings");
});

test("cycles: ⌃Tab and ⌥Tab commit once, on releasing the held modifier", async () => {
  // Two more tabs in the fixture Workspace and two more Projects.
  const tabs = [herdr.tab];
  for (const label of ["second", "third"]) {
    const made = herdr.run([
      "tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", label, "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]) as { result: { tab: { tab_id: string } } };
    tabs.push(made.result.tab.tab_id);
  }
  const extraWorkspaces: string[] = [];
  for (const name of ["beta", "gamma"]) {
    fs.mkdirSync(path.join(herdr.root, name), { recursive: true });
    const made = herdr.run([
      "workspace", "create", "--cwd", path.join(herdr.root, name), "--label", name, "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]) as { result: { workspace: { workspace_id: string } } };
    extraWorkspaces.push(made.result.workspace.workspace_id);
  }
  try {
    ({ app } = await launch(run.env));
    const page = await app.firstWindow();
    const sent = countSent(page);
    await enterWorkspace(page, "fixture");
    const canvas = page.locator("[data-canvas]").first();
    const cycleRow = (kind: string) => page.locator(`[data-cycle=${kind}] [aria-selected=true]`);

    // Recent order third, second, first: the first tab is current.
    for (const tab of [tabs[2]!, tabs[1]!, tabs[0]!]) {
      await page.locator(`[data-tab="${tab}"]`).click();
      await expect(canvas).toHaveAttribute("data-canvas", tab);
    }

    // ⌃Tab twice with ⌃ held walks two back; releasing ⌃ commits one focus_tab.
    let focused = sent.get("focus_tab") ?? 0;
    await page.keyboard.down("Control");
    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
    await expect(cycleRow("panels")).toHaveAttribute("data-cycle-row", tabs[2]!);
    expect(sent.get("focus_tab") ?? 0).toBe(focused);
    await page.keyboard.up("Control");
    await expect(page.locator("[data-cycle]")).toHaveCount(0);
    await expect(canvas).toHaveAttribute("data-canvas", tabs[2]!);
    await exactlyOnce(sent, "focus_tab", focused + 1, page);

    // ⌃⇧Tab walks back toward the start: from third, two forward then one
    // back is first, the order Recent Panels holds across every checkout.
    focused = sent.get("focus_tab") ?? 0;
    await page.keyboard.down("Control");
    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
    await page.keyboard.press("Shift+Tab");
    await expect(cycleRow("panels")).toHaveAttribute("data-cycle-row", tabs[0]!);
    await page.keyboard.up("Control");
    await expect(canvas).toHaveAttribute("data-canvas", tabs[0]!);
    await exactlyOnce(sent, "focus_tab", focused + 1, page);

    // Recent Projects gamma, beta, fixture: fixture is current.
    await page.locator('[data-sidebar-mode="projects"]').click();
    const checkout = (name: string) => page.locator("[data-project]", { hasText: name }).locator("[data-checkout]").first();
    for (const name of ["gamma", "beta", "fixture"]) {
      await checkout(name).click();
      await expect(checkout(name)).toHaveAttribute("aria-current", "true");
    }

    // ⌥Tab twice walks to gamma; releasing ⌥ commits one focus_tab, back on
    // the tab gamma was last used on.
    focused = sent.get("focus_tab") ?? 0;
    await page.keyboard.down("Alt");
    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
    await expect(cycleRow("projects")).toBeVisible();
    expect(sent.get("focus_tab") ?? 0).toBe(focused);
    await page.keyboard.up("Alt");
    await expect(page.locator("[data-cycle]")).toHaveCount(0);
    await expect(checkout("gamma")).toHaveAttribute("aria-current", "true");
    await exactlyOnce(sent, "focus_tab", focused + 1, page);

    // ⌥⇧Tab walks backward: from gamma, one back past the end is beta.
    focused = sent.get("focus_tab") ?? 0;
    await page.keyboard.down("Alt");
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.up("Alt");
    await expect(checkout("beta")).toHaveAttribute("aria-current", "true");
    await exactlyOnce(sent, "focus_tab", focused + 1, page);
  } finally {
    for (const id of extraWorkspaces) herdr.run(["workspace", "close", id]);
    for (const id of tabs.slice(1)) herdr.run(["tab", "close", id]);
  }
});
