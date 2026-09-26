// What the shell may ask of the browser views, checked before anything is
// placed or loaded. The shell is a page served over loopback; its messages
// are input like any other, so every field is bounded here and a message
// that fails is dropped whole (issue 155).

import type { BrowserCommand, BrowserPlacement, BrowserRect, BrowserSync } from "../../../web/src/host";

/** A Workspace holds at most this many displays (`MAX_VIEW_DISPLAYS`). */
export const MAX_SYNCED_DISPLAYS = 64;
/** Live pages at once; a hidden one past this is closed and loads again when shown. */
export const MAX_LIVE_VIEWS = 12;
const MAX_TEXT = 8192;
const MAX_EXTENT = 100_000;
const COMMANDS: ReadonlySet<string> = new Set<BrowserCommand>(["back", "forward", "reload", "stop"]);
const LOADABLE_PROTOCOLS: ReadonlySet<string> = new Set(["http:", "https:", "file:"]);

function text(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= MAX_TEXT;
}

function rect(value: unknown): BrowserRect | null | undefined {
  if (value === null) return null;
  if (typeof value !== "object") return undefined;
  const { x, y, width, height } = value as Record<string, unknown>;
  const numbers = [x, y, width, height];
  if (!numbers.every((n) => typeof n === "number" && Number.isFinite(n) && Math.abs(n) <= MAX_EXTENT)) return undefined;
  if ((width as number) < 0 || (height as number) < 0) return undefined;
  return { x: x as number, y: y as number, width: width as number, height: height as number };
}

/** A sync message, or null when any part of it is out of shape. */
export function parseSync(value: unknown): BrowserSync | null {
  if (typeof value !== "object" || value === null) return null;
  const { workspace, displays } = value as Record<string, unknown>;
  if (workspace !== null && !text(workspace)) return null;
  if (!Array.isArray(displays) || displays.length > MAX_SYNCED_DISPLAYS) return null;
  const parsed: BrowserPlacement[] = [];
  const ids = new Set<string>();
  for (const entry of displays as unknown[]) {
    if (typeof entry !== "object" || entry === null) return null;
    const { id, url, load, visible } = entry as Record<string, unknown>;
    const placed = rect((entry as Record<string, unknown>).rect);
    if (!text(id) || ids.has(id) || !text(url) || typeof load !== "number" || !Number.isSafeInteger(load) || load < 0) return null;
    if (placed === undefined || typeof visible !== "boolean") return null;
    ids.add(id);
    parsed.push({ id, url, load, rect: placed, visible });
  }
  if (workspace === null && parsed.length > 0) return null;
  return { workspace: workspace as string | null, displays: parsed };
}

/** A display named by the shell for a still or a command. */
export function parseTarget(value: unknown): { workspace: string; id: string } | null {
  if (typeof value !== "object" || value === null) return null;
  const { workspace, id } = value as Record<string, unknown>;
  return text(workspace) && text(id) ? { workspace, id } : null;
}

export function parseCommand(value: unknown): BrowserCommand | null {
  return typeof value === "string" && COMMANDS.has(value) ? (value as BrowserCommand) : null;
}

/** Whether a view may load `url`: the web, a local file, or a blank page. */
export function loadable(url: string): boolean {
  if (url === "about:blank") return true;
  try {
    return LOADABLE_PROTOCOLS.has(new URL(url).protocol);
  } catch {
    return false;
  }
}

/** The one key a page lives under: display ids repeat across Workspaces. */
export function viewKey(workspace: string, id: string): string {
  return `${workspace}\u0001${id}`;
}

export type LiveView = { key: string; visible: boolean; shownAt: number };

/**
 * The hidden pages to close so no more than `cap` stay alive, least recently
 * shown first. A page on screen is never closed; there are at most as many
 * of those as View areas.
 */
export function overCap(views: readonly LiveView[], cap: number): string[] {
  const excess = views.length - cap;
  if (excess <= 0) return [];
  return views
    .filter((view) => !view.visible)
    .sort((a, b) => a.shownAt - b.shownAt)
    .slice(0, excess)
    .map((view) => view.key);
}

/** A rect in the shell's CSS pixels as whole window points. */
export function toBounds(placed: BrowserRect, zoom: number): BrowserRect {
  const scale = Number.isFinite(zoom) && zoom > 0 ? zoom : 1;
  const x = Math.round(placed.x * scale);
  const y = Math.round(placed.y * scale);
  return { x, y, width: Math.max(0, Math.round((placed.x + placed.width) * scale) - x), height: Math.max(0, Math.round((placed.y + placed.height) * scale) - y) };
}
