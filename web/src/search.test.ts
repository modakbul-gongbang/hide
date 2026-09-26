import { describe, expect, it } from "vitest";
import { filterEntries, fuzzyScore, groupEntries, searchEntries, type SearchEntry } from "./search";
import type { SnapshotRest, ViewNode } from "./snapshot";
import { viewGeometry } from "./viewLayout";

describe("fuzzy score", () => {
  it("needs the query's characters in order", () => {
    expect(fuzzyScore("src/main.rs", "nope")).toBeNull();
    expect(fuzzyScore("src/main.rs", "smr")).not.toBeNull();
  });

  it("scores a tight and word-boundary match above a scattered one", () => {
    expect(fuzzyScore("src/main.rs", "main")!).toBeGreaterThan(fuzzyScore("server/migrations/init.rs", "main")!);
    expect(fuzzyScore("readme.md", "r")!).toBeGreaterThan(fuzzyScore("docs/readme.md", "r")!);
  });
});

const REST = {
  navigator: {
    agents: [
      { id: "a1", pane_id: "p1", identity_label: "Agent one", agent_kind: "claude", symbol: "●", group: "working", status_label: "Working", elapsed: "1m", emphasized: false, unread: false },
      { id: "a2", pane_id: "p9", identity_label: "Agent elsewhere", agent_kind: "codex", symbol: "○", group: "seen", status_label: "Idle", detail: "Waiting for review", elapsed: "3m", emphasized: false, unread: false },
    ],
    workspaces: [
      {
        id: "w1",
        label: "fixture",
        path: "/tmp/fixture",
        device_id: "local",
        registered: true,
        temporary: false,
        pinned: false,
        checkouts: [{ id: "c1", workspace_id: "w1", label: "main", path: "/tmp/fixture", branch: "main", purpose: null, is_worktree: false, exists: true, has_panes: true, pull_request: null, tabs: [{ id: "t1", workspace_id: "w1", checkout_id: "c1", label: "Tab 1", empty: false, delegated: false, panes: [{ id: "p1" }] }], active_tab_id: null, strip: [], next_tab_label: "Tab 2" }],
        inactive_checkouts: { expanded: false, checkout_ids: [] },
      },
    ],
  },
} as unknown as SnapshotRest;

describe("search entries", () => {
  it("lists agents, projects and checkouts with the ids that activate them", () => {
    const entries = searchEntries(REST);
    expect(entries.map((entry) => entry.kind)).toEqual(["agent", "agent", "project", "checkout"]);
    expect(entries[0]).toMatchObject({ kind: "agent", paneId: "p1" });
    expect(entries[3]).toMatchObject({ kind: "checkout", workspaceId: "w1", checkoutId: "c1" });
  });

  it("heads each entry in the Swift search view's form, an agent under the project holding its pane (issue 154)", () => {
    const heads = Object.fromEntries(searchEntries(REST).map((entry) => [entry.id, entry.group.label]));
    expect(heads).toEqual({
      "agent:p1": "fixture > AGENTS",
      "agent:p9": "AGENTS",
      "project:w1": "WORKSPACES > PROJECTS",
      "checkout:c1": "WORKSPACES > CHECKOUTS",
    });
  });

  it("gives an agent row its mark's kind and its state sentence, else the status word", () => {
    const [one, elsewhere] = searchEntries(REST);
    expect(one).toMatchObject({ agentKind: "claude", subtitle: "Working" });
    expect(elsewhere).toMatchObject({ agentKind: "codex", subtitle: "Waiting for review" });
  });

  it("filters by the fuzzy score and keeps the best first", () => {
    const group = { id: "projects", label: "WORKSPACES > PROJECTS" };
    const entries: SearchEntry[] = [
      { id: "1", title: "Alpha", subtitle: "/a", kind: "project", group },
      { id: "2", title: "Beta", subtitle: "/b", kind: "project", group },
    ];
    expect(filterEntries(entries, "beta").map((entry) => entry.id)).toEqual(["2"]);
    expect(filterEntries(entries, "").map((entry) => entry.id)).toEqual(["1", "2"]);
  });
});

