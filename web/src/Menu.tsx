import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useUiStore } from "./ui";

// The menu a target opens by right-click, by the keyboard's menu key or
// ⇧F10, or by its own overflow control (PRD S6 D-13, B18). It acts on the
// target it was opened on; opening it changes nothing - no focus, no read
// state, no selection - and Escape closes it without choosing. An item the
// target cannot use is drawn disabled with its reason.

export type MenuEntry<Id extends string = string> = {
  id: Id;
  label: string;
  /** Why the action is not offered here; the item is drawn disabled with this as its hint. */
  unavailable: string | null;
  /** A destructive or closing action, drawn after a separator. */
  separated?: boolean;
};

export function MenuList<Id extends string>({
  label,
  items,
  onSelect,
  onClose,
  className,
  style,
}: {
  label: string;
  items: MenuEntry<Id>[];
  onSelect: (id: Id) => void;
  onClose: () => void;
  className: string;
  style?: React.CSSProperties;
}) {
  const list = useRef<HTMLUListElement>(null);
  // The latest close, so a parent that passes a new closure on every render
  // (a snapshot arriving) does not re-run the mount: that would focus the
  // first item again and lose the keyboard's place.
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });
  useEffect(() => {
    const node = list.current;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const remove = useUiStore.getState().pushEscape(() => close.current());
    node?.querySelector<HTMLButtonElement>("button:not([disabled])")?.focus();
    const outside = (event: PointerEvent) => {
      if (!node?.contains(event.target as Node)) close.current();
    };
    window.addEventListener("pointerdown", outside, true);
    return () => {
      remove();
      window.removeEventListener("pointerdown", outside, true);
      // Closing hands the keyboard back to the control that opened the menu,
      // unless the close already put it somewhere (`ContextMenu`, a click).
      const active = document.activeElement;
      if (opener?.isConnected && (!active || active === document.body || node?.contains(active))) opener.focus({ preventScroll: true });
    };
  }, []);
  const move = (event: KeyboardEvent<HTMLUListElement>) => {
    if (event.key === "Tab") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const buttons = [...(list.current?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = buttons[(index + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length];
    next?.focus();
  };
  return (
    <ul
      ref={list}
      role="menu"
      aria-label={label}
      onKeyDown={move}
      style={style}
      className={`z-30 w-[var(--size-settings-control-w)] rounded-md border border-border bg-popover py-xxs text-body shadow-lg ${className}`}
    >
      {items.map((item) => (
        <li key={item.id} role="none" className={item.separated ? "mt-xxs border-t border-border pt-xxs" : undefined}>
          <button
            type="button"
            role="menuitem"
            disabled={item.unavailable !== null}
            title={item.unavailable ?? undefined}
            data-menu-item={item.id}
            className="flex w-full flex-col items-start px-sm py-xxs text-left text-foreground outline-none hover:bg-accent focus-visible:bg-accent disabled:text-muted-foreground"
            onClick={() => {
              onClose();
              onSelect(item.id);
            }}
          >
            <span>{item.label}</span>
            {item.unavailable ? <span className="text-caption text-muted-foreground">{item.unavailable}</span> : null}
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * Wraps a target so a right-click or the menu key opens its menu at the
 * pointer or under the target. `items` is read when the menu opens, so a
 * target whose state changed while the menu was closed offers what it can
 * do now.
 */
export function ContextMenu<Id extends string>({
  label,
  items,
  onSelect,
  children,
  className = "",
  ...data
}: {
  label: string;
  items: () => MenuEntry<Id>[];
  onSelect: (id: Id) => void;
  children: ReactNode;
  className?: string;
} & Record<`data-${string}`, string>) {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const host = useRef<HTMLDivElement>(null);
  // Closing hands the keyboard back to the target the menu was opened on.
  const close = useRef(() => {
    setAt(null);
    host.current?.querySelector<HTMLElement>("[tabindex], button")?.focus({ preventScroll: true });
  }).current;
  // Viewport coordinates, drawn fixed: a target inside a scrolling strip
  // (the tab rows) would otherwise clip its own menu.
  const openAt = (x: number, y: number) => setAt({ x, y });
  const entries = at ? items() : [];
  return (
    <div
      ref={host}
      className={`relative ${className}`}
      {...data}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
        openAt(event.clientX, event.clientY);
      }}
      onKeyDown={(event) => {
        if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
          event.preventDefault();
          const box = host.current?.getBoundingClientRect();
          openAt((box?.left ?? 0) + 8, (box?.bottom ?? 0));
        }
      }}
    >
      {children}
      {at && entries.length > 0 ? (
        <MenuList
          label={label}
          items={entries}
          onSelect={onSelect}
          onClose={close}
          className="fixed"
          style={{ left: at.x, top: at.y }}
        />
      ) : null}
    </div>
  );
}
