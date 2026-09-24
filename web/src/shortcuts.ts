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
  | "settings"
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
  { id: "settings", title: "Settings", group: "Help", browser: { code: "Comma", alt: true }, electron: null, moved: true, movedFrom: "⌘," },
  { id: "shortcuts", title: "Keyboard shortcuts", group: "Help", browser: { code: "Slash", meta: true }, electron: null, moved: false },
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

/** The command a keydown names on the browser host, or null. */
export function matchBrowser(event: KeyEventLike, registry: readonly Command[] = REGISTRY): Command | null {
  const chord = chordFromEvent(event);
  return registry.find((command) => command.browser && !command.passthrough && chordEquals(command.browser, chord)) ?? null;
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

export function displayBrowser(id: CommandId, registry: readonly Command[] = REGISTRY): string {
  const command = registry.find((row) => row.id === id);
  return command?.browser ? displayChord(command.browser) : "";
}
