// S2 flows on an isolated pinned Herdr (PRD web-shell-pivot-s2 B16): two
// checkouts, three tabs, two splits, one registration, one refusal, plus
// the shortcut sheet, zoom, a pane close and a divider drag. Every command
// runs against a private server; the operator's Herdr is never touched.

import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { startHerdr } from "./herdr-fixture";
import { startHided } from "./hided-fixture";
import { countSent, screenshot } from "./wire";

type Daemon = { origin: string; token: string; home: string; stop: () => void };

/**
 * Waits until the shell has drawn `paneId` as the focused pane and kept it
 * for a second. Herdr confirms a focus later than the core moves it, and
 * on a slow runner the confirmation of one request can reach the core after
 * the next request went out, naming the earlier pane for a moment; keys
 * typed in that moment follow it.
 */
async function settledFocus(page: Page, paneId: string): Promise<void> {
  const deadline = Date.now() + 15_000;
  let held = 0;
  while (held < 5) {
    if (Date.now() > deadline) throw new Error(`focus did not settle on ${paneId}`);
    await new Promise((resolve) => setTimeout(resolve, 200));
    const focused = await page.locator('[data-pane-view][data-focused="true"]').getAttribute("data-pane-view");
    held = focused === paneId ? held + 1 : 0;
  }
}

async function screen(page: Page): Promise<string> {
  return page.evaluate(() => window.__hideProbe?.screenText() ?? "");
}

test.describe.configure({ timeout: 90_000 });

