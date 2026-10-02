// Hide's action menus on the System menu parts (PRD S6 D-13, B18; S5 B11,
// B19). A target's menu acts on the target it was opened on; opening it
// changes nothing - no focus, read state or selection - and Escape closes it
// without choosing. An item the target cannot use is drawn disabled with its
// reason, never hidden behind an action that would fail.

import { DropdownMenu as MenuPrimitive } from "radix-ui";
import { Fragment, useState, type CSSProperties, type ReactNode } from "react";
import { cn } from "../lib/utils";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "./ui/context-menu";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { useEscapeLayer, useReturnFocus } from "./ui/layer";
import { menuContent } from "./ui/menu-styles";
import { Hint } from "./ui/tooltip";

export type MenuEntry<Id extends string = string> = {
  id: Id;
  label: string;
  /** Why the action is not offered here; the item is drawn disabled with this as its reason. */
  unavailable: string | null;
  /** A destructive or closing action, drawn after a separator. */
  separated?: boolean;
  /** An action that removes or discards something, drawn in the destructive color. */
  destructive?: boolean;
  /** The chord that does the same, drawn at the item's end; "" or absent draws none. */
  shortcut?: string;
};

type Parts = { Item: typeof DropdownMenuItem | typeof ContextMenuItem; Separator: typeof DropdownMenuSeparator | typeof ContextMenuSeparator };

function EntryItems<Id extends string>({ items, onSelect, parts }: { items: MenuEntry<Id>[]; onSelect: (id: Id) => void; parts: Parts }) {
  const { Item, Separator } = parts;
  return (
    <>
      {items.map((item) => (
        <Fragment key={item.id}>
          {item.separated ? <Separator /> : null}
          <Item disabled={item.unavailable !== null} variant={item.destructive ? "destructive" : "default"} data-menu-item={item.id} className="flex-col items-stretch gap-none" onSelect={() => onSelect(item.id)}>
            <span className="flex items-center gap-sm">
              <span data-menu-label="">{item.label}</span>
              {item.shortcut ? <ContextMenuShortcut data-menu-shortcut={item.shortcut}>{item.shortcut}</ContextMenuShortcut> : null}
            </span>
            {item.unavailable ? <span data-menu-reason="" className="text-caption text-muted-foreground">{item.unavailable}</span> : null}
          </Item>
        </Fragment>
      ))}
    </>
  );
}

/**
 * Wraps a target so a right-click opens its menu at the pointer, and the menu
 * key or ⇧F10 under the target. `items` is read when the menu opens, so a
 * target whose state changed while it was closed offers what it can do now.
 * The `data-*` hooks name both the target and its menu, which opens in a
 * portal outside the target. `asChild` makes the one child element the
 * target itself (a list row) instead of wrapping it. A `disabled` target, or
 * one whose `items` are empty when asked, offers no menu at all rather than
 * opening one with nothing in it.
 */
export function EntryContextMenu<Id extends string>({
  label,
  items,
  onSelect,
  children,
  className,
  style,
  onCloseAutoFocus,
  asChild = false,
  disabled = false,
  ...data
}: {
  label: string;
  items: () => MenuEntry<Id>[];
  onSelect: (id: Id) => void;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  onCloseAutoFocus?: (event: Event) => void;
  asChild?: boolean;
  disabled?: boolean;
} & Record<`data-${string}`, string>) {
  const [entries, setEntries] = useState<MenuEntry<Id>[]>([]);
  return (
    <ContextMenu
      canOpen={() => {
        const next = items();
        setEntries(next);
        return next.length > 0;
      }}
      onOpenChange={(open) => {
        if (!open) setEntries([]);
      }}
    >
      <ContextMenuTrigger asChild={asChild} disabled={disabled} className={cn("relative", className)} style={style} {...data}>
        {children}
      </ContextMenuTrigger>
      {entries.length ? (
        <ContextMenuContent aria-label={label} onCloseAutoFocus={onCloseAutoFocus} {...data}>
          <EntryItems items={entries} onSelect={onSelect} parts={{ Item: ContextMenuItem, Separator: ContextMenuSeparator }} />
        </ContextMenuContent>
      ) : null}
    </ContextMenu>
  );
}

/**
 * A menu a control opens, anchored under it. `trigger` is the control itself;
 * `hint` names an icon-only trigger in a tooltip and as its accessible name.
 */
export function EntryDropdown<Id extends string>({
  label,
  items,
  onSelect,
  trigger,
  hint,
  align = "end",
}: {
  label: string;
  items: MenuEntry<Id>[];
  onSelect: (id: Id) => void;
  trigger: ReactNode;
  hint?: string;
  align?: "start" | "center" | "end";
}) {
  const control = <DropdownMenuTrigger asChild>{trigger}</DropdownMenuTrigger>;
  return (
    <DropdownMenu>
      {hint ? <Hint label={hint}>{control}</Hint> : control}
      <DropdownMenuContent aria-label={label} align={align}>
        <EntryItems items={items} onSelect={onSelect} parts={{ Item: DropdownMenuItem, Separator: DropdownMenuSeparator }} />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * A menu opened at a point the caller chose (a pane header's click, a View
 * tab's overflow), closed by `onClose`. Closing gives the keyboard back to
 * whatever held it, as a context menu does.
 */
export function EntryPointMenu<Id extends string>({
  label,
  items,
  onSelect,
  at,
  onClose,
  ...data
}: {
  label: string;
  items: MenuEntry<Id>[];
  onSelect: (id: Id) => void;
  /** The viewport point the menu opens at. */
  at: { x: number; y: number } | null;
  onClose: () => void;
} & Record<`data-${string}`, string>) {
  useEscapeLayer(at !== null, onClose);
  const returnFocus = useReturnFocus(at !== null);
  return (
    <MenuPrimitive.Root open={at !== null} onOpenChange={(open) => (open ? undefined : onClose())}>
      <MenuPrimitive.Trigger asChild>
        <span aria-hidden="true" className="pointer-events-none fixed size-(--spacing-none)" style={{ left: at?.x ?? 0, top: at?.y ?? 0 }} />
      </MenuPrimitive.Trigger>
      <MenuPrimitive.Portal>
        <MenuPrimitive.Content aria-label={label} align="start" sideOffset={0} className={menuContent} onCloseAutoFocus={returnFocus} {...data}>
          <EntryItems items={items} onSelect={onSelect} parts={{ Item: DropdownMenuItem, Separator: DropdownMenuSeparator }} />
        </MenuPrimitive.Content>
      </MenuPrimitive.Portal>
    </MenuPrimitive.Root>
  );
}
