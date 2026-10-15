// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { useShellStore } from "./store";
import type { AgentRow, SnapshotRest } from "./snapshot";
import type { DispatchFn } from "./ws";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

const saved = useShellStore.getState();
afterEach(() => useShellStore.setState(saved, true));

const MEMORY = { kind: "memory_promote", target: "m72" };

function open(agents: Partial<AgentRow>[]) {
  const rest = { navigator: { devices: [{ id: "mac", kind: "local" }], workspaces: [{ id: "ws-1", device_id: "mac", path: "/repo", checkouts: [] }], focused_device_id: "mac" }, status: { remote: [] } } as unknown as SnapshotRest;
  useShellStore.setState({ rest, agents: agents as AgentRow[], connection: "live" });
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => (events.push(event), true));
  return { events, actions };
}

// PRD hide-ai-work-home B11: a row its home owns opens the home, any other opens its pane.
it("opens the Memory tab on the attempt's card for a row its home owns, and the pane for every other row", () => {
  const { events, actions } = open([
    { pane_id: "w1:p1", hide_ai: { task: MEMORY, home: "memory", workspace_id: "ws-1", turn: { kind: "pr_review", pr_number: 924 } } },
    { pane_id: "w1:p2" },
    // A kind this build has no home for, and a home whose Project the core has not named yet.
    { pane_id: "w1:p3", hide_ai: { task: { kind: "future_kind" } } },
    { pane_id: "w1:p4", hide_ai: { task: MEMORY, home: "memory" } },
  ]);
  actions.openListedAgent("w1:p1");
  expect(events.map((event) => [event.kind, event.payload])).toEqual(expect.arrayContaining([
    ["memory_open", { workspace_id: "ws-1" }],
    ["memory_expand", { workspace_id: "ws-1", number: 72 }],
  ]));
  expect(events.some((event) => event.kind === "focus_pane")).toBe(false);
  for (const pane of ["w1:p2", "w1:p3", "w1:p4"]) {
    events.length = 0;
    actions.openListedAgent(pane);
    expect(events.map((event) => event.kind), pane).toContain("focus_pane");
    expect(events.some((event) => event.kind === "memory_open"), pane).toBe(false);
  }
});
