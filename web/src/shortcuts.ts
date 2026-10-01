// The shortcut registry: one table, command -> key per host (PRD S2 D-01).
//
// The browser column is what Chrome lets a page claim. Five chords Chrome keeps
// for itself (⌘T ⌘W ⌘⇧T ⌃Tab ⌃⇧Tab) moved to the ⌥ family, and ⌘⇧W
// (Chrome's close-window) moved with them (D-06); `moved` marks each so the
// sheet can say so. ⌘⇧N, Add project, has no browser chord: adding a project
// needs the desktop app's folder picker, which a browser tab does not have. The Electron column is the macOS chord set: the desktop host
// has no browser keeping chords, so the moved ones return to their native
// keys there. The one exception is ⌘E, which shows the Explorer in both
// hosts (issue 170) while the macOS set keeps it on the sidebar switch; the
// switch has no default chord here and can be bound in Settings. Its app
// menu is built from this same column (`desktop/src/main/menu.ts`), so the
// menu and the sheet cannot disagree.
//
// Chords match on `KeyboardEvent.code`, not `key`: macOS turns ⌥-letters into
// dead keys and symbols (⌥T is "†"), and a physical position is what the
// operator's fingers know.
//
// The numbered commands (PRD electron-digit-shortcuts-hints D-02) are the
// desktop app's ⌘1-9 (the nth tab of the strip in front) and ⌥1-9 (the nth
// row the sidebar's Agents list draws). Chrome keeps ⌘1-9 for its own tabs
// and a page cannot claim ⌥-digits reliably either, so the browser column has
// none: the sheet says so and nothing is intercepted there. The number itself
// is the screen order at the moment of the press, assigned by the web shell
// (`numbering.ts`), and the hold hint (`hints.ts`) shows that order.

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
  | "start_agent"
  | "recent_area_tab"
  | "previous_recent_area_tab"
  | "recent_panel"
  | "previous_recent_panel"
  | "recent_project"
  | "previous_recent_project"
  | "search"
  | "open_file"
  | "toggle_left_sidebar"
  | "toggle_sidebar_view"
  | "toggle_device_rail"
  | "toggle_explorer"
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
  | AreaCommandId
  | "settings"
  | "shortcuts"
  | NumberedCommandId;

/**
 * Moving between the Agent areas or the View areas and resizing the one in
 * use (PRD cmdk-navigation D-05): commands ⌘K used to carry, with no default
 * chord, bindable in Settings and in the desktop menu.
 */
export const AREA_COMMANDS = [
  "focus_next_agent_area",
  "focus_previous_agent_area",
  "grow_agent_area",
  "shrink_agent_area",
  "focus_next_view_area",
  "focus_previous_view_area",
  "grow_view_area",
  "shrink_view_area",
] as const;
export type AreaCommandId = (typeof AREA_COMMANDS)[number];

export type Digit = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9;
export const DIGITS: readonly Digit[] = [1, 2, 3, 4, 5, 6, 7, 8, 9];

/** What a numbered chord selects: a tab of the strip in front, or a row of the Agents list. */
export type NumberedFamily = "tabs" | "agents";
export type NumberedCommandId = `select_tab_${Digit}` | `select_agent_${Digit}`;

type NumberedFamilySpec = {
  family: NumberedFamily;
  prefix: "select_tab" | "select_agent";
  title: string;
  group: Command["group"];
  /** The modifiers every chord of the family shares on the desktop host; the hold hint reveals the family on exactly these. */
  modifiers: Omit<Chord, "code">;
};

export const NUMBERED_FAMILIES: readonly NumberedFamilySpec[] = [
  { family: "tabs", prefix: "select_tab", title: "Select tab", group: "Tabs", modifiers: { meta: true } },
  { family: "agents", prefix: "select_agent", title: "Select agent", group: "Navigate", modifiers: { alt: true } },
];

function numberedEntries(spec: NumberedFamilySpec): Command[] {
  return DIGITS.map((digit) => ({
    id: `${spec.prefix}_${digit}` as NumberedCommandId,
    title: `${spec.title} ${digit}`,
    group: spec.group,
    browser: null,
    electron: { code: `Digit${digit}`, ...spec.modifiers },
    moved: false,
  }));
}

