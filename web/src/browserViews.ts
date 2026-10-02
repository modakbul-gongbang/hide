// Browser displays in the desktop app (issue 155). The core owns which
// browser displays exist, their URL and the load stamp; the host owns each
// page and what it says (its address as it navigates, its title, whether it
// can go back); this page owns where each page sits, since only it has the
// geometry.
//
// Everything that moves a page goes through one sync: the front Workspace's
// browser displays, each with the rect its slot occupies now or null. A
// shell overlay (the palette, a menu, a dialog, a popover, the Recent Panels
// or Recent Projects list, the Tools overlay of a narrow window) that meets a
// page's rect cannot draw over a native view, so the page is hidden in the
// same sync that first sees the overlay, the still taken while the page was
// idle is drawn in its place, and a fresh capture, asked for before that sync
// hides the page, replaces it when it arrives. A shell drag (`shellDrag.ts`)
// does the same to every page while it runs, so its guide or preview draws
// over them and its drop lands in the shell, never in a page.

import { create } from "zustand";
import { browserBridge, type BrowserBridge, type BrowserHostEvent, type BrowserPageState, type BrowserPlacement, type BrowserRect, type BrowserSync } from "./host";
import { SHELL_DRAG_ATTRIBUTES, shellDragging } from "./shellDrag";
import type { ViewLayoutSnapshot } from "./snapshot";
import { areasOf, workspaceKey, type ViewWorkspace } from "./viewLayout";

// --- pure rules ---------------------------------------------------------------

/** A browser display as the host needs it. */
export type BrowserDisplayRow = { id: string; url: string; load: number };

/** Every browser display of a layout, in tree order. */
export function browserDisplays(layout: ViewLayoutSnapshot | null | undefined): BrowserDisplayRow[] {
  if (!layout) return [];
  return areasOf(layout.root).flatMap((area) =>
    area.displays.flatMap((display) => (display.kind === "browser" && display.url ? [{ id: display.id, url: display.url, load: display.load ?? 0 }] : [])),
  );
}

export function intersects(a: BrowserRect, b: BrowserRect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

/** The Workspace a host key names; the key is `workspaceKey`'s. */
export function parseWorkspaceKey(key: string): ViewWorkspace | null {
  const at = key.indexOf("\u0000");
  if (at <= 0 || at === key.length - 1) return null;
  return { device_id: key.slice(0, at), path: key.slice(at + 1) };
}

const LOOPBACK = /^(localhost|127(?:\.\d{1,3}){3}|0\.0\.0\.0|\[::1\])(?::\d+)?(?:[/?#]|$)/i;

/**
 * A local file's absolute path as a `file:` URL, escaped the way Chromium
 * writes a file URL's path (plus `%`), which is also the one spelling the
 * daemon writes back after it checks the path.
 */
export function fileUrl(path: string): string {
  // eslint-disable-next-line no-control-regex
  return `file://${path.replace(/[\u0000-\u0020"#%<>?`{}\u007f]|[^\u0000-\u007f]+/gu, encodeURIComponent)}`;
}

/**
 * What the address field's text asks to load, or null for nothing: a URL
 * with a scheme as written, an absolute path as a file, a loopback host over
 * http (a dev server), and any other host over https.
 */
export function addressUrl(input: string): string | null {
  const text = input.trim();
  if (!text) return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(text) && !/^[^/:]+:\d+(?:[/?#]|$)/.test(text)) return text;
  if (text.startsWith("/")) return fileUrl(text);
  if (/\s/.test(text)) return null;
  return `${LOOPBACK.test(text) ? "http" : "https"}://${text}`;
}

