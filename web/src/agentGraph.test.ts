// The Agents graph (PRD agents-graph-view): columns, bands, rows, folds,
// filters and the lines' routes. The expected answers are the PRD's
// Behaviors read against small fixtures; a line that runs through a box is
// the failure B8 forbids, so the routing test measures every segment
// against every box.

import { describe, expect, it } from "vitest";
import {
  attentionOf,
  buildGraph,
  chainOf,
  edgeKindOf,
  entryBox,
  foldId,
  forwardPath,
  graphDevices,
  graphTargets,
  matchesFilter,
  NO_GRAPH_FILTER,
  routePoints,
  THIS_DEVICE,
  type GraphBoard,
  type GraphFilter,
  type GraphGeometry,
  type ProjectGraph,
} from "./agentGraph";
import { scopeAgents } from "./overviewLens";
import type { BoardProject } from "./projectBoard";
import type { AgentRow, Checkout, PullRequest, Task, Workspace } from "./snapshot";

const GEOMETRY: GraphGeometry = {
  boxWidth: 272,
  boxBorder: 1,
  headHeight: 74,
  rowHeight: 32,
  askingRowHeight: 52,
  boxPadBottom: 6,
  columnGap: 96,
  boxGap: 16,
  pad: 8,
  portOffset: 16,
  trunkInset: 20,
  trunkStep: 12,
  corridorClear: 4,
  trayInsetX: 6,
  trayInsetY: 2,
};

type CheckoutOptions = { primary?: boolean; tabs?: string[][]; merged?: boolean; missing?: boolean; pr?: PullRequest; task?: string };

function checkout(id: string, options: CheckoutOptions = {}): Checkout {
  const primary = options.primary ?? false;
  const tabs = options.tabs ?? [];
  return {
    id,
    workspace_id: "project",
    label: id,
    path: `/fixture/${id}`,
    branch: id,
    purpose: null,
    is_worktree: true,
    is_primary: primary,
    exists: !options.missing,
    has_panes: tabs.length > 0,
    worktree: { merged: options.merged ?? null, is_main: primary, missing: options.missing ?? false, changed_file_count: 0, dirty: false } as Checkout["worktree"],
    pull_request: options.pr ?? null,
    task_key: options.task ?? null,
    changed_file_count: 0,
    ahead: 0,
    tabs: tabs.map((panes, index) => ({ id: `${id}:tab${index}`, workspace_id: "project", checkout_id: id, label: `Tab ${index + 1}`, empty: false, delegated: false, panes: panes.map((pane) => ({ id: pane })) as never })),
    active_tab_id: `${id}:tab0`,
    strip: [],
    next_tab_label: "Tab",
  };
}

function workspace(checkouts: Checkout[], id = "project", tasks: Task[] = []): Workspace {
  return {
    id,
    label: id,
    path: `/fixture/${id}`,
    device_id: "local",
    is_git: true,
    default_branch: "main",
    registered: true,
    temporary: false,
    pinned: false,
    checkouts,
    inactive_checkouts: { expanded: false, checkout_ids: [] },
    tasks: { source: { kind: "github", label: "GitHub", name: "acme/project", reading: false, failure: null, last_read_at_unix_ms: 0 }, tasks, overflow: false },
  };
}

let clock = 0;
function agent(pane: string, extra: Partial<AgentRow> = {}): AgentRow {
  clock += 1;
  return {
    id: pane,
    pane_id: pane,
    identity_label: pane,
    agent_kind: "claude",
    symbol: "○",
    group: "seen",
    status_label: "Idle",
    changed_at_unix_ms: null,
    emphasized: false,
    unread: false,
    demand: "none",
    activity: "stopped",
    last_activity: String(1000 + clock).padStart(13, "0"),
    ...extra,
  };
}

const WORKING: Partial<AgentRow> = { group: "working", activity: "working", symbol: "●" };
const ASKING: Partial<AgentRow> = { group: "needs_you", demand: "question", detail: "범위를 넓혀도 될까요?", symbol: "?" };
const DONE: Partial<AgentRow> = { group: "done", symbol: "✓", unread: true };

function child(pane: string, parent: string, extra: Partial<AgentRow> = {}): AgentRow {
  return agent(pane, { lineage_parent_pane_id: parent, delegated: true, ...extra });
}

