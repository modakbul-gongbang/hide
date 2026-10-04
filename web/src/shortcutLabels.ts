// The chords the screens print: a command's chord in the running host and
// on the system the operator types on, with that host's stored set applied,
// so a label never names a key the window listener does not answer.

import { hostKind, keySystem } from "./host";
import { displayChord, displayCommand, fieldChord, hostRegistry, type Chord, type CommandId, type StoredShortcutSets } from "./shortcuts";
import { useShellStore } from "./store";
import type { TFunction } from "i18next";

/** Shared Overview navigation labels; other command surfaces await issue 339. */
export function overviewCommandTitle(id: string, title: string, t: TFunction<"translation">): string {
  switch (id) {
    case "overview": return t("commands.overview");
    case "sidebar_projects": return t("commands.sidebar_projects");
    case "sidebar_agents": return t("commands.sidebar_agents");
    default: return title;
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