/** An address as the toolbar shows it at rest: a web address without its scheme, anything else whole. */
export function addressShown(url: string): string {
  return url.replace(/^https?:\/\//i, "");
}

/**
 * The page state the core should record for a display, or null when it
 * already holds it: the address the page moved to, and its title once it
 * has one. `sent` is the last report still waiting for the core's echo.
 */
/** Whether Explorer offers Open in Browser for a file. */
export function isHtmlFile(path: string): boolean {
  return /\.x?html?$/i.test(path);
}

/** How the host places each display: its rect, and whether the page itself shows there. */
export function placements(rows: BrowserDisplayRow[], rects: ReadonlyMap<string, BrowserRect>, hidden: ReadonlySet<string>): BrowserPlacement[] {
  return rows.map((row) => {
    const rect = rects.get(row.id) ?? null;
    return { id: row.id, url: row.url, load: row.load, rect, visible: rect !== null && !hidden.has(row.id) };
  });
}

// --- page state the host reports -----------------------------------------------

type BrowserStore = {
  /** What each page says, by host key (`hostKey`). */
  pages: Record<string, BrowserPageState>;
  /** The still drawn in place of a covered page, by display id of the front Workspace; null draws nothing. */
  stills: Record<string, string | null>;
};

export const useBrowserStore = create<BrowserStore>(() => ({ pages: {}, stills: {} }));

export function hostKey(workspace: string, id: string): string {
  return `${workspace}\u0001${id}`;
}

/**
 * `entries` without the keys of `workspace`'s displays it no longer holds,
 * or `entries` itself when none has gone. A closed display's page is closed
 * by the host on the same sync, so what it said is dropped with it.
 */
export function withoutClosed<T>(entries: Readonly<Record<string, T>>, workspace: string, ids: ReadonlySet<string>): Readonly<Record<string, T>> {
  const prefix = hostKey(workspace, "");
  const gone = Object.keys(entries).filter((key) => key.startsWith(prefix) && !ids.has(key.slice(prefix.length)));
  if (gone.length === 0) return entries;
  const kept = { ...entries };
  for (const key of gone) delete kept[key];
  return kept;
}

// --- the idle still -------------------------------------------------------------

/** A page's still and what it was taken of: the page's address and its slot's size in whole points. */
export type CachedStill = { url: string; width: number; height: number; still: string | null };

/** How long a shown page must stay unchanged before its still is taken. */
export const STILL_QUIET_MS = 500;

/** Whether `cached` was taken of what a page at `url` in `rect` shows: a still of another address or size is never drawn. */
export function stillFits(cached: CachedStill | undefined, url: string, rect: BrowserRect): cached is CachedStill {
  return cached !== undefined && cached.url === url && cached.width === Math.round(rect.width) && cached.height === Math.round(rect.height);
}

/**
 * Whether a shown, uncovered, loaded page wants its still taken again: it has
 * none of its address and size, or `stale` says its load just finished or it
 * just came into view. Nothing else asks, so an idle page is not captured
 * again and again.
 */
export function stillWanted(cached: CachedStill | undefined, url: string, rect: BrowserRect, stale: boolean): boolean {
  return stale || !stillFits(cached, url, rect);
}

// --- the sync loop -------------------------------------------------------------

/** A shell layer that stays where it opened; the others can move with their anchor and are followed every frame. */
const STILL_LAYER_SELECTOR = "[data-tools-overlay]";

/** Where a shell layer is drawn; a tooltip is only read and stays under a page. */
const OVERLAY_SELECTOR = [
  "[data-radix-popper-content-wrapper]",
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[data-slot$="-overlay"]',
  STILL_LAYER_SELECTOR,
].join(",");

function overlayRects(): { rects: BrowserRect[]; moving: boolean } {
  const rects: BrowserRect[] = [];
  let moving = false;
  for (const element of document.querySelectorAll<HTMLElement>(OVERLAY_SELECTOR)) {
    if (element.querySelector('[data-slot="tooltip-content"]')) continue;
    const box = element.getBoundingClientRect();
    if (box.width <= 0 || box.height <= 0) continue;
    rects.push({ x: box.left, y: box.top, width: box.width, height: box.height });
    if (!element.matches(STILL_LAYER_SELECTOR)) moving = true;
  }
  return { rects, moving };
}

type Front = { workspace: string; rows: BrowserDisplayRow[] };

class BrowserSyncLoop {
  private front: Front | null = null;
  private retained: { workspace: string; id: string }[] = [];
  private readonly slots = new Map<string, HTMLElement>();
  /** Displays hidden behind their still until what covers them is gone. */
  private readonly frozen = new Set<string>();
  /** One still per shown page of the front Workspace. */
  private readonly cache = new Map<string, CachedStill>();
  /** Pages with a capture in flight: at most one each. */
  private readonly capturing = new Set<string>();
  /** Pages whose load just finished or that just came into view. */
  private readonly stale = new Set<string>();
  /** The quiet wait before a page's still is taken, by display id, with what it waits on. */
  private readonly quiet = new Map<string, { taken: string; timer: ReturnType<typeof setTimeout> }>();
  /** Pages whose quiet wait ran out: the next flush takes their still if they still want one. */
  private readonly due = new Map<string, string>();
  /** Whether each page shown last flush was loading, by display id; a page missing here was not shown. */
  private shown = new Map<string, boolean>();
  /** Bumped when the front Workspace changes, so a capture that answers late is dropped. */
  private epoch = 0;
  private visible: BrowserRect[] = [];
  private readonly resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => this.schedule());
  private frame = 0;
  private lastSent = "";
  private watching = false;
  private focus: { workspace: string; id: string } | null = null;

  constructor(private readonly bridge: BrowserBridge) {}

  requestFocus(workspace: string, id: string): void {
    this.focus = { workspace, id };
    this.schedule();
  }

  /** The rects of the pages the last sync showed, in the shell's points. */
  visibleRects(): readonly BrowserRect[] {
    return this.visible;
  }

  setFront(front: Front | null, retained: { workspace: string; id: string }[]): void {
    if (front?.workspace !== this.front?.workspace) {
      this.epoch += 1;
      this.frozen.clear();
      this.capturing.clear();
      this.forget(() => true);
      useBrowserStore.setState({ stills: {} });
    }
    if (front) {
      const ids = new Set(front.rows.map((row) => row.id));
      this.forget((id) => !ids.has(id));
      useBrowserStore.setState((current) => {
        const pages = withoutClosed(current.pages, front.workspace, ids);
        return pages === current.pages ? current : { pages };
      });
    }
    this.front = front;
    this.retained = retained;
    this.schedule();
  }

  register(id: string, element: HTMLElement): () => void {
    this.slots.set(id, element);
    this.resize?.observe(element);
    this.watch();
    this.schedule();
    return () => {
      if (this.slots.get(id) === element) this.slots.delete(id);
      this.resize?.unobserve(element);
      this.schedule();
    };
  }

  schedule(): void {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.flush();
    });
  }

  private watch(): void {
    if (this.watching) return;
    this.watching = true;
    // Radix layers and the Recent cycle mount as children of the body; a
    // shell drag marks the root; the Tools overlay says so itself.
    new MutationObserver(() => this.schedule()).observe(document.body, { childList: true });
    new MutationObserver(() => this.schedule()).observe(document.documentElement, { attributes: true, attributeFilter: [...SHELL_DRAG_ATTRIBUTES] });
    window.addEventListener("resize", () => this.schedule());
  }

  private flush(): void {
    const front = this.front;
    const pages = useBrowserStore.getState().pages;
    const overlays = this.slots.size > 0 ? overlayRects() : { rects: [], moving: false };
    const dragging = this.slots.size > 0 && shellDragging();
    const rects = new Map<string, BrowserRect>();
    const hidden = new Set<string>();
    const shown = new Map<string, boolean>();
    for (const row of front?.rows ?? []) {
      const element = this.slots.get(row.id);
      if (!front || !element?.isConnected) continue;
      const box = element.getBoundingClientRect();
      if (box.width <= 0 || box.height <= 0) continue;
      const rect = { x: box.left, y: box.top, width: box.width, height: box.height };
      rects.set(row.id, rect);
      const page = pages[hostKey(front.workspace, row.id)];
      const url = page?.url || row.url;
      const failed = Boolean(page?.failure);
      if (failed) hidden.add(row.id);
      const covered = dragging || overlays.rects.some((overlay) => intersects(overlay, rect));
      this.follow(front.workspace, row.id, url, rect, covered, failed);
      if (this.frozen.has(row.id)) hidden.add(row.id);
      const loading = page?.loading ?? true;
      if (!this.shown.has(row.id) || (this.shown.get(row.id) && !loading)) this.stale.add(row.id);
      shown.set(row.id, loading);
      if (!covered && !failed && !loading) this.idle(front.workspace, row.id, url, rect);
      else this.unwait(row.id);
    }
    this.shown = shown;
    this.forget((id) => !shown.has(id));
    const sync: BrowserSync = front ? { workspace: front.workspace, displays: placements(front.rows, rects, hidden), retained: this.retained } : { workspace: null, displays: [], retained: this.retained };
    this.visible = sync.displays.flatMap((row) => (row.visible && row.rect ? [row.rect] : []));
    const text = JSON.stringify(sync);
    if (text !== this.lastSent) {
      this.lastSent = text;
      this.bridge.sync(sync);
    }
    const focus = this.focus;
    this.focus = null;
    if (focus?.workspace === sync.workspace && sync.displays.some(row => row.id === focus.id && row.visible && row.rect)) {
      this.bridge.command(focus.workspace, focus.id, "focus");
    }
    // A layer that stays open can still move (a popover following its
    // anchor, a dragged tab); follow it while one is drawn over a page.
    if (overlays.moving && rects.size > 0) this.schedule();
  }

  /**
   * Moves one display's still in step with what covers it. A page newly
   * covered is hidden by the sync this flush sends, its idle still (or
   * nothing) is drawn at once, and a fresh capture is asked for now, ahead of
   * that sync, while the host still shows the page.
   */
  private follow(workspace: string, id: string, url: string, rect: BrowserRect, covered: boolean, failed: boolean): void {
    if (covered && !this.frozen.has(id)) {
      this.frozen.add(id);
      // A failed page's place already holds its notice, which is shell HTML.
      if (failed) return;
      const cached = this.cache.get(id);
      const still = stillFits(cached, url, rect) ? cached.still : null;
      useBrowserStore.setState((current) => ({ stills: { ...current.stills, [id]: still } }));
      this.capture(workspace, id, url, rect);
      return;
    }
    if (!covered && this.frozen.has(id)) {
      this.frozen.delete(id);
      // The page shows again with this sync; its still goes a frame later, so
      // nothing flashes between the two.
      requestAnimationFrame(() => {
        if (this.frozen.has(id)) return;
        useBrowserStore.setState((current) => {
          if (!(id in current.stills)) return current;
          const stills = { ...current.stills };
          delete stills[id];
          return { stills };
        });
      });
    }
  }

  /** Takes a shown, uncovered, loaded page's still once it has been quiet for `STILL_QUIET_MS`, if it wants one. */
  private idle(workspace: string, id: string, url: string, rect: BrowserRect): void {
    if (!stillWanted(this.cache.get(id), url, rect, this.stale.has(id))) return this.unwait(id);
    const taken = `${url}\n${Math.round(rect.width)}x${Math.round(rect.height)}`;
    if (this.due.get(id) === taken) {
      this.due.delete(id);
      this.stale.delete(id);
      this.capture(workspace, id, url, rect);
      return;
    }
    this.due.delete(id);
    if (this.quiet.get(id)?.taken === taken) return;
    this.unwait(id);
    const timer = setTimeout(() => {
      this.quiet.delete(id);
      this.due.set(id, taken);
      this.schedule();
    }, STILL_QUIET_MS);
    this.quiet.set(id, { taken, timer });
  }

  private unwait(id: string): void {
    const waiting = this.quiet.get(id);
    if (waiting) clearTimeout(waiting.timer);
    this.quiet.delete(id);
    this.due.delete(id);
  }

  /** Drops what is kept for the displays `gone` names: their still, their wait and their triggers. */
  private forget(gone: (id: string) => boolean): void {
    for (const id of new Set([...this.cache.keys(), ...this.quiet.keys(), ...this.due.keys(), ...this.stale])) {
      if (!gone(id)) continue;
      this.cache.delete(id);
      this.stale.delete(id);
      this.unwait(id);
    }
  }

  /**
   * Asks the host for one page's still, unless one is already on its way:
   * whichever arrives becomes the page's cached still and, while the page is
   * covered, the still drawn in its place. A capture that fails leaves
   * nothing on screen; the host logs why.
   */
  private capture(workspace: string, id: string, url: string, rect: BrowserRect): void {
    if (this.capturing.has(id)) return;
    this.capturing.add(id);
    const epoch = this.epoch;
    const taken = { url, width: Math.round(rect.width), height: Math.round(rect.height) };
    void this.bridge
      .capture(workspace, id)
      .catch(() => null)
      .then((still) => {
        if (this.epoch !== epoch) return;
        this.capturing.delete(id);
        if (!this.shown.has(id) && !this.frozen.has(id)) return;
        const kept = this.cache.get(id);
        this.cache.set(id, { ...taken, still: still ?? (stillFits(kept, url, rect) ? kept.still : null) });
        if (still && this.frozen.has(id)) useBrowserStore.setState((current) => ({ stills: { ...current.stills, [id]: still } }));
      });
  }
}