function one(project: Workspace, agents: AgentRow[]): BoardProject[] {
  return [{ workspace: project, agents, device: null }];
}

function graph(projects: BoardProject[], options: Partial<{ scope: "project" | "all"; openFolds: string[]; selectedBox: string | null; filter: GraphFilter }> = {}): GraphBoard {
  return buildGraph(projects, scopeAgents(projects), { scope: "project", openFolds: [], selectedBox: null, filter: NO_GRAPH_FILTER, ...options, geometry: GEOMETRY });
}

function only(board: GraphBoard): ProjectGraph {
  expect(board.sections).toHaveLength(1);
  return board.sections[0]!;
}

const box = (section: ProjectGraph, id: string) => section.boxes.find((candidate) => candidate.id === id)!;

describe("the buckets of attention", () => {
  it("ranks the operator's turn first, then working, waiting on children and resting, and counts an asking delegated row as the operator's attention (B5, B17)", () => {
    const rank = (row: AgentRow) => attentionOf({ agent: row, bucket: row.group === "needs_you" || row.group === "done" ? "turn" : row.group === "working" ? "working" : "resting" } as never);
    expect(rank(agent("a", ASKING))).toBe(0);
    expect(rank(agent("a", DONE))).toBe(0);
    expect(rank(agent("a", WORKING))).toBe(1);
    expect(rank(agent("a", { descendant_counts: { error: 0, approval: 0, question: 1, working: 0, done: 0 } }))).toBe(2);
    expect(rank(agent("a"))).toBe(3);
    expect(rank(agent("a", { demand: "question", group: "seen" }))).toBe(0);
  });

  it("colours a line by the child it leads to (D-04, B9)", () => {
    expect(edgeKindOf(agent("a", { demand: "approval" }))).toBe("ask");
    expect(edgeKindOf(agent("a", WORKING))).toBe("flow");
    expect(edgeKindOf(agent("a", { waiting_on_descendants: true }))).toBe("wait");
    expect(edgeKindOf(agent("a", { descendant_counts: { error: 0, approval: 0, question: 0, working: 2, done: 0 } }))).toBe("wait");
    expect(edgeKindOf(agent("a"))).toBe("rest");
  });
});

