import { describe, expect, it } from "vitest";
import { transitiveReduction } from "../projectBoard";
import type { CardView, FactoryView } from "./model";
import { boardColumns, factoryGraph, taskChain } from "./view";

function card(task: string, patch: Partial<CardView> = {}): CardView {
  return {
    task, display_id: task, column: "waiting", title: task, state: "waiting", state_label: "", needs_person: false, waiting_for: null,
    priority: 0, since: 0, unread: false, folded: false, archived: false, failures: 0, external: [], revive_until: null, worker_pane: null, ...patch,
  };
}

function factory(cards: CardView[], dependencies: [string, string][], edges: [string, string][] = []): FactoryView {
  return {
    id: "f-1", project: "/p", project_name: "p", source: "local", verification: "verify", closed: false,
    flow: { drafting: 0, waiting: 0, running: 0, done_today: 0 }, my_turn: 0,
    columns: [{ column: "waiting", label: "", cards }], cancelled: [],
    graph: { nodes: cards.map((value) => value.task), edges, unrelated: [] }, dependencies,
    outside_read_at: null, stale: false, main_broken: false, auto_merge_available: true, merge_mode: "auto",
  };
}

describe("the Factory graph", () => {
  it("draws no arrow a longer path already implies", () => {
    expect(transitiveReduction([{ from: "A", to: "B" }, { from: "B", to: "C" }, { from: "A", to: "C" }])).toEqual([{ from: "A", to: "B" }, { from: "B", to: "C" }]);
  });

  it("keeps the engine's reduced edges and lays unrelated Tasks below", () => {
    const view = factory([card("T-1"), card("T-2"), card("T-3"), card("T-4")], [["T-1", "T-2"], ["T-2", "T-3"], ["T-1", "T-3"]], [["T-1", "T-2"], ["T-2", "T-3"]]);
    const graph = factoryGraph(view);
    const engine = view.graph.edges.map(([from, to]) => ({ from: `f-1/${from}`, to: `f-1/${to}` }));
    expect(graph.edges).toEqual(engine);
    expect(graph.layers.map((layer) => layer.map((node) => node.card.task))).toEqual([["T-1"], ["T-2"], ["T-3"]]);
    expect(graph.unrelated.map((node) => node.card.task)).toEqual(["T-4"]);
  });

  it("leaves folded completions out of the drawing", () => {
    const graph = factoryGraph(factory([card("T-1", { folded: true, state: "done" }), card("T-2")], [["T-1", "T-2"]]));
    expect(graph.layers).toEqual([]);
    expect(graph.unrelated.map((node) => node.card.task)).toEqual(["T-2"]);
  });

  it("names a Task's predecessors and the Tasks waiting on it", () => {
    const chain = taskChain(factory([card("T-1"), card("T-2"), card("T-3")], [["T-1", "T-2"], ["T-2", "T-3"]]), "T-2");
    expect(chain.before.map((value) => value.task)).toEqual(["T-1"]);
    expect(chain.after.map((value) => value.task)).toEqual(["T-3"]);
  });
});

describe("the Factory board", () => {
  it("keeps the engine's order, folds old completions and drops archived ones", () => {
    const view = factory([card("T-2"), card("T-1"), card("T-3", { folded: true }), card("T-4", { archived: true, folded: true })], []);
    const [column] = boardColumns([view], null);
    expect(column!.groups[0]!.cards.map((value) => value.task)).toEqual(["T-2", "T-1"]);
    expect(column!.groups[0]!.folded.map((value) => value.task)).toEqual(["T-3"]);
    expect(boardColumns([view], "done")).toEqual([]);
  });
});
