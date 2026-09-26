// Browser displays (issue 155): one WebContentsView per browser display the
// shell has shown, placed over the rect the shell reports through the
// hideHost bridge. The core owns which displays exist and the URL each was
// asked to load; this owns the pages. A page lives in one persistent session
// partition with no preload and no Node, so nothing in it can reach the
// shell's bridge or the daemon's token.
//
//   shown      -> created on first show, then placed and made visible
//   not shown  -> hidden, still alive; a move between areas never reloads
//   closed     -> the front Workspace's list no longer names it: destroyed,
//                 which ends its renderer process
//
// A hidden page past `MAX_LIVE_VIEWS` is closed and loads again when shown.

import { BrowserWindow, ipcMain, session, shell, WebContentsView, type IpcMainEvent, type IpcMainInvokeEvent, type Session } from "electron";
import type { BrowserHostEvent, BrowserPageState, BrowserPlacement } from "../../../web/src/host";
import { BROWSER_CAPTURE_CHANNEL, BROWSER_COMMAND_CHANNEL, BROWSER_EVENT_CHANNEL, BROWSER_SYNC_CHANNEL } from "../channel";
import { loadable, MAX_LIVE_VIEWS, overCap, parseCommand, parseSync, parseTarget, toBounds, viewKey } from "./browserSync";
import type { HostLog } from "./log";

export const BROWSER_PARTITION = "persist:hide-browser";
/** Page state reports coalesce to one per view in this window, so a title ticking every frame costs one event. */
const REPORT_COALESCE_MS = 100;
/** Chromium's code for a load a newer one replaced; not a failure. */
const ERR_ABORTED = -3;
const PAGE_PERMISSIONS = new Set(["clipboard-sanitized-write"]);

type Page = {
  key: string;
  workspace: string;
  id: string;
  view: WebContentsView;
  /** The load stamp this page last loaded. */
  applied: number;
  visible: boolean;
  shownAt: number;
  state: BrowserPageState;
  report: NodeJS.Timeout | null;
};

export class BrowserViews {
  private readonly pages = new Map<string, Page>();
  private readonly session: Session;
  private window: BrowserWindow | null = null;

  constructor(
    private readonly log: HostLog,
    /** Whether an IPC message came from the shell the daemon serves, not the status page or a page here. */
    private readonly trusted: (event: IpcMainEvent | IpcMainInvokeEvent) => boolean,
  ) {
    this.session = session.fromPartition(BROWSER_PARTITION);
    // A page gets no permission but writing the clipboard; a prompt it
    // would raise has nowhere to show in a View area.
    this.session.setPermissionRequestHandler((_contents, permission, callback) => callback(PAGE_PERMISSIONS.has(permission)));
    this.session.setPermissionCheckHandler((_contents, permission) => PAGE_PERMISSIONS.has(permission));
    this.session.on("will-download", (_event, item) => this.log.event("browser.download", { mime: item.getMimeType() }));
    ipcMain.on(BROWSER_SYNC_CHANNEL, (event, value: unknown) => {
      if (!this.trusted(event)) return this.log.event("browser.ipc_refused", { channel: "sync" });
      const sync = parseSync(value);
      if (!sync) return this.log.event("browser.sync_invalid", {});
      this.sync(sync.workspace, sync.displays);
    });
    ipcMain.handle(BROWSER_CAPTURE_CHANNEL, async (event, value: unknown) => {
      if (!this.trusted(event)) return null;
      const target = parseTarget(value);
      const page = target ? this.pages.get(viewKey(target.workspace, target.id)) : undefined;
      if (!page || !page.visible) return null;
      try {
        const image = await page.view.webContents.capturePage();
        return image.isEmpty() ? null : image.toDataURL();
      } catch (error) {
        this.log.event("browser.capture_failed", { detail: String(error) });
        return null;
      }
    });
    ipcMain.on(BROWSER_COMMAND_CHANNEL, (event, value: unknown, name: unknown) => {
      if (!this.trusted(event)) return this.log.event("browser.ipc_refused", { channel: "command" });
      const target = parseTarget(value);
      const command = parseCommand(name);
      const page = target ? this.pages.get(viewKey(target.workspace, target.id)) : undefined;
      if (!page || !command) return;
      const contents = page.view.webContents;
      if (command === "back" && contents.navigationHistory.canGoBack()) contents.navigationHistory.goBack();
      else if (command === "forward" && contents.navigationHistory.canGoForward()) contents.navigationHistory.goForward();
      else if (command === "stop") contents.stop();
      else if (command === "reload") {
        // A page that failed to load has nothing to reload; its address loads again.
        if (page.state.failure) this.load(page, page.state.url);
        else contents.reload();
      }
    });
  }

  /** The window the pages are drawn in. A new window starts with no pages. */
  attach(window: BrowserWindow): void {
    this.window = window;
    // A shell that loads again, or moves to another daemon, has not placed
    // anything yet: its pages hide until it says where they go.
    window.webContents.on("did-start-navigation", (details) => {
      if (details.isMainFrame && !details.isSameDocument) this.hideAll();
    });
    window.on("closed", () => {
      for (const page of [...this.pages.values()]) this.destroy(page, "window_closed");
      if (this.window === window) this.window = null;
    });
  }

  /** Whether a page holds the keyboard, so an app-menu command gives it back to the shell first. */
  hasFocus(): boolean {
    for (const page of this.pages.values()) if (page.visible && page.view.webContents.isFocused()) return true;
    return false;
  }