describe("columns and bands", () => {
  it("puts a box with no incoming delegation in the first column and a delegated one right of its deepest parent (B3)", () => {
    const project = workspace([checkout("main", { primary: true, tabs: [["m"]] }), checkout("a", { tabs: [["a"]] }), checkout("b", { tabs: [["b"]] }), checkout("c", { tabs: [["c"]] }), checkout("solo", { tabs: [["s"]] })]);
    const agents = [agent("m", WORKING), child("a", "m", WORKING), child("b", "a", WORKING), child("c", "m", WORKING), agent("s", WORKING)];
    const board = only(graph(one(project, agents)));
    expect(board.boxes.map((candidate) => [candidate.id, candidate.col]).sort()).toEqual([["a", 1], ["b", 2], ["c", 1], ["main", 0], ["solo", 0]]);
    // A box delegated into from two columns stands right of the deeper one.
    const both = workspace([checkout("main", { primary: true, tabs: [["m"]] }), checkout("a", { tabs: [["a"]] }), checkout("x", { tabs: [["x", "x2"]] })]);
    const crossing = only(graph(one(both, [agent("m", WORKING), child("a", "m", WORKING), child("x", "m", WORKING), child("x2", "a", WORKING)])));
    expect(box(crossing, "x").col).toBe(2);
  });

  it("stands a child box level with the row that delegated it when it can, never above it, and keeps boxes of one column and bands from overlapping (B4)", () => {
    const project = workspace([checkout("main", { primary: true, tabs: [["m1"], ["m2"]] }), checkout("a", { tabs: [["a"]] }), checkout("b", { tabs: [["b"]] }), checkout("solo", { tabs: [["s"]] })]);
    // m2 is the more recent, so it is main's first row and m1 its second.
    const agents = [agent("m1", { ...WORKING, last_activity: "0000000000001" }), agent("m2", { ...WORKING, last_activity: "0000000000002" }), child("a", "m1", WORKING), child("b", "m2", WORKING), agent("s", WORKING)];
    const board = only(graph(one(project, agents)));
    const port = (checkoutId: string, pane: string) => {
      const owner = box(board, checkoutId);
      const row = owner.rows.find((candidate) => candidate.paneId === pane)!;
      return owner.y + 1 + GEOMETRY.headHeight + row.top + GEOMETRY.portOffset;
    };
    expect(box(board, "main").rows.map((row) => row.paneId)).toEqual(["m2", "m1"]);
    // The first row's child stands level with it; the second row's cannot, a box being taller than a row, and stands under the first.
    expect(port("b", "b")).toBe(port("main", "m2"));
    expect(port("a", "a")).toBeGreaterThan(port("main", "m1"));
    expect(box(board, "a").y).toBeGreaterThanOrEqual(box(board, "b").y + box(board, "b").height + GEOMETRY.boxGap);
    // The primary band stands first; a second start point stands under the whole of it.
    const main = box(board, "main");
    const solo = box(board, "solo");
    expect(main.y).toBeLessThan(solo.y);
    const mainBand = board.boxes.filter((candidate) => candidate.id !== "solo");
    expect(solo.y).toBeGreaterThanOrEqual(Math.max(...mainBand.map((candidate) => candidate.y + candidate.height)));
  });

  it("orders bands and a column's boxes by attention, the operator's turn first, the primary box first only on a tie (B4, B5, D-37)", () => {
    const project = workspace([checkout("quiet", { tabs: [["q"]] }), checkout("main", { primary: true, tabs: [["m"]] }), checkout("busy", { tabs: [["w"]] }), checkout("asks", { tabs: [["k"]] })]);
    const agents = [agent("q", { demand: "none", descendant_counts: { error: 0, approval: 0, question: 0, working: 1, done: 0 } }), agent("m"), agent("w", WORKING), agent("k", ASKING)];
    const board = only(graph(one(project, agents)));
    expect(board.boxes.slice().sort((a, b) => a.y - b.y).map((candidate) => candidate.id)).toEqual(["asks", "busy", "quiet", "main"]);
    const all = only(graph(one(project, agents), { scope: "all" }));
    // On All projects the primary is ranked by its agents like any other box; a resting one folds.
    expect(all.boxes.slice().sort((a, b) => a.y - b.y).map((candidate) => candidate.id)).toEqual(["asks", "busy", "quiet"]);
  });

  it("ranks a band by its most urgent row, boxes below it included, and keeps the primary band first on a tie (D-37)", () => {
    const project = workspace([checkout("main", { primary: true, tabs: [["m"]] }), checkout("kid", { tabs: [["k"]] }), checkout("asks", { tabs: [["q"]] })]);
    const calm = [agent("m", WORKING), child("k", "m", WORKING), agent("q", ASKING)];
    // The asking band stands above a larger working main band.
    const ranked = only(graph(one(project, calm)));
    expect(box(ranked, "asks").y).toBeLessThan(box(ranked, "main").y);
    // A question deep in the main band lifts that whole band to the same rank, and the primary leads the tie.
    const deep = [agent("m", WORKING), child("k", "m", ASKING), agent("q", ASKING)];
    const tied = only(graph(one(project, deep)));
    expect(box(tied, "main").y).toBeLessThan(box(tied, "asks").y);
  });

  it("cuts a cycle where it closes and marks its closing line back (D-03, B10)", () => {
    const project = workspace([checkout("a", { tabs: [["a"]] }), checkout("b", { tabs: [["b"]] })]);
    const agents = [agent("a", { ...WORKING, lineage_parent_pane_id: "b" }), agent("b", { ...WORKING, lineage_parent_pane_id: "a" })];
    const board = only(graph(one(project, agents)));
    expect(board.edges).toHaveLength(2);
    expect(board.edges.filter((edge) => edge.back)).toHaveLength(1);
    expect(board.boxes.map((candidate) => candidate.col).sort()).toEqual([0, 1]);
  });

  it("makes a child whose parent pane is gone, or never declared, a first-column start point with no line (B11)", () => {
    const project = workspace([checkout("main", { primary: true, tabs: [["m"]] }), checkout("w", { tabs: [["w"]] })]);
    const board = only(graph(one(project, [agent("m", WORKING), child("w", "gone", WORKING)])));
    expect(board.edges).toHaveLength(0);
    expect(box(board, "w").col).toBe(0);
  });
});

