// The shortcut registry: one table, command -> key per host (PRD S2 D-01).
//
// The browser column is what Chrome lets a page claim. Six chords Chrome keeps
// for itself (⌘T ⌘W ⌘⇧T ⌘⇧N ⌃Tab ⌃⇧Tab) moved to the ⌥ family, and ⌘⇧W
// (Chrome's close-window) moved with them (D-06); `moved` marks each so the
// sheet can say so. The Electron column is the Swift chord set
// (`ShellMenuCommand.swift`, `PaneShortcutSettings.swift`): the desktop host
// has no browser keeping chords, so the moved ones return to their native
// keys there. Its app menu is built from this same column
// (`desktop/src/main/menu.ts`), so the menu and the sheet cannot disagree.
//
// Chords match on `KeyboardEvent.code`, not `key`: macOS turns ⌥-letters into
// dead keys and symbols (⌥T is "†"), and a physical position is what the
// operator's fingers know.

import type { HostKind } from "./host";

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
  | "settings"
  | "shortcuts";

export type Command = {
  id: CommandId;
  title: string;
  group: "Tabs" | "Navigate" | "Panels" | "Panes" | "Help";
  browser: Chord | null;
  /** The Electron host's chord: the Swift chord, or the browser one where Swift has none. */
  electron: Chord | null;
  /** True when the browser chord differs from the Swift chord because Chrome reserves the original. */
  moved: boolean;
  /** The Swift chord the browser one replaced, for the sheet's note. */
  movedFrom?: string;
  /** Not intercepted at the window: the chord reaches the terminal (⌘⌫ is ^U there) and its shell owner is a later stage. */
  passthrough?: string;
};

export const REGISTRY: readonly Command[] = [
  { id: "new_tab", title: "New tab", group: "Tabs", browser: { code: "KeyT", alt: true }, electron: { code: "KeyT", meta: true }, moved: true, movedFrom: "⌘T" },
  { id: "close_tab", title: "Close tab", group: "Tabs", browser: { code: "KeyW", alt: true }, electron: { code: "KeyW", meta: true }, moved: true, movedFrom: "⌘W" },
  { id: "reopen_closed_tab", title: "Reopen closed tab", group: "Tabs", browser: { code: "KeyT", alt: true, shift: true }, electron: { code: "KeyT", meta: true, shift: true }, moved: true, movedFrom: "⌘⇧T" },
  { id: "recent_tab", title: "Next recent tab", group: "Tabs", browser: { code: "Backquote", alt: true }, electron: { code: "Tab", ctrl: true }, moved: true, movedFrom: "⌃Tab" },
  { id: "previous_recent_tab", title: "Previous recent tab", group: "Tabs", browser: { code: "Backquote", alt: true, shift: true }, electron: { code: "Tab", ctrl: true, shift: true }, moved: true, movedFrom: "⌃⇧Tab" },
  { id: "new_workspace", title: "New workspace", group: "Navigate", browser: { code: "KeyN", alt: true, shift: true }, electron: { code: "KeyN", meta: true, shift: true }, moved: true, movedFrom: "⌘⇧N" },
  { id: "recent_project", title: "Next recent project", group: "Navigate", browser: { code: "Tab", alt: true }, electron: { code: "Tab", alt: true }, moved: false },
  { id: "previous_recent_project", title: "Previous recent project", group: "Navigate", browser: { code: "Tab", alt: true, shift: true }, electron: { code: "Tab", alt: true, shift: true }, moved: false },
  { id: "search", title: "Search", group: "Navigate", browser: { code: "KeyK", meta: true }, electron: { code: "KeyK", meta: true }, moved: false },
  { id: "open_file", title: "Open file", group: "Navigate", browser: { code: "KeyP", meta: true }, electron: { code: "KeyP", meta: true }, moved: false },
  { id: "project_home", title: "Project home", group: "Navigate", browser: { code: "KeyH", meta: true, shift: true }, electron: { code: "KeyH", meta: true, shift: true }, moved: false },
  { id: "toggle_left_sidebar", title: "Toggle left sidebar", group: "Panels", browser: { code: "KeyB", meta: true }, electron: { code: "KeyB", meta: true }, moved: false },
  { id: "toggle_sidebar_view", title: "Toggle sidebar view", group: "Panels", browser: { code: "KeyE", meta: true }, electron: { code: "KeyE", meta: true }, moved: false },
  { id: "toggle_right_panel", title: "Toggle right panel", group: "Panels", browser: { code: "KeyB", meta: true, shift: true }, electron: { code: "KeyB", meta: true, shift: true }, moved: false },
  { id: "find_in_pane", title: "Find in pane", group: "Panes", browser: { code: "KeyF", meta: true }, electron: { code: "KeyF", meta: true }, moved: false },
  { id: "save_file", title: "Save file", group: "Panes", browser: { code: "KeyS", meta: true }, electron: { code: "KeyS", meta: true }, moved: false },
  { id: "keep_open", title: "Keep open", group: "Panes", browser: { code: "KeyK", meta: true, shift: true }, electron: { code: "KeyK", meta: true, shift: true }, moved: false },
  { id: "split_right", title: "Split right", group: "Panes", browser: { code: "KeyD", meta: true }, electron: { code: "KeyD", meta: true }, moved: false },
  { id: "split_down", title: "Split down", group: "Panes", browser: { code: "KeyD", meta: true, shift: true }, electron: { code: "KeyD", meta: true, shift: true }, moved: false },
  { id: "toggle_zoom", title: "Zoom pane", group: "Panes", browser: { code: "Enter", meta: true, alt: true }, electron: { code: "Enter", meta: true, alt: true }, moved: false },
  { id: "close_pane", title: "Close pane", group: "Panes", browser: { code: "KeyW", alt: true, shift: true }, electron: { code: "KeyW", meta: true, shift: true }, moved: true, movedFrom: "⌘⇧W" },
  { id: "text_larger", title: "Larger text", group: "Panes", browser: { code: "Equal", meta: true }, electron: { code: "Equal", meta: true }, moved: false },
  { id: "text_smaller", title: "Smaller text", group: "Panes", browser: { code: "Minus", meta: true }, electron: { code: "Minus", meta: true }, moved: false },
  { id: "text_reset", title: "Reset text size", group: "Panes", browser: { code: "Digit0", meta: true }, electron: { code: "Digit0", meta: true }, moved: false },
  { id: "move_to_trash", title: "Move to Trash", group: "Panes", browser: { code: "Backspace", meta: true }, electron: { code: "Backspace", meta: true }, moved: false, passthrough: "Explorer only; in a terminal ⌘⌫ clears the line" },
  { id: "settings", title: "Settings", group: "Help", browser: { code: "Comma", alt: true }, electron: { code: "Comma", meta: true }, moved: true, movedFrom: "⌘," },
  { id: "shortcuts", title: "Keyboard shortcuts", group: "Help", browser: { code: "Slash", meta: true }, electron: { code: "Slash", meta: true }, moved: false },
];

