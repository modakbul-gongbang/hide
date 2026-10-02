// Browser displays (issue 155): one WebContentsView per browser display the
// shell has shown, placed over the rect the shell reports through the
// hideHost bridge. The core owns which displays exist and the URL each was
// asked to load; this owns the pages. A page lives in a browser session
// partition with no preload and no Node, so nothing in it can reach the
// shell's bridge or the daemon's token.
//
//   shown      -> created on first show, then placed and made visible
//   not shown  -> hidden, still alive; a move between areas never reloads
//   closed     -> the core's inventory no longer names it: destroyed,
//                 which ends its renderer process
//
// A hidden page past `MAX_LIVE_VIEWS` is closed and loads again when shown.
//
// A sized popup a page on screen opens (a sign-in window) is a real child
// window that keeps its opener, belongs to that page and closes with it; a
// link to another app leaves only after the operator agrees.

import { app, BrowserWindow, dialog, ipcMain, Menu, screen, session, shell, WebContentsView, type BrowserWindowConstructorOptions, type Input, type IpcMainEvent, type IpcMainInvokeEvent, type MenuItem, type Session, type WebContents, type WindowOpenHandlerResponse } from "electron";
import path from "node:path";
import { pathToFileURL } from "node:url";
import type { BrowserHostEvent, BrowserPageState, BrowserPlacement } from "../../../web/src/host";
import { hostChord, isCycleCommand, matchHost, releaseModifier, REGISTRY, type Command, type CommandId } from "../../../web/src/shortcuts";
import { BROWSER_CAPTURE_CHANNEL, BROWSER_COMMAND_CHANNEL, BROWSER_CYCLE_END_CHANNEL, BROWSER_EVENT_CHANNEL, BROWSER_SYNC_CHANNEL } from "../channel";
import { appScheme, browserPartition, isPopup, loadable, MAX_LIVE_VIEWS, MAX_POPUPS, nextZoomFactor, overCap, parseCommand, parseSync, parseTarget, popupBounds, remoteRequest, toBounds, viewKey, type PageZoom } from "./browserSync";
import type { HostLog } from "./log";
import { accelerator } from "./menu";

/** Page state reports coalesce to one per view in this window, so a title ticking every frame costs one event. */
const REPORT_COALESCE_MS = 100;
/** Chromium's code for a load a newer one replaced; not a failure. */
const ERR_ABORTED = -3;
const PAGE_PERMISSIONS = new Set(["clipboard-sanitized-write"]);
/** The text-size commands, which zoom a focused page the way Chrome's ⌘= / ⌘- / ⌘0 do. */
const PAGE_ZOOM_COMMANDS: Readonly<Partial<Record<CommandId, PageZoom>>> = { text_larger: "in", text_smaller: "out", text_reset: "reset" };
/** Chrome's second zoom-in chord, ⌘+ (⌘⇧= on a US keyboard), as an app-menu accelerator. */
const ZOOM_IN_ALIAS = accelerator({ code: "Equal", meta: true, shift: true });
/** Pinch zoom on a page, as visual zoom levels; Electron turns it off by default. */
const PINCH_ZOOM_LIMITS = [1, 3] as const;

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
  route: ResolvedPage | null;
  partition: string;
};

export type ResolvedPage = { url: string; source_url: string; load: number };

/** A popup window a page opened, under the page that owns it (for a popup's own popup, the same page). */
type Popup = { window: BrowserWindow; owner: Page };

export class BrowserViews {
  private readonly pages = new Map<string, Page>();
  /** Popup windows by their contents' id. */
  private readonly popups = new Map<number, Popup>();
  private readonly configuredSessions = new Set<string>();
  private window: BrowserWindow | null = null;
  /** The question open about a link to another app; one at a time. */
  private asking: symbol | null = null;
  /** Contents whose app link the operator declined; they ask again only after their next navigation. */
  private readonly declined = new Set<number>();
  private registry: readonly Command[] = REGISTRY;
  private cycleInput: { page: Page; release: string; cycleId: number } | null = null;
  private cycleSequence = 0;

  setRegistry(registry: readonly Command[]): void { this.registry = registry; }

  private cancelCycle(): void {
    const held = this.cycleInput;
    this.cycleInput = null;
    if (held) this.emit({ kind: "cycle-cancel", cycleId: held.cycleId, workspace: held.page.workspace, id: held.page.id });
  }

