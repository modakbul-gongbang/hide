// Shared e2e helpers: the client-event counter both flow specs assert with,
// the screenshot that writes only when a run directory was named, and the
// way into a Workspace from the Main a first run opens on.

import { expect, type Locator, type Page, type WebSocket } from "@playwright/test";
import path from "node:path";

/**
 * Counts client events by kind as the page sends them; one action must be
 * one event. `last` keeps the newest payload per kind for shape assertions.
 * An event whose payload names an `action` (`view_layout`) is counted and
 * kept under `kind.action` as well, so one action can be told from another.
 */
export function countSent(page: Page, last: Map<string, Record<string, unknown>> = new Map()): Map<string, number> {
  const counts = new Map<string, number>();
  // With HIDE_E2E_TRACE the wire is narrated with timestamps, so a failed
  // flow shows which side moved and when; Playwright prints it for a failure.
  const trace = process.env.HIDE_E2E_TRACE ? (line: string) => console.log(`[${Date.now()}] ${line}`) : null;
  page.on("pageerror", (error) => console.log(`[pageerror] ${error.message}`));
  page.on("websocket", (ws: WebSocket) => {
    ws.on("socketerror", (error) => console.log(`[ws error] ${error}`));
    ws.on("close", () => trace?.("ws closed"));
    ws.on("framereceived", (frame) => {
      if (!trace) return;
      const head = String(frame.payload);
      const focused = /"focused_pane_id":"([^"]*)"/.exec(head);
      trace(`recv ${head.slice(0, 40)}${focused ? ` focused_pane_id=${focused[1]}` : ""}`);
    });
    ws.on("framesent", (frame) => {
      try {
        const event = JSON.parse(String(frame.payload)) as { kind?: string; payload?: Record<string, unknown> };
        if (event.kind) {
          trace?.(`sent ${event.kind} ${JSON.stringify(event.payload ?? {}).slice(0, 100)}`);
          const action = typeof event.payload?.action === "string" ? `${event.kind}.${event.payload.action}` : null;
          // The usage hints ride their own UI-state update, which the page
          // sends on its own schedule, not for an action; it is counted apart.
          const hint = event.kind === "ui_state_update" && ["usage_window_visible", "usage_popover_open"].some((field) => field in (event.payload ?? {}));
          for (const key of hint ? ["ui_state_update.usage_hint"] : action ? [event.kind, action] : [event.kind]) {
            counts.set(key, (counts.get(key) ?? 0) + 1);
            last.set(key, event.payload ?? {});
          }
        }
      } catch {
        /* the handshake is not an event */
      }
    });
  });
  return counts;
}

/**
 * A first run opens on Main (S6 D-11); a spec about the Workspace goes in
 * through its Project's Overview (the one named `project`, else the first)
 * to that Project's first Workspace: the Overview opens on the Agents lens,
 * whose first lane head is a checkout's Workspace (overview-lenses B11, B17);
 * a project whose only worktrees have no agent shows no lane, so it goes
 * through the sidebar's project list, put back on the list it showed. A page
 * that already shows a Workspace is left where it is.
 */
export async function enterWorkspace(page: Page, project?: string): Promise<void> {
  const main = page.locator("[data-main-screen]");
  const workspace = page.locator("[data-workspace-screen]");
  await expect(main.or(workspace)).toBeVisible({ timeout: 20_000 });
  if ((await workspace.count()) > 0) return;
  await main.locator('[data-main-tab="projects"]').click();
  await main.locator("[data-main-project]:not([disabled])", project ? { hasText: project } : {}).first().click();
  const overview = page.locator("[data-overview-screen]");
  const head = overview.locator("[data-lens-lane] [data-lens-head-open]").first();
  await expect(head.or(overview.locator("[data-lens-empty], [data-lens-fold]")).first()).toBeVisible();
  if ((await head.count()) > 0) {
    await head.click();
  } else {
    const id = await overview.getAttribute("data-overview-screen");
    const mode = await page.locator("[data-sidebar]").getAttribute("data-sidebar");
    await page.locator('[data-sidebar-mode="projects"]').click();
    await page.locator(`[data-project="${id}"] [data-checkout]`).first().click();
    if (mode && mode !== "projects") await page.locator(`[data-sidebar-mode="${mode}"]`).click();
  }
  await expect(workspace).toBeVisible();
}

/**
 * Captures the page once the terminals have painted: xterm draws a written
 * buffer on the next animation frame, so a capture taken straight after a
 * probe read the rows can still show the cleared canvas of a resize.
 */
export async function screenshot(page: Page, name: string): Promise<unknown> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  if (!dir) return;
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
  return page.screenshot({ path: path.join(dir, `${name}.png`) });
}

/**
 * Shows one tool in the side panel's column: a Workspace starts with its
 * panel closed, the panel toggle opens it, the column's toggle shows the
 * column, and its icon tabs choose the tool (issue 170).
 */
export async function showTool(page: Page, tool: "explorer" | "changes"): Promise<void> {
  const shown = page.locator(`[data-tool="${tool}"]`);
  if (await shown.isVisible()) return;
  const panel = page.locator("[data-side-panel]");
  if ((await panel.count()) === 0) await page.locator('[data-panel-toggle="off"]').click();
  await expect(panel).toBeVisible();
  const tab = page.locator(`[data-tool-tab="${tool}"]`);
  await expect(tab.or(page.locator('[data-tools-toggle="off"]')).first()).toBeVisible();
  if (!(await tab.isVisible())) await page.locator('[data-tools-toggle="off"]').click();
  if ((await tab.getAttribute("aria-selected")) !== "true") await tab.click();
  await expect(shown).toBeVisible();
}

