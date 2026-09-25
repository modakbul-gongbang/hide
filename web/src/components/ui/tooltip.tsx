import { Tooltip as TooltipPrimitive } from "radix-ui";
import { useRef, useState, type ComponentProps, type ReactNode } from "react";
import { cn } from "../../lib/utils";

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
  // A press on the trigger (a click that opens a menu or a dialog, or a
  // right-click that opens a context menu) ends the hint until the pointer
  // comes back to the trigger or the keyboard leaves it for somewhere else. The
  // hover delay that started before the press, and the focus a closing menu
  // hands back to its trigger, would otherwise open it over or after whatever
  // the press opened; Tab to the trigger later still shows it.
  const [open, setOpen] = useState(false);
  const pressed = useRef(false);
  const press = () => {
    pressed.current = true;
    setOpen(false);
  };
  return (
    <Tooltip open={open} onOpenChange={(next) => setOpen(next && !pressed.current)}>
      <TooltipTrigger
        asChild
        aria-label={reveals ? undefined : label}
        onPointerDown={press}
        onContextMenu={press}
        onPointerEnter={() => {
          pressed.current = false;
        }}
        onBlur={(event) => {
          const next = event.relatedTarget;
          if (!(next instanceof Element && next.closest(LAYER))) pressed.current = false;
        }}
      >
        {children}
      </TooltipTrigger>
      <TooltipContent side={side}>
        <span className="inline-flex items-center gap-sm">
          {label}
          {shortcut ? <span className="text-muted-foreground">{shortcut}</span> : null}
        </span>
      </TooltipContent>
    </Tooltip>
  );
}

export { Hint, Tooltip, TooltipContent, TooltipProvider, TooltipTrigger };
