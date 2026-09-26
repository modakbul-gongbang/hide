// Hide's action menus on the System menu parts (PRD S6 D-13, B18; S5 B11,
// B19). A target's menu acts on the target it was opened on; opening it
// changes nothing - no focus, read state or selection - and Escape closes it
// without choosing. An item the target cannot use is drawn disabled with its
// reason, never hidden behind an action that would fail.

import { DropdownMenu as MenuPrimitive } from "radix-ui";
import { EllipsisIcon } from "lucide-react";
import { Fragment, useState, type ReactNode } from "react";
import { cn } from "../lib/utils";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "./ui/context-menu";
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
};

type Parts = { Item: typeof DropdownMenuItem | typeof ContextMenuItem; Separator: typeof DropdownMenuSeparator | typeof ContextMenuSeparator };

function EntryItems<Id extends string>({ items, onSelect, parts }: { items: MenuEntry<Id>[]; onSelect: (id: Id) => void; parts: Parts }) {
  const { Item, Separator } = parts;
  return (
    <>
      {items.map((item) => (
        <Fragment key={item.id}>
          {item.separated ? <Separator /> : null}
          <Item disabled={item.unavailable !== null} variant={item.destructive ? "destructive" : "default"} data-menu-item={item.id} className="flex-col items-start gap-none" onSelect={() => onSelect(item.id)}>
            <span>{item.label}</span>
            {item.unavailable ? <span className="text-caption text-muted-foreground">{item.unavailable}</span> : null}
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
 * portal outside the target.
 */
export function EntryContextMenu<Id extends string>({
  label,
  items,
  onSelect,
  children,
  className,
  ...data
}: {
  label: string;
  items: () => MenuEntry<Id>[];
  onSelect: (id: Id) => void;
  children: ReactNode;
  className?: string;
} & Record<`data-${string}`, string>) {
  const [entries, setEntries] = useState<MenuEntry<Id>[]>([]);
  return (
    <ContextMenu onOpenChange={(open) => setEntries(open ? items() : [])}>
      <ContextMenuTrigger className={cn("relative", className)} {...data}>
        {children}
      </ContextMenuTrigger>
      {entries.length ? (
        <ContextMenuContent aria-label={label} {...data}>
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

/**
 * The `⋯` menu on a sidebar row, also opened by a right-click on the row, the
 * way the native project and checkout menus open (WorktreeMenuPolicy). The
 * trigger shows only while the row is under the pointer, focused or open; the
 * row places it, filling a positioned slot, so it can stand in for whatever
 * that slot shows at rest.
 */
export function RowMenu<Id extends string>({
  label,
  items,
  onSelect,
  children,
  ...data
}: {
  label: string;
  items: MenuEntry<Id>[];
  onSelect: (id: Id) => void;
  /** The row itself, given the trigger to place; a right-click on it opens the same menu. */
  children: (trigger: ReactNode) => ReactNode;
} & Record<`data-${string}`, string>) {
  const [open, setOpen] = useState(false);
  const trigger = (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <Hint label={label}>
        <DropdownMenuTrigger
          className={cn(
            "absolute inset-0 flex items-center justify-center text-muted-foreground outline-none hover:text-foreground focus-visible:text-foreground",
            open ? "text-foreground" : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100",
          )}
          {...data}
        >
          <EllipsisIcon className="size-(--size-icon)" />
        </DropdownMenuTrigger>
      </Hint>
      <DropdownMenuContent aria-label={label} align="end">
        <EntryItems items={items} onSelect={onSelect} parts={{ Item: DropdownMenuItem, Separator: DropdownMenuSeparator }} />
      </DropdownMenuContent>
    </DropdownMenu>
  );
  return (
    <EntryContextMenu label={label} items={() => items} onSelect={onSelect} className="group flex items-stretch">
      {children(trigger)}
    </EntryContextMenu>
  );
}
