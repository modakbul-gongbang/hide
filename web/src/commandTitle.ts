// A command's title in the operator's language. Kept apart from
// `shortcutLabels.ts`, which reads the shell's store, so the desktop host's
// main process can name the app menu's commands without that module graph.

import type { TFunction } from "i18next";
import { isNumberedCommand, numberedCommand, type CommandId } from "./shortcuts";

/** The registry's `title` stays the English the e2e specs and the sheet's search read; this is what a screen or the menu draws. */
export function commandTitle(id: CommandId, t: TFunction<"translation">): string {
  if (isNumberedCommand(id)) {
    const { family, number } = numberedCommand(id)!;
    return t(family === "tabs" ? "commands.select_tab" : "commands.select_agent", { number });
  }
  return t(`commands.${id}`);
}
