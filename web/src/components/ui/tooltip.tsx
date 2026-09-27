import { Tooltip as TooltipPrimitive } from "radix-ui";
import { useEffect, useRef, useState, type ComponentProps, type FocusEvent, type ReactNode } from "react";
import { cn } from "../../lib/utils";
import { useUiStore } from "../../ui";

// A hint for a control whose meaning is its icon or its shortcut. The label a
// tooltip shows is also the control's accessible name, so a screen reader and
// a pointer read the same words.

// A hint is read, never used: it holds nothing to click, so it closes when the
// pointer leaves its trigger and lets a click through to whatever it covers.
function TooltipProvider({ delayDuration = 500, disableHoverableContent = true, ...props }: ComponentProps<typeof TooltipPrimitive.Provider>) {
  return <TooltipPrimitive.Provider data-slot="tooltip-provider" delayDuration={delayDuration} disableHoverableContent={disableHoverableContent} {...props} />;
}

function Tooltip(props: ComponentProps<typeof TooltipPrimitive.Root>) {
  return <TooltipPrimitive.Root data-slot="tooltip" {...props} />;
}

function TooltipTrigger(props: ComponentProps<typeof TooltipPrimitive.Trigger>) {
  return <TooltipPrimitive.Trigger data-slot="tooltip-trigger" {...props} />;
}

function TooltipContent({ className, sideOffset = 4, children, ...props }: ComponentProps<typeof TooltipPrimitive.Content>) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        data-slot="tooltip-content"
        sideOffset={sideOffset}
        className={cn("pointer-events-none z-50 max-w-(--size-tooltip-max-width) text-balance rounded-sm border border-border bg-popover px-sm py-xs text-caption text-popover-foreground shadow-lg", className)}
        {...props}
      >
        {children}
      </TooltipPrimitive.Content>
    </TooltipPrimitive.Portal>
  );
}

/** What a press on a hinted trigger opens: a menu, a popover, or a dialog. */
const LAYER = '[data-radix-popper-content-wrapper], [role="dialog"], [role="alertdialog"]';

/**
 * The open state of a hint or a card that a trigger shows on hover and focus.
 * A press on the trigger (a click that opens a menu or a dialog, or a
 * right-click that opens a context menu) ends it until the pointer comes back
 * to the trigger or the keyboard leaves it for somewhere outside the layer the
 * press opened. The hover delay that started before the press, and the focus
 * a closing menu hands back to its trigger, would otherwise open it over or
 * after whatever the press opened; Tab to the trigger later still shows it.
 * While open it is registered with the shell as a hint, not an Escape layer:
 * Escape closes it through the tooltip's own dismiss and still reaches the
 * terminal or screen it was meant for, and an Escape the shell consumes
 * closes it from `keyboard.ts` instead.
 */
function useHintOpen() {
  const [open, setOpen] = useState(false);
  const pressed = useRef(false);
  useEffect(() => {
    if (!open) return;
    return useUiStore.getState().pushHint(() => setOpen(false));
  }, [open]);
  const press = () => {
    pressed.current = true;
    setOpen(false);
  };
  return {
    open,
    onOpenChange: (next: boolean) => setOpen(next && !pressed.current),
    triggerProps: {
      onPointerDown: press,
      onContextMenu: press,
      onPointerEnter: () => {
        pressed.current = false;
      },
      onBlur: (event: FocusEvent<HTMLElement>) => {
        const next = event.relatedTarget;
        if (!(next instanceof Element && next.closest(LAYER))) pressed.current = false;
      },
    },
  };
}

/**
 * The shell's one way to give a control a hint: the trigger keeps its own
 * element (asChild) and gets `label` as its accessible name unless it already
 * has one, and the hint shows `label` and an optional shortcut. `reveals`
 * marks a hint that shows the whole of a clipped name or path instead; that
 * text is already the element's own, so it does not become a second name.
 */
function Hint({
  label,
  shortcut,
  side,
  reveals = false,
  children,
}: {
  label: string;
  shortcut?: ReactNode;
  side?: "top" | "right" | "bottom" | "left";
  reveals?: boolean;
  children: ReactNode;
}) {
  const { open, onOpenChange, triggerProps } = useHintOpen();
  return (
    <Tooltip open={open} onOpenChange={onOpenChange}>
      <TooltipTrigger asChild aria-label={reveals ? undefined : label} {...triggerProps}>
        {children}
      </TooltipTrigger>
      <TooltipContent side={side}>
        <span className="inline-flex items-center gap-sm whitespace-pre-line">
          {label}
          {shortcut ? <span className="text-muted-foreground">{shortcut}</span> : null}
        </span>
      </TooltipContent>
    </Tooltip>
  );
}

export { Hint, Tooltip, TooltipContent, TooltipProvider, TooltipTrigger, useHintOpen };
