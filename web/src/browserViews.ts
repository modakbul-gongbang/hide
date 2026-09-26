// Browser displays in the desktop app (issue 155). The core owns which
// browser displays exist, their URL and the load stamp; the host owns each
// page and what it says (its address as it navigates, its title, whether it
// can go back); this page owns where each page sits, since only it has the
// geometry.
//
// Everything that moves a page goes through one sync: the front Workspace's
// browser displays, each with the rect its slot occupies now or null. A
// shell overlay (the palette, a menu, a dialog, a popover, a tab drag) that
// meets a page's rect cannot draw over a native view, so the page is
// captured, its still is drawn in its place, and the page is hidden until
// the overlay is gone.

import { create } from "zustand";
import { browserBridge, type BrowserBridge, type BrowserHostEvent, type BrowserPageState, type BrowserPlacement, type BrowserRect, type BrowserSync } from "./host";
import type { ViewDisplaySnapshot, ViewLayoutSnapshot } from "./snapshot";
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

/**
 * The page state the core should record for a display, or null when it
 * already holds it: the address the page moved to, and its title once it
 * has one. `sent` is the last report still waiting for the core's echo.
 */
export function stateReport(
  core: Pick<ViewDisplaySnapshot, "url" | "title">,
  page: BrowserPageState,
  sent: { url: string; title: string } | null,
): { url: string; title: string } | null {
  const url = page.url || core.url || "";
  const title = page.title || core.title || "";
  if (!url) return null;
  if (url === (core.url ?? "") && title === (core.title ?? "")) return null;
  if (sent && sent.url === url && sent.title === title) return null;
  return { url, title };
}

/** Whether Explorer offers Open in Browser for a file. */
export function isHtmlFile(path: string): boolean {
  return /\.x?html?$/i.test(path);
}

/**
 * Why Explorer's Open in Browser cannot run for a file on `device`, or null.
 * A page loads on this Mac, and a device's file is not here.
 */
export function browserOpenUnavailable(device: string): string | null {
  return device === "local" ? null : "Pages load on this Mac, so a file on a device cannot be opened as one";
}

/** How the host places each display: its rect, and whether the page itself shows there. */
export function placements(rows: BrowserDisplayRow[], rects: ReadonlyMap<string, BrowserRect>, hidden: ReadonlySet<string>): BrowserPlacement[] {
  return rows.map((row) => {
    const rect = rects.get(row.id) ?? null;
    return { id: row.id, url: row.url, load: row.load, rect, visible: rect !== null && !hidden.has(row.id) };
  });
}

// --- page state the host reports -----------------------------------------------

type Freeze = "capturing" | "frozen";

type BrowserStore = {
  /** What each page says, by host key (`hostKey`). */
  pages: Record<string, BrowserPageState>;
  /** The still drawn in place of a covered page, by display id of the front Workspace. */
  stills: Record<string, string | null>;
};

export const useBrowserStore = create<BrowserStore>(() => ({ pages: {}, stills: {} }));

export function hostKey(workspace: string, id: string): string {
  return `${workspace}\u0001${id}`;
}

// --- the sync loop -------------------------------------------------------------

/** Where a shell layer is drawn; a tooltip is only read and stays under a page. */
const OVERLAY_SELECTOR = [
  "[data-radix-popper-content-wrapper]",
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[data-slot$="-overlay"]',
  "[data-view-drop]",
  "[data-view-drag-tab]",
].join(",");

function overlayRects(): BrowserRect[] {
  const rects: BrowserRect[] = [];
  for (const element of document.querySelectorAll<HTMLElement>(OVERLAY_SELECTOR)) {
    if (element.querySelector('[data-slot="tooltip-content"]')) continue;
    const box = element.getBoundingClientRect();
    if (box.width > 0 && box.height > 0) rects.push({ x: box.left, y: box.top, width: box.width, height: box.height });
  }
  return rects;
}

type Front = { workspace: string; rows: BrowserDisplayRow[] };

