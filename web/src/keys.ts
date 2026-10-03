/** The modified-key input policy, for xterm.js attachCustomKeyEventHandler. */

import { modChord, type Chord, type KeySystem } from "./shortcuts";

export const SHIFT_ENTER = new Uint8Array([0x1b, 0x0d]);
export const COMMAND_DELETE = new Uint8Array([0x15]);
export const LINE_START = new Uint8Array([0x01]);
export const LINE_END = new Uint8Array([0x05]);

/** Copy and paste in a terminal, as macOS chords: ⌘C and ⌘V, which the rule makes Ctrl+Shift+C and Ctrl+Shift+V elsewhere. */
export const TERMINAL_COPY: Chord = { code: "KeyC", meta: true };
export const TERMINAL_PASTE: Chord = { code: "KeyV", meta: true };

/**
 * What a modified key does in a terminal on `system`, or null when xterm
 * handles it as typed input:
 * - bytes the shell reads for a macOS text-editing key (⇧↩ on every system;
 *   ⌘⌫, ⌘← and ⌘→ on macOS, which Windows and Linux spell with
 *   Ctrl+Backspace, Home and End, keys xterm already sends);
 * - a copy of the selection: ⌘C on macOS is the menu's own; elsewhere
 *   Ctrl+Shift+C, and Ctrl+C while there is a selection, as Windows
 *   Terminal does, so Ctrl+C still interrupts when nothing is selected;
 * - a paste: Ctrl+Shift+V elsewhere, which the page's own paste carries out.
 */
export type TerminalKey = { kind: "bytes"; bytes: Uint8Array } | { kind: "copy" } | { kind: "paste" };

type KeyEventLike = Pick<KeyboardEvent, "key" | "code" | "shiftKey" | "metaKey" | "ctrlKey" | "altKey" | "isComposing">;

function presses(event: KeyEventLike, chord: Chord): boolean {
  return event.code === chord.code && event.metaKey === !!chord.meta && event.ctrlKey === !!chord.ctrl && event.shiftKey === !!chord.shift && event.altKey === !!chord.alt;
}

export function terminalKey(event: KeyEventLike, system: KeySystem, selection: boolean, composing = event.isComposing): TerminalKey | null {
  if (composing) return null;
  const bytes = (value: Uint8Array): TerminalKey => ({ kind: "bytes", bytes: value });
  const chord = !event.ctrlKey && !event.altKey;
  if (event.key === "Enter" && event.shiftKey && !event.metaKey && chord) return bytes(SHIFT_ENTER);
  if (system === "mac") {
    if (!event.metaKey || event.shiftKey || !chord) return null;
    if (event.key === "Backspace") return bytes(COMMAND_DELETE);
    if (event.key === "ArrowLeft") return bytes(LINE_START);
    if (event.key === "ArrowRight") return bytes(LINE_END);
    return null;
  }
  if (presses(event, modChord(TERMINAL_PASTE, system))) return { kind: "paste" };
  if (!selection) return null;
  if (presses(event, modChord(TERMINAL_COPY, system)) || presses(event, { code: "KeyC", ctrl: true })) return { kind: "copy" };
  return null;
}
