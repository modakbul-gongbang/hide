import type { Actions } from "./actions";
import { ExplorerTree } from "./ExplorerTree";
import { HistoryList } from "./HistoryList";
import { focusedRemoteDevice } from "./snapshot";
import { useShellStore } from "./store";

// Core UI state owns section and visibility. Switching sections does not
// touch the editor, focused pane or Explorer expansion state.

export function RightPanel({ actions }: { actions: Actions }) {
  const visible = useShellStore((s) => s.rest?.ui_state?.right_panel_visible ?? false);
  const section = useShellStore((s) => s.rest?.ui_state?.right_panel_section ?? "explorer");
  const remoteLabel = useShellStore((s) => focusedRemoteDevice(s.rest)?.label ?? null);
  if (!visible || (section !== "explorer" && section !== "changes")) return null;
  return (
    <aside
      className="flex h-full w-[var(--size-panel-ideal)] shrink-0 flex-col bg-panel text-primary"
      aria-label="Right panel"
      data-right-panel={section}
      data-right-panel-section={section}
    >
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-xs px-md text-caption">
        <button type="button" aria-pressed={section === "explorer"} className={section === "explorer" ? "text-primary" : "text-muted hover:text-secondary"} onClick={() => actions.showRightPanelSection("explorer")}>Explorer</button>
        <button type="button" aria-pressed={section === "changes"} className={section === "changes" ? "text-primary" : "text-muted hover:text-secondary"} onClick={() => actions.showRightPanelSection("changes")}>History</button>
        <span className="flex-1" />
        <button
          type="button"
          data-right-panel-collapse="true"
          aria-label="Hide right panel"
          title="Hide right panel (⌘⇧B)"
          className="text-muted hover:text-secondary"
          onClick={() => actions.toggleRightPanel()}
        >
          ⌘⇧B
        </button>
      </div>
      {section === "explorer" ? (
        // The tree reads the selected device's checkout through its helper.
        <ExplorerTree actions={actions} />
      ) : remoteLabel ? (
        // History reads this machine's Git until the device's helper serves
        // Git; showing it here would put this machine's changes behind the
        // selected device.
        <p className="p-md text-caption text-muted" data-right-panel-remote="true">
          History for {remoteLabel} is not available yet. Its files are in the Explorer.
        </p>
      ) : (
        <HistoryList actions={actions} />
      )}
    </aside>
  );
}
