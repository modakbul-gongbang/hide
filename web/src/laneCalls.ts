import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import { readCalls } from "./workspace";

// An unpinned lane comes up when it is called (PRD right-lane B16, B17): an
// open, a reveal, a History selection or a Recent Panels display, from this
// page or not (a CLI `--reveal`, another page), and every such call reaches
// the page the same way: the core numbers it and publishes the front
// Workspace's last number. This reads that number on every snapshot, whether
// or not the Workspace screen is drawn, so a call made while the Overview
// showed still counts. Another Workspace in front starts with its unpinned
// lane put away, unless the call brought it there.

useShellStore.subscribe((state, previous) => {
  const view = state.rest?.workspace_view;
  if (view === previous.rest?.workspace_view) return;
  const ui = useUiStore.getState();
  const { seen, reset, call } = readCalls(ui.laneCallsSeen, view);
  if (seen !== ui.laneCallsSeen) useUiStore.setState({ laneCallsSeen: seen });
  if (reset) ui.putLaneAway();
  if (call) ui.showLane();
});
