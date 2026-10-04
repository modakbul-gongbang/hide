import { FolderIcon, GitBranchIcon } from "lucide-react";
import type { Actions } from "./actions";
import { Hint } from "./components/ui/tooltip";
import { ExplorerTree } from "./ExplorerTree";
import { HistoryList } from "./HistoryList";
import type { Tool } from "./workspace";

// The Tools column (PRD S6 D-05, B10; PRD three-column-panel D-01, D-04): the
// Explorer or History, one at a time, docked at the Workspace body's right
// edge. The tool is the front Workspace's, so switching it here leaves every
// other Workspace's alone. The column has no title row and no close: its
// first row is its Explorer and History icon tabs, level with the other
// columns' tab rows, and the toolbar's Tools icon and ⌘E turn it off. The
// names are docs/UI_BEHAVIOR.md's current ones.

export const TOOL_NAMES: Record<Tool, string> = { explorer: "Explorer", changes: "History" };

export function Tools({ tool, actions }: { tool: Tool; actions: Actions }) {
  return (
    <aside className="flex h-full min-h-0 min-w-0 flex-1 flex-col text-foreground" aria-label="Workspace tools" data-workspace-tools={tool}>
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center border-b border-border" data-column-row="tools">
        <ToolTabs tool={tool} actions={actions} />
      </div>
      <section className="flex min-h-0 flex-1 flex-col" aria-label={TOOL_NAMES[tool]} data-tool={tool} data-right-panel={tool}>
        {/* The tree reads the selected device's checkout through its helper;
            History reads the front checkout's Git through its own host. */}
        {tool === "explorer" ? <ExplorerTree actions={actions} /> : <HistoryList actions={actions} />}
      </section>
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
