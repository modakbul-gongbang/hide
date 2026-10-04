// Shared e2e helpers: the client-event counter both flow specs assert with,
// the screenshot that writes only when a run directory was named, and the
// way into a Workspace from the Main a first run opens on.

import { expect, type Locator, type Page, type WebSocket } from "@playwright/test";
import path from "node:path";
import { chord } from "./chords";
import type { SnapshotRest } from "../src/snapshot";
import { areasOf } from "../src/areaLayout";
import { fixturePagePath } from "./platform-fixture";

/** Explorer attributes use the shared wire spelling, not native filesystem
 * spelling. Escape in the actual page so punctuation cannot change selection. */
export async function explorerRow(page: Page, nativePath: string): Promise<Locator> {
  const escaped = await page.evaluate(value => CSS.escape(value), fixturePagePath(nativePath));
  return page.locator(`[data-explorer-row=${escaped}]`);
}

/** Observe the same published frames the shell consumes, without dispatch or probes. */
export function observeTabProjection(page: Page, root: string) {
  let rest: SnapshotRest | null = null;
  let frames = 0;
  const normalized = (value: string) => value.replace(/\\/g, "/");
  page.on("websocket", (ws) => ws.on("framereceived", (frame) => {
    const text = String(frame.payload);
    if (!text.startsWith("{")) return;
    if (Buffer.byteLength(text) > 8 * 1024 * 1024) throw new Error("projection observation frame cap exceeded");
    const value = JSON.parse(text) as { type?: string; payload?: { rest?: SnapshotRest } };
    if (value.type !== "snapshot" && value.type !== "delta") return;
    if (++frames > 20_000) throw new Error("projection observation frame count cap exceeded");
    rest = value.type === "snapshot" ? value.payload?.rest ?? null : { ...rest, ...value.payload?.rest };
  }));
  return () => {
    const checkouts = rest?.navigator?.workspaces?.flatMap(workspace => workspace.checkouts.map(checkout => ({
      workspace: workspace.id, checkout: checkout.id, owner: checkout.workspace_id,
      rootMatches: normalized(checkout.path) === normalized(root), active: checkout.active_tab_id,
      tabs: checkout.tabs.map(tab => ({ id: tab.id, owner: tab.workspace_id, panes: tab.panes.map(pane => pane.id) })),
    }))) ?? [];
    if (checkouts.length > 256 || checkouts.some(checkout => checkout.tabs.length > 256)) throw new Error("projection observation inventory cap exceeded");
    const layout = rest?.workspace_view?.agent_layout;
    return { frames, checkouts, selectedPane: rest?.focused?.pane_id ?? null,
      groups: layout ? areasOf(layout.root).map(area => ({ id: area.id, active: area.active, tabs: area.displays.map(tab => tab.id) })) : [] };
  };
}

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
 * to that Project's first Workspace: the Overview opens on the request view,
 * whose Agents tile is the graph, whose first box head is a checkout's
 * Workspace (overview-request-view D-05, agents-graph-view B15);
 * a project that draws no box goes
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
 * Shows one tool in the Tools column: a Workspace starts with Tools off, the
 * toolbar's Tools icon turns it on, and its icon tabs choose the tool (PRD
 * three-column-panel D-04).
 */
export async function showTool(page: Page, tool: "explorer" | "changes"): Promise<void> {
  const shown = page.locator(`[data-tool="${tool}"]`);
  if (await shown.isVisible()) return;
  const column = page.locator('[data-column="tools"]');
  if ((await column.count()) === 0) await page.locator('[data-column-toggle="tools"]').click();
  await expect(column).toBeVisible();
  const tab = page.locator(`[data-tool-tab="${tool}"]`);
  if ((await tab.getAttribute("aria-selected")) !== "true") await tab.click();
  await expect(shown).toBeVisible();
}

/** Shows or hides a column from the Workspace toolbar's menu (B15). */
export async function chooseColumn(page: Page, column: "views" | "tools"): Promise<void> {
  await page.locator("[data-workspace-location]").click({ button: "right" });
  await page.locator(`[data-menu-item="${column}"]`).click();
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
}

/**
 * Binds a chord to a command that has none by default in Settings, Shortcuts
 * (the area commands and the global Recent Panels pair have no other way to run
 * in a browser tab), then closes Settings.
 */
export async function bindChordlessCommand(page: Page, id: string, keys: string): Promise<void> {
  await page.keyboard.press(chord("settings"));
  await page.locator('[data-settings-tab="shortcuts"]').click();
  await expect(page.locator(`[data-shortcut-effective="${id}"]`)).toHaveText("-");
  await page.locator(`[data-shortcut-record="${id}"]`).click();
  await page.keyboard.press(keys);
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
 * B4): the row's height, where the row after it starts, and the edge each
 * named part is anchored at: the left edge of the title, the right edge of
 * the elapsed time, whose text grows as the seconds count. Rounded to the
 * device pixel, since a sub-pixel difference is not a visible move.
 */
export async function rowGeometry(row: Locator, next: Locator, parts: Locator[]): Promise<number[]> {
  const box = await row.boundingBox();
  const after = await next.boundingBox();
  if (!box || !after) throw new Error("a measured row is not on screen");
  const xs: number[] = [];
  for (const part of parts) {
    const partBox = await part.boundingBox();
    if (!partBox) throw new Error("a measured part is not on screen");
    const rightAnchored = (await part.getAttribute("data-agent-elapsed")) !== null;
    xs.push(Math.round(rightAnchored ? partBox.x + partBox.width : partBox.x));
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