  constructor(
    private readonly log: HostLog,
    /** Whether an IPC message came from the shell the daemon serves, not the status page or a page here. */
    private readonly trusted: (event: IpcMainEvent | IpcMainInvokeEvent) => boolean,
    private readonly resolve: (workspace: string, id: string, load: number) => Promise<ResolvedPage>,
    private readonly release: (workspace: string, id: string, load: number) => void,
    /** Shows a popup window the way the host shows its own, so a test run never activates the app. */
    private readonly present: (window: BrowserWindow, focus: boolean) => void,
  ) {
    ipcMain.on(BROWSER_CYCLE_END_CHANNEL, (event, cycleId: unknown) => {
      if (!this.trusted(event)) return this.log.event("browser.ipc_refused", { channel: "cycle_end" });
      if (this.cycleInput?.cycleId === cycleId) this.cycleInput = null;
    });
    ipcMain.on(BROWSER_SYNC_CHANNEL, (event, value: unknown) => {
      if (!this.trusted(event)) return this.log.event("browser.ipc_refused", { channel: "sync" });
      const sync = parseSync(value);
      if (!sync) return this.log.event("browser.sync_invalid", {});
      this.sync(sync.workspace, sync.displays, sync.retained);
    });
    ipcMain.handle(BROWSER_CAPTURE_CHANNEL, async (event, value: unknown) => {
      if (!this.trusted(event)) return null;
      const target = parseTarget(value);
      const page = target ? this.pages.get(viewKey(target.workspace, target.id)) : undefined;
      // A covered page's still: the shell asks while the page still shows, ahead of the sync that hides it.
      if (!page || !page.visible) {
        this.log.event("browser.capture_refused", { id: target?.id ?? null, reason: page ? "hidden" : "missing" });
        return null;
      }
      try {
        const image = await page.view.webContents.capturePage();
        if (!image.isEmpty()) return image.toDataURL();
        this.log.event("browser.capture_empty", { id: page.id });
        return null;
      } catch (error) {
        this.log.event("browser.capture_failed", { id: page.id, detail: String(error) });
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
      if (command === "focus") {
        if (page.visible && this.window?.isFocused()) contents.focus();
      } else if (command === "back" && contents.navigationHistory.canGoBack()) contents.navigationHistory.goBack();
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
    // Once a native page starts a hold, its shell input uses the same IPC
    // route. Even a quick release cannot overtake the forwarded first chord.
    window.webContents.on("before-input-event", (event, input) => this.routeCycleInput(event, input));
    // A shell that loads again, or moves to another daemon, has not placed
    // anything yet: its pages hide until it says where they go.
    window.webContents.on("did-start-navigation", (details) => {
      if (details.isMainFrame && !details.isSameDocument) this.hideAll();
    });
    window.on("blur", () => {
      const held = this.cycleInput;
      this.cancelCycle();
      // The hold moved the native responder to the shell. When the window
      // comes back, the page that started it takes the keyboard again, as
      // it would after Escape, unless it has since been hidden or closed.
      if (held) window.once("focus", () => {
        if (!held.page.visible || this.pages.get(held.page.key) !== held.page) return;
        held.page.view.webContents.focus();
        // One attempt at the moment the window becomes key: whether the page
        // held the keyboard right after it says if the attempt was refused
        // (the page not yet accepting focus) or taken back later by the shell.
        this.log.event("browser.window_return", { page_focused: held.page.view.webContents.isFocused() });
      });
    });
    window.on("closed", () => {
      this.cancelCycle();
      for (const page of [...this.pages.values()]) this.destroy(page, "window_closed");
      if (this.window === window) this.window = null;
    });
  }

  /** Whether a page holds the keyboard, so an app-menu command gives it back to the shell first. */
  hasFocus(): boolean {
    return this.focused() !== undefined;
  }

  /**
   * A text-size command while a page holds the keyboard zooms that page and
   * leaves it the keyboard; any other command, or no focused page, is not
   * handled here. Routed by command, so a rebound chord zooms too.
   */
  zoomFocused(command: CommandId): boolean {
    const zoom = PAGE_ZOOM_COMMANDS[command];
    const page = zoom && this.focused();
    if (!zoom || !page) return false;
    this.zoom(page, zoom);
    return true;
  }

  private focused(): Page | undefined {
    for (const page of this.pages.values()) if (page.visible && page.view.webContents.isFocused()) return page;
    return undefined;
  }

  private zoom(page: Page, zoom: PageZoom): void {
    zoomContents(page.view.webContents, zoom);
  }

  /**
   * An app-menu command while a popup window holds the keyboard: Close closes
   * the popup, text size zooms it, and nothing else reaches the shell behind
   * it, so ⌘W in a sign-in window never closes one of the operator's views.
   */
  popupCommand(command: CommandId): boolean {
    const focused = BrowserWindow.getFocusedWindow();
    const popup = focused ? [...this.popups.values()].find((row) => row.window === focused) : undefined;
    if (!popup) return false;
    const zoom = PAGE_ZOOM_COMMANDS[command];
    if (command === "close_tab" || command === "close_pane") popup.window.close();
    else if (zoom) zoomContents(popup.window.webContents, zoom);
    return true;
  }

  private sync(workspace: string | null, displays: BrowserPlacement[], retained: { workspace: string; id: string }[]): void {
    const window = this.window;
    if (!window) return;
    const listed = new Set(retained.map((row) => viewKey(row.workspace, row.id)));
    for (const page of [...this.pages.values()]) {
      if (!listed.has(page.key)) this.destroy(page, "closed");
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
      else if (page.partition !== browserPartition(page.workspace, display.url)) {
        this.destroy(page, "closed");
        page = this.create(window, workspace ?? "", display);
      } else if (display.load > page.applied) {
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
    const partition = browserPartition(workspace, display.url);
    const view = new WebContentsView({
      webPreferences: { session: this.sessionFor(partition), sandbox: true, contextIsolation: true, nodeIntegration: false, webSecurity: true },
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
      route: null,
      partition,
    };
    view.setVisible(false);
    window.contentView.addChildView(view);
    this.pages.set(page.key, page);
    this.watch(page);
    this.load(page, display.url);
    this.log.event("browser.view_created", { live: this.pages.size });
    return page;
  }

  private sessionFor(partition: string): Session {
    const pageSession = session.fromPartition(partition);
    if (this.configuredSessions.has(partition)) return pageSession;
    this.configuredSessions.add(partition);
    pageSession.setPermissionRequestHandler((contents, permission, callback, details) => {
      // A link to another app that no navigation guard saw (a frame's, say)
      // reaches Chromium's external protocol handler, which asks here. The
      // question is hide's own, so Chromium's opener never runs.
      if (permission !== "openExternal") return callback(PAGE_PERMISSIONS.has(permission));
      callback(false);
      const page = this.ownerOf(contents.id);
      const url = "externalURL" in details ? details.externalURL : undefined;
      if (page && url) this.openInApp(page, contents, url, details.requestingUrl);
      else this.log.event("browser.app_link_refused", { reason: "no_page" });
    });
    pageSession.setPermissionCheckHandler((_contents, permission) => PAGE_PERMISSIONS.has(permission));
    pageSession.on("will-download", (_event, item) => this.log.event("browser.download", { mime: item.getMimeType() }));
    pageSession.webRequest.onBeforeRequest({ urls: ["<all_urls>"] }, (details, callback) => {
      const page = this.ownerOf(details.webContentsId);
      const outcome = !page
        ? partition === "persist:hide-browser-web" ? remoteRequest({ source_url: details.url, url: details.url }, details.url) : { cancel: true }
        : !page.route ? { cancel: true }
        : page.workspace.startsWith("local\u0000") ? {}
        : remoteRequest(page.route, details.url);
      callback(outcome);
    });
    return pageSession;
  }

  private load(page: Page, url: string): void {
    if (!loadable(url)) {
      this.log.event("browser.load_refused", { protocol: protocolOf(url) });
      this.update(page, { failure: "This address cannot be shown here" });
      return;
    }
    const stamp = page.applied;
    this.update(page, { url, loading: true, failure: null });
    void this.resolve(page.workspace, page.id, stamp).then((route) => {
      if (this.pages.get(page.key) !== page || page.applied !== stamp || route.load !== stamp || route.source_url !== url) return;
      page.route = route;
      if (!loadable(route.url)) throw new Error("Resolved page address cannot be loaded");
      return page.view.webContents.loadURL(route.url).catch((error: unknown) => {
        // did-fail-load reports it; a load a newer one replaced is not a failure.
        const code = (error as { errno?: number }).errno;
        if (code !== ERR_ABORTED) this.log.event("browser.load_failed", { code });
      });
    }).catch((error: unknown) => {
      if (this.pages.get(page.key) !== page || page.applied !== stamp) return;
      this.log.event("browser.route_failed", { detail: String(error) });
      this.update(page, { url, loading: false, failure: `Page route unavailable: ${String(error)}` });
    });
  }

  private watch(page: Page): void {
    const contents = page.view.webContents;
    const history = () => ({ canGoBack: contents.navigationHistory.canGoBack(), canGoForward: contents.navigationHistory.canGoForward() });
    contents.on("did-start-loading", () => this.update(page, { loading: true, failure: null }));
    contents.on("did-stop-loading", () => this.update(page, { loading: false, ...history() }));
    contents.on("did-navigate", (_event, url) => {
      this.update(page, { url: this.sourceAddress(page, url), ...history() });
      // The limits live in the page's renderer, so each committed document,
      // which may be a new renderer, gets them again.
      void contents.setVisualZoomLevelLimits(...PINCH_ZOOM_LIMITS).catch((error: unknown) => this.log.event("browser.pinch_zoom_failed", { detail: String(error) }));
    });
    contents.on("before-input-event", (event, input) => {
      if (this.routeCycleInput(event, input, page)) return;
      // ⌘+ reaches no menu item (the menu's zoom in is ⌘=), so the page
      // answers it here unless the operator bound that chord to a command.
      if (input.type !== "keyDown" || input.code !== "Equal" || !input.meta || !input.shift || input.alt || input.control) return;
      if (menuHas(Menu.getApplicationMenu()?.items ?? [], ZOOM_IN_ALIAS)) return;
      event.preventDefault();
      this.zoom(page, "in");
    });
    contents.on("did-navigate-in-page", (_event, url, isMainFrame) => {
      if (isMainFrame) this.update(page, { url: this.sourceAddress(page, url), ...history() });
    });
    contents.on("page-title-updated", (_event, title) => this.update(page, { title }));
    contents.on("did-fail-load", (_event, code, description, url, isMainFrame) => {
      if (!isMainFrame || code === ERR_ABORTED) return;
      this.update(page, { url: this.sourceAddress(page, url), loading: false, failure: description || `Load failed (${code})` });
    });
    contents.on("render-process-gone", (_event, details) => {
      if (this.cycleInput?.page === page) this.cancelCycle();
      this.log.event("browser.page_gone", { reason: details.reason });
      this.update(page, { loading: false, failure: `The page stopped (${details.reason})` });
    });
    contents.on("focus", () => {
      if (page.visible) this.emit({ kind: "focus", workspace: page.workspace, id: page.id });
    });
    this.guard(page, contents);
  }

  /** Where the contents of `page`, or of a popup it owns, may go and what may open from them. */
  private guard(page: Page, contents: WebContents): void {
    contents.setWindowOpenHandler(({ url, disposition, features, referrer }) => this.openWindow(page, contents, url, isPopup(disposition, features), referrer.url));
    const forget = () => this.declined.delete(contents.id);
    contents.on("did-navigate", forget);
    contents.once("destroyed", forget);
    const navigation = (event: { preventDefault(): void }, url: string) => {
      if (page.route && isFileAddress(page.route.source_url) && remoteRequest(page.route, url).cancel) {
        event.preventDefault();
        this.log.event("browser.navigation_refused", { reason: "remote_file_boundary" });
        return;
      }
      if (loadable(url)) return;
      event.preventDefault();
      if (appScheme(url)) this.openInApp(page, contents, url);
      else this.log.event("browser.navigation_refused", { protocol: protocolOf(url) });
    };
    contents.on("will-navigate", navigation);
    contents.on("will-redirect", navigation);
  }

  /**
   * A page asking for a new window. A sized popup (`window.open` with window
   * features, the way a sign-in button opens one) is a real window that keeps
   * its opener, so the sign-in can post its result back and close itself;
   * a tab (a `target=_blank` link, a shift-click, `window.open` without
   * features) is another browser display, which the core opens.
   */
  private openWindow(page: Page, contents: WebContents, url: string, popup: boolean, referrer: string): WindowOpenHandlerResponse {
    if (page.route && isFileAddress(page.route.source_url) && remoteRequest(page.route, url).cancel) {
      this.log.event("browser.window_open_refused", { reason: "remote_file_boundary" });
      return { action: "deny" };
    }
    if (!loadable(url)) {
      // A frame's window.open names its frame only through the referrer, when its policy leaves one.
      if (appScheme(url)) this.openInApp(page, contents, url, referrer || undefined);
      else this.log.event("browser.window_open_refused", { protocol: protocolOf(url) });
      return { action: "deny" };
    }
    if (popup) return this.popup(page, contents);
    if (url !== "about:blank") this.emit({ kind: "open", workspace: page.workspace, id: page.id, url: this.sourceAddress(page, url) });
    else this.log.event("browser.window_open_refused", { protocol: "about:" });
    return { action: "deny" };
  }

  /**
   * A popup from a page on screen, at most `MAX_POPUPS` at once. Its window is
   * built here from hide's own options, so of the window's geometry and chrome
   * only a size the page asked for, held to the work area, reaches it: never a
   * frameless, always-on-top, unclosable, modal or off-screen window. It shares its opener's
   * session and, with `outlivesOpener` left false, Electron closes it with its
   * opener, so it ends with its page.
   */
  private popup(page: Page, contents: WebContents): WindowOpenHandlerResponse {
    const parent = this.window;
    if (!parent || parent.isDestroyed()) return { action: "deny" };
    if (!this.shown(page, contents)) {
      this.log.event("browser.popup_refused", { reason: "hidden" });
      return { action: "deny" };
    }
    if (this.popups.size >= MAX_POPUPS) {
      this.log.event("browser.popup_refused", { reason: "cap", live: this.popups.size });
      return { action: "deny" };
    }
    return {
      action: "allow",
      createWindow: (options) => {
        const bounds = parent.getBounds();
        const window = new BrowserWindow({
          ...popupBounds({ width: options.width, height: options.height }, bounds, screen.getDisplayMatching(bounds).workArea),
          parent,
          show: false,
          minimizable: false,
          fullscreenable: false,
          // Electron's preferences for the popup, the opener's with Node, the
          // sandbox and isolation forced, and the contents Chromium made for
          // it: the window must carry them, or the opener is lost.
          webPreferences: options.webPreferences,
          webContents: (options as { webContents?: WebContents }).webContents,
        } as BrowserWindowConstructorOptions);
        this.adopt(page, window, contents.isFocused());
        return window.webContents;
      },
    };
  }

  /** A popup takes the keyboard only from a page that held it, so a page cannot pull focus from the operator's typing. */
  private adopt(owner: Page, window: BrowserWindow, focus: boolean): void {
    const id = window.webContents.id;
    this.popups.set(id, { window, owner });
    window.once("closed", () => {
      this.popups.delete(id);
      this.log.event("browser.popup_closed", { live: this.popups.size });
    });
    this.guard(owner, window.webContents);
    this.present(window, focus);
    this.log.event("browser.popup_opened", { live: this.popups.size });
  }

  /** Whether the contents asking are on screen: a page shown in the front Workspace, or a popup's visible window. */
  private shown(page: Page, contents: WebContents): boolean {
    if (contents.id === page.view.webContents.id) return page.visible;
    return this.popups.get(contents.id)?.window.isVisible() ?? false;
  }

  /** The page a request or a question comes from: a page's own contents, or a popup it owns. */
  private ownerOf(contentsId: number | undefined): Page | undefined {
    if (contentsId === undefined) return undefined;
    for (const page of this.pages.values()) if (page.view.webContents.id === contentsId) return page;
    return this.popups.get(contentsId)?.owner;
  }

  /**
   * A link a page on screen hands to another app on this Mac (`slack:`,
   * `zoommtg:`). It opens there once the operator agrees, the way Chrome asks
   * first, with the asking origin and the link on the question, and Cancel is
   * the default so a stray Return opens nothing; a mail link opens without
   * asking, as it always has. A scheme no app claims, a link from a hidden page
   * or an HTML file preview, one from contents whose last link was declined
   * before they navigated again, and one that arrives while a question is open
   * go no further than the log.
   */
  private openInApp(page: Page, contents: WebContents, url: string, requestingUrl?: string): void {
    const scheme = appScheme(url);
    const refuse = (reason: string) => this.log.event("browser.app_link_refused", { protocol: scheme ?? protocolOf(url), reason });
    if (!scheme) return refuse("not_app_scheme");
    if (page.route && isFileAddress(page.route.source_url)) return refuse("file_page");
    if (!this.shown(page, contents)) return refuse("hidden");
    if (scheme === "mailto:") return void this.handOff(url, scheme);
    const name = app.getApplicationNameForProtocol(url);
    if (!name) return refuse("no_app");
    if (this.declined.has(contents.id)) return refuse("declined");
    if (this.asking) return refuse("asking");
    const sheet = BrowserWindow.fromWebContents(contents) ?? this.window;
    if (!sheet || sheet.isDestroyed()) return refuse("no_window");
    const question = Symbol("app link");
    this.asking = question;
    // A popup that closes under its question never answers it; its window closing does.
    const release = () => { if (this.asking === question) this.asking = null; };
    sheet.once("closed", release);
    const options = {
      type: "question" as const,
      message: `Open ${name}?`,
      detail: `${requester(requestingUrl ?? this.sourceAddress(page, contents.getURL()))} wants to open this link in ${name}.\n\n${shorten(url)}`,
      buttons: [`Open ${name}`, "Cancel"],
      defaultId: 1,
      cancelId: 1,
      noLink: true,
    };
    dialog.showMessageBox(sheet, options)
      .then(({ response }) => {
        if (response === 0) return this.handOff(url, scheme);
        if (!contents.isDestroyed()) this.declined.add(contents.id);
        this.log.event("browser.app_link_declined", { protocol: scheme });
      })
      .catch((error: unknown) => this.log.event("browser.app_link_failed", { protocol: scheme, detail: String(error) }))
      .finally(() => {
        if (!sheet.isDestroyed()) sheet.removeListener("closed", release);
        release();
      });
  }

  /** The link itself never reaches the log: an app link often carries a sign-in code. */
  private async handOff(url: string, scheme: string): Promise<void> {
    try {
      await shell.openExternal(url);
      this.log.event("browser.app_link_opened", { protocol: scheme });
    } catch {
      this.log.event("browser.app_link_failed", { protocol: scheme });
    }
  }

  /** One held origin and one ordered delivery route, independent of overlay coverage. */
  private routeCycleInput(event: { preventDefault(): void }, input: Input, page?: Page): boolean {
    const held = this.cycleInput;
    if (!held && !page) return false;
    const eventLike = { code: input.code, metaKey: input.meta, altKey: input.alt, shiftKey: input.shift, ctrlKey: input.control };
    const command = input.type === "keyDown" && !input.isComposing ? matchHost(eventLike, this.registry, "electron") : null;
    const starts = !held && command && isCycleCommand(command.id) && page?.visible && page.view.webContents.isFocused();
    const continues = held && command && isCycleCommand(command.id);
    const ends = held && ((input.type === "keyUp" && input.key === held.release) || (input.type === "keyDown" && input.key === "Escape"));
    if (!starts && !continues && !ends) {
      // A cycle chord that reached a page and did not start a hold: say which
      // condition refused it, since the operator sees only a chord that did nothing.
      if (!held && command && isCycleCommand(command.id) && page) {
        this.log.event("browser.cycle_start_refused", { page_visible: page.visible, page_focused: page.view.webContents.isFocused(), window_focused: this.window?.isFocused() ?? false });
      }
      return false;
    }
    const chord = command && hostChord(command, "electron");
    const release = chord && releaseModifier(chord);
    if (starts && release && page) {
      this.cycleSequence = (this.cycleSequence + 1) % Number.MAX_SAFE_INTEGER;
      this.cycleInput = { page, release, cycleId: this.cycleSequence };
    }
    const cycle = held ?? this.cycleInput;
    if (!cycle) return false;
    if (ends) this.cycleInput = null;
    event.preventDefault();
    this.emit({ kind: "cycle-input", cycleId: cycle.cycleId, workspace: cycle.page.workspace, id: cycle.page.id, type: input.type as "keyDown" | "keyUp", key: input.key, code: input.code, control: input.control, alt: input.alt, meta: input.meta, shift: input.shift });
    // The overlay can be outside the originating page. Transfer its native
    // response at the start, while its logical owner stays frozen in the shell.
    if (starts && this.window?.isFocused()) this.window.webContents.focus();
    return true;
  }

  private update(page: Page, change: Partial<BrowserPageState>): void {
    page.state = { ...page.state, ...change };
    if (page.report) return;
    page.report = setTimeout(() => {
      page.report = null;
      if (this.pages.get(page.key) !== page) return;
      this.emit({ kind: "state", workspace: page.workspace, id: page.id, load: page.applied, state: page.state });
    }, REPORT_COALESCE_MS);
  }

  private sourceAddress(page: Page, raw: string): string {
    const route = page.route;
    if (!route || route.url === route.source_url) return raw;
    if (raw === route.url) return route.source_url;
    try {
      const address = new URL(raw);
      const local = new URL(route.url);
      const source = new URL(route.source_url);
      if (address.origin !== local.origin) return raw;
      if (source.protocol === "file:") {
        const workspace = page.workspace.split("\u0000");
        const prefix = `/${local.pathname.split("/")[1]}/`;
        if (workspace.length !== 2 || !address.pathname.startsWith(prefix)) return raw;
        return `${pathToFileURL(path.join(workspace[1]!, decodeURIComponent(address.pathname.slice(prefix.length)))).href}${address.search}${address.hash}`;
      }
      address.host = source.host;
      address.protocol = source.protocol;
      return address.href;
    } catch { return raw; }
  }

  private emit(event: BrowserHostEvent): void {
    const contents = this.window?.webContents;
    if (contents && !contents.isDestroyed()) contents.send(BROWSER_EVENT_CHANNEL, event);
  }

  private show(page: Page, visible: boolean): void {
    if (page.visible === visible) return;
    // A hidden WebContentsView is no longer a native keyboard responder.
    // Keep the held cycle's remaining OS input in this same window: the
    // shell preserves its logical page owner and consumes release/Escape.
    page.visible = visible;
    page.view.setVisible(visible);
    if (!visible && this.cycleInput?.page === page && this.window?.isFocused()) this.window.webContents.focus();
  }

  private hideAll(): void {
    for (const page of this.pages.values()) this.show(page, false);
  }

  private destroy(page: Page, reason: "closed" | "evicted" | "window_closed"): void {
    if (reason === "evicted") this.emit({ kind: "gone", workspace: page.workspace, id: page.id, load: page.applied, url: page.state.url });
    if (this.cycleInput?.page === page) this.cancelCycle();
    this.pages.delete(page.key);
    this.release(page.workspace, page.id, page.applied);
    if (page.report) clearTimeout(page.report);
    const window = this.window;
    if (window && !window.isDestroyed()) window.contentView.removeChildView(page.view);
    // Closing the contents ends the page's renderer process; a view left
    // alone keeps it running after the view is gone.
    if (!page.view.webContents.isDestroyed()) page.view.webContents.close();
    this.log.event("browser.view_closed", { reason, live: this.pages.size });
  }
}

function zoomContents(contents: WebContents, zoom: PageZoom): void {
  contents.setZoomFactor(nextZoomFactor(contents.getZoomFactor(), zoom));
}

/** A link as the question shows it, cut to a length a sheet holds. */
function shorten(url: string): string {
  return url.length > 160 ? `${url.slice(0, 159)}…` : url;
}

/** Who is asking, as the origin the operator would recognize, or a page with none. */
function requester(url: string): string {
  try {
    const origin = new URL(url).origin;
    if (origin !== "null") return origin;
  } catch { /* falls through */ }
  return "This page";
}

function protocolOf(url: string): string {
  try {
    return new URL(url).protocol;
  } catch {
    return "unparsable";
  }
}

function menuHas(items: readonly MenuItem[], accelerator: string): boolean {
  return items.some((item) => item.accelerator === accelerator || (item.submenu ? menuHas(item.submenu.items, accelerator) : false));
}

function isFileAddress(url: string): boolean {
  return protocolOf(url) === "file:";
}
