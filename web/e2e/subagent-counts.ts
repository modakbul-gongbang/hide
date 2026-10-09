// What the snapshot frames a page receives say about a pane's in-process subagents, for the specs that move
// a pane's counts through an agent's hook (OpenCode's plugin, Pi's and omp's extension).

import type { Page } from "@playwright/test";

/** An object anywhere in `node` that is `pane`'s row with its children, as the snapshot frames carry it. */
function paneWorking(node: unknown, pane: string): number | null | undefined {
  if (Array.isArray(node)) {
    for (const item of node) {
      const found = paneWorking(item, pane);
      if (found !== undefined) return found;
    }
    return undefined;
  }
  if (!node || typeof node !== "object") return undefined;
  const row = node as { id?: unknown; children?: { subagents?: { working?: number | null } } | null };
  if (row.id === pane && row.children?.subagents) return row.children.subagents.working ?? null;
  for (const value of Object.values(node)) {
    const found = paneWorking(value, pane);
    if (found !== undefined) return found;
  }
  return undefined;
}

/** The in-process subagents working in `pane`, as the last snapshot frame the page received that names it says. */
export function snapshotWorking(page: Page, pane: string): () => number | null | undefined {
  let last: number | null | undefined;
  page.on("websocket", (ws) => ws.on("framereceived", (frame) => {
    const text = String(frame.payload);
    if (!text.includes('"subagents"') || !text.includes(pane)) return;
    try {
      const found = paneWorking(JSON.parse(text), pane);
      if (found !== undefined) last = found;
    } catch {
      /* not a JSON frame */
    }
  }));
  return () => last;
}
