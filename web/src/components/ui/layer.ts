// How a System layer (menu, dialog, popover, select) joins the shell's one
// Escape owner. `keyboard.ts` answers Escape in the window's capture phase,
// before Radix's own document listener and before xterm could send the byte,
// and closes the innermost registered layer first; so every open layer
// registers its close here, and nested layers close innermost-first.

import { useCallback, useEffect, useRef, useState } from "react";
import { restoreFocus } from "../../terminals";
import { useUiStore } from "../../ui";

/** While `open`, Escape runs `close` before any layer opened earlier. */
export function useEscapeLayer(open: boolean, close: () => void) {
  const latest = useRef(close);
  useEffect(() => {
    latest.current = close;
  });
  useEffect(() => {
    if (!open) return;
    return useUiStore.getState().pushEscape(() => latest.current());
  }, [open]);
}

/**
 * A Radix root's open state, controlled or not, that also registers the
 * Escape layer. A controlled caller keeps its own state; an uncontrolled one
 * gets it here.
 */
export function useLayerOpen(open: boolean | undefined, defaultOpen: boolean | undefined, onOpenChange: ((open: boolean) => void) | undefined) {
  const [own, setOwn] = useState(defaultOpen ?? false);
  const current = open ?? own;
  const change = useCallback(
    (next: boolean) => {
      if (open === undefined) setOwn(next);
      onOpenChange?.(next);
    },
    [open, onOpenChange],
  );
  useEscapeLayer(current, () => change(false));
  return [current, change] as const;
}

/**
 * The element that held the keyboard when a layer opened, so closing it can
 * hand the keyboard back through `restoreFocus`, which knows that a terminal
 * is focused through its pane rather than through a stale textarea.
 */
export function useReturnFocus(open: boolean) {
  const previous = useRef<HTMLElement | null>(null);
  const wasOpen = useRef(false);
  // Read on the render that opens the layer, before Radix moves focus into it;
  // Radix hands focus back after a timeout, so the value must outlive the close.
  if (open && !wasOpen.current && typeof document !== "undefined") {
    previous.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }
  wasOpen.current = open;
  return useCallback((event: Event) => {
    const target = previous.current;
    if (!target) return;
    event.preventDefault();
    restoreFocus(target);
  }, []);
}
