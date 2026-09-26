// Shared e2e helpers: the client-event counter both flow specs assert with,
// the screenshot that writes only when a run directory was named, and the
// way into a Workspace from the Main a first run opens on.

import { expect, type Page, type WebSocket } from "@playwright/test";
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
 * to that Project's first Workspace. A Project with no agent has no card on
 * its Overview (web-project-overview B10), so its first checkout is opened
 * from the sidebar's project list, and the sidebar is put back on the list
 * it showed. A page that already shows a Workspace is left where it is.
 */
export async function enterWorkspace(page: Page, project?: string): Promise<void> {
  const main = page.locator("[data-main-screen]");
  const workspace = page.locator("[data-workspace-screen]");
  await expect(main.or(workspace)).toBeVisible({ timeout: 20_000 });
  if ((await workspace.count()) > 0) return;
  await main.locator("[data-main-project]:not([disabled])", project ? { hasText: project } : {}).first().click();
  const overview = page.locator("[data-overview-screen]");
  const card = overview.locator("[data-overview-workspace]").first();
  await expect(card.or(overview.locator("[data-overview-empty]"))).toBeVisible();
  if ((await card.count()) > 0) {
    await card.click();
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
 * Shows the Explorer: a Workspace starts with its side panel closed, and the
 * Explorer's toggle opens the panel with it (issue 170).
 */
export async function showExplorer(page: Page): Promise<void> {
  const toggle = page.locator('[data-tool-toggle="explorer"]');
  if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
  await expect(page.locator('[data-tool="explorer"]')).toBeVisible();
}
