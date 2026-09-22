// The shortcut registry: one table, command -> key per host (PRD S2 D-01).
//
// The browser column is what Chrome lets a page claim. Six chords Chrome keeps
// for itself (⌘T ⌘W ⌘⇧T ⌘⇧N ⌃Tab ⌃⇧Tab) moved to the ⌥ family, and ⌘⇧W
// (Chrome's close-window) moved with them (D-06); `moved` marks each so the
// sheet can say so. The Electron column is the Swift chord set and is empty
// until the Electron host exists (TODO: fill from `ShellMenuCommand.swift`
// and `PaneShortcutSettings.swift` when the host lands).
//
// Chords match on `KeyboardEvent.code`, not `key`: macOS turns ⌥-letters into
// dead keys and symbols (⌥T is "†"), and a physical position is what the
// operator's fingers know.

export type Chord = {
  code: string;
  meta?: boolean;
  alt?: boolean;
  shift?: boolean;
  ctrl?: boolean;
};

export type CommandId =
  | "new_tab"
  | "close_tab"
  | "reopen_closed_tab"
  | "new_workspace"
  | "recent_tab"
  | "previous_recent_tab"
  | "recent_project"
  | "previous_recent_project"
  | "search"
  | "open_file"
  | "toggle_left_sidebar"
  | "toggle_sidebar_view"
  | "toggle_right_panel"
  | "project_home"
  | "keep_open"
  | "find_in_pane"
  | "save_file"
  | "move_to_trash"
  | "split_right"
  | "split_down"
  | "toggle_zoom"
  | "close_pane"
  | "text_larger"
  | "text_smaller"
  | "text_reset"
  | "shortcuts";

export type Command = {
  id: CommandId;
  title: string;
  group: "Tabs" | "Navigate" | "Panels" | "Panes" | "Help";
  browser: Chord | null;
  /** The Electron host's chord; every row is null until that host exists. */
  electron: Chord | null;
  /** True when the browser chord differs from the Swift chord because Chrome reserves the original. */
  moved: boolean;
  /** The Swift chord the browser one replaced, for the sheet's note. */
  movedFrom?: string;
  /** Not intercepted at the window: the chord reaches the terminal (⌘⌫ is ^U there) and its shell owner is a later stage. */
  passthrough?: string;
};

export const REGISTRY: readonly Command[] = [
  { id: "new_tab", title: "New tab", group: "Tabs", browser: { code: "KeyT", alt: true }, electron: null, moved: true, movedFrom: "⌘T" },
  { id: "close_tab", title: "Close tab", group: "Tabs", browser: { code: "KeyW", alt: true }, electron: null, moved: true, movedFrom: "⌘W" },
  { id: "reopen_closed_tab", title: "Reopen closed tab", group: "Tabs", browser: { code: "KeyT", alt: true, shift: true }, electron: null, moved: true, movedFrom: "⌘⇧T" },
  { id: "recent_tab", title: "Next recent tab", group: "Tabs", browser: { code: "Backquote", alt: true }, electron: null, moved: true, movedFrom: "⌃Tab" },
  { id: "previous_recent_tab", title: "Previous recent tab", group: "Tabs", browser: { code: "Backquote", alt: true, shift: true }, electron: null, moved: true, movedFrom: "⌃⇧Tab" },
  { id: "new_workspace", title: "New workspace", group: "Navigate", browser: { code: "KeyN", alt: true, shift: true }, electron: null, moved: true, movedFrom: "⌘⇧N" },
  { id: "recent_project", title: "Next recent project", group: "Navigate", browser: { code: "Tab", alt: true }, electron: null, moved: false },
  { id: "previous_recent_project", title: "Previous recent project", group: "Navigate", browser: { code: "Tab", alt: true, shift: true }, electron: null, moved: false },
  { id: "search", title: "Search", group: "Navigate", browser: { code: "KeyK", meta: true }, electron: null, moved: false },
  { id: "open_file", title: "Open file", group: "Navigate", browser: { code: "KeyP", meta: true }, electron: null, moved: false },
  { id: "project_home", title: "Project home", group: "Navigate", browser: { code: "KeyH", meta: true, shift: true }, electron: null, moved: false },
  { id: "toggle_left_sidebar", title: "Toggle left sidebar", group: "Panels", browser: { code: "KeyB", meta: true }, electron: null, moved: false },
  { id: "toggle_sidebar_view", title: "Toggle sidebar view", group: "Panels", browser: { code: "KeyE", meta: true }, electron: null, moved: false },
  { id: "toggle_right_panel", title: "Toggle right panel", group: "Panels", browser: { code: "KeyB", meta: true, shift: true }, electron: null, moved: false },
  { id: "find_in_pane", title: "Find in pane", group: "Panes", browser: { code: "KeyF", meta: true }, electron: null, moved: false },
  { id: "save_file", title: "Save file", group: "Panes", browser: { code: "KeyS", meta: true }, electron: null, moved: false },
  { id: "keep_open", title: "Keep open", group: "Panes", browser: { code: "KeyK", meta: true, shift: true }, electron: null, moved: false },
  { id: "split_right", title: "Split right", group: "Panes", browser: { code: "KeyD", meta: true }, electron: null, moved: false },
  { id: "split_down", title: "Split down", group: "Panes", browser: { code: "KeyD", meta: true, shift: true }, electron: null, moved: false },
  { id: "toggle_zoom", title: "Zoom pane", group: "Panes", browser: { code: "Enter", meta: true, alt: true }, electron: null, moved: false },
  { id: "close_pane", title: "Close pane", group: "Panes", browser: { code: "KeyW", alt: true, shift: true }, electron: null, moved: true, movedFrom: "⌘⇧W" },
  { id: "text_larger", title: "Larger text", group: "Panes", browser: { code: "Equal", meta: true }, electron: null, moved: false },
  { id: "text_smaller", title: "Smaller text", group: "Panes", browser: { code: "Minus", meta: true }, electron: null, moved: false },
  { id: "text_reset", title: "Reset text size", group: "Panes", browser: { code: "Digit0", meta: true }, electron: null, moved: false },
  { id: "move_to_trash", title: "Move to Trash", group: "Panes", browser: { code: "Backspace", meta: true }, electron: null, moved: false, passthrough: "Explorer only; in a terminal ⌘⌫ clears the line" },
  { id: "shortcuts", title: "Keyboard shortcuts", group: "Help", browser: { code: "Slash", meta: true }, electron: null, moved: false },
];

