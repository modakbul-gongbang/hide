// Which host runs the shell: a browser tab, or the desktop app
// (`desktop/`), whose preload exposes `window.hideHost` and nothing else.
// The bridge carries the app menu's commands in, and places the pages of
// browser displays (issue 155); the shell never reaches the host any other
// way (desktop PRD B11).

export type HostKind = "browser" | "electron";

/** A browser display's place in the window, in the shell's CSS pixels. */
export type BrowserRect = { x: number; y: number; width: number; height: number };

/** One browser display of the front Workspace as the host places it. */
export type BrowserPlacement = {
  /** The display id; unique within `BrowserSync.workspace` only. */
  id: string;
  url: string;
  /** The core's load stamp: a larger one than the page last loaded loads `url` again. */
  load: number;
  /** Where its page shows; null while it is not on screen, which keeps the page alive but hidden. */
  rect: BrowserRect | null;
  /** False while a still or a notice stands in its place. */
  visible: boolean;
};

/**
 * Every browser display of the front Workspace. The host closes a page of
 * that Workspace the list no longer names, hides the pages of any other,
 * and creates a page only once its display is on screen.
 */
export type BrowserSync = { workspace: string | null; displays: BrowserPlacement[] };

export type BrowserCommand = "back" | "forward" | "reload" | "stop";

/** What the page itself says, which only the host can read. */
export type BrowserPageState = {
  url: string;
  title: string;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  /** Why the last load failed or the page stopped; null while it shows. */
  failure: string | null;
};

export type BrowserHostEvent =
  | { kind: "state"; workspace: string; id: string; state: BrowserPageState }
  /** The page asked for a new window: it becomes another browser display. */
  | { kind: "open"; workspace: string; id: string; url: string }
  /** The operator clicked or tabbed into the page. */
  | { kind: "focus"; workspace: string; id: string };

export type BrowserBridge = {
  sync(state: BrowserSync): void;
  /** A still of the page as a data URL, or null when it has none to give. */
  capture(workspace: string, id: string): Promise<string | null>;
  command(workspace: string, id: string, command: BrowserCommand): void;
  onEvent(listener: (event: BrowserHostEvent) => void): () => void;
};

export type HostBridge = {
  kind: "electron";
  /** Delivers each app-menu command id; returns the unsubscribe. */
  onCommand(listener: (id: string) => void): () => void;
  browser: BrowserBridge;
};

declare global {
  interface Window {
    hideHost?: HostBridge;
  }
}

export function hostBridge(): HostBridge | null {
  return typeof window !== "undefined" && window.hideHost?.kind === "electron" ? window.hideHost : null;
}

export function hostKind(): HostKind {
  return hostBridge() ? "electron" : "browser";
}

/** The host's browser views, or null in a plain browser tab, which cannot draw a page in a View area. */
export function browserBridge(): BrowserBridge | null {
  return hostBridge()?.browser ?? null;
}
