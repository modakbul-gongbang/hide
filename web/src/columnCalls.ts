import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { readCalls } from "./workspace";

// Which column a narrow Workspace body shows follows the calls (PRD
// three-column-panel D-07). File Views is called by an open, a reveal, a
// History selection or a Recent Panels display, from this page or not (a CLI
// `--reveal`, another page), and every such call reaches the page the same
// way: the core numbers it and publishes the front Workspace's last number.
// This reads that number on every snapshot, whether or not the Workspace
// screen is drawn, so a call made while the Overview showed still counts.

useShellStore.subscribe((state, previous) => {
  const view = state.rest?.workspace_view;
  if (view === previous.rest?.workspace_view) return;
  const ui = useUiStore.getState();
  const { seen, reset, call } = readCalls(ui.viewsCallsSeen, view);
  if (seen !== ui.viewsCallsSeen) useUiStore.setState({ viewsCallsSeen: seen });
  if (reset) ui.resetColumnSlots();
  if (call) ui.callColumn("views");
});