describe("rows", () => {
  it("stands agents of one tab together on a tray, a delegation inside the box indented under its parent, and gives a lone tab no tray (B6, B7)", () => {
    const project = workspace([checkout("main", { primary: true, tabs: [["a", "b"], ["c"], ["d"]] })]);
    // Newest first inside a bucket: a, b, c, then d, which is a's.
    const at = (n: number): Partial<AgentRow> => ({ ...WORKING, last_activity: String(n).padStart(13, "0") });
    const agents = [agent("a", at(9)), agent("b", at(8)), agent("c", at(7)), child("d", "a", at(6))];
    const section = only(graph(one(project, agents)));
    const main = box(section, "main");
    expect(main.rows.map((row) => [row.paneId, row.depth, row.tray !== null])).toEqual([["a", 0, true], ["b", 0, true], ["d", 1, false], ["c", 0, false]]);
    expect(main.trays).toHaveLength(1);
    expect(main.trays[0]!.paneIds).toEqual(["a", "b"]);
    expect(main.trays[0]!.top).toBe(0);
    expect(main.trays[0]!.bottom).toBe(2 * GEOMETRY.rowHeight);
  });

  it("gives only an asking row a second line and the taller height (D-27, B17)", () => {
    const project = workspace([checkout("main", { primary: true, tabs: [["q"], ["w"], ["d"]] })]);
    const section = only(graph(one(project, [agent("q", ASKING), agent("w", WORKING), agent("d", { ...DONE, detail: "끝났습니다" })])));
    const rows = box(section, "main").rows;
    expect(rows.find((row) => row.paneId === "q")).toMatchObject({ line: "범위를 넓혀도 될까요?", height: GEOMETRY.askingRowHeight });
    expect(rows.find((row) => row.paneId === "w")).toMatchObject({ line: null, height: GEOMETRY.rowHeight });
    expect(rows.find((row) => row.paneId === "d")).toMatchObject({ line: null, height: GEOMETRY.rowHeight });
    expect(box(section, "main").height).toBe(2 + GEOMETRY.headHeight + GEOMETRY.askingRowHeight + 2 * GEOMETRY.rowHeight + GEOMETRY.boxPadBottom);
  });
});

describe("folds", () => {
  const checkouts = () => [
    checkout("main", { primary: true, tabs: [["m"]] }),
    checkout("rest", { tabs: [["r1"], ["r2"]] }),
    checkout("unread", { tabs: [["u"]] }),
    checkout("none"),
    checkout("shipped", { merged: true, tabs: [["s"]] }),
    checkout("gone", { missing: true }),
  ];
  const agents = () => [agent("m", WORKING), child("r1", "m"), child("r2", "r1"), agent("u", DONE), agent("s", { demand: "none" })];

  it("folds a box whose agents all rest, keeps one with an unread root completion open, and folds the rest into their lines (B21, B23, D-19)", () => {
    const section = only(graph(one(workspace(checkouts()), agents())));
    expect(section.boxes.map((candidate) => candidate.id).sort()).toEqual(["main", "unread"]);
    expect(section.folds.map((fold) => [fold.kind, fold.count])).toEqual([["empty", 1], ["cleanup", 2], ["resting", 1]]);
    expect(section.folds.find((fold) => fold.kind === "cleanup")!.names).toEqual(["shipped", "gone"]);
  });

  it("never folds the primary box on one project, and on All projects folds it like any other (B21, D-30)", () => {
    const quiet = workspace([checkout("main", { primary: true, tabs: [["m"]] })]);
    expect(only(graph(one(quiet, [agent("m")]))).boxes.map((candidate) => candidate.id)).toEqual(["main"]);
    const all = graph(one(quiet, [agent("m")]), { scope: "all" });
    expect(all.sections[0]!.boxes).toEqual([]);
    expect(all.sections[0]!.folds.map((fold) => fold.kind)).toEqual(["resting"]);
    // A project with no agent is a head-only primary box on one project and a `Worktrees without agents` line on All.
    const bare = workspace([checkout("main", { primary: true })]);
    expect(only(graph(one(bare, []))).boxes).toHaveLength(1);
    expect(graph(one(bare, []), { scope: "all" }).sections[0]!.folds.map((fold) => fold.kind)).toEqual(["empty"]);
  });

  it("badges the nearest drawn parent with a folded box's agents instead of drawing their line (B22)", () => {
    const section = only(graph(one(workspace(checkouts()), agents())));
    expect(section.edges).toHaveLength(0);
    expect(box(section, "main").rows[0]!.tucked).toEqual({ idle: 2 });
  });

  it("draws an opened fold's boxes and their lines, and leaves main selected when the selected box is folded (B1, B23)", () => {
    const opened = only(graph(one(workspace(checkouts()), agents()), { openFolds: [foldId("resting", "project")] }));
    expect(opened.boxes.map((candidate) => candidate.id).sort()).toEqual(["main", "rest", "unread"]);
    expect(opened.edges).toHaveLength(1);
    expect(box(opened, "main").rows[0]!.tucked).toBeNull();
    const selected = only(graph(one(workspace(checkouts()), agents()), { selectedBox: "shipped" }));
    expect(selected.boxes.map((candidate) => candidate.id)).not.toContain("shipped");
    expect(selected.folds.find((fold) => fold.kind === "cleanup")!.open).toBe(false);
    expect(selected.selected).toBe("main");
    const drawn = only(graph(one(workspace(checkouts()), agents()), { selectedBox: "unread" }));
    expect(drawn.selected).toBe("unread");
  });

  it("holds nothing at all as empty, and a project with only fold lines as a section (B30, B31)", () => {
    expect(graph([{ workspace: workspace([]), agents: [], device: null }], { scope: "all" })).toMatchObject({ empty: true, sections: [] });
    const onlyFolds = graph(one(workspace([checkout("main", { primary: true }), checkout("wt")]), []), { scope: "all" });
    expect(onlyFolds.empty).toBe(false);
    expect(onlyFolds.sections[0]!.boxes).toHaveLength(0);
    expect(onlyFolds.sections[0]!.folds[0]).toMatchObject({ kind: "empty", count: 2 });
  });
});

