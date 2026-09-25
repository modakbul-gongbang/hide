import { useEffect, useRef } from "react";
import type { Actions } from "./actions";
import { ExplorerTree } from "./ExplorerTree";
import { HistoryList } from "./HistoryList";
import { useUiStore } from "./ui";

// The Workspace's tools (PRD S6 D-05, B10): Explorer and History, each opened
// and closed on its own, beside whichever areas the layout shows. Both are the
// front Workspace's, so closing one here leaves every other Workspace's alone.
// With both open they share the column, Explorer above History. The names are
// DESIGN.md's current ones.
//
// When the window cannot give the work area its minimum beside the column,
// the same tools float over the work area instead (S7 B12). Escape or a click
// outside dismisses them and the keyboard goes back to their toggle; the
// dismissal is this page's only, so the core still holds them shown and a
// wider window brings the column back.

export function Tools({ explorer, changes, overlay, actions }: { explorer: boolean; changes: boolean; overlay: boolean; actions: Actions }) {
  const panel = useRef<HTMLElement>(null);
  const open = overlay && (explorer || changes);
  useEffect(() => {
    if (!open) return undefined;
    const dismiss = () => {
      useUiStore.getState().setToolsDismissed(true);
      document.querySelector<HTMLElement>(`[data-tool-toggle="${explorer ? "explorer" : "changes"}"]`)?.focus({ preventScroll: true });
    };
    const removeEscape = useUiStore.getState().pushEscape(dismiss);
    // A press on the toggles is theirs to answer, and a dialog the tools
    // opened (Move to Trash) is part of them.
    const outside = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      if (!target || panel.current?.contains(target) || target.closest('[data-tool-toggle], [role="dialog"], [role="alertdialog"]')) return;
      dismiss();
    };
    window.addEventListener("pointerdown", outside, true);
    return () => {
      removeEscape();
      window.removeEventListener("pointerdown", outside, true);
    };
  }, [open, explorer]);
  if (!explorer && !changes) return null;
  return (
    <aside
      ref={panel}
      className={
        overlay
          ? "absolute inset-y-0 right-0 z-20 flex w-[var(--size-panel-ideal)] max-w-full flex-col border-l border-divider bg-panel text-primary shadow-lg"
          : "flex h-full min-w-[var(--size-panel-min)] shrink-[1000] grow-0 basis-[var(--size-panel-ideal)] flex-col border-l border-divider bg-panel text-primary"
      }
      aria-label="Workspace tools"
      data-workspace-tools={[explorer ? "explorer" : "", changes ? "changes" : ""].filter(Boolean).join(" ")}
      data-tools-overlay={overlay ? "true" : undefined}
    >
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