  private sync(workspace: string | null, displays: BrowserPlacement[]): void {
    const window = this.window;
    if (!window) return;
    const listed = new Set(displays.map((display) => viewKey(workspace ?? "", display.id)));
    for (const page of [...this.pages.values()]) {
      if (page.workspace === workspace && !listed.has(page.key)) this.destroy(page, "closed");
      else if (page.workspace !== workspace) this.show(page, false);
    }
    const now = Date.now();
    const zoom = window.webContents.getZoomFactor();
    for (const display of displays) {
      const key = viewKey(workspace ?? "", display.id);
      let page = this.pages.get(key);
      if (!display.rect) {
        if (page) this.show(page, false);
        continue;
      }
      if (!page) page = this.create(window, workspace ?? "", display);
      else if (display.load > page.applied) {
        page.applied = display.load;
        this.load(page, display.url);
      }
      page.view.setBounds(toBounds(display.rect, zoom));
      this.show(page, display.visible);
      page.shownAt = now;
    }
    for (const key of overCap([...this.pages.values()], MAX_LIVE_VIEWS)) {
      const page = this.pages.get(key);
      if (page) this.destroy(page, "evicted");
    }
  }

  private create(window: BrowserWindow, workspace: string, display: BrowserPlacement): Page {
    const view = new WebContentsView({
      webPreferences: { session: this.session, sandbox: true, contextIsolation: true, nodeIntegration: false, webSecurity: true },
    });
    const page: Page = {
      key: viewKey(workspace, display.id),
      workspace,
      id: display.id,
      view,
      applied: display.load,
      visible: false,
      shownAt: 0,
      state: { url: display.url, title: "", loading: true, canGoBack: false, canGoForward: false, failure: null },
      report: null,
    };
    view.setVisible(false);
    window.contentView.addChildView(view);
    this.pages.set(page.key, page);
    this.watch(page);
    this.load(page, display.url);
    this.log.event("browser.view_created", { live: this.pages.size });
    return page;
  }

  private load(page: Page, url: string): void {
    if (!loadable(url)) {
      this.log.event("browser.load_refused", { protocol: protocolOf(url) });
      this.update(page, { failure: "This address cannot be shown here" });
      return;
    }
    page.view.webContents.loadURL(url).catch((error: unknown) => {
      // did-fail-load reports it; a load a newer one replaced is not a failure.
      const code = (error as { errno?: number }).errno;
      if (code !== ERR_ABORTED) this.log.event("browser.load_failed", { code });
    });
  }

  private watch(page: Page): void {
    const contents = page.view.webContents;
    const history = () => ({ canGoBack: contents.navigationHistory.canGoBack(), canGoForward: contents.navigationHistory.canGoForward() });
    contents.on("did-start-loading", () => this.update(page, { loading: true, failure: null }));
    contents.on("did-stop-loading", () => this.update(page, { loading: false, ...history() }));
    contents.on("did-navigate", (_event, url) => this.update(page, { url, ...history() }));
    contents.on("did-navigate-in-page", (_event, url, isMainFrame) => {
      if (isMainFrame) this.update(page, { url, ...history() });
    });
    contents.on("page-title-updated", (_event, title) => this.update(page, { title }));
    contents.on("did-fail-load", (_event, code, description, url, isMainFrame) => {
      if (!isMainFrame || code === ERR_ABORTED) return;
      this.update(page, { url, loading: false, failure: description || `Load failed (${code})` });
    });
    contents.on("render-process-gone", (_event, details) => {
      this.log.event("browser.page_gone", { reason: details.reason });
      this.update(page, { loading: false, failure: `The page stopped (${details.reason})` });
    });
    contents.on("focus", () => this.emit({ kind: "focus", workspace: page.workspace, id: page.id }));
    contents.setWindowOpenHandler(({ url }) => {
      // A new window is another browser display, which the core opens.
      if (loadable(url) && url !== "about:blank") this.emit({ kind: "open", workspace: page.workspace, id: page.id, url });
      else this.log.event("browser.window_open_refused", { protocol: protocolOf(url) });
      return { action: "deny" };
    });
    const guard = (event: { preventDefault(): void }, url: string) => {
      if (loadable(url)) return;
      event.preventDefault();
      const protocol = protocolOf(url);
      if (protocol === "mailto:") shell.openExternal(url).catch(() => undefined);
      this.log.event("browser.navigation_refused", { protocol });
    };
    contents.on("will-navigate", guard);
    contents.on("will-redirect", guard);
  }

  private update(page: Page, change: Partial<BrowserPageState>): void {
    page.state = { ...page.state, ...change };
    if (page.report) return;
    page.report = setTimeout(() => {
      page.report = null;
      if (this.pages.get(page.key) !== page) return;
      this.emit({ kind: "state", workspace: page.workspace, id: page.id, state: page.state });
    }, REPORT_COALESCE_MS);
  }

  private emit(event: BrowserHostEvent): void {
    const contents = this.window?.webContents;
    if (contents && !contents.isDestroyed()) contents.send(BROWSER_EVENT_CHANNEL, event);
  }

  private show(page: Page, visible: boolean): void {
    if (page.visible === visible) return;
    page.visible = visible;
    page.view.setVisible(visible);
  }

  private hideAll(): void {
    for (const page of this.pages.values()) this.show(page, false);
  }

  private destroy(page: Page, reason: "closed" | "evicted" | "window_closed"): void {
    this.pages.delete(page.key);
    if (page.report) clearTimeout(page.report);
    const window = this.window;
    if (window && !window.isDestroyed()) window.contentView.removeChildView(page.view);
    // Closing the contents ends the page's renderer process; a view left
    // alone keeps it running after the view is gone.
    if (!page.view.webContents.isDestroyed()) page.view.webContents.close();
    this.log.event("browser.view_closed", { reason, live: this.pages.size });
  }
}

function protocolOf(url: string): string {
  try {
    return new URL(url).protocol;
  } catch {
    return "unparsable";
  }
}
