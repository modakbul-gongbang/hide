/** Swift ModifiedTerminalInputPolicy, for xterm.js attachCustomKeyEventHandler. */

export const SHIFT_ENTER = new Uint8Array([0x1b, 0x0d]);
export const COMMAND_DELETE = new Uint8Array([0x15]);
export const LINE_START = new Uint8Array([0x01]);
export const LINE_END = new Uint8Array([0x05]);

export function mapModifiedKey(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "metaKey" | "ctrlKey" | "altKey" | "isComposing">,
  composing = event.isComposing,
): Uint8Array | null {
  if (composing) return null;
  const chord = !event.ctrlKey && !event.altKey;
  if (event.key === "Enter" && event.shiftKey && !event.metaKey && chord) {
    return SHIFT_ENTER;
  }
  if (event.key === "Backspace" && event.metaKey && !event.shiftKey && chord) {
    return COMMAND_DELETE;
  }
  if (event.key === "ArrowLeft" && event.metaKey && !event.shiftKey && chord) {
    return LINE_START;
  }
  if (event.key === "ArrowRight" && event.metaKey && !event.shiftKey && chord) {
    return LINE_END;
  }
  return null;
}
