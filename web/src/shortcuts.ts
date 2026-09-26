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
  | "recent_panel"
  | "previous_recent_panel"
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
  { id: "new_workspace", title: "New workspace", group: "Navigate", browser: { code: "KeyN", alt: true, shift: true }, electron: { code: "KeyN", meta: true, shift: true }, moved: true, movedFrom: "⌘⇧N" },
  { id: "recent_panel", title: "Next recent panel", group: "Navigate", browser: { code: "Backquote", alt: true }, electron: { code: "Tab", ctrl: true }, moved: true, movedFrom: "⌃Tab" },
  { id: "previous_recent_panel", title: "Previous recent panel", group: "Navigate", browser: { code: "Backquote", alt: true, shift: true }, electron: { code: "Tab", ctrl: true, shift: true }, moved: true, movedFrom: "⌃⇧Tab" },
  { id: "recent_project", title: "Next recent project", group: "Navigate", browser: { code: "Tab", alt: true }, electron: { code: "Tab", alt: true }, moved: false },
  { id: "previous_recent_project", title: "Previous recent project", group: "Navigate", browser: { code: "Tab", alt: true, shift: true }, electron: { code: "Tab", alt: true, shift: true }, moved: false },
  { id: "search", title: "Search", group: "Navigate", browser: { code: "KeyK", meta: true }, electron: { code: "KeyK", meta: true }, moved: false },
  { id: "open_file", title: "Open file", group: "Navigate", browser: { code: "KeyP", meta: true }, electron: { code: "KeyP", meta: true }, moved: false },
  { id: "project_home", title: "Project home", group: "Navigate", browser: { code: "KeyH", meta: true, shift: true }, electron: { code: "KeyH", meta: true, shift: true }, moved: false },
  { id: "toggle_left_sidebar", title: "Toggle left sidebar", group: "Panels", browser: { code: "KeyB", meta: true }, electron: { code: "KeyB", meta: true }, moved: false },
  { id: "toggle_sidebar_view", title: "Toggle sidebar view", group: "Panels", browser: { code: "KeyE", meta: true }, electron: { code: "KeyE", meta: true }, moved: false },
  { id: "toggle_right_panel", title: "Toggle side panel", group: "Panels", browser: { code: "KeyB", meta: true, shift: true }, electron: { code: "KeyB", meta: true, shift: true }, moved: false },
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

// The pane commands an operator may rebind (PRD S5 D-07): the eight Swift
// pane commands less Toggle Conversation, which the web shell has no surface
// for. Each host keeps its own set in the core, because the hosts reserve
// different keys (Chrome keeps ⌘W): the browser's in
// `ui_state.browser_shortcut_bindings`, and the desktop app's in
// `ui_state.shortcut_bindings`, the macOS set it shares with the Swift app
// in the Swift app's own format and command names (user decision 2026-09-26:
// the desktop app honours the operator's Swift shortcut settings).
export const EDITABLE_PANE_COMMANDS: readonly CommandId[] = [
  "split_right",
  "split_down",
  "toggle_zoom",
  "close_pane",
  "text_larger",
  "text_smaller",
  "text_reset",
];

/** The macOS set's name for each editable command (`PaneCommand` in the Swift app). */
const MACOS_KEYS: Readonly<Partial<Record<CommandId, string>>> = {
  split_right: "split_right",
  split_down: "split_down",
  toggle_zoom: "toggle_zoom",
  close_pane: "close_pane",
  text_larger: "increase_text_size",
  text_smaller: "decrease_text_size",
  text_reset: "reset_text_size",
};

/** Swift pane commands the web shell has no surface for: kept in the set, never run here. */
const MACOS_ONLY_KEYS: readonly string[] = ["toggle_conversation"];

// A physical key a chord may use. Anything else (a dead key, an IME process
// key, a lone modifier) is not a chord this registry can match reliably.
const BINDABLE_CODE = /^(Key[A-Z]|Digit[0-9]|Enter|Backquote|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash|Arrow(Up|Down|Left|Right))$/;

/** One stored browser chord: its modifiers in a fixed order, then the physical key. */
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