describe("the filter", () => {
  const project = () => workspace([checkout("main", { primary: true, tabs: [["m"]] }), checkout("prd/272-links", { tabs: [["a"], ["b"]], task: "github:acme/project#272", pr: { number: 301, title: "t", url: "u", badge: "open", review: null, is_draft: false, checks: "passing" } }), checkout("other", { tabs: [["c"]] })], "project", [{ key: "github:acme/project#272", source: "github", id: "#272", url: "u", title: "Links", open: true, updated_at_unix_ms: 0 }]);
  const agents = () => [agent("m", { ...WORKING, identity_label: "Observer" }), child("a", "m", { ...ASKING, identity_label: "링크 수정" }), child("b", "m", { ...WORKING, identity_label: "테스트" }), child("c", "m", { identity_label: "정리" })];

  it("lights any of several chips, working with a parent waiting on children, and narrows with the search and the device (B25, B26, D-35, D-36)", () => {
    const value = (row: AgentRow) => scopeAgents(one(project(), [row, ...agents().filter((other) => other.pane_id !== row.pane_id)])).find((candidate) => candidate.agent.pane_id === row.pane_id)!;
    const waiting = value(agent("m", { ...WORKING, waiting_on_descendants: true }));
    expect(matchesFilter(waiting, { ...NO_GRAPH_FILTER, chips: ["working"] })).toBe(true);
    expect(matchesFilter(waiting, { ...NO_GRAPH_FILTER, chips: ["turn"] })).toBe(false);
    expect(matchesFilter(waiting, { ...NO_GRAPH_FILTER, chips: ["turn", "working"] })).toBe(true);
    const found = value(agents()[1]!);
    expect(matchesFilter(found, { chips: [], query: "#272", device: null })).toBe(true);
    expect(matchesFilter(found, { chips: [], query: "272", device: null })).toBe(true);
    expect(matchesFilter(found, { chips: [], query: "301", device: null })).toBe(true);
    expect(matchesFilter(found, { chips: [], query: "PRD/272", device: null })).toBe(true);
    expect(matchesFilter(found, { chips: [], query: "링크", device: null })).toBe(true);
    expect(matchesFilter(found, { chips: ["working"], query: "272", device: null })).toBe(false);
    expect(matchesFilter(found, { chips: ["turn"], query: "272", device: "mini" })).toBe(false);
    expect(matchesFilter(found, { chips: ["turn"], query: "272", device: THIS_DEVICE })).toBe(true);
  });

  it("hides what does not match, keeps the parent chain faded so lines stay unbroken, and folds nothing (B27)", () => {
    const section = only(graph(one(project(), agents()), { filter: { chips: ["turn"], query: "", device: null } }));
    expect(section.boxes.map((candidate) => candidate.id).sort()).toEqual(["main", "prd/272-links"]);
    expect(box(section, "main").rows.map((row) => [row.paneId, row.dim])).toEqual([["m", true]]);
    expect(box(section, "prd/272-links").rows.map((row) => [row.paneId, row.dim])).toEqual([["a", false]]);
    expect(section.edges).toHaveLength(1);
    expect(section.edges[0]).toMatchObject({ from: "m", to: "a", dim: true });
    expect(section.folds).toEqual([]);
    // A resting chip finds a resting box a fold would have hidden.
    const resting = only(graph(one(project(), agents()), { filter: { chips: ["resting"], query: "", device: null } }));
    expect(resting.boxes.map((candidate) => candidate.id)).toContain("other");
  });

  it("answers a filter nothing matches as filterEmpty, and removing one filter redraws with the rest (B27, B28)", () => {
    const none = graph(one(project(), agents()), { filter: { chips: [], query: "zzz", device: null } });
    expect(none).toMatchObject({ filterEmpty: true, empty: false, sections: [] });
    const both = only(graph(one(project(), agents()), { filter: { chips: ["working"], query: "테스트", device: null } }));
    expect(both.rows.size).toBe(2);
    const chipOnly = only(graph(one(project(), agents()), { filter: { chips: ["working"], query: "", device: null } }));
    expect(chipOnly.rows.size).toBe(2);
    expect([...chipOnly.rows.keys()].sort()).toEqual(["b", "m"]);
  });

  it("names the devices of the scope, this machine first, so the control stands only with two or more (B24)", () => {
    const rows = scopeAgents([{ workspace: project(), agents: agents(), device: null }]);
    expect(graphDevices(rows)).toEqual([THIS_DEVICE]);
    const remote = scopeAgents([{ workspace: project(), agents: agents(), device: "mini" }]);
    expect(graphDevices([...rows, ...remote])).toEqual([THIS_DEVICE, "mini"]);
  });
});

