import { XIcon } from "lucide-react";
import { useLayoutEffect, useRef } from "react";
import type { Actions } from "./actions";
import { Button } from "./components/ui/button";
import { Hint } from "./components/ui/tooltip";
import { ExplorerTree } from "./ExplorerTree";
import { HistoryList } from "./HistoryList";
import { useUiStore } from "./ui";

// The Workspace's tools (PRD S6 D-05, B10; issue 170): Explorer and History,
// each opened and closed on its own, in the side panel's tool column right of
// the View areas, or alone in the panel while no view is open. Both are the
// front Workspace's, so closing one here leaves every other Workspace's alone.
// With both open they share the column, Explorer above History. The names are
// docs/UI_BEHAVIOR.md's current ones. The column's top row holds the panel's
// own actions (`header`), at the right end of the panel's one strip.
//
// When the panel cannot give a View area its minimum beside the column, the
// same tools float over the View areas instead, and only once the operator
// asks for one (S7 B12, D-08): the overlay never opens by itself.
// It takes the keyboard when it opens; Escape inside it, or a click outside,
// closes it and gives the keyboard back to whatever had it, unless the click
// itself gave the keyboard to what it landed on. Closing it is this page's
// only, so the core still holds the tools shown and a wider window brings the
// column back.

export function Tools({
  explorer,
  changes,
  overlay,
  alone = false,
  header = null,
  actions,
}: {
  explorer: boolean;
  changes: boolean;
  overlay: boolean;
  /** The panel holds nothing else, so the column is the panel. */
  alone?: boolean;
  header?: React.ReactNode;
  actions: Actions;
}) {
  const panel = useRef<HTMLElement>(null);
  // What had the keyboard when the overlay opened (a toggle, a terminal).
  const invoker = useRef<HTMLElement | null>(null);
  const open = overlay && (explorer || changes);
  const toggle = explorer ? "explorer" : "changes";
  const giveBack = () => {
    const back = invoker.current?.isConnected ? invoker.current : document.querySelector<HTMLElement>(`[data-tool-toggle="${toggle}"]`);
    back?.focus({ preventScroll: true });
  };
  const giveBackRef = useRef(giveBack);
  giveBackRef.current = giveBack;
  useLayoutEffect(() => {
    if (!open) return undefined;
    const active = document.activeElement;
    invoker.current = active instanceof HTMLElement && active !== document.body && !panel.current?.contains(active) ? active : null;
    panel.current?.focus({ preventScroll: true });
    // A press on the toggles is theirs to answer, and a dialog the tools
    // opened (Move to Trash) is part of them. A press elsewhere closes the
    // overlay and keeps the focus it gives; a press on something that takes
    // no focus would leave the keyboard nowhere once the overlay is gone, so
    // it goes back where the overlay was asked for (B12). The check waits
    // for the press to have focused what it landed on.
    const outside = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      if (!target || panel.current?.contains(target) || target.closest('[data-tool-toggle], [role="dialog"], [role="alertdialog"]')) return;
      useUiStore.getState().closeTools();
      window.setTimeout(() => {
        const active = document.activeElement;
        if (active === null || active === document.body || !active.isConnected) giveBackRef.current();
      }, 0);
    };
    window.addEventListener("pointerdown", outside, true);
    return () => window.removeEventListener("pointerdown", outside, true);
  }, [open]);
  if (!explorer && !changes) return null;
  // Escape reaches here only from inside the overlay, after anything inside
  // it that answers Escape itself (a name field, a menu) has had it.
  const closeFromKeyboard = (event: React.KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    event.stopPropagation();
    useUiStore.getState().closeTools();
    giveBack();
  };
  return (
    <aside
      ref={panel}
      tabIndex={overlay ? -1 : undefined}
      onKeyDown={overlay ? closeFromKeyboard : undefined}
      className={
        overlay
          ? "absolute inset-y-0 right-0 z-20 flex w-[var(--size-panel-ideal)] max-w-full flex-col border-l border-border bg-card text-foreground shadow-lg outline-none"
          : alone
            ? "flex h-full min-w-0 flex-1 flex-col bg-card text-foreground"
            : "flex h-full min-w-[var(--size-panel-min)] shrink-[1000] grow-0 basis-[var(--size-panel-ideal)] flex-col border-l border-border bg-card text-foreground"
      }
      aria-label="Workspace tools"
      data-workspace-tools={[explorer ? "explorer" : "", changes ? "changes" : ""].filter(Boolean).join(" ")}
      data-tools-overlay={overlay ? "true" : undefined}
    >
      {header && !overlay ? <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center justify-end">{header}</div> : null}
      {explorer ? (
        <ToolSection title="Explorer" tool="explorer" onClose={() => actions.setTool("explorer", false)}>
          {/* The tree reads the selected device's checkout through its helper. */}
          <ExplorerTree actions={actions} />
        </ToolSection>
      ) : null}
      {changes ? (
        <ToolSection title="History" tool="changes" onClose={() => actions.setTool("changes", false)} divided={explorer}>
          {/* History reads the front checkout's Git through its own host. */}
          <HistoryList actions={actions} />
        </ToolSection>
      ) : null}
    </aside>
  );
}

function ToolSection({ title, tool, divided = false, onClose, children }: { title: string; tool: string; divided?: boolean; onClose: () => void; children: React.ReactNode }) {
  return (
    <section className={`flex min-h-0 flex-1 flex-col ${divided ? "border-t border-border" : ""}`} aria-label={title} data-tool={tool} data-right-panel={tool === "explorer" ? "explorer" : "changes"}>
      <header className="flex h-[var(--size-pane-header)] shrink-0 items-center gap-xs px-md text-caption text-subtle-foreground">
        <h2 className="min-w-0 flex-1 truncate font-semibold uppercase text-muted-foreground">{title}</h2>
        <Hint label={`Hide ${title}`}>
          <Button variant="ghost" size="icon-sm" data-tool-close={tool} onClick={onClose}>
            <XIcon />
          </Button>
        </Hint>
      </header>
      <div className="flex min-h-0 flex-1 flex-col">{children}</div>
    </section>
  );
}
