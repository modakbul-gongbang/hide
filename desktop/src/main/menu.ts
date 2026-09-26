// The app menu. Its commands and chords come from the web shell's registry
// (`web/src/shortcuts.ts`, Electron column), so the menu, the ⌘/ sheet and
// the window listener read one table (desktop PRD D-04). A click sends the
// command id to the shell over the bridge; a chord the shell's listener
// answers is consumed there and never reaches the menu's accelerator. The
// pane chords follow the operator's macOS set, which the shell reports and
// `menuBindings` resolves with the rules the shell's listener runs.

import type { MenuItemConstructorOptions } from "electron";
import { effectiveRegistry, REGISTRY, type Chord, type Command, type CommandId, type EffectiveRegistry } from "../../../web/src/shortcuts";

const KEY_NAMES: Record<string, string> = {
  Enter: "Return",
  Backspace: "Backspace",
  Tab: "Tab",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
};

/** A registry chord as an Electron accelerator ("Command+Shift+T"). */
export function accelerator(chord: Chord): string {
  const key = chord.code.startsWith("Key")
    ? chord.code.slice(3)
    : chord.code.startsWith("Digit")
      ? chord.code.slice(5)
      : KEY_NAMES[chord.code];
  if (!key) throw new Error(`no accelerator for key code ${chord.code}`);
  return [chord.ctrl && "Control", chord.alt && "Alt", chord.shift && "Shift", chord.meta && "Command", key].filter(Boolean).join("+");
}

/**
 * Which commands each menu carries, in order; null is a separator. The
 * cycles (held-modifier ⌃Tab and ⌥Tab walks) and Move to Trash (a chord
 * the terminal keeps) have no click form and stay keyboard-only.
 */
export const MENU_LAYOUT: Readonly<Record<"app" | "File" | "Edit" | "View" | "Pane" | "Help", readonly (CommandId | null)[]>> = {
  app: ["settings"],
  File: ["new_tab", "new_workspace", "reopen_closed_tab", null, "save_file", null, "close_pane", "close_tab"],
  Edit: ["find_in_pane"],
  View: [
    "search",
    "open_file",
    "project_home",
    null,
    "toggle_left_sidebar",
    "toggle_sidebar_view",
    "toggle_right_panel",
    null,
    "text_larger",
    "text_smaller",
    "text_reset",
  ],
  Pane: ["split_right", "split_down", "toggle_zoom", null, "keep_open"],
  Help: ["shortcuts"],
};

export const KEYBOARD_ONLY: readonly CommandId[] = [
  "recent_tab",
  "previous_recent_tab",
  "recent_project",
  "previous_recent_project",
  "move_to_trash",
];

function commandItem(command: Command, send: (id: CommandId) => void): MenuItemConstructorOptions {
  if (!command.electron) throw new Error(`${command.id} has no electron chord`);
  return { id: command.id, label: command.title, accelerator: accelerator(command.electron), click: () => send(command.id) };
}

// A reported set is a few pane commands; anything past these caps is not one.
const BINDINGS_CAP = 16;
const BINDING_TEXT_CAP = 64;

/**
 * The registry the menu shows for a set the page reported, or why the report
 * was refused. The page is the shell hided serves, but what crosses the
 * bridge is checked here all the same: a plain map of short strings, then
 * the shell's own resolution, whose fallback to the defaults the menu shares.
 */
export function menuBindings(reported: unknown): EffectiveRegistry | { refused: string } {
  if (typeof reported !== "object" || reported === null || Array.isArray(reported)) return { refused: "not a map" };
  const entries = Object.entries(reported);
  if (entries.length > BINDINGS_CAP) return { refused: "too many entries" };
  const stored: Record<string, string> = {};
  for (const [key, value] of entries) {
    if (typeof value !== "string" || key.length > BINDING_TEXT_CAP || value.length > BINDING_TEXT_CAP) return { refused: "not short strings" };
    stored[key] = value;
  }
  return effectiveRegistry(stored, "electron");
}

function items(layout: readonly (CommandId | null)[], send: (id: CommandId) => void, registry: readonly Command[]): MenuItemConstructorOptions[] {
  return layout.map((id) => {
    if (id === null) return { type: "separator" };
    const command = registry.find((row) => row.id === id);
    if (!command) throw new Error(`menu names an unknown command ${id}`);
    return commandItem(command, send);
  });
}

export function menuTemplate(options: {
  appName: string;
  send: (id: CommandId) => void;
  developer: boolean;
  registry?: readonly Command[];
}): MenuItemConstructorOptions[] {
  const { appName, send, developer } = options;
  const registry = options.registry ?? REGISTRY;
  return [
    {
      label: appName,
      submenu: [
        { role: "about" },
        { type: "separator" },
        ...items(MENU_LAYOUT.app, send, registry),
        { type: "separator" },
        { role: "services" },
        { type: "separator" },
        { role: "hide" },
        { role: "hideOthers" },
        { role: "unhide" },
        { type: "separator" },
        { role: "quit" },
      ],
    },
    { label: "File", submenu: items(MENU_LAYOUT.File, send, registry) },
    {
      label: "Edit",
      submenu: [
        { role: "undo" },
        { role: "redo" },
        { type: "separator" },
        { role: "cut" },
        { role: "copy" },
        { role: "paste" },
        { role: "pasteAndMatchStyle" },
        { role: "selectAll" },
        { type: "separator" },
        ...items(MENU_LAYOUT.Edit, send, registry),
      ],
    },
    {
      label: "View",
      submenu: [
        ...items(MENU_LAYOUT.View, send, registry),
        ...(developer ? ([{ type: "separator" }, { role: "reload" }, { role: "toggleDevTools" }] as MenuItemConstructorOptions[]) : []),
        { type: "separator" },
        { role: "togglefullscreen" },
      ],
    },
    { label: "Pane", submenu: items(MENU_LAYOUT.Pane, send, registry) },
    { role: "windowMenu" },
    { role: "help", submenu: items(MENU_LAYOUT.Help, send, registry) },
  ];
}