/** Chords Chrome or macOS never hands to a page; a browser chord using one is a registry error. */
const CHROME_RESERVED: readonly Chord[] = [
  { code: "KeyT", meta: true },
  { code: "KeyW", meta: true },
  { code: "KeyT", meta: true, shift: true },
  { code: "KeyN", meta: true, shift: true },
  { code: "KeyW", meta: true, shift: true },
  { code: "Tab", ctrl: true },
  { code: "Tab", ctrl: true, shift: true },
  { code: "KeyN", meta: true },
  { code: "KeyQ", meta: true },
  { code: "KeyH", meta: true },
  { code: "KeyM", meta: true },
  { code: "Comma", meta: true },
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

type KeyEventLike = Pick<KeyboardEvent, "code" | "metaKey" | "altKey" | "shiftKey" | "ctrlKey">;

export function chordFromEvent(event: KeyEventLike): Chord {
  return { code: event.code, meta: event.metaKey, alt: event.altKey, shift: event.shiftKey, ctrl: event.ctrlKey };
}

/** A command's chord on `host`. */
export function hostChord(command: Command, host: HostKind): Chord | null {
  return host === "electron" ? command.electron : command.browser;
}

/** The command a keydown names on `host`, or null. */
export function matchHost(event: KeyEventLike, registry: readonly Command[], host: HostKind): Command | null {
  const chord = chordFromEvent(event);
  return (
    registry.find((command) => {
      const bound = hostChord(command, host);
      return bound && !command.passthrough && chordEquals(bound, chord);
    }) ?? null
  );
}

// The pane commands an operator may rebind on the browser host (PRD S5 D-07):
// the eight Swift pane commands less Toggle Conversation, which the web shell
// has no surface for. Their overrides live in the core's
// `ui_state.browser_shortcut_bindings`, apart from the Swift host's own map.
export const EDITABLE_PANE_COMMANDS: readonly CommandId[] = [
  "split_right",
  "split_down",
  "toggle_zoom",
  "close_pane",
  "text_larger",
  "text_smaller",
  "text_reset",
];

// A physical key a chord may use. Anything else (a dead key, an IME process
// key, a lone modifier) is not a chord this registry can match reliably.
const BINDABLE_CODE = /^(Key[A-Z]|Digit[0-9]|Enter|Backquote|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash|Arrow(Up|Down|Left|Right))$/;

/** One stored chord: its modifiers in a fixed order, then the physical key. */
export function serializeChord(chord: Chord): string {
  return [chord.ctrl && "ctrl", chord.alt && "alt", chord.shift && "shift", chord.meta && "meta", chord.code].filter(Boolean).join("+");
}

export function parseChord(text: string): Chord | null {
  const parts = text.split("+");
  const code = parts.pop() ?? "";
  if (!BINDABLE_CODE.test(code)) return null;
  const chord: Chord = { code };
  for (const part of parts) {
    if (part !== "ctrl" && part !== "alt" && part !== "shift" && part !== "meta") return null;
    const key = part === "ctrl" ? "ctrl" : part === "alt" ? "alt" : part === "shift" ? "shift" : "meta";
    if (chord[key]) return null;
    chord[key] = true;
  }
  return chord;
}

/**
 * Why `chord` cannot become `id`'s binding in `registry`, or null when it can.
 * The same rules decide a stored override on load, so a chord the editor
 * refuses can never become effective by being written to the core directly.
 */
export function bindingProblem(id: CommandId, chord: Chord, registry: readonly Command[]): string | null {
  if (!BINDABLE_CODE.test(chord.code)) return "Use a letter, a digit, Return, an arrow or a punctuation key.";
  if (!chord.meta && !chord.ctrl && !chord.alt) return "Include ⌘, ⌥ or ⌃ so typing in a terminal stays typing.";
  if (isChromeReserved(chord)) return `${displayChord(chord)} is kept by Chrome or macOS and never reaches the page.`;
  const taken = registry.find((command) => command.id !== id && command.browser && chordEquals(command.browser, chord));
  if (taken) return `${displayChord(chord)} is already ${taken.title}.`;
  return null;
}

export type EffectiveRegistry = { registry: readonly Command[]; diagnostic: string | null };

/**
 * The registry the browser host runs, with the operator's stored pane chords
 * applied. A stored map that names an unknown command, holds a chord this
 * host cannot use, or collides with another command is dropped as a whole and
 * the defaults run, the way the Swift host resolves its own map; the
 * diagnostic says why.
 */
export function effectiveRegistry(stored: Record<string, string> | null | undefined): EffectiveRegistry {
  const entries = Object.entries(stored ?? {});
  if (entries.length === 0) return { registry: REGISTRY, diagnostic: null };
  let registry: Command[] = [...REGISTRY];
  for (const [id, text] of entries) {
    if (!EDITABLE_PANE_COMMANDS.includes(id as CommandId)) {
      return { registry: REGISTRY, diagnostic: `Stored browser shortcuts name an unknown command (${id}); defaults are in use.` };
    }
    const chord = parseChord(text);
    const problem = chord ? bindingProblem(id as CommandId, chord, registry) : "unreadable chord";
    if (!chord || problem) {
      return { registry: REGISTRY, diagnostic: `Stored browser shortcut for ${id} was not usable (${problem}); defaults are in use.` };
    }
    registry = registry.map((command) => (command.id === id ? { ...command, browser: chord, moved: false, movedFrom: undefined } : command));
  }
  return { registry, diagnostic: null };
}

let resolved: { stored: Record<string, string> | null | undefined; value: EffectiveRegistry } | null = null;

/**
 * `effectiveRegistry` for the stored map the snapshot carries now, computed
 * once per map: the store shares an unchanged section by reference, so a
 * keystroke reuses the last resolution instead of re-validating every chord.
 */
export function resolvedRegistry(stored: Record<string, string> | null | undefined): EffectiveRegistry {
  if (!resolved || resolved.stored !== stored) resolved = { stored, value: effectiveRegistry(stored) };
  return resolved.value;
}

/**
 * The registry `host` runs. The stored overrides are the browser host's
 * (`browser_shortcut_bindings`, validated against Chrome's reserved keys);
 * the Electron host runs its own column as it stands.
 */
export function hostRegistry(stored: Record<string, string> | null | undefined, host: HostKind): EffectiveRegistry {
  return host === "electron" ? { registry: REGISTRY, diagnostic: null } : resolvedRegistry(stored);
}

/** The default browser chord for a command, before any override. */
export function defaultBrowserChord(id: CommandId): Chord | null {
  return REGISTRY.find((command) => command.id === id)?.browser ?? null;
}

const CODE_GLYPHS: Record<string, string> = {
  Comma: ",",
  Period: ".",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
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

/** A command's chord on `host` as the operator reads it, or "" when it has none. */
export function displayCommand(id: CommandId, host: HostKind, registry: readonly Command[] = REGISTRY): string {
  const command = registry.find((row) => row.id === id);
  const chord = command ? hostChord(command, host) : null;
  return chord ? displayChord(chord) : "";
}
