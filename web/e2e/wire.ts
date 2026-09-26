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

/**
 * The sidebar at a given width, and whether anything in it overflows
 * sideways: the list scrolling horizontally, or a row's time or control
 * running past the row's right edge (PRD sidebar-readability B25).
 */
export async function sidebarOverflow(page: Page, width: string): Promise<string[]> {
  return page.evaluate((value) => {
    const nav = document.querySelector<HTMLElement>("nav[data-sidebar]");
    if (!nav) return ["no sidebar"];
    nav.style.width = value;
    const problems: string[] = [];
    for (const list of document.querySelectorAll<HTMLElement>("[data-agent-list], [data-project-list]")) {
      if (list.scrollWidth > list.clientWidth) problems.push(`list scrolls sideways ${list.scrollWidth} > ${list.clientWidth}`);
    }
    for (const row of document.querySelectorAll<HTMLElement>("[data-pane], [data-checkout-row] > *, [data-project] > *")) {
      const right = row.getBoundingClientRect().right;
      for (const part of row.querySelectorAll<HTMLElement>("[data-agent-elapsed], [data-checkout-age], [data-project-activity], button")) {
        if (part.getBoundingClientRect().right > right + 0.5) problems.push(`${part.textContent ?? part.tagName} passes its row`);
      }
    }
    return problems;
  }, width);
}

/**
 * Whether every sidebar row still holds its text at the interface font size
 * in force (PRD sidebar-readability B26): no text runs past the bottom of
 * any box around it up to its list item, which is where a fixed-height row
 * spills, unless that box clips it, and no two of them overlap, which is how
 * a spill shows on screen. A clipping box's own glyphs are judged by eye.
 */
export async function sidebarRowsFit(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const problems: string[] = [];
    // Every element in the lists that holds text itself: names, lines, places, times, chips.
    const parts = [...document.querySelectorAll<HTMLElement>("nav[data-sidebar] :is([data-agent-list], [data-project-list]) *")].filter(
      (part) => part.getClientRects().length > 0 && [...part.childNodes].some((node) => node.nodeType === Node.TEXT_NODE && node.textContent?.trim()),
    );
    // What of a part can show: a box that clips its overflow (a badge, a
    // truncated label) is the visible edge of the text inside it.
    const shown = new Map<HTMLElement, DOMRect>();
    for (const part of parts) {
      const rect = DOMRect.fromRect(part.getBoundingClientRect());
      for (let box = part.parentElement; box && box.tagName !== "NAV"; box = box.parentElement) {
        const edge = box.getBoundingClientRect().bottom;
        if (rect.bottom > edge + 0.5) {
          if (getComputedStyle(box).overflowY === "visible") problems.push(`${part.textContent} spills below its ${box.tagName.toLowerCase()}`);
          else rect.height = Math.max(0, edge - rect.top);
        }
        if (box.tagName === "LI") break;
      }
      shown.set(part, rect);
    }
    const boxes = [...shown];
    for (const [i, [a, ra]] of boxes.entries()) {
      for (const [b, rb] of boxes.slice(i + 1)) {
        if (a.contains(b) || b.contains(a)) continue;
        const across = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
        const down = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
        if (across > 1 && down > 1) problems.push(`${a.textContent} overlaps ${b.textContent}`);
      }
    }
    return problems;
  });
}
