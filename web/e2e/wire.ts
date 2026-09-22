// Shared e2e helpers: the client-event counter both flow specs assert with,
// and the screenshot that writes only when a run directory was named.

import type { Page, WebSocket } from "@playwright/test";
import path from "node:path";

/**
 * Counts client events by kind as the page sends them; one action must be
 * one event. `last` keeps the newest payload per kind for shape assertions.
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

export function screenshot(page: Page, name: string): Promise<unknown> {
  const dir = process.env.HIDE_E2E_SCREENSHOT_DIR;
  return dir ? page.screenshot({ path: path.join(dir, `${name}.png`) }) : Promise.resolve();
}