/** The family and number a command id names, or null for every other command. */
export function numberedCommand(id: CommandId): { family: NumberedFamily; number: Digit } | null {
  const match = /^(select_tab|select_agent)_([1-9])$/.exec(id);
  if (!match) return null;
  const spec = NUMBERED_FAMILIES.find((row) => row.prefix === match[1]);
  return spec ? { family: spec.family, number: Number(match[2]) as Digit } : null;
}

export function isNumberedCommand(id: CommandId): id is NumberedCommandId {
  return numberedCommand(id) !== null;
}

export type Command = {
  id: CommandId;
  title: string;
  group: "Tabs" | "Navigate" | "Panels" | "Panes" | "Help";
  browser: Chord | null;
  /** The Electron host's chord: the macOS chord, or the browser one where the macOS set has none. */
  electron: Chord | null;
  /** True when the browser chord differs from the macOS chord because Chrome reserves the original. */
  moved: boolean;
  /** The macOS chord the browser one replaced, for the sheet's note. */
  movedFrom?: string;
  /** Not intercepted at the window: the chord reaches the terminal (⌘⌫ is ^U there) and its shell owner is a later stage. */
  passthrough?: string;
};

export const REGISTRY: readonly Command[] = [
  { id: "new_tab", title: "New tab", group: "Tabs", browser: { code: "KeyT", alt: true }, electron: { code: "KeyT", meta: true }, moved: true, movedFrom: "⌘T" },
  { id: "close_tab", title: "Close focused view or pane", group: "Tabs", browser: { code: "KeyW", alt: true }, electron: { code: "KeyW", meta: true }, moved: true, movedFrom: "⌘W" },
  { id: "reopen_closed_tab", title: "Reopen closed tab", group: "Tabs", browser: { code: "KeyT", alt: true, shift: true }, electron: { code: "KeyT", meta: true, shift: true }, moved: true, movedFrom: "⌘⇧T" },
  ...numberedEntries(NUMBERED_FAMILIES[0]!),
  { id: "new_workspace", title: "Add project", group: "Navigate", browser: null, electron: { code: "KeyN", meta: true, shift: true }, moved: false },
  { id: "start_agent", title: "Start agent", group: "Navigate", browser: null, electron: { code: "KeyN", meta: true }, moved: false },
  { id: "recent_area_tab", title: "Next recent Agent pane or View tab", group: "Navigate", browser: { code: "Backquote", alt: true }, electron: { code: "Tab", ctrl: true }, moved: true, movedFrom: "⌃Tab" },
  { id: "previous_recent_area_tab", title: "Previous recent Agent pane or View tab", group: "Navigate", browser: { code: "Backquote", alt: true, shift: true }, electron: { code: "Tab", ctrl: true, shift: true }, moved: true, movedFrom: "⌃⇧Tab" },
  { id: "recent_panel", title: "Next global recent panel", group: "Navigate", browser: null, electron: null, moved: false },
  { id: "previous_recent_panel", title: "Previous global recent panel", group: "Navigate", browser: null, electron: null, moved: false },
  { id: "recent_project", title: "Next recent project", group: "Navigate", browser: { code: "Tab", alt: true }, electron: { code: "Tab", alt: true }, moved: false },
  { id: "previous_recent_project", title: "Previous recent project", group: "Navigate", browser: { code: "Tab", alt: true, shift: true }, electron: { code: "Tab", alt: true, shift: true }, moved: false },
  ...numberedEntries(NUMBERED_FAMILIES[1]!),
  { id: "search", title: "Search", group: "Navigate", browser: { code: "KeyK", meta: true }, electron: { code: "KeyK", meta: true }, moved: false },
  { id: "open_file", title: "Open file", group: "Navigate", browser: { code: "KeyP", meta: true }, electron: { code: "KeyP", meta: true }, moved: false },
  { id: "project_home", title: "Project home", group: "Navigate", browser: { code: "KeyH", meta: true, shift: true }, electron: { code: "KeyH", meta: true, shift: true }, moved: false },
  { id: "toggle_left_sidebar", title: "Toggle left sidebar", group: "Panels", browser: { code: "KeyB", meta: true }, electron: { code: "KeyB", meta: true }, moved: false },
  { id: "toggle_sidebar_view", title: "Toggle sidebar view", group: "Panels", browser: null, electron: null, moved: false },
  { id: "toggle_device_rail", title: "Toggle device rail", group: "Panels", browser: null, electron: null, moved: false },
  { id: "toggle_explorer", title: "Toggle tools", group: "Panels", browser: { code: "KeyE", meta: true }, electron: { code: "KeyE", meta: true }, moved: false },
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
  { id: "focus_next_agent_area", title: "Focus next Agent area", group: "Panes", browser: null, electron: null, moved: false },
  { id: "focus_previous_agent_area", title: "Focus previous Agent area", group: "Panes", browser: null, electron: null, moved: false },
  { id: "grow_agent_area", title: "Grow Agent area", group: "Panes", browser: null, electron: null, moved: false },
  { id: "shrink_agent_area", title: "Shrink Agent area", group: "Panes", browser: null, electron: null, moved: false },
  { id: "focus_next_view_area", title: "Focus next View area", group: "Panes", browser: null, electron: null, moved: false },
  { id: "focus_previous_view_area", title: "Focus previous View area", group: "Panes", browser: null, electron: null, moved: false },
  { id: "grow_view_area", title: "Grow View area", group: "Panes", browser: null, electron: null, moved: false },
  { id: "shrink_view_area", title: "Shrink View area", group: "Panes", browser: null, electron: null, moved: false },
  { id: "move_to_trash", title: "Move to Trash", group: "Panes", browser: { code: "Backspace", meta: true }, electron: { code: "Backspace", meta: true }, moved: false, passthrough: "Explorer only; in a terminal ⌘⌫ clears the line" },
  { id: "settings", title: "Settings", group: "Help", browser: { code: "Comma", alt: true }, electron: { code: "Comma", meta: true }, moved: true, movedFrom: "⌘," },
  { id: "shortcuts", title: "Keyboard shortcuts", group: "Help", browser: { code: "Slash", meta: true }, electron: { code: "Slash", meta: true }, moved: false },
];

