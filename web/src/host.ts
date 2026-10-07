// Which host runs the shell: a browser tab, or the desktop app
// (`desktop/`), whose preload exposes `window.hideHost` and nothing else.
// The bridge carries the app menu's commands in, the operator's macOS pane
// chords out, so the menu shows and answers the chords the page runs, the
// host's OS, a file or folder to show in the OS file manager, the paths a terminal link names and handing one
// to macOS, and places the pages of browser displays (issue 155); the shell
// never reaches the host any other way (desktop PRD B11).

import { revealLabel, type RevealHost } from "./revealExternal";
import { holdsFieldModifier, keySystemOf, type KeySystem } from "./shortcuts";

export type HostKind = "browser" | "electron";

/** A browser display's place in the window, in the shell's CSS pixels. */
export type BrowserRect = { x: number; y: number; width: number; height: number };

/** One browser display of the front Workspace as the host places it. */
export type BrowserPlacement = {
  /** The display id; unique within `BrowserSync.workspace` only. */
  id: string;
  /** The authoritative View area containing this display. */
  area_id: string;
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
export type BrowserSync = {
  workspace: string | null;
  displays: BrowserPlacement[];
  /** Core-owned inventory; a hidden native page absent here is closed. */
  retained: { workspace: string; id: string; area_id: string }[];
  /** Changes on controller mount or reconnect so the host replays actual attachment facts once. */
  attachment_epoch?: string;
  /** Positive core area authority, including empty areas; absent grants no debugger access. Incarnation changes on revoke/regrant. */
  authorized_scopes?: { workspace: string; area_id: string; incarnation: number }[];
  /** The core's own node id, whose pages load this computer's loopback directly; absent until the snapshot names it. */
  node?: string;
};

export type BrowserCommand = "back" | "forward" | "reload" | "stop" | "focus";

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
  | { kind: "state"; workspace: string; id: string; load: number; state: BrowserPageState }
  /** Actual debugger ownership, scoped to `device_id + NUL + checkout_path`. */
  | { kind: "attached"; workspace: string; id: string; attached: boolean }
  | { kind: "gone"; workspace: string; id: string; load: number; url: string }
  /** The page opened a tab: it becomes another browser display, beside the page `id`. */
  | { kind: "open"; workspace: string; id: string; url: string }
  /** The operator clicked or tabbed into the page. */
  | { kind: "focus"; workspace: string; id: string }
  | { kind: "cycle-input"; cycleId: number; workspace: string; id: string; type: "keyDown" | "keyUp"; key: string; code: string; control: boolean; alt: boolean; meta: boolean; shift: boolean }
  | { kind: "cycle-cancel"; cycleId: number; workspace: string; id: string };

export type BrowserBridge = {
  sync(state: BrowserSync): void;
  /** The shell ended a held cycle, including a release delivered to the shell after hiding a page. */
  endCycle(cycleId: number): void;
  /** A still of the page as a data URL, or null when it has none to give. */
  capture(workspace: string, id: string): Promise<string | null>;
  command(workspace: string, id: string, command: BrowserCommand): void;
  onEvent(listener: (event: BrowserHostEvent) => void): () => void;
};

/** A path on this computer as the host found it: its physical path in the wire spelling (`/` between names, `C:/...` on Windows) and whether it is a folder; null when it does not exist. */
export type ProbedPath = { real: string; kind: "file" | "directory" } | null;

export type HostBridge = {
  kind: "electron";
  /** The host's OS as Node names it (`process.platform`): what the OS file manager is called. */
  platform: string;
  /** Delivers each app-menu command id; returns the unsubscribe. */
  onCommand(listener: (id: string) => void): () => void;
  /** Hands the host the stored macOS pane chords (`ui_state.shortcut_bindings`) it builds the menu from. */
  reportBindings(bindings: Record<string, string>): void;
  /** Hands the host the core's explicit interface language (`ui_state.interface_language`, null for the system's) once a snapshot confirms it; the app menu and status page are drawn in it. */
  reportLanguage(language: string | null): void;
  /** Shows a file or folder of this computer, named in the wire spelling, in the OS file manager, selected in its parent folder; nothing is opened. */
  revealPath(path: string): void;
  /** The native folder picker, modal to the window; the chosen folder in the wire spelling, or null when the operator cancelled. */
  pickFolder(): Promise<string | null>;
  /** What each absolute (in the wire spelling) or `~/` path names on this computer, in order; at most `MAX_PROBE_PATHS` (64) per call. */
  probePaths(paths: string[]): Promise<ProbedPath[]>;
  /** Hands an absolute path, in the wire spelling, to the system: its default application, a folder window, or a file manager reveal when opening would run it. */
  openPath(path: string): void;
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

/**
 * The keyboard convention of the machine the operator types on: the desktop
 * app's OS, or the browser's. The shell's chords, their glyphs and its
 * field chords all follow it (`shortcuts.ts`).
 */
export function keySystem(): KeySystem {
  const bridge = hostBridge();
  if (bridge) return keySystemOf(bridge.platform);
  if (typeof navigator === "undefined") return "pc";
  const agent = navigator as Navigator & { userAgentData?: { platform?: string } };
  return keySystemOf(agent.userAgentData?.platform || navigator.platform || navigator.userAgent);
}

/**
 * Whether a press or a click holds the system's own command key: ⌘ on macOS,
 * Ctrl elsewhere (`fieldChord`), as ⌘↵ in a palette does and as a click that
 * asks for the operating system rather than the shell does (Ctrl-click is
 * not the context menu off macOS).
 */
export function holdsCommandKey(event: { metaKey: boolean; ctrlKey: boolean }): boolean {
  return holdsFieldModifier(event, keySystem());
}

/** The OS file manager item's host: its label on this OS, or null in a plain browser tab, which has none. */
export function revealHost(): RevealHost {
  const bridge = hostBridge();
  return bridge ? { label: revealLabel(bridge.platform) } : null;
}

export function hostKind(): HostKind {
  return hostBridge() ? "electron" : "browser";
}

/** The host's browser views, or null in a plain browser tab, which cannot draw a page in a View area. */
export function browserBridge(): BrowserBridge | null {
  return hostBridge()?.browser ?? null;
}
