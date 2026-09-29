// What the shell may ask of the browser views, checked before anything is
// placed or loaded. The shell is a page served over loopback; its messages
// are input like any other, so every field is bounded here and a message
// that fails is dropped whole (issue 155).

import { createHash } from "node:crypto";
import type { BrowserCommand, BrowserPlacement, BrowserRect, BrowserSync } from "../../../web/src/host";

/** A Workspace holds at most this many displays (`MAX_VIEW_DISPLAYS`). */
export const MAX_SYNCED_DISPLAYS = 64;
export const MAX_RETAINED_DISPLAYS = 16_384;
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
  const { workspace, displays, retained } = value as Record<string, unknown>;
  if (workspace !== null && !text(workspace)) return null;
  if (!Array.isArray(displays) || displays.length > MAX_SYNCED_DISPLAYS) return null;
  if (!Array.isArray(retained) || retained.length > MAX_RETAINED_DISPLAYS) return null;
  const owned: BrowserSync["retained"] = [];
  const ownedKeys = new Set<string>();
  for (const entry of retained as unknown[]) {
    if (typeof entry !== "object" || entry === null) return null;
    const { workspace: owner, id } = entry as Record<string, unknown>;
    if (!text(owner) || !text(id)) return null;
    const key = viewKey(owner, id);
    if (ownedKeys.has(key)) return null;
    ownedKeys.add(key);
    owned.push({ workspace: owner, id });
  }
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
  if (workspace && parsed.some((display) => !ownedKeys.has(viewKey(workspace, display.id)))) return null;
  return { workspace: workspace as string | null, displays: parsed, retained: owned };
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

/** Web logins are shared across Workspaces; file previews and each remote device's loopback stay separate. */
export function browserPartition(workspace: string, url: string): string {
  const separator = workspace.indexOf("\u0000");
  if (separator < 1) throw new Error("Browser Workspace key is missing its device");
  const device = workspace.slice(0, separator);
  const address = new URL(url);
  if (address.protocol === "file:") {
    return `persist:hide-browser-file-${createHash("sha256").update(workspace).digest("hex").slice(0, 32)}`;
  }
  if (device !== "local" && isLoopbackHost(address.hostname)) {
    return `persist:hide-browser-loopback-${createHash("sha256").update(device).digest("hex").slice(0, 32)}`;
  }
  return "persist:hide-browser-web";
}

function isLoopbackHost(hostname: string): boolean {
  const host = hostname.toLowerCase().replace(/\.$/, "");
  const mapped = /^\[::(?:(?:ffff:)?([0-9a-f]{1,4}):([0-9a-f]{1,4}))\]$/.exec(host);
  const embeddedLoopback = mapped ? Number.parseInt(mapped[1]!, 16) >> 8 === 127 : false;
  return host === "localhost" || host === "0.0.0.0" || host === "[::1]" || /^127\./.test(host) || embeddedLoopback;
}

/** Route a remote page's absolute loopback requests through its owned View.
 * An unsupported local address is refused instead of reaching this Mac. */
export function remoteRequest(route: { url: string; source_url: string }, raw: string): { redirectURL?: string; cancel?: boolean } {
  try {
    const address = new URL(raw);
    const local = new URL(route.url);
    const source = new URL(route.source_url);
    if (source.protocol === "file:") return address.origin === local.origin && address.protocol === local.protocol ? {} : { cancel: true };
    const loopback = isLoopbackHost(address.hostname);
    if (route.url === route.source_url) return loopback ? { cancel: true } : {};
    if (address.origin === local.origin || (address.protocol === "ws:" && local.protocol === "http:" && address.host === local.host) || (address.protocol === "wss:" && local.protocol === "https:" && address.host === local.host)) return {};
    if (!loopback) return {};
    const sourcePort = source.port || (source.protocol === "https:" ? "443" : "80");
    const addressPort = address.port || ((address.protocol === "https:" || address.protocol === "wss:") ? "443" : "80");
    const matchingScheme = address.protocol === source.protocol || (address.protocol === "ws:" && source.protocol === "http:") || (address.protocol === "wss:" && source.protocol === "https:");
    if (!matchingScheme || addressPort !== sourcePort) return { cancel: true };
    address.host = local.host;
    if (address.protocol === "http:" || address.protocol === "https:") address.protocol = local.protocol;
    return { redirectURL: address.href };
  } catch {
    return { cancel: true };
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

/** Chrome's page zoom steps (`kPresetZoomFactors`), as zoom factors. */
const PAGE_ZOOM_FACTORS = [0.25, 1 / 3, 0.5, 2 / 3, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
/** How near two factors count as the same step, so a stored 0.333… is 1/3. */
const ZOOM_EPSILON = 0.001;

export type PageZoom = "in" | "out" | "reset";

/** The zoom factor one step from `current`, as Chrome steps it; past either end it stays. */
export function nextZoomFactor(current: number, zoom: PageZoom): number {
  if (zoom === "reset") return 1;
  if (zoom === "in") return PAGE_ZOOM_FACTORS.find((step) => step > current + ZOOM_EPSILON) ?? current;
  return PAGE_ZOOM_FACTORS.findLast((step) => step < current - ZOOM_EPSILON) ?? current;
}
