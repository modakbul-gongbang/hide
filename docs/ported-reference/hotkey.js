// Turning a keyboard event into a Tauri accelerator string.
//
// Kept apart from the settings view so the mapping is testable without a
// browser: this is the piece that decides what the user's key press actually
// binds to, and getting it wrong produces a shortcut that silently never
// fires.

const MODIFIER_KEYS = new Set(['Meta', 'Control', 'Alt', 'Shift', 'CapsLock']);

// macOS glyphs for display; the accelerator string itself stays in Tauri's
// spelling so what we store is what the plugin parses.
const MODIFIER_GLYPHS = { Cmd: '⌘', Alt: '⌥', Shift: '⇧', Ctrl: '⌃' };

const NAMED_KEYS = {
  ' ': 'Space',
  Escape: 'Escape',
  Enter: 'Enter',
  Tab: 'Tab',
  Backspace: 'Backspace',
  Delete: 'Delete',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
};

/** The base key of an accelerator, or null when this event carries none. */
function baseKey(event) {
  const key = event.key;
  if (!key || MODIFIER_KEYS.has(key)) return null;
  if (NAMED_KEYS[key]) return NAMED_KEYS[key];
  if (/^F\d{1,2}$/.test(key)) return key;
  // Use the physical key rather than the produced character: with Alt held,
  // macOS reports "π" for the P key, and "Alt+π" parses as nothing.
  const code = event.code || '';
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (key.length === 1) return key.toUpperCase();
  return null;
}

/**
 * Build an accelerator from a keydown event.
 *
 * Returns `{ accelerator, parts }` once the press is a complete binding, or
 * `null` while it is not one yet. Two presses are deliberately rejected:
 * modifier-only (the user is still reaching for the key) and a bare key with
 * no modifier at all, which would swallow that key system-wide - typing "p"
 * anywhere would hide the pet.
 */
export function accelaratorFromEvent(event) {
  const parts = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  if (event.metaKey) parts.push('Cmd');
  const key = baseKey(event);
  if (!key) return null;
  if (parts.length === 0) return null;
  parts.push(key);
  return { accelerator: parts.join('+'), parts };
}

/** Split a stored accelerator into the chips the settings row renders. */
export function acceleratorParts(accelerator) {
  if (!accelerator) return [];
  return accelerator.split('+').filter(Boolean);
}

/** macOS-facing label for one accelerator part. */
export function partGlyph(part) {
  return MODIFIER_GLYPHS[part] || part;
}