/** Chords Chrome never hands to a page; a browser chord using one is a registry error. */
const CHROME_RESERVED: readonly Chord[] = [
  { code: "KeyT", meta: true },
  { code: "KeyW", meta: true },
  { code: "KeyT", meta: true, shift: true },
  { code: "KeyN", meta: true, shift: true },
  { code: "KeyW", meta: true, shift: true },
  { code: "Tab", ctrl: true },
  { code: "Tab", ctrl: true, shift: true },
];

export function chordEquals(a: Chord, b: Chord): boolean {
  return (
    a.code === b.code &&
    !!a.meta === !!b.meta &&
    !!a.alt === !!b.alt &&
    !!a.shift === !!b.shift &&
    !!a.ctrl === !!b.ctrl
  );
}

export function isChromeReserved(chord: Chord): boolean {
  return CHROME_RESERVED.some((reserved) => chordEquals(reserved, chord));
}

/** The command a keydown names on the browser host, or null. */
export function matchBrowser(event: Pick<KeyboardEvent, "code" | "metaKey" | "altKey" | "shiftKey" | "ctrlKey">): Command | null {
  const chord: Chord = { code: event.code, meta: event.metaKey, alt: event.altKey, shift: event.shiftKey, ctrl: event.ctrlKey };
  return REGISTRY.find((command) => command.browser && !command.passthrough && chordEquals(command.browser, chord)) ?? null;
}

const CODE_GLYPHS: Record<string, string> = {
  Backquote: "`",
  Tab: "⇥",
  Enter: "↩",
  Equal: "=",
  Minus: "-",
  Slash: "/",
  Backspace: "⌫",
};

/** The chord as the operator reads it, in macOS modifier order. */
export function displayChord(chord: Chord): string {
  const key =
    CODE_GLYPHS[chord.code] ??
    (chord.code.startsWith("Key") ? chord.code.slice(3) : chord.code.startsWith("Digit") ? chord.code.slice(5) : chord.code);
  return `${chord.ctrl ? "⌃" : ""}${chord.alt ? "⌥" : ""}${chord.shift ? "⇧" : ""}${chord.meta ? "⌘" : ""}${key}`;
}

export function displayBrowser(id: CommandId): string {
  const command = REGISTRY.find((row) => row.id === id);
  return command?.browser ? displayChord(command.browser) : "";
}
