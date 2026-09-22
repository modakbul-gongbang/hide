// S2 flows on an isolated pinned Herdr (PRD web-shell-pivot-s2 B16): two
// checkouts, three tabs, two splits, one registration, one refusal, plus
// the shortcut sheet, zoom, a pane close and a divider drag. Every command
// runs against a private server; the operator's Herdr is never touched.

import { expect, test, type Page, type WebSocket } from "@playwright/test";
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { startHerdr, type HerdrFixture } from "./herdr-fixture";

type Daemon = { origin: string; token: string; home: string; stop: () => void };

async function startHided(herdr: HerdrFixture): Promise<Daemon> {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-e2e-s2-"));
  const home = path.join(dir, "home");
  fs.mkdirSync(path.join(home, "projects", "alpha"), { recursive: true });
  fs.mkdirSync(path.join(home, "projects", ".hidden"), { recursive: true });
  fs.writeFileSync(path.join(home, "projects", "notes.txt"), "x");
  const env = { ...process.env };
  for (const key of ["HERDR_SOCKET_PATH", "HERDR_PANE_ID", "HERDR_TAB_ID", "HERDR_WORKSPACE_ID", "HERDR_ENV"]) delete env[key];
  const child = spawn(path.resolve("..", "target", "debug", "hided"), [], {
    env: {
      ...env,
      HOME: home,
      HIDE_STATE_DIR: path.join(dir, "hide"),
      HIDE_KEEP_ALIVE: "1",
      HIDE_PORT: "0",
      HIDED_UI_DIR: path.resolve("dist"),
      HERDR_SOCKET_PATH: herdr.socket,
      HERDR_BIN_PATH: herdr.bin,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logDir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (logDir) {
    const log = fs.createWriteStream(path.join(logDir, `hided-s2-${path.basename(dir)}.log`), { flags: "a" });
    child.stdout?.pipe(log);
    child.stderr?.pipe(log);
  }
  const stop = () => {
    child.kill();
    fs.rmSync(dir, { recursive: true, force: true });
  };
  for (let i = 0; i < 50; i += 1) {
    const statePath = path.join(dir, "hide", "hided.json");
    if (fs.existsSync(statePath)) {
      try {
        // The file may be mid-write on the first read; the next tick reads it whole.
        const state = JSON.parse(fs.readFileSync(statePath, "utf8")) as { port: number; token: string };
        const origin = `http://127.0.0.1:${state.port}`;
        if ((await fetch(`${origin}/health`)).ok) return { origin, token: state.token, home: fs.realpathSync(home), stop };
      } catch {
        /* still starting */
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  stop();
  throw new Error("hided did not write a state file");
}

/**
 * Counts client events by kind as the page sends them; one action must be
 * one event. `last` keeps the newest payload per kind for shape assertions.
 */
function countSent(page: Page, last: Map<string, Record<string, unknown>> = new Map()): Map<string, number> {
  const counts = new Map<string, number>();
  page.on("pageerror", (error) => console.log(`[pageerror] ${error.message}`));
  page.on("websocket", (ws: WebSocket) => {
    ws.on("socketerror", (error) => console.log(`[ws error] ${error}`));
    ws.on("framesent", (frame) => {
      try {
        const event = JSON.parse(String(frame.payload)) as { kind?: string; payload?: Record<string, unknown> };
        if (event.kind) {
          counts.set(event.kind, (counts.get(event.kind) ?? 0) + 1);
          last.set(event.kind, event.payload ?? {});
        }
      } catch {
        /* the handshake is not an event */
      }
    });
  });
  return counts;
}

function screenshot(page: Page, name: string): Promise<unknown> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  return dir ? page.screenshot({ path: path.join(dir, `${name}.png`) }) : Promise.resolve();
}

async function screen(page: Page): Promise<string> {
  return page.evaluate(() => window.__hideProbe?.screenText() ?? "");
}

test.describe.configure({ timeout: 90_000 });

test("checkouts, tabs, splits, zoom, close and the sheet", async ({ page }) => {
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

    // A project row goes back to that project's last checkout.
    await firstProject.locator("[data-project-row]").click();
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
    await expect.poll(() => screen(page), { timeout: 15_000 }).toContain("fixture %");
    await shellPane.locator(".xterm-helper-textarea").focus();
    await page.keyboard.type("seq 1 100\n");
    await expect.poll(() => screen(page), { timeout: 15_000 }).toMatch(/100\s+fixture %/);
    const shellBox = (await shellPane.boundingBox())!;
    await page.mouse.move(shellBox.x + shellBox.width / 2, shellBox.y + shellBox.height / 2);
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
    await page.keyboard.press("Meta+Slash");
    await expect(page.locator("[data-shortcut-sheet]")).toBeVisible();
    await expect(page.locator("[data-shortcut]")).toHaveCount(25);
    await expect(page.locator("[data-shortcut-sheet]").getByText("moved for Chrome")).toHaveCount(7);
    await screenshot(page, "s2-shortcut-sheet");
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-shortcut-sheet]")).toHaveCount(0);

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