/**
 * Chooses the side panel's state from the Workspace toolbar's menu, the one
 * place that names all three states (the palette no longer does).
 */
export async function choosePanel(page: Page, state: "closed" | "open" | "expanded"): Promise<void> {
  await page.locator("[data-workspace-location]").click({ button: "right" });
  await page.locator(`[data-menu-item="panel:${state}"]`).click();
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
}

/**
 * Binds a chord to a command that has none by default in Settings, Shortcuts
 * (the area commands and the global Recent Panels pair have no other way to run
 * in a browser tab), then closes Settings.
 */
export async function bindChordlessCommand(page: Page, id: string, chord: string): Promise<void> {
  await page.keyboard.press("Alt+Comma");
  await page.locator('[data-settings-tab="shortcuts"]').click();
  await expect(page.locator(`[data-shortcut-effective="${id}"]`)).toHaveText("-");
  await page.locator(`[data-shortcut-record="${id}"]`).click();
  await page.keyboard.press(chord);
  await expect(page.locator(`[data-shortcut-problem="${id}"]`)).toHaveCount(0);
  await page.locator(`[data-shortcut-apply="${id}"]`).click();
  await expect(page.locator(`[data-shortcut-effective="${id}"]`)).not.toHaveText("-");
  await page.locator("[data-settings-close]").click();
  await expect(page.locator("[data-settings]")).toHaveCount(0);
}

export async function showExplorer(page: Page): Promise<void> {
  await showTool(page, "explorer");
}

/**
 * What must not move when a row changes state (PRD sidebar-readability B2,
 * B4): the row's height, where the row after it starts, and the left edge of
 * each named part (the title, the time). Rounded to the device pixel, since a
 * sub-pixel difference is not a visible move.
 */
export async function rowGeometry(row: Locator, next: Locator, parts: Locator[]): Promise<number[]> {
  const box = await row.boundingBox();
  const after = await next.boundingBox();
  if (!box || !after) throw new Error("a measured row is not on screen");
  const xs: number[] = [];
  for (const part of parts) {
    const partBox = await part.boundingBox();
    if (!partBox) throw new Error("a measured part is not on screen");
    xs.push(Math.round(partBox.x));
  }
  return [Math.round(box.height), Math.round(after.y), ...xs];
}

/** Puts the pointer on the canvas and drops keyboard focus, so no row is hovered or focused. */
export async function rest(page: Page): Promise<void> {
  await page.mouse.move(900, 600);
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
}

/**
 * Keyboard focus on a control, the way Tab puts it there: a key press first,
 * so the browser draws `:focus-visible` rather than treating it as a click's focus.
 */
export async function keyboardFocus(page: Page, control: Locator): Promise<void> {
  await page.keyboard.press("Shift");
  await control.focus();
  expect(await control.evaluate((element) => element.matches(":focus-visible"))).toBe(true);
}

// The sidebar's geometry is measured in one module, shared with the design
// review command, so a spec and a review judge the same boxes.
export { sidebarColumns, sidebarOverflow, sidebarRowsFit } from "./sidebar-geometry.mjs";

/**
 * Sends one client event on a socket of its own, as a second client would,
 * and returns the first frame `answered` accepts, or null right after the
 * send when no answer is awaited. The browser shell has no Add project (the
 * desktop app's folder picker is its only way in), so a spec registers a
 * folder this way.
 */
export async function sendEvent(
  page: Page,
  daemon: { origin: string; token: string },
  kind: string,
  payload: Record<string, unknown>,
  answered?: string,
): Promise<Record<string, unknown> | null> {
  return page.evaluate(
    async ({ origin, token, kind, payload, answered }) =>
      new Promise<Record<string, unknown> | null>((resolve, reject) => {
        const ws = new WebSocket(`${origin.replace(/^http/, "ws")}/ws`);
        const timer = setTimeout(() => reject(new Error(`${kind}: no ${answered ?? "handshake"} frame`)), 15_000);
        let sent = false;
        ws.onerror = () => reject(new Error(`${kind}: socket failed`));
        ws.onopen = () => ws.send(JSON.stringify({ token, schema_version: 2 }));
        ws.onmessage = (message) => {
          if (!sent) {
            sent = true;
            ws.send(JSON.stringify({ schema_version: 2, kind, payload }));
            if (answered) return;
          } else {
            const frame = JSON.parse(String(message.data)) as Record<string, unknown>;
            if (frame.type !== answered) return;
            clearTimeout(timer);
            ws.close();
            resolve(frame);
            return;
          }
          clearTimeout(timer);
          ws.close();
          resolve(null);
        };
      }),
    { origin: daemon.origin, token: daemon.token, kind, payload, answered },
  );
}

/** Registers a folder of this machine the way Add a project does: one `create_workspace`. */
export async function registerFolder(page: Page, daemon: { origin: string; token: string }, folder: string): Promise<void> {
  await sendEvent(page, daemon, "create_workspace", { path: folder, label: path.basename(folder), initialize_git: false });
}