// The macOS set's keys: one printable ASCII key, named by the character it
// types unshifted, or Return (`PaneShortcut.parse` in the Swift app).
const MACOS_KEY_CODES: Readonly<Record<string, string>> = {
  return: "Enter",
  "`": "Backquote",
  "-": "Minus",
  "=": "Equal",
  "[": "BracketLeft",
  "]": "BracketRight",
  "\\": "Backslash",
  ";": "Semicolon",
  "'": "Quote",
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
};

function macosKeyCode(key: string): string | null {
  if (/^[a-z]$/.test(key)) return `Key${key.toUpperCase()}`;
  if (/^[0-9]$/.test(key)) return `Digit${key}`;
  return MACOS_KEY_CODES[key] ?? null;
}

function macosKeyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  return Object.entries(MACOS_KEY_CODES).find(([, known]) => known === code)?.[0] ?? null;
}

/** A macOS set chord ("command+shift+return"), read as leniently as the Swift app reads it. */
export function parseMacosChord(text: string): Chord | null {
  const parts = text.toLowerCase().replace(/ /g, "").split("+");
  const key = parts.pop() ?? "";
  if (parts.length === 0) return null;
  const code = macosKeyCode(key === "enter" ? "return" : key);
  if (!code) return null;
  const chord: Chord = { code };
  for (const part of parts) {
    const modifier =
      part === "cmd" || part === "command" ? "meta" : part === "ctrl" || part === "control" ? "ctrl" : part === "opt" || part === "option" || part === "alt" ? "alt" : part === "shift" ? "shift" : null;
    if (!modifier || chord[modifier]) return null;
    chord[modifier] = true;
  }
  return chord;
}

/** A chord in the macOS set's canonical form (modifiers command, control, option, shift), or null when its key has no name there. */
export function serializeMacosChord(chord: Chord): string | null {
  const key = macosKeyName(chord.code);
  if (!key) return null;
  return [chord.meta && "command", chord.ctrl && "control", chord.alt && "option", chord.shift && "shift", key].filter(Boolean).join("+");
}

/** Chords macOS or the app keeps on the desktop host (`PaneShortcutPolicy.reserved` in the Swift app). */
const MACOS_RESERVED: readonly Chord[] = [
  ...["KeyQ", "KeyH", "KeyM", "KeyS", "KeyW", "Comma"].map((code) => ({ code, meta: true })),
  ...Array.from({ length: 9 }, (_, index) => ({ code: `Digit${index + 1}`, meta: true })),
];

/** The key a command's chord is stored under in `host`'s set. */
export function storedKey(id: CommandId, host: HostKind): string {
  return host === "electron" ? (MACOS_KEYS[id] ?? id) : id;
}

export function parseStoredChord(text: string, host: HostKind): Chord | null {
  return host === "electron" ? parseMacosChord(text) : parseChord(text);
}

export function serializeStoredChord(chord: Chord, host: HostKind): string | null {
  return host === "electron" ? serializeMacosChord(chord) : serializeChord(chord);
}

/** The stored maps the snapshot's `ui_state` carries, one per host. */
export type StoredShortcutSets = {
  shortcut_bindings?: Record<string, string>;
  browser_shortcut_bindings?: Record<string, string>;
} | null | undefined;

/** `host`'s own stored set. */
export function storedBindings(uiState: StoredShortcutSets, host: HostKind): Record<string, string> | undefined {
  return host === "electron" ? uiState?.shortcut_bindings : uiState?.browser_shortcut_bindings;
}

function withChord(command: Command, chord: Chord, host: HostKind): Command {
  return host === "electron" ? { ...command, electron: chord } : { ...command, browser: chord, moved: false, movedFrom: undefined };
}

/**
 * Why `chord` cannot become `id`'s binding on `host` in `registry`, or null
 * when it can. The same rules decide a stored set on load, so a chord the
 * editor refuses can never become effective by being written to the core
 * directly. The desktop host's rules are the Swift app's, because both run
 * the one macOS set.
 */
