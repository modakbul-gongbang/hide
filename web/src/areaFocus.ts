// A focus that lands while Tab is held is the operator moving the keyboard;
// any other focus without a pointer press is the page's own (a menu handing
// the keyboard back, a restored caret) and asks nothing of the core (S7 B20).
let tabbing = false;
let owners = 0;
let detach: (() => void) | null = null;

/** Watches the Tab key for `focusFromKeyboard`; returns the removal. */
function attachFocusModality(): () => void {
  const down = (event: KeyboardEvent) => {
    if (event.key === "Tab") tabbing = true;
  };
  const up = (event: KeyboardEvent) => {
    if (event.key === "Tab") tabbing = false;
  };
  const reset = () => {
    tabbing = false;
  };
  window.addEventListener("keydown", down, true);
  window.addEventListener("keyup", up, true);
  window.addEventListener("blur", reset);
  return () => {
    window.removeEventListener("keydown", down, true);
    window.removeEventListener("keyup", up, true);
    window.removeEventListener("blur", reset);
    tabbing = false;
  };
}

/** Whether the focus landing now was moved by the keyboard. */
export function focusFromKeyboard(): boolean {
  return tabbing;
}

/** All area columns share one listener set, released with its last owner. */
export function installFocusModality(): () => void {
  if (owners++ === 0) detach = attachFocusModality();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    if (--owners === 0) { detach?.(); detach = null; }
  };
}