let loop: BrowserSyncLoop | null = null;

function syncLoop(): BrowserSyncLoop | null {
  if (loop) return loop;
  const bridge = browserBridge();
  if (!bridge) return null;
  loop = new BrowserSyncLoop(bridge);
  return loop;
}

/** The front Workspace's browser displays, whenever the snapshot changes them. */
export function syncBrowserFront(workspace: ViewWorkspace | null, layout: ViewLayoutSnapshot | null | undefined, inventory: { device_id: string; path: string; view_id: string }[]): void {
  syncLoop()?.setFront(workspace ? { workspace: workspaceKey(workspace), rows: browserDisplays(layout) } : null,
    inventory.map((row) => ({ workspace: workspaceKey(row), id: row.view_id })));
}

/** The rects of the pages the host shows now, in the shell's points; none in a plain browser tab. */
export function visiblePageRects(): readonly BrowserRect[] {
  return loop?.visibleRects() ?? [];
}

/** Places the pages again: a shell layer that mounts outside the body's children says it opened or closed. */
export function noteShellLayer(): void {
  loop?.schedule();
}

/** A display's page slot: the host places the page over this element. */
export function registerBrowserSlot(id: string, element: HTMLElement): () => void {
  return syncLoop()?.register(id, element) ?? (() => undefined);
}

/** One focus intent, delivered after the host has the current visible slots. */
export function focusBrowserDisplay(workspace: string, id: string): void {
  syncLoop()?.requestFocus(workspace, id);
}

/** Records what a page says and places it again, since a failed page gives its place to a notice. */
export function notePageState(event: Extract<BrowserHostEvent, { kind: "state" }>): void {
  useBrowserStore.setState((current) => ({ pages: { ...current.pages, [hostKey(event.workspace, event.id)]: event.state } }));
  loop?.schedule();
}
