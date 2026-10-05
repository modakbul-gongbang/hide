// The chords the screens print: a command's chord in the running host and
// on the system the operator types on, with that host's stored set applied,
// so a label never names a key the window listener does not answer.

import { hostKind, keySystem } from "./host";
import {
  displayChord,
  displayCommand,
  fieldChord,
  hostRegistry,
  isNumberedCommand,
  numberedCommand,
  type BindingProblem,
  type Chord,
  type Command,
  type CommandId,
  type Passthrough,
  type SheetRow,
  type StoredShortcutSets,
} from "./shortcuts";
import { useShellStore } from "./store";
import type { TFunction } from "i18next";

/** A command's title in the operator's language; the registry's `title` stays the English the desktop menu reads. */
export function commandTitle(id: CommandId, t: TFunction<"translation">): string {
  if (isNumberedCommand(id)) {
    const { family, number } = numberedCommand(id)!;
    return t(family === "tabs" ? "commands.select_tab" : "commands.select_agent", { number });
  }
  return t(`commands.${id}`);
}

/** A sheet row's title: a folded numbered family reads with its range ("1-9"). */
export function sheetRowTitle(row: SheetRow, t: TFunction<"translation">): string {
  if (!row.range) return commandTitle(row.id, t);
  return t(numberedCommand(row.id)!.family === "tabs" ? "commands.select_tab" : "commands.select_agent", { number: row.range });
}

export function commandGroupTitle(group: Command["group"], t: TFunction<"translation">): string {
  return t(`commands.group.${group.toLowerCase() as Lowercase<Command["group"]>}`);
}

export function passthroughText(passthrough: Passthrough, t: TFunction<"translation">): string {
  return t(passthrough === "explorer" ? "commands.explorerOnly" : "commands.trashMacHint");
}

/** Why a chord is refused, in the operator's language. */
export function bindingProblemText(problem: BindingProblem, t: TFunction<"translation">): string {
  switch (problem.code) {
    case "system_key": return t("commands.binding.systemKey");
    case "native_key": return t("commands.binding.nativeKey");
    case "browser_key": return t("commands.binding.browserKey");
    case "modifier": return t("commands.binding.modifier", { modifier: modifierText(problem.modifier, t) });
    case "mac_reserved": return t("commands.binding.macReserved", { chord: problem.chord });
    case "system_reserved": return t("commands.binding.systemReserved", { chord: problem.chord });
    case "browser_reserved": return t("commands.binding.browserReserved", { chord: problem.chord, system: problem.macos ? "macOS" : t("commands.binding.system") });
    case "terminal_copy": return t("commands.binding.terminalCopy", { chord: problem.chord });
    case "terminal_paste": return t("commands.binding.terminalPaste", { chord: problem.chord });
    case "conflict": return t("commands.binding.conflict", { chord: problem.chord, command: commandTitle(problem.commandId, t) });
  }
}

function modifierText(modifier: Extract<BindingProblem, { code: "modifier" }>["modifier"], t: TFunction<"translation">): string {
  switch (modifier) {
    case "desktop_mac": return "⌘";
    case "desktop_pc": return t("commands.binding.modifierDesktopPc");
    case "browser_mac": return t("commands.binding.modifierBrowserMac");
    case "browser_pc": return t("commands.binding.modifierBrowserPc");
  }
}

/** `id`'s chord as the operator reads it, or "" when this host has none. */
export function commandLabel(id: CommandId, uiState: StoredShortcutSets = useShellStore.getState().rest?.ui_state): string {
  const host = hostKind();
  const system = keySystem();
  return displayCommand(id, host, hostRegistry(uiState, host, system).registry, system);
}

/** A chord written as this system presses it, in that system's glyphs. */
export function chordLabel(chord: Chord): string {
  return displayChord(chord, keySystem());
}

/** The system's command key with `code` inside a text field or palette: "⌘↵" on macOS, "Ctrl+Enter" elsewhere. */
export function fieldLabel(code: string): string {
  return chordLabel(fieldChord(code, keySystem()));
}