describe("workspace commands", () => {
  const rest = { workspace_view: { device_id: "local", path: "/repo", panel: "open", pinned: false, explorer: true, changes: false, views_over_share: 0.6 } } as unknown as SnapshotRest;
  const wide = { drawn: null, placement: "column" } as const;

  it("offers the side panel's other states, its pin, and each tool by what it would do (issue 170)", () => {
    expect(searchEntries(rest, wide).map((entry) => entry.title)).toEqual(["Close side panel", "Expand side panel", "Pin side panel", "Hide Explorer", "Show History"]);
    const closed = { workspace_view: { ...(rest.workspace_view as object), panel: "closed", pinned: true } } as unknown as SnapshotRest;
    expect(searchEntries(closed, wide).map((entry) => entry.title)).toEqual(["Open side panel", "Expand side panel", "Unpin side panel", "Show Explorer", "Show History"]);
  });

  it("offers a tool a narrow window's closed overlay keeps out of sight as one to show (S7 B12)", () => {
    const explorer = (placement: "closed" | "open") => searchEntries(rest, { drawn: null, placement }).find((entry) => entry.id === "command:tool:explorer");
    expect(explorer("closed")).toMatchObject({ title: "Show Explorer", command: { tool: "explorer", visible: true } });
    expect(explorer("open")).toMatchObject({ title: "Hide Explorer", command: { tool: "explorer", visible: false } });
  });

  it("heads every Workspace command as one commands group", () => {
    expect(new Set(searchEntries(rest, wide).map((entry) => entry.group.label))).toEqual(new Set(["WORKSPACE > COMMANDS"]));
  });

  it("offers no Workspace command when no Workspace is on screen", () => {
    expect(searchEntries(rest, null)).toEqual([]);
  });

  it("offers every View tab menu command and the area commands, with the reason one cannot run now", () => {
    const display = { id: "d1", tab_id: "file:a", path: "/repo/a.md", label: "a.md", kind: "file", committed: null, preview: true, state: "open", reason: null };
    const layout = { root: { area: { id: "a1", active: "d1", displays: [display] } }, active_area: "a1", limits: { areas: 6, depth: 3, displays: 64 }, display_count: 1 };
    const withViews = { workspace_view: { ...(rest.workspace_view as object), layout } } as unknown as SnapshotRest;
    const sizes = { areaMinWidth: 224, areaMinHeight: 144, divider: 2, tabStrip: 32 };
    const drawn = { geometry: viewGeometry(layout.root as ViewNode, { x: 0, y: 0, width: 1000, height: 600 }, sizes), sizes };
    const commands = searchEntries(withViews, { drawn, placement: "column" }).filter((entry) => entry.subtitle === "View areas");
    expect(commands.map((entry) => entry.title)).toEqual([
      "Keep open",
      "Split right",
      "Split left",
      "Split up",
      "Split down",
      "Move right",
      "Move left",
      "Move up",
      "Move down",
      "Copy path",
      "Reveal in Explorer",
      "Close view",
      "Focus next view area",
      "Focus previous view area",
      "Grow view area",
      "Shrink view area",
      "Open file to the side",
    ]);
    expect(commands.find((entry) => entry.title === "Split right")?.unavailable).toBe("This is the only view in its area.");
    expect(commands.find((entry) => entry.title === "Move up")?.unavailable).toBe("There is no view area above.");
    expect(commands.find((entry) => entry.title === "Keep open")?.unavailable).toBeNull();
  });
});

describe("grouping (issue 154)", () => {
  const agentsHere = { id: "agents:w1", label: "herdr-ide > AGENTS" };
  const agentsThere = { id: "agents:w2", label: "sasu > AGENTS" };
  const projects = { id: "projects", label: "WORKSPACES > PROJECTS" };
  const entry = (id: string, group: SearchEntry["group"]): SearchEntry => ({ id, title: id, subtitle: "", kind: "agent", group });

  it("stands each group where its best entry ranked and keeps the rank inside it", () => {
    const ranked = [entry("a", agentsHere), entry("p", projects), entry("b", agentsThere), entry("c", agentsHere), entry("q", projects)];
    const sections = groupEntries(ranked);
    expect(sections.map((section) => section.group.label)).toEqual(["herdr-ide > AGENTS", "WORKSPACES > PROJECTS", "sasu > AGENTS"]);
    expect(sections.map((section) => section.entries.map((row) => row.id))).toEqual([["a", "c"], ["p", "q"], ["b"]]);
  });

  it("draws no group for no entries", () => {
    expect(groupEntries([])).toEqual([]);
  });

  it("keeps the best match first once grouped", () => {
    const ranked = filterEntries(searchEntries(REST), "fixture");
    expect(groupEntries(ranked)[0]?.entries[0]?.id).toBe(ranked[0]?.id);
  });
});
