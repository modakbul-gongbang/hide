import { FolderIcon, GitBranchIcon } from "lucide-react";
import { useLayoutEffect, useRef } from "react";
import type { Actions } from "./actions";
import { Hint } from "./components/ui/tooltip";
import { ExplorerTree } from "./ExplorerTree";
import { HistoryList } from "./HistoryList";
import { useUiStore } from "./ui";
import type { Tool } from "./workspace";

// The Workspace's tools (PRD S6 D-05, B10; issue 170, "Side panel hierarchy,
// revised"): the Explorer or History, one at a time, in the side panel's tool
// column right of the View areas, or as the whole panel while no view is
// open. The tool is the front Workspace's, so switching it here leaves every
// other Workspace's alone. The column carries its icon tabs on the panel's
// second row, level with the agents' tab strip; there is no title row and no
// close, since the panel's first row holds the column's toggle. The names are
// docs/UI_BEHAVIOR.md's current ones.
//
// When the panel cannot give a View area its minimum beside the column, the
// same tool floats over the View areas below the panel's first row instead,
// and only once the operator asks for it (S7 B12, D-08): the overlay never
// opens by itself. It takes the keyboard when it opens; Escape inside it, or
// a click outside, closes it and gives the keyboard back to whatever had it,
// unless the click itself gave the keyboard to what it landed on. Closing it
// is this page's only, so the core still holds the column shown and a wider
// window brings it back.

export const TOOL_NAMES: Record<Tool, string> = { explorer: "Explorer", changes: "History" };

export function Tools({
  tool,
  overlay,
  alone = false,
  header = null,
  actions,
}: {
  /** The tool shown, or null while the column is hidden. */
  tool: Tool | null;
  overlay: boolean;
  /** The panel holds nothing else, so the column is the panel and its tabs sit on the first row beside `header`. */
  alone?: boolean;
  /** The panel's actions, over the column on the first row. */
  header?: React.ReactNode;
  actions: Actions;
}) {
  const panel = useRef<HTMLElement>(null);
  // What had the keyboard when the overlay opened (the column toggle, a terminal).
  const invoker = useRef<HTMLElement | null>(null);
  const open = overlay && tool !== null;
  const giveBack = () => {
    const back = invoker.current?.isConnected ? invoker.current : document.querySelector<HTMLElement>("[data-tools-toggle]");
    back?.focus({ preventScroll: true });
  };
  const giveBackRef = useRef(giveBack);
  giveBackRef.current = giveBack;
  useLayoutEffect(() => {
    if (!open) return undefined;
    const active = document.activeElement;
    invoker.current = active instanceof HTMLElement && active !== document.body && !panel.current?.contains(active) ? active : null;
    panel.current?.focus({ preventScroll: true });
    // A press on the column toggle is its to answer, and a dialog the tools
    // opened (Move to Trash) is part of them. A press elsewhere closes the
    // overlay and keeps the focus it gives; a press on something that takes
    // no focus would leave the keyboard nowhere once the overlay is gone, so
    // it goes back where the overlay was asked for (B12). The check waits
    // for the press to have focused what it landed on.
    const outside = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      if (!target || panel.current?.contains(target) || target.closest('[data-tools-toggle], [role="dialog"], [role="alertdialog"]')) return;
      useUiStore.getState().closeTools();
      window.setTimeout(() => {
        const active = document.activeElement;
        if (active === null || active === document.body || !active.isConnected) giveBackRef.current();
      }, 0);
    };
    window.addEventListener("pointerdown", outside, true);
    return () => window.removeEventListener("pointerdown", outside, true);
  }, [open]);
  if (tool === null) return null;
  // Escape reaches here only from inside the overlay, after anything inside
  // it that answers Escape itself (a name field, a menu) has had it.
  const closeFromKeyboard = (event: React.KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    event.stopPropagation();
    useUiStore.getState().closeTools();
    giveBack();
  };
  const body = (
    <section className="flex min-h-0 flex-1 flex-col" aria-label={TOOL_NAMES[tool]} data-tool={tool} data-right-panel={tool}>
      {/* The tree reads the selected device's checkout through its helper;
          History reads the front checkout's Git through its own host. */}
      {tool === "explorer" ? <ExplorerTree actions={actions} /> : <HistoryList actions={actions} />}
    </section>
  );
  if (alone) {
    return (
      <aside className="flex h-full min-w-0 flex-1 flex-col text-foreground" aria-label="Workspace tools" data-workspace-tools={tool}>
        <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center border-b border-border" data-panel-row="1">
          <ToolTabs tool={tool} actions={actions} />
          <span className="min-w-0 flex-1" />
          {header}
        </div>
        {body}
      </aside>
    );
  }
  return (
    <aside
      ref={panel}
      tabIndex={overlay ? -1 : undefined}
      onKeyDown={overlay ? closeFromKeyboard : undefined}
      className={
        overlay
          ? "absolute bottom-0 right-0 top-[var(--size-tab-strip)] z-20 flex w-[var(--size-panel-ideal)] max-w-full flex-col border-l border-border bg-card text-foreground outline-none"
          : "flex h-full min-w-[var(--size-panel-min)] shrink-[1000] grow-0 basis-[var(--size-panel-ideal)] flex-col text-foreground"
      }
      aria-label="Workspace tools"
      data-workspace-tools={tool}
      data-tools-overlay={overlay ? "true" : undefined}
    >
      {header && !overlay ? (
        <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center justify-end border-b border-border" data-panel-row="1">
          {header}
        </div>
      ) : null}
      <div className={`flex min-h-0 flex-1 flex-col ${overlay ? "" : "border-l border-border"}`}>
        <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center border-b border-border" data-panel-row="2">
          <ToolTabs tool={tool} actions={actions} />
        </div>
        {body}
      </div>
    </aside>
  );
}

/** The Explorer and History icon tabs: the shown one marked, a press on the other swaps the column onto it. */
function ToolTabs({ tool, actions }: { tool: Tool; actions: Actions }) {
  return (
    <div role="tablist" aria-label="Workspace tools" className="flex h-full shrink-0 items-stretch" data-tool-tabs="true">
      {(["explorer", "changes"] as const).map((each) => {
        const selected = each === tool;
        const Icon = each === "explorer" ? FolderIcon : GitBranchIcon;
        return (
          <Hint key={each} label={TOOL_NAMES[each]}>
            <button
              type="button"
              role="tab"
              aria-selected={selected}
              aria-label={TOOL_NAMES[each]}
              data-tool-tab={each}
              className={`relative flex w-[var(--size-tab-strip)] items-center justify-center outline-none hover:bg-accent focus-visible:bg-accent ${selected ? "text-foreground" : "text-subtle-foreground"}`}
              onClick={() => actions.showTool(each)}
            >
              <Icon aria-hidden="true" className="size-(--size-icon)" />
              {selected ? <span className="absolute inset-x-0 bottom-0 h-[var(--size-tab-indicator)] bg-primary" /> : null}
            </button>
          </Hint>
        );
      })}
    </div>
  );
}