/** Chords Chrome or macOS never hands to a page; a browser chord using one is a registry error. */
const CHROME_RESERVED: readonly Chord[] = [
  { code: "Tab", meta: true },
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

// The pane commands an operator may rebind (PRD S5 D-07): the eight macOS
// pane commands less Toggle Conversation, which the web shell has no surface
// for, plus the two sidebar switches (view and device rail) that have no macOS
// pane command. Each host keeps its own set in the core, because the hosts reserve
// different keys (Chrome keeps ⌘W): the browser's in
// `ui_state.browser_shortcut_bindings`, and the desktop app's in
// `ui_state.shortcut_bindings`, the macOS set in the removed native app's
// format and command names (user decision 2026-09-26: the desktop app
// honours the operator's existing shortcut settings).
export const EDITABLE_PANE_COMMANDS: readonly CommandId[] = [
  "recent_area_tab", "previous_recent_area_tab", "recent_panel", "previous_recent_panel",
  "toggle_sidebar_view",
  "toggle_device_rail",
  "split_right",
  "split_down",
  "toggle_zoom",
  "close_pane",
  "text_larger",
  "text_smaller",
  "text_reset",
  ...AREA_COMMANDS,
];

/**
 * The macOS set's name for each editable command. `toggle_sidebar_view`
 * and `toggle_device_rail` have no pane command there: the set keeps the key and the desktop host
 * ignores it, the way this shell ignores
 * `toggle_conversation`.
 */
const MACOS_KEYS: Readonly<Partial<Record<CommandId, string>>> = {
  toggle_sidebar_view: "toggle_sidebar_view",
  toggle_device_rail: "toggle_device_rail",
  split_right: "split_right",
  split_down: "split_down",
  toggle_zoom: "toggle_zoom",
  close_pane: "close_pane",
  text_larger: "increase_text_size",
  text_smaller: "decrease_text_size",
  text_reset: "reset_text_size",
};

/** macOS pane commands the web shell has no surface for: kept in the set, never run here. */
const MACOS_ONLY_KEYS: readonly string[] = ["toggle_conversation"];

// A physical key a chord may use. Anything else (a dead key, an IME process
// key, a lone modifier) is not a chord this registry can match reliably.
const BINDABLE_CODE = /^(Key[A-Z]|Digit[0-9]|Enter|Tab|Backquote|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash|Arrow(Up|Down|Left|Right))$/;

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
// types unshifted, or Return.
const MACOS_KEY_CODES: Readonly<Record<string, string>> = {
  tab: "Tab",
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

/** A macOS set chord ("command+shift+return"), read leniently: case and order do not matter. */
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

/**
 * Chords macOS or the app keeps on the desktop host. ⌘1-9 are not here: the
 * registry's own numbered commands hold them, so a pane chord bound onto one
 * is refused as that command's, by name.
 */
const MACOS_RESERVED: readonly Chord[] = ["Tab", "KeyQ", "KeyH", "KeyM", "KeyS", "KeyW", "Comma"].map((code) => ({ code, meta: true }));

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
 * directly. The desktop host's rules are the macOS set's own.
 */
export function bindingProblem(id: CommandId, chord: Chord, registry: readonly Command[], host: HostKind = "browser"): string | null {
  if (host === "electron") {
    if (!macosKeyName(chord.code)) return "Use one letter, digit or punctuation key, or Return.";
    if (!chord.meta && !(isCycleCommand(id) && (chord.ctrl || chord.alt))) return "Include ⌘ so typing in a terminal stays typing.";
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
 * defaults run; the diagnostic
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

/**
 * A numbered family's chords on `host` in number order, or null where the
 * host has none: the hold hint reveals a family when the held modifiers are
 * exactly its chords' (PRD electron-digit-shortcuts-hints D-03, D-04).
 */
export function familyChords(family: NumberedFamily, registry: readonly Command[], host: HostKind): Chord[] | null {
  const spec = NUMBERED_FAMILIES.find((row) => row.family === family);
  if (!spec) return null;
  const chords = DIGITS.map((digit) => {
    const command = registry.find((row) => row.id === `${spec.prefix}_${digit}`);
    return command ? hostChord(command, host) : null;
  });
  return chords.every((chord): chord is Chord => chord !== null) ? chords : null;
}

/** The modifiers a family's chords share on `host`, as a chord without a key, or null where the host has none. */
export function familyModifiers(family: NumberedFamily, registry: readonly Command[], host: HostKind): Omit<Chord, "code"> | null {
  const chords = familyChords(family, registry, host);
  if (!chords) return null;
  const [first] = chords;
  const modifiers = { meta: !!first!.meta, alt: !!first!.alt, shift: !!first!.shift, ctrl: !!first!.ctrl };
  const shared = chords.every((chord) => !!chord.meta === modifiers.meta && !!chord.alt === modifiers.alt && !!chord.shift === modifiers.shift && !!chord.ctrl === modifiers.ctrl);
  return shared ? modifiers : null;
}

/** One line of the ⌘/ sheet: a command, or a numbered family folded into one row with its range. */
export type SheetRow = { id: CommandId; title: string; chord: string | null; moved: boolean; movedFrom?: string; passthrough?: string };

/**
 * The sheet's rows for `group` on `host`, in registry order, with each
 * numbered family folded into one row ("Select tab 1-9", "⌘1 … ⌘9"), or a
 * null chord where the host has none (B3, B4).
 */
export function sheetRows(group: Command["group"], registry: readonly Command[], host: HostKind): SheetRow[] {
  const rows: SheetRow[] = [];
  const folded = new Set<NumberedFamily>();
  for (const command of registry) {
    if (command.group !== group) continue;
    const numbered = numberedCommand(command.id);
    if (!numbered) {
      const chord = hostChord(command, host);
      rows.push({ id: command.id, title: command.title, chord: chord ? displayChord(chord) : null, moved: command.moved, movedFrom: command.movedFrom, passthrough: command.passthrough });
      continue;
    }
    if (folded.has(numbered.family)) continue;
    folded.add(numbered.family);
    const spec = NUMBERED_FAMILIES.find((row) => row.family === numbered.family)!;
    const chords = familyChords(numbered.family, registry, host);
    rows.push({
      id: command.id,
      title: `${spec.title} ${DIGITS[0]}-${DIGITS[DIGITS.length - 1]}`,
      chord: chords ? `${displayChord(chords[0]!)} … ${displayChord(chords[chords.length - 1]!)}` : null,
      moved: false,
    });
  }
  return rows;
}

/** Held navigation commands bypass native menu accelerators and share release routing. */
export function isCycleCommand(id: CommandId): boolean {
  return ["recent_area_tab", "previous_recent_area_tab", "recent_panel", "previous_recent_panel", "recent_project", "previous_recent_project"].includes(id);
}
export function releaseModifier(chord: Chord): "Control" | "Alt" | "Meta" | null {
  return chord.ctrl ? "Control" : chord.alt ? "Alt" : chord.meta ? "Meta" : null;
}
