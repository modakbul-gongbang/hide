import { describe, expect, it } from "vitest";
import { paneMenuItems, terminalMenuItems, type TerminalMenuContext } from "./PaneRelations";
import type { PaneRow } from "./snapshot";

const pane = { id: "w1:p1", children: null, lineage_path: [] } as unknown as PaneRow;
const chords = { copy: "⌘C", paste: "⌘V", find: "⌘F", splitRight: "⌘D", splitDown: "⌘⇧D", zoom: "⌥⌘↩" };
const context = (over: Partial<TerminalMenuContext>): TerminalMenuContext => ({ selection: false, zoomed: false, paneCount: 2, chords, ...over });

describe("a right-click in a terminal", () => {
  it("edits the text, then lays out the tab, then offers the header's pane actions", () => {
    const items = terminalMenuItems(pane, "reviewer", context({ selection: true }));
    expect(items.map((item) => item.id)).toEqual(["copy", "paste", "select_all", "find", "split_right", "split_down", "toggle_zoom", "copy_name", "copy_pane_id", "close_pane"]);
    expect(items.filter((item) => item.separated).map((item) => item.id)).toEqual(["split_right", "copy_name", "close_pane"]);
    expect(items.find((item) => item.id === "split_down")?.shortcut).toBe("⌘⇧D");
  });

  it("offers Copy only over a selection", () => {
    expect(terminalMenuItems(pane, "reviewer", context({})).some((item) => item.id === "copy")).toBe(false);
  });

  it("zooms a pane that has siblings, unzooms a zoomed one, and says why a lone pane cannot zoom", () => {
    const zoom = (over: Partial<TerminalMenuContext>) => terminalMenuItems(pane, "reviewer", context(over)).find((item) => item.id === "toggle_zoom");
    expect(zoom({ paneCount: 2 })).toMatchObject({ label: "Zoom pane", unavailable: null });
    expect(zoom({ paneCount: 3, zoomed: true })).toMatchObject({ label: "Unzoom pane", unavailable: null });
    expect(zoom({ paneCount: 1 })).toMatchObject({ label: "Zoom pane", unavailable: "This pane is the only one in its tab" });
  });
});

describe("the pane menu's Fork agent item", () => {
  const forkItem = (fork?: PaneRow["fork"], sleep_action?: PaneRow["sleep_action"]) =>
    paneMenuItems({ id: "w1:p1", children: null, lineage_path: [], fork, sleep_action } as unknown as PaneRow, "reviewer").find((item) => item.id === "fork_agent");

  it("is offered on a pane the core can fork", () => {
    expect(forkItem({ available: true })).toMatchObject({ label: "Fork agent", unavailable: null });
  });

  it("is drawn disabled with the core's reason on an agent that cannot fork yet", () => {
    expect(forkItem({ available: false, reason: "This agent has not reported its conversation yet" })).toMatchObject({
      unavailable: "This agent has not reported its conversation yet",
    });
  });

  it("is absent on a shell and on an agent with no fork command", () => {
    expect(forkItem({ available: false, reason: null })).toBeUndefined();
    expect(forkItem()).toBeUndefined();
  });

  it("sits with Sleep agent, apart from the copy items", () => {
    const items = paneMenuItems({ id: "w1:p1", children: null, lineage_path: [], fork: { available: true }, sleep_action: { available: true } } as unknown as PaneRow, "reviewer");
    expect(items.map((item) => item.id)).toEqual(["sleep_agent", "fork_agent", "copy_name", "copy_pane_id", "close_pane"]);
    expect(items.filter((item) => item.separated).map((item) => item.id)).toEqual(["close_pane"]);
  });
});
