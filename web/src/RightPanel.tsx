import type { Actions } from "./actions";
import { ExplorerTree } from "./ExplorerTree";
import { useShellStore } from "./store";

// The right panel (D-13): the Explorer's home, drawn where the core's
// `right_panel_visible` and `right_panel_section` say. The core owns both, so
// this reads them and dispatches the change the operator asked for; the
// panel's other sections arrive with S4 and S5, which is why only the
// Explorer section has a body.

export function RightPanel({ actions }: { actions: Actions }) {
  const visible = useShellStore((s) => s.rest?.ui_state?.right_panel_visible ?? false);
  const section = useShellStore((s) => s.rest?.ui_state?.right_panel_section ?? null);
  if (!visible || section !== "explorer") return null;
  return (
    <aside
      className="flex h-full w-[var(--size-panel-ideal)] shrink-0 flex-col bg-panel text-primary"
      data-right-panel="explorer"
      data-right-panel-section="explorer"
    >
      <div className="flex h-[var(--size-tab-strip)] shrink-0 items-center gap-sm px-md text-caption">
        <span data-right-panel-title="true">Explorer</span>
        <span className="flex-1" />
        <button
          type="button"
          data-right-panel-collapse="true"
          className="text-muted hover:text-secondary"
          onClick={() => actions.toggleRightPanel()}
        >
          ⌘⇧B
        </button>
      </div>
      <ExplorerTree actions={actions} />
    </aside>
  );
}