class BrowserSyncLoop {
  private front: Front | null = null;
  private readonly slots = new Map<string, HTMLElement>();
  private readonly freezes = new Map<string, Freeze>();
  private readonly resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => this.schedule());
  private frame = 0;
  private lastSent = "";
  private watching = false;

  constructor(private readonly bridge: BrowserBridge) {}

  setFront(front: Front | null): void {
    if (front?.workspace !== this.front?.workspace) {
      this.freezes.clear();
      useBrowserStore.setState({ stills: {} });
    }
    this.front = front;
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
    // Radix layers mount as children of the body; a tab drag marks the root.
    new MutationObserver(() => this.schedule()).observe(document.body, { childList: true });
    new MutationObserver(() => this.schedule()).observe(document.documentElement, { attributes: true, attributeFilter: ["data-view-drag"] });
    window.addEventListener("resize", () => this.schedule());
  }

  private flush(): void {
    const front = this.front;
    const pages = useBrowserStore.getState().pages;
    const overlays = this.slots.size > 0 ? overlayRects() : [];
    const rects = new Map<string, BrowserRect>();
    const hidden = new Set<string>();
    for (const row of front?.rows ?? []) {
      const element = this.slots.get(row.id);
      if (!element?.isConnected) continue;
      const box = element.getBoundingClientRect();
      if (box.width <= 0 || box.height <= 0) continue;
      const rect = { x: box.left, y: box.top, width: box.width, height: box.height };
      rects.set(row.id, rect);
      if (front && pages[hostKey(front.workspace, row.id)]?.failure) hidden.add(row.id);
      const covered = overlays.some((overlay) => intersects(overlay, rect));
      this.follow(row.id, covered, hidden.has(row.id));
      if (this.freezes.get(row.id) === "frozen") hidden.add(row.id);
    }
    const sync: BrowserSync = front ? { workspace: front.workspace, displays: placements(front.rows, rects, hidden) } : { workspace: null, displays: [] };
    const text = JSON.stringify(sync);
    if (text !== this.lastSent) {
      this.lastSent = text;
      this.bridge.sync(sync);
    }
    // A layer that stays open can still move (a popover following its
    // anchor, a dragged tab); follow it while one is drawn over a page.
    if (overlays.length > 0 && rects.size > 0) this.schedule();
  }

  /** Moves one display's still in step with what covers it. */
  private follow(id: string, covered: boolean, alreadyHidden: boolean): void {
    const state = this.freezes.get(id);
    if (covered && state === undefined) {
      const front = this.front;
      if (!front || alreadyHidden) {
        this.freezes.set(id, "frozen");
        return;
      }
      this.freezes.set(id, "capturing");
      void this.bridge.capture(front.workspace, id).then(
        (still) => this.frozen(front.workspace, id, still),
        () => this.frozen(front.workspace, id, null),
      );
      return;
    }
    if (!covered && state !== undefined) {
      this.freezes.delete(id);
      // The page shows again with this sync; its still goes a frame later, so
      // nothing flashes between the two.
      requestAnimationFrame(() => {
        if (this.freezes.has(id)) return;
        useBrowserStore.setState((current) => {
          if (!(id in current.stills)) return current;
          const stills = { ...current.stills };
          delete stills[id];
          return { stills };
        });
      });
    }
  }

  private frozen(workspace: string, id: string, still: string | null): void {
    if (this.front?.workspace !== workspace || this.freezes.get(id) !== "capturing") return;
    this.freezes.set(id, "frozen");
    useBrowserStore.setState((current) => ({ stills: { ...current.stills, [id]: still } }));
    this.schedule();
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
export function syncBrowserFront(workspace: ViewWorkspace | null, layout: ViewLayoutSnapshot | null | undefined): void {
  syncLoop()?.setFront(workspace ? { workspace: workspaceKey(workspace), rows: browserDisplays(layout) } : null);
}

/** A display's page slot: the host places the page over this element. */
export function registerBrowserSlot(id: string, element: HTMLElement): () => void {
  return syncLoop()?.register(id, element) ?? (() => undefined);
}

/** Records what a page says and places it again, since a failed page gives its place to a notice. */
export function notePageState(event: Extract<BrowserHostEvent, { kind: "state" }>): void {
  useBrowserStore.setState((current) => ({ pages: { ...current.pages, [hostKey(event.workspace, event.id)]: event.state } }));
  loop?.schedule();
}