test("checkouts, tabs, splits, zoom, close and the sheet", async ({ page, context }) => {
  // A Workspace opens with the Explorer beside its agents; the room keeps
  // the split panes wide enough that typed lines do not wrap.
  await page.setViewportSize({ width: 1680, height: 900 });
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    // A second checkout and two more tabs in the first, all without focus.
    fs.mkdirSync(path.join(herdr.root, "beta"), { recursive: true });
    const beta = herdr.run([
      "workspace", "create", "--cwd", path.join(herdr.root, "beta"), "--label", "beta", "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
    ]) as { result: { workspace: { workspace_id: string }; tab: { tab_id: string } } };
    const tabs = [herdr.tab];
    for (const label of ["second", "third"]) {
      const made = herdr.run([
        "tab", "create", "--workspace", herdr.workspace, "--cwd", path.join(herdr.root, "fixture"), "--label", label, "--env", `PATH=${herdr.fixturePath}`, "--no-focus",
      ]) as { result: { tab: { tab_id: string } } };
      tabs.push(made.result.tab.tab_id);
    }
    daemon = await startHided(herdr);
    const lastSent = new Map<string, Record<string, unknown>>();
    const sent = countSent(page, lastSent);
    await page.goto(`${daemon.origin}/?probe=1#token=${daemon.token}`);

    // Two checkouts in the Projects sidebar. Which one the core focuses at
    // boot is its own choice, so the flow starts by choosing the first.
    await page.locator('[data-sidebar-mode="projects"]').click();
    const checkouts = page.locator("[data-checkout]");
    await expect(checkouts).toHaveCount(2);
    // The core names projects by directory, not by Herdr's label or id.
    const firstProject = page.locator("[data-project]", { hasText: "fixture" });
    const betaProject = page.locator("[data-project]", { hasText: "beta" });
    const firstRow = firstProject.locator("[data-checkout]").first();
    await firstRow.click();
    await expect(firstRow).toHaveAttribute("aria-current", "true");
    await expect(page.locator("[role=tab]")).toHaveCount(3);
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    await screenshot(page, "s2-projects-two-checkouts");
    const focusEvents = sent.get("focus_checkout") ?? 0;

    // Switching checkouts is one focus_checkout; the tab bar and center follow.
    const betaRow = betaProject.locator("[data-checkout]").first();
    await betaRow.click();
    await expect(betaRow).toHaveAttribute("aria-current", "true");
    await expect(page.locator("[role=tab]")).toHaveCount(1);
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-canvas", beta.result.tab.tab_id);
    await expect(page.locator("[data-pane-view]")).toHaveCount(1);
    await expect.poll(() => sent.get("focus_checkout")).toBe(focusEvents + 1);

    // A plain folder's sidebar row opens its checkout, so its Overview is
    // reached from All projects (S6 B1), and its Workspace row enters the Workspace (B2).
    await page.locator("[data-go-main]").click();
    await page.locator('[data-main-tab="projects"]').click();
    await page.locator("[data-main-project]", { hasText: /^fixture/ }).click();
    await expect(page.locator("[data-overview-screen]")).toBeVisible();
    await page.locator("[data-overview-workspace]").first().click();
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-canvas", herdr.tab);
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);

    // Tab switch: the previous tab's instances are gone, the new one's mounted.
    await page.locator(`[data-tab="${tabs[1]}"]`).click();
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-canvas", tabs[1]);
    await expect(page.locator("[data-pane-view]")).toHaveCount(1);
    await expect.poll(() => sent.get("focus_tab")).toBe(1);
    await expect.poll(() => screen(page), { timeout: 15_000 }).toContain("fixture %");

    // ⌥T is one create_tab; the new tab is active with the core's next label.
    const nextLabel = (await page.getByRole("button", { name: /^New tab / }).getAttribute("aria-label"))!.replace("New tab ", "");
    await page.keyboard.press("Alt+KeyT");
    await expect(page.locator("[role=tab]")).toHaveCount(4);
    await expect(page.locator("[role=tab][aria-selected=true]")).toContainText(nextLabel);
    await expect.poll(() => sent.get("create_tab")).toBe(1);

    // ⌥` cycles recent tabs: the previous tab (second) is the first candidate.
    await page.keyboard.down("Alt");
    await page.keyboard.press("Backquote");
    await expect(page.locator("[data-cycle=tabs] [aria-selected=true]")).toHaveAttribute("data-cycle-row", tabs[1]);
    await page.keyboard.up("Alt");
    await expect(page.locator("[data-cycle]")).toHaveCount(0);
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-canvas", tabs[1]);
    await expect.poll(() => sent.get("focus_tab")).toBe(2);

    // Back on the split tab: ⌘D splits the focused pane (second split), ⌘⌥↩ zooms it.
    await page.locator(`[data-tab="${herdr.tab}"]`).click();
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    await expect.poll(() => screen(page), { timeout: 15_000 }).toContain("claude");
    await page.keyboard.press("Meta+KeyD");
    await expect(page.locator("[data-pane-view]")).toHaveCount(3);
    await expect(page.locator("[data-split]")).toHaveCount(2);
    await expect.poll(() => sent.get("create_pane")).toBe(1);
    await screenshot(page, "s2-two-splits");

    // The new pane runs the fixture shell. A wheel over it is one
    // terminal_scroll per batch and the core's viewport follows (B19).
    const shellPane = page.locator('[data-pane-view][data-focused="true"]');
    await expect(shellPane).toHaveAttribute("data-transport", /connected|controlling|idle/, { timeout: 15_000 });
    const shellPaneId = (await shellPane.getAttribute("data-pane-view"))!;
    await expect.poll(() => screen(page), { timeout: 15_000 }).toContain("fixture %");
    await shellPane.locator(".xterm-helper-textarea").focus();
    await page.keyboard.type("seq 1 100\n");
    await expect.poll(() => screen(page), { timeout: 15_000 }).toMatch(/100\s+fixture %/);
    const shellBox = (await shellPane.boundingBox())!;
    await page.mouse.move(shellBox.x + shellBox.width / 2, shellBox.y + shellBox.height / 2);
    const keysBeforeWheel = sent.get("key") ?? 0;
    await page.mouse.wheel(0, -120);
    await expect.poll(() => sent.get("terminal_scroll")).toBe(1);
    expect(lastSent.get("terminal_scroll")).toMatchObject({ direction: "up", modifiers: 0 });
    expect(lastSent.get("terminal_scroll")!.lines as number).toBeGreaterThan(1);
    await expect.poll(() => screen(page), { timeout: 10_000 }).not.toMatch(/100\s+fixture %/);
    await page.mouse.wheel(0, 400);
    await expect.poll(() => sent.get("terminal_scroll")).toBe(2);
    expect(lastSent.get("terminal_scroll")).toMatchObject({ direction: "down" });
    await expect.poll(() => screen(page), { timeout: 10_000 }).toMatch(/100\s+fixture %/);
    // ⌥ + wheel is the browser's, not Herdr's.
    await page.keyboard.down("Alt");
    await page.mouse.wheel(0, -120);
    await page.keyboard.up("Alt");
    await expect.poll(() => sent.get("terminal_scroll")).toBe(2);
    // The wheel never became key bytes: xterm did not get to turn it into
    // cursor keys (which would walk the shell's history).
    expect(sent.get("key") ?? 0).toBe(keysBeforeWheel);

    // An alternate-screen program (less) gets the same treatment: the
    // wheel is one terminal_scroll and no cursor-key bytes reach the PTY.
    await page.keyboard.type("seq 1 200 | less\n");
    await expect.poll(() => screen(page), { timeout: 15_000 }).toMatch(/^1\s/);
    const keysBeforeLess = sent.get("key") ?? 0;
    await page.mouse.wheel(0, 240);
    await expect.poll(() => sent.get("terminal_scroll")).toBe(3);
    expect(lastSent.get("terminal_scroll")).toMatchObject({ direction: "down" });
    await page.mouse.wheel(0, -240);
    await expect.poll(() => sent.get("terminal_scroll")).toBe(4);
    expect(sent.get("key") ?? 0).toBe(keysBeforeLess);
    await page.keyboard.press("q");
    await expect.poll(() => screen(page), { timeout: 15_000 }).toMatch(/fixture %\s*$/);

    // A single click in a pane is one terminal_click with the pressed cell
    // (B20). The core answers it with the SGR report a mouse-tracking
    // program asked for, and that report is the only thing the PTY gets:
    // the shim logs every byte it reads, and no key event went out, so
    // xterm wrote no mouse report of its own.
    const agentPane = herdr.panes[1];
    // The cell comes from xterm's screen element, exactly cols by rows; the
    // host around it is larger by the fit's remainder, and a cell read from
    // the host drifts toward the far edge (the delta is printed so a run
    // shows it: the last row of a 41-row pane lands a row off).
    const agentHost = (await page.locator(`[data-pane-view="${agentPane}"] [data-terminal]`).boundingBox())!;
    const agentScreen = (await page.locator(`[data-pane-view="${agentPane}"] .xterm-screen`).boundingBox())!;
    const agentGrid = (await page.evaluate((id) => window.__hideProbe?.paneGrid(id) ?? null, agentPane))!;
    const lastRowFromHost = Math.floor((agentScreen.y + (agentGrid.rows - 0.5) * (agentScreen.height / agentGrid.rows) - agentHost.y) / (agentHost.height / agentGrid.rows));
    console.log(
      `cell grid: host ${agentHost.width}x${agentHost.height}, screen ${agentScreen.width}x${agentScreen.height}, ${agentGrid.cols}x${agentGrid.rows}; ` +
        `the last row's centre read from the host is row ${lastRowFromHost}`,
    );
    const cell = { column: 4, row: 2 };
    const focusBefore = sent.get("focus_pane") ?? 0;
    const keysBeforeClick = sent.get("key") ?? 0;
    const inputBefore = fs.existsSync(herdr.inputLogs[1]) ? fs.statSync(herdr.inputLogs[1]).size : 0;
    await page.mouse.click(
      agentScreen.x + (cell.column + 0.5) * (agentScreen.width / agentGrid.cols),
      agentScreen.y + (cell.row + 0.5) * (agentScreen.height / agentGrid.rows),
    );
    await expect.poll(() => sent.get("terminal_click")).toBe(1);
    expect(lastSent.get("terminal_click")).toEqual({ pane_id: agentPane, column: cell.column, row: cell.row, modifiers: 0 });
    await expect.poll(() => sent.get("focus_pane")).toBe(focusBefore + 1);
    // Herdr confirms a focus later than the core moves it, and a confirmation
    // still in flight when the next focus_pane goes out is followed as a
    // move of Herdr's own (ARCHITECTURE.md), which would flip keyboard focus
    // back mid-typing on a slow runner; each focus waits for Herdr's word.
    const herdrFocused = () => (herdr.run(["pane", "current"]) as { result: { pane: { pane_id: string } } }).result.pane.pane_id;
    await expect.poll(herdrFocused, { timeout: 10_000 }).toBe(agentPane);
    await settledFocus(page, agentPane);
    const report = `\x1b[<0;${cell.column + 1};${cell.row + 1}M\x1b[<0;${cell.column + 1};${cell.row + 1}m`;
    await expect
      .poll(() => (fs.existsSync(herdr.inputLogs[1]) ? fs.readFileSync(herdr.inputLogs[1], "latin1").slice(inputBefore) : ""), { timeout: 10_000 })
      .toBe(report);
    expect(sent.get("key") ?? 0).toBe(keysBeforeClick);

    // A drag selects locally and is not a click. Its copy is what a native
    // terminal gives (B20): no padding to the column edge, the wrapped line
    // joined back into one, and the real line ends kept.
    const shellView = page.locator(`[data-pane-view="${shellPaneId}"]`);
    await shellView.locator(".xterm-helper-textarea").focus();
    await expect(shellView).toHaveAttribute("data-focused", "true");
    await expect.poll(herdrFocused, { timeout: 10_000 }).toBe(shellPaneId);
    await settledFocus(page, shellPaneId);
    // Two operator focus changes are two focus_pane events; the focus the
    // shell moves to follow the snapshot is never reported back.
    expect(sent.get("focus_pane")).toBe(focusBefore + 2);
    const shellGrid = (await page.evaluate((id) => window.__hideProbe?.paneGrid(id) ?? null, shellPaneId))!;
    const wrapped = "w".repeat(shellGrid.cols + 7);
    await page.keyboard.type(`clear; echo ${wrapped}; echo short; echo; echo '  in'; echo '    deeper'; echo end\n`);
    await expect.poll(() => screen(page), { timeout: 15_000 }).toMatch(/^w+\s*\n\s*w+\s*\n\s*short\s*\n\s*\n\s+in\s*\n\s+deeper\s*\n\s*end/);
    const shellScreen = (await shellView.locator(".xterm-screen").boundingBox())!;
    const shellCell = (column: number, row: number) => ({
      x: shellScreen.x + (column + 0.5) * (shellScreen.width / shellGrid.cols),
      y: shellScreen.y + (row + 0.5) * (shellScreen.height / shellGrid.rows),
    });
    const clicksBeforeDrag = sent.get("terminal_click") ?? 0;
    // Dragged from the empty row back to the first cell: the split's divider
    // grab area covers the pane's first column, so the press starts inside.
    const pressAt = shellCell(shellGrid.cols - 2, 3);
    const dragTo = shellCell(0, 0);
    await page.mouse.move(pressAt.x, pressAt.y);
    await page.mouse.down();
    await page.mouse.move(dragTo.x - shellScreen.width / shellGrid.cols, dragTo.y, { steps: 8 });
    await page.mouse.up();
    await expect.poll(() => page.evaluate((id) => window.__hideProbe?.paneSelection(id) ?? null, shellPaneId)).toBe(`${wrapped}\nshort\n`);
    expect(sent.get("terminal_click") ?? 0).toBe(clicksBeforeDrag);
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    await page.keyboard.press("Meta+KeyC");
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(`${wrapped}\nshort\n`);
    // Lines that share a margin lose it and keep their relative indentation.
    const indentFrom = shellCell(shellGrid.cols - 2, 5);
    const indentTo = shellCell(0, 4);
    await page.mouse.move(indentFrom.x, indentFrom.y);
    await page.mouse.down();
    await page.mouse.move(indentTo.x - shellScreen.width / shellGrid.cols, indentTo.y, { steps: 8 });
    await page.mouse.up();
    await expect.poll(() => page.evaluate((id) => window.__hideProbe?.paneSelection(id) ?? null, shellPaneId)).toBe("in\n  deeper");
    await page.keyboard.press("Meta+KeyC");
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe("in\n  deeper");

    await page.keyboard.press("Meta+Alt+Enter");
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-zoomed", "true");
    await expect.poll(() => sent.get("toggle_zoom")).toBe(1);
    // The zoomed pane takes the whole canvas, not just its own split cell,
    // and Herdr's wider PTY is what its terminal_resize follows.
    const canvasBox = (await page.locator("[data-canvas]").boundingBox())!;
    const zoomedBox = (await page.locator('[data-pane-view][data-focused="true"]').boundingBox())!;
    expect(Math.abs(zoomedBox.width - canvasBox.width)).toBeLessThan(2);
    expect(Math.abs(zoomedBox.height - canvasBox.height)).toBeLessThan(2);
    expect(Math.abs(zoomedBox.x - canvasBox.x)).toBeLessThan(2);
    await screenshot(page, "s2-zoomed");
    await page.keyboard.press("Meta+Alt+Enter");
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-zoomed", "false");
    await expect(page.locator("[data-pane-view]")).toHaveCount(3);

    // A divider drag sends one resize_pane on release and the grid follows Herdr.
    const outer = page.locator("[data-split=right]").first();
    const divider = outer.locator("> [data-divider]");
    const box = (await divider.boundingBox())!;
    const before = await outer.evaluate((el) => (el as HTMLElement).style.gridTemplateColumns);
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 60, box.y + box.height / 2, { steps: 6 });
    await expect(page.locator("[data-resize-guide]")).toHaveCount(1);
    await page.mouse.move(box.x + box.width / 2 + 120, box.y + box.height / 2, { steps: 6 });
    await page.mouse.up();
    await expect(page.locator("[data-resize-guide]")).toHaveCount(0);
    await expect.poll(() => sent.get("resize_pane")).toBe(1);
    await expect
      .poll(() => outer.evaluate((el) => (el as HTMLElement).style.gridTemplateColumns), { timeout: 10_000 })
      .not.toBe(before);

    // ⌥⇧W closes the focused idle pane; Herdr's new geometry redraws the grid,
    // and the closed pane's terminal is disposed, not parked (D-05).
    const closingPane = (await page.evaluate(() => window.__hideProbe?.paneId()))!;
    await page.keyboard.press("Alt+Shift+KeyW");
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    await expect.poll(() => sent.get("close_pane")).toBe(1);
    await expect(page.locator("[data-confirm-close]")).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => window.__hideProbe?.liveTerminals() ?? [])).not.toContain(closingPane);
    await expect(page.locator("[data-terminal-parking] [data-terminal]")).toHaveCount(0);

    // Dragging a tab onto another sends one reorder_tab; the strip redraws in the core's order.
    const secondTab = page.locator(`[data-tab="${tabs[1]}"]`);
    const firstTab = page.locator(`[data-tab="${herdr.tab}"]`);
    const from = (await secondTab.boundingBox())!;
    const to = (await firstTab.boundingBox())!;
    await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
    await page.mouse.down();
    await page.mouse.move(from.x + from.width / 2 - 30, from.y + from.height / 2, { steps: 4 });
    await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 8 });
    await page.mouse.up();
    await expect.poll(() => sent.get("reorder_tab")).toBe(1);
    await expect.poll(() => page.locator("[role=tab]").first().getAttribute("data-tab"), { timeout: 10_000 }).toBe(tabs[1]);

    // ⌥W closes the visible tab (its panes are idle, so no confirmation).
    await page.locator(`[data-tab="${tabs[2]}"]`).click();
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-canvas", tabs[2]);
    await page.keyboard.press("Alt+KeyW");
    await expect.poll(() => sent.get("close_tab")).toBe(1);
    await expect(page.locator("[role=tab]")).toHaveCount(3);
    await expect(page.locator(`[data-tab="${tabs[2]}"]`)).toHaveCount(0);
    await page.locator(`[data-tab="${herdr.tab}"]`).click();
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);

    // ⌘/ opens the sheet from the registry with the seven moved chords marked.
    // The pane keeps keyboard focus under the sheet, and the Escape that
    // closes it is the shell's alone: no ESC byte reaches the program.
    await page.locator('[data-pane-view][data-focused="true"] .xterm-helper-textarea').focus();
    const keysBeforeSheet = sent.get("key") ?? 0;
    await page.keyboard.press("Meta+Slash");
    await expect(page.locator("[data-shortcut-sheet]")).toBeVisible();
    // The S5 Settings row (⌥, in place of Chrome's ⌘,) is the 27th and the eighth move.
    await expect(page.locator("[data-shortcut]")).toHaveCount(27);
    await expect(page.locator("[data-shortcut-sheet]").getByText("moved for Chrome")).toHaveCount(8);
    await screenshot(page, "s2-shortcut-sheet");
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-shortcut-sheet]")).toHaveCount(0);
    expect(sent.get("key") ?? 0).toBe(keysBeforeSheet);

    // ⌘F is intercepted from Chrome: the find bar opens instead.
    await page.keyboard.press("Meta+KeyF");
    await expect(page.locator("[data-find-bar]")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-find-bar]")).toHaveCount(0);

    // Typed text still echoes in the focused pane after all of that.
    await page.locator('[data-pane-view][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.type("s2-echo-4b2e");
    await expect.poll(() => screen(page), { timeout: 10_000 }).toContain("s2-echo-4b2e");

    // Leaving the tab parks its terminals instead of disposing them (D-05):
    // they are still live while away, and coming back shows the last frame
    // in the same tick as the click, before any frame could arrive.
    const echoPane = (await page.evaluate(() => window.__hideProbe?.paneId()))!;
    await page.locator(`[data-tab="${tabs[1]}"]`).click();
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-canvas", tabs[1]);
    expect(await page.evaluate(() => window.__hideProbe?.liveTerminals() ?? [])).toContain(echoPane);
    expect(await page.evaluate((id) => window.__hideProbe?.paneText(id) ?? "", echoPane)).toContain("s2-echo-4b2e");
    const backAtOnce = await page.evaluate(
      ({ tab, id }) => {
        document.querySelector<HTMLElement>(`[data-tab="${tab}"]`)!.click();
        return window.__hideProbe?.paneText(id) ?? "";
      },
      { tab: herdr.tab, id: echoPane },
    );
    expect(backAtOnce).toContain("s2-echo-4b2e");
    await expect(page.locator("[data-canvas]")).toHaveAttribute("data-canvas", herdr.tab);
    await expect(page.locator(`[data-pane-view="${echoPane}"]`)).toHaveAttribute("data-transport", /connected|controlling|idle/);
    await expect(page.locator(`[data-pane-view="${echoPane}"] [data-terminal]`)).toHaveCount(1);
    // Coming back is a fit on the same size, not a request for a full frame.
    await expect.poll(() => lastSent.get("terminal_viewport")?.new_view).toBe(false);

    // A dropped socket comes back with the tab bar and splits from the core (B14).
    await page.evaluate(() => window.__hideProbe?.dropSocket());
    await expect(page.locator("[data-connection]")).toHaveText(/reconnecting/, { timeout: 15_000 });
    await expect(page.locator("[data-connection]")).toHaveCount(0, { timeout: 15_000 });
    await expect(page.locator("[role=tab]")).toHaveCount(3);
    await expect(page.locator("[data-pane-view]")).toHaveCount(2);
    await expect(page.locator("[data-split]")).toHaveCount(1);
    await page.locator('[data-pane-view][data-focused="true"] .xterm-helper-textarea').focus();
    await page.keyboard.type("after-reconnect-1d7c");
    await expect.poll(() => screen(page), { timeout: 10_000 }).toContain("after-reconnect-1d7c");
    await screenshot(page, "s2-after-reconnect");
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});

