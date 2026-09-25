import type { Actions } from "./actions";
import { ExplorerTree } from "./ExplorerTree";
import { HistoryList } from "./HistoryList";
import { useShellStore } from "./store";
import { workspaceViewOf } from "./workspace";

// The Workspace's tools (PRD S6 D-05, B10): Explorer and History, each opened
// and closed on its own, beside whichever areas the layout shows. Both are the
// front Workspace's, so closing one here leaves every other Workspace's alone.
// With both open they share the column, Explorer above History. The names are
// DESIGN.md's current ones.

export function Tools({ actions }: { actions: Actions }) {
  const explorer = useShellStore((s) => workspaceViewOf(s.rest)?.explorer ?? false);
  const changes = useShellStore((s) => workspaceViewOf(s.rest)?.changes ?? false);
  if (!explorer && !changes) return null;
  return (
    <aside className="flex h-full min-w-[var(--size-panel-min)] shrink-[1000] grow-0 basis-[var(--size-panel-ideal)] flex-col border-l border-divider bg-panel text-primary" aria-label="Workspace tools" data-workspace-tools={[explorer ? "explorer" : "", changes ? "changes" : ""].filter(Boolean).join(" ")}>
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
    <section className={`flex min-h-0 flex-1 flex-col ${divided ? "border-t border-divider" : ""}`} aria-label={title} data-tool={tool} data-right-panel={tool === "explorer" ? "explorer" : "changes"}>
      <header className="flex h-[var(--size-pane-header)] shrink-0 items-center gap-xs px-md text-caption text-secondary">
        <h2 className="min-w-0 flex-1 truncate font-semibold uppercase text-muted">{title}</h2>
        <button
          type="button"
          aria-label={`Hide ${title}`}
          title={`Hide ${title}`}
          data-tool-close={tool}
          className="flex h-[var(--size-icon-button-toolbar)] w-[var(--size-icon-button-toolbar)] items-center justify-center rounded-xs text-muted hover:bg-elevated hover:text-primary focus-visible:bg-elevated"
          onClick={onClose}
        >
          ×
        </button>
      </header>
      <div className="flex min-h-0 flex-1 flex-col">{children}</div>
    </section>
  );
}