describe("All projects", () => {
  it("draws each project's own graph, the project with the operator's turn first (D-11, B30)", () => {
    const quiet = workspace([checkout("main", { primary: true, tabs: [["q"]] })], "quiet");
    const busy = workspace([checkout("main2", { primary: true, tabs: [["b"]] })], "busy");
    const projects: BoardProject[] = [
      { workspace: quiet, agents: [agent("q", WORKING)], device: null },
      { workspace: busy, agents: [agent("b", ASKING)], device: null },
    ];
    expect(graph(projects, { scope: "all" }).sections.map((section) => section.project.id)).toEqual(["busy", "quiet"]);
  });
});

describe("the lines", () => {
  /** A graph wide and busy enough to need corridors: twenty agents over eight boxes. */
  function busy(): ProjectGraph {
    const names = ["main", "a", "b", "c", "d", "e", "f", "g"];
    const project = workspace(names.map((name, index) => checkout(name, { primary: index === 0, tabs: name === "main" ? [["m1"], ["m2"], ["m3"]] : [[`${name}1`, `${name}2`], [`${name}3`]] })));
    const agents = [
      agent("m1", WORKING),
      agent("m2", WORKING),
      agent("m3", WORKING),
      child("a1", "m1", WORKING),
      child("a2", "a1", WORKING),
      child("a3", "a2", ASKING),
      child("b1", "m2", WORKING),
      child("b2", "b1", WORKING),
      child("b3", "b2", WORKING),
      child("c1", "m3", WORKING),
      child("c2", "c1", WORKING),
      child("c3", "m1", WORKING),
      child("d1", "a1", WORKING),
      child("d2", "c3", WORKING),
      child("d3", "m2", WORKING),
      // Delegated from m1 (column 0) and from a deeper box, so it stands two columns out.
      child("e1", "m1", WORKING),
      child("e2", "d1", WORKING),
      child("f1", "m3", WORKING),
      child("f2", "e1", WORKING),
      child("g1", "f1", WORKING),
    ];
    // e's row e2 hangs under d1 (column 2), so e is column 3 and m1 -> e1 spans three columns.
    return only(graph(one(project, agents)));
  }

  it("never lets a forward line cross a box: every segment of every route keeps outside every box's interior (B8)", () => {
    const section = busy();
    expect(section.edges.length).toBeGreaterThan(8);
    const long = section.edges.filter((edge) => !edge.back && box(section, edge.toBox).col - box(section, edge.fromBox).col >= 2);
    expect(long.length).toBeGreaterThan(0);
    // At least one of them could not run level with its parent or its child, so it goes by a corridor between boxes.
    expect(long.some((edge) => edge.route.cy !== edge.route.ty && edge.route.cy !== edge.route.sy)).toBe(true);
    for (const edge of section.edges.filter((candidate) => !candidate.back)) {
      const points = routePoints(edge.route);
      for (let i = 0; i + 1 < points.length; i++) {
        const [x1, y1] = points[i]!;
        const [x2, y2] = points[i + 1]!;
        for (const candidate of section.boxes) {
          const inX = Math.min(x1, x2) < candidate.x + GEOMETRY.boxWidth && Math.max(x1, x2) > candidate.x;
          const inY = Math.min(y1, y2) < candidate.y + candidate.height && Math.max(y1, y2) > candidate.y;
          expect(inX && inY, `${edge.id} crosses ${candidate.id}`).toBe(false);
        }
      }
    }
  });

  it("holds for two hundred random delegation forests: no forward line crosses a box (B8)", () => {
    let seed = 7;
    const next = (n: number) => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed % n;
    };
    for (let round = 0; round < 200; round++) {
      const boxes = 3 + next(8);
      const names = Array.from({ length: boxes }, (_, index) => (index === 0 ? "main" : `w${index}`));
      const agents: AgentRow[] = [];
      const perBox = names.map(() => [] as string[]);
      const total = boxes + next(boxes * 2);
      for (let index = 0; index < total; index++) {
        const where = index < boxes ? index : next(boxes);
        const id = `p${index}`;
        const parent = index === 0 || next(5) === 0 ? null : agents[next(agents.length)]!.pane_id;
        perBox[where]!.push(id);
        agents.push(parent ? child(id, parent, next(3) === 0 ? ASKING : WORKING) : agent(id, WORKING));
      }
      const project = workspace(names.map((name, index) => checkout(name, { primary: index === 0, tabs: perBox[index]!.map((pane) => [pane]) })));
      const section = only(graph(one(project, agents)));
      for (const edge of section.edges.filter((candidate) => !candidate.back)) {
        const points = routePoints(edge.route);
        for (let i = 0; i + 1 < points.length; i++) {
          const [x1, y1] = points[i]!;
          const [x2, y2] = points[i + 1]!;
          for (const candidate of section.boxes) {
            const inX = Math.min(x1, x2) < candidate.x + GEOMETRY.boxWidth && Math.max(x1, x2) > candidate.x;
            const inY = Math.min(y1, y2) < candidate.y + candidate.height && Math.max(y1, y2) > candidate.y;
            expect(inX && inY, `round ${round}: ${edge.id} crosses ${candidate.id}`).toBe(false);
          }
        }
      }
    }
  });

  it("runs a line from its parent's port out of the parent box's right side into its child's left side (B8)", () => {
    const section = busy();
    for (const edge of section.edges.filter((candidate) => !candidate.back)) {
      const from = box(section, edge.fromBox);
      const to = box(section, edge.toBox);
      expect(edge.route.sx).toBe(from.x + GEOMETRY.boxWidth);
      expect(edge.route.tx).toBe(to.x);
      expect(edge.route.tx).toBeGreaterThan(edge.route.sx);
    }
  });

  it("draws a short line as three rounded segments and a bend only where the line turns (B8)", () => {
    const path = forwardPath({ sx: 0, sy: 10, t1x: 50, cy: 90, t2x: 50, ty: 90, tx: 100 });
    expect(path.match(/Q/g)).toHaveLength(2);
    expect(forwardPath({ sx: 0, sy: 10, t1x: 50, cy: 10, t2x: 50, ty: 10, tx: 100 })).toBe("M 0 10 L 100 10");
  });

  it("shares one trunk among a parent's lines and keeps different parents' trunks apart in a gap (B8)", () => {
    const project = workspace([checkout("main", { primary: true, tabs: [["p"], ["q"]] }), checkout("a", { tabs: [["a"]] }), checkout("b", { tabs: [["b"]] }), checkout("c", { tabs: [["c"]] })]);
    const section = only(graph(one(project, [agent("p", { ...WORKING, last_activity: "0000000000009" }), agent("q", { ...WORKING, last_activity: "0000000000001" }), child("a", "p", WORKING), child("b", "p", WORKING), child("c", "q", WORKING)])));
    const edge = (to: string) => section.edges.find((candidate) => candidate.to === to)!.route;
    expect(edge("a").t1x).toBe(edge("b").t1x);
    expect(edge("a").t1x).not.toBe(edge("c").t1x);
    // p stands above q, so its lane is the one further right: q's way out never crosses it.
    expect(edge("a").sy).toBeLessThan(edge("c").sy);
    expect(edge("a").t1x).toBeGreaterThan(edge("c").t1x);
  });

  it("keeps a trunk inside its gap however many parents share it (B8)", () => {
    const names = Array.from({ length: 12 }, (_, index) => `w${index}`);
    const project = workspace([checkout("main", { primary: true, tabs: names.map((name) => [`p-${name}`]) }), ...names.map((name) => checkout(name, { tabs: [[name]] }))]);
    const agents = [...names.map((name) => agent(`p-${name}`, WORKING)), ...names.map((name) => child(name, `p-${name}`, WORKING))];
    const section = only(graph(one(project, agents)));
    for (const edge of section.edges) {
      expect(edge.route.t1x).toBeGreaterThan(edge.route.sx);
      expect(edge.route.t1x).toBeLessThan(edge.route.tx);
    }
  });

  it("reads a route and the hover chain back out of the animated numbers (D-20, B19)", () => {
    const section = busy();
    const targets = graphTargets(section);
    const edge = section.edges[0]!;
    expect(targets.get(`e:${edge.id}:cy`)).toBe(edge.route.cy);
    const chain = chainOf(section, "a2");
    expect(chain.has("m1")).toBe(true);
    expect(chain.has("a1")).toBe(true);
    expect(chain.has("a3")).toBe(true);
    expect(chain.has("m2")).toBe(false);
  });
});