export function bindingProblem(id: CommandId, chord: Chord, registry: readonly Command[], host: HostKind = "browser"): string | null {
  if (host === "electron") {
    if (!macosKeyName(chord.code)) return "Use one letter, digit or punctuation key, or Return.";
    if (!chord.meta) return "Include ⌘ so typing in a terminal stays typing.";
    if (MACOS_RESERVED.some((reserved) => chordEquals(reserved, chord))) return `${displayChord(chord)} is kept by macOS or the app menu.`;
  } else {
    if (!BINDABLE_CODE.test(chord.code)) return "Use a letter, a digit, Return, an arrow or a punctuation key.";
    if (!chord.meta && !chord.ctrl && !chord.alt) return "Include ⌘, ⌥ or ⌃ so typing in a terminal stays typing.";
    if (isChromeReserved(chord)) return `${displayChord(chord)} is kept by Chrome or macOS and never reaches the page.`;
  }
  const taken = registry.find((command) => {
    const bound = command.id === id ? null : hostChord(command, host);
    return bound && chordEquals(bound, chord);
  });
  if (taken) return `${displayChord(chord)} is already ${taken.title}.`;
  return null;
}

export type EffectiveRegistry = { registry: readonly Command[]; diagnostic: string | null };

/**
 * The registry `host` runs, with the operator's stored pane chords applied.
 * A stored set that names an unknown command, holds a chord this host cannot
 * use, or collides with another command is dropped as a whole and the
 * defaults run, the way the Swift app resolves its own set; the diagnostic
 * says why. Collisions are judged on the whole set, so two commands that
 * trade chords are one valid set.
 */
export function effectiveRegistry(stored: Record<string, string> | null | undefined, host: HostKind = "browser"): EffectiveRegistry {
  const which = host === "electron" ? "pane" : "browser";
  const entries = Object.entries(stored ?? {}).filter(([key]) => !(host === "electron" && MACOS_ONLY_KEYS.includes(key)));
  if (entries.length === 0) return { registry: REGISTRY, diagnostic: null };
  let registry: Command[] = [...REGISTRY];
  const applied: [CommandId, Chord][] = [];
  for (const [key, text] of entries) {
    const id = EDITABLE_PANE_COMMANDS.find((command) => storedKey(command, host) === key);
    if (!id) return { registry: REGISTRY, diagnostic: `Stored ${which} shortcuts name an unknown command (${key}); defaults are in use.` };
    const chord = parseStoredChord(text, host);
    if (!chord) return { registry: REGISTRY, diagnostic: `Stored ${which} shortcut for ${key} was not usable (unreadable chord); defaults are in use.` };
    registry = registry.map((command) => (command.id === id ? withChord(command, chord, host) : command));
    applied.push([id, chord]);
  }
  for (const [id, chord] of applied) {
    const problem = bindingProblem(id, chord, registry, host);
    if (problem) return { registry: REGISTRY, diagnostic: `Stored ${which} shortcut for ${storedKey(id, host)} was not usable (${problem}); defaults are in use.` };
  }
  return { registry, diagnostic: null };
}

const resolved = new Map<HostKind, { stored: Record<string, string> | null | undefined; value: EffectiveRegistry }>();

/**
 * `effectiveRegistry` for the stored set the snapshot carries now, computed
 * once per set: the store shares an unchanged section by reference, so a
 * keystroke reuses the last resolution instead of re-validating every chord.
 */
export function resolvedRegistry(stored: Record<string, string> | null | undefined, host: HostKind = "browser"): EffectiveRegistry {
  const last = resolved.get(host);
  if (last && last.stored === stored) return last.value;
  const value = effectiveRegistry(stored, host);
  resolved.set(host, { stored, value });
  return value;
}

/** The registry `host` runs: its column with its own stored set applied. */
export function hostRegistry(uiState: StoredShortcutSets, host: HostKind): EffectiveRegistry {
  return resolvedRegistry(storedBindings(uiState, host), host);
}

/** A command's default chord on `host`, before any stored set. */
export function defaultChord(id: CommandId, host: HostKind): Chord | null {
  const command = REGISTRY.find((row) => row.id === id);
  return command ? hostChord(command, host) : null;
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
