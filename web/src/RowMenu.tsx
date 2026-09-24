// The `⋯` menu on a sidebar row, also opened by a right-click on the row, the
// way the native project and checkout menus open (WorktreeMenuPolicy). An
// item the row cannot use is drawn disabled with its reason, never hidden
// behind an action that would fail (PRD S5 B11, B19).

import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useUiStore } from "./ui";
import type { MenuItem } from "./workspaceManage";

export function RowMenu({
  label,
  items,
  onSelect,
  children,
  ...data
}: {
  label: string;
  items: MenuItem[];
  onSelect: (item: MenuItem["id"]) => void;
  /** The row itself; a right-click on it opens the same menu. */
  children: ReactNode;
} & Record<`data-${string}`, string>) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (!open) return;
    const remove = useUiStore.getState().pushEscape(() => {
      setOpen(false);
      trigger.current?.focus();
    });
    list.current?.querySelector<HTMLButtonElement>("button:not([disabled])")?.focus();
    const outside = (event: PointerEvent) => {
      if (!list.current?.contains(event.target as Node) && event.target !== trigger.current) setOpen(false);
    };
    window.addEventListener("pointerdown", outside, true);
    return () => {
      remove();
      window.removeEventListener("pointerdown", outside, true);
    };
  }, [open]);
  const move = (event: KeyboardEvent<HTMLUListElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const buttons = [...(list.current?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = buttons[(index + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length];
    next?.focus();
  };
  return (
    <div
      className="group relative flex items-stretch"
      onContextMenu={(event) => {
        event.preventDefault();
        setOpen(true);
      }}
    >
      <div className="min-w-0 flex-1">{children}</div>
      <button
        ref={trigger}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        title={label}
        className={`w-[var(--size-icon-button-standard)] shrink-0 text-muted outline-none hover:text-primary focus-visible:text-primary ${open ? "text-primary" : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100"}`}
        onClick={() => setOpen(!open)}
        {...data}
      >
        ⋯
      </button>
      {open ? (
        <ul ref={list} role="menu" aria-label={label} onKeyDown={move} className="absolute right-xs top-full z-30 w-[var(--size-settings-control-w)] rounded-md border border-divider bg-balloon py-xxs text-body shadow-lg">
          {items.map((item) => (
            <li key={item.id} role="none">
              <button
                type="button"
                role="menuitem"
                disabled={item.unavailable !== null}
                title={item.unavailable ?? undefined}
                data-menu-item={item.id}
                className="flex w-full flex-col items-start px-sm py-xxs text-left text-primary outline-none hover:bg-elevated focus-visible:bg-elevated disabled:text-muted"
                onClick={() => {
                  setOpen(false);
                  onSelect(item.id);
                }}
              >
                <span>{item.label}</span>
                {item.unavailable ? <span className="text-caption text-muted">{item.unavailable}</span> : null}
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