describe("what moves", () => {
  it("answers identical input with identical targets, so a snapshot that changes nothing starts no motion (D-26)", () => {
    const project = workspace([checkout("main", { primary: true, tabs: [["m"]] }), checkout("w", { tabs: [["w"]] })]);
    const first = graphTargets(only(graph(one(project, [agent("m", WORKING), child("w", "m", WORKING)]))));
    const second = graphTargets(only(graph(one(project, [agent("m", { ...WORKING, changed_at_unix_ms: 120_000 }), child("w", "m", { ...WORKING, changed_at_unix_ms: 120_000 })]))));
    expect(second).toEqual(first);
    const moved = graphTargets(only(graph(one(project, [agent("m", WORKING), child("w", "m", ASKING)]))));
    expect(moved).not.toEqual(first);
  });

  it("lays twenty agents out in under eight milliseconds (D-26, B39)", () => {
    const names = Array.from({ length: 10 }, (_, index) => `w${index}`);
    const project = workspace([checkout("main", { primary: true, tabs: [["m1"], ["m2"]] }), ...names.map((name) => checkout(name, { tabs: [[`${name}a`, `${name}b`]] }))]);
    const agents = [agent("m1", WORKING), agent("m2", WORKING), ...names.flatMap((name, index) => [child(`${name}a`, index % 2 === 0 ? "m1" : "m2", WORKING), child(`${name}b`, `${name}a`, WORKING)])];
    expect(agents).toHaveLength(22);
    const projects = one(project, agents);
    const lensAgents = scopeAgents(projects);
    const times: number[] = [];
    for (let run = 0; run < 40; run++) {
      const start = performance.now();
      buildGraph(projects, lensAgents, { scope: "project", openFolds: [], selectedBox: null, filter: NO_GRAPH_FILTER, geometry: GEOMETRY });
      times.push(performance.now() - start);
    }
    expect(Math.min(...times)).toBeLessThan(8);
  });
});

describe("the way in", () => {
  it("selects the box of the checkout in front when it is this project's, else the primary one (D-22, B1)", () => {
    const project = workspace([checkout("main", { primary: true }), checkout("wt")]);
    expect(entryBox(project, "wt")).toBe("wt");
    expect(entryBox(project, "elsewhere")).toBe("main");
    expect(entryBox(project, null)).toBe("main");
    expect(entryBox(null, "wt")).toBeNull();
  });
});