test("registration under home succeeds; outside home and a .. path are refused", async ({ page }) => {
  const herdr = await startHerdr();
  let daemon: Daemon | null = null;
  try {
    daemon = await startHided(herdr);
    const sent = countSent(page);
    await page.goto(`${daemon.origin}/#token=${daemon.token}`);
    await page.locator('[data-sidebar-mode="projects"]').click();
    await expect(page.locator("[data-project]")).toHaveCount(1);

    // The field lives in the sidebar, so ⌥⇧N with the sidebar hidden brings
    // the sidebar back (one ui_state_update) and then opens the field.
    await page.keyboard.press("Meta+KeyB");
    await expect(page.locator("[data-sidebar]")).toHaveCount(0);
    await expect.poll(() => sent.get("ui_state_update") ?? 0).toBe(1);
    await page.keyboard.press("Alt+Shift+KeyN");
    await expect(page.locator("[data-sidebar]")).toHaveCount(1);
    await expect.poll(() => sent.get("ui_state_update") ?? 0).toBe(2);
    const input = page.getByLabel("Workspace path");
    await expect(input).toBeVisible();
    await expect(input).toHaveValue(`${daemon.home}/`);
    // The listing came from hided: only directories, no hidden one, no file.
    await expect(page.locator("[data-suggestion]")).toHaveCount(1);
    await expect(page.locator(`[data-suggestion="${daemon.home}/projects"]`)).toBeVisible();

    // Outside home is refused by the shell before any event goes out.
    await input.fill(herdr.root);
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-registration-reason]")).toHaveAttribute("data-registration-reason", "outside_home");
    await expect.poll(() => sent.get("create_workspace") ?? 0).toBe(0);
    await screenshot(page, "s2-registration-refused-outside-home");

    // A path that names its way with .. reaches hided, which refuses it with a reason code.
    await input.fill(`${daemon.home}/projects/../projects/alpha`);
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-registration-reason]")).toHaveAttribute("data-registration-reason", "invalid_path");
    await expect.poll(() => sent.get("create_workspace")).toBe(1);

    // A directory under home registers: one create_workspace, a new project in the sidebar.
    await input.fill(`${daemon.home}/projects/`);
    await expect(page.locator(`[data-suggestion="${daemon.home}/projects/alpha"]`)).toBeVisible();
    await expect(page.locator("[data-suggestion]")).toHaveCount(1);
    await input.fill(`${daemon.home}/projects/alpha`);
    await page.keyboard.press("Enter");
    await expect.poll(() => sent.get("create_workspace")).toBe(2);
    await expect(page.locator("[data-project]")).toHaveCount(2, { timeout: 20_000 });
    await expect(page.locator("[data-project-list]")).toContainText("alpha");
    await expect(page.locator("[data-registration-reason]")).toHaveCount(0);
    await screenshot(page, "s2-registration-alpha");

    // Registering it again is refused by the shell from the snapshot.
    await input.fill(`${daemon.home}/projects/alpha`);
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-registration-reason]")).toHaveAttribute("data-registration-reason", "already_registered");
    await expect.poll(() => sent.get("create_workspace")).toBe(2);
  } finally {
    daemon?.stop();
    herdr.stop();
  }
});
