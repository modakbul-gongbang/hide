import { describe, expect, it } from "vitest";
import { filterEntries, fuzzyScore, searchEntries, type SearchEntry } from "./search";
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
        checkouts: [{ id: "c1", workspace_id: "w1", label: "main", path: "/tmp/fixture", branch: "main", purpose: null, is_worktree: false, exists: true, has_panes: true, pull_request: null, tabs: [], active_tab_id: null, strip: [], next_tab_label: "Tab 2" }],
        inactive_checkouts: { expanded: false, checkout_ids: [] },
      },
    ],
  },
} as unknown as SnapshotRest;

describe("search entries", () => {
  it("lists agents, projects and checkouts with the ids that activate them", () => {
    const entries = searchEntries(REST);
    expect(entries.map((entry) => entry.kind)).toEqual(["agent", "project", "checkout"]);
    expect(entries[0]).toMatchObject({ kind: "agent", paneId: "p1" });
    expect(entries[2]).toMatchObject({ kind: "checkout", workspaceId: "w1", checkoutId: "c1" });
  });

  it("filters by the fuzzy score and keeps the best first", () => {
    const entries: SearchEntry[] = [
      { id: "1", title: "Alpha", subtitle: "/a", kind: "project" },
      { id: "2", title: "Beta", subtitle: "/b", kind: "project" },
    ];
    expect(filterEntries(entries, "beta").map((entry) => entry.id)).toEqual(["2"]);
    expect(filterEntries(entries, "").map((entry) => entry.id)).toEqual(["1", "2"]);
  });
});

describe("workspace commands", () => {
  const rest = { workspace_view: { device_id: "local", path: "/repo", mode: "together", explorer: true, changes: false, agent_share: 0.5 } } as unknown as SnapshotRest;

  it("offers the other layouts by the menu's names and each tool by what it would do", () => {
    expect(searchEntries(rest, true).map((entry) => entry.title)).toEqual(["Layout: Agents only", "Layout: Views only", "Hide Explorer", "Show History"]);
  });

  it("offers no Workspace command when no Workspace is on screen", () => {
    expect(searchEntries(rest, false)).toEqual([]);
  });

  it("offers the View area commands with the reason one cannot run now", () => {
    const display = { id: "d1", tab_id: "file:a", path: "/repo/a.md", label: "a.md", kind: "file", committed: null, preview: false, state: "open", reason: null };
    const layout = { root: { area: { id: "a1", active: "d1", displays: [display] } }, active_area: "a1", limits: { areas: 6, depth: 3, displays: 64 }, display_count: 1 };
    const withViews = { workspace_view: { ...(rest.workspace_view as object), layout } } as unknown as SnapshotRest;
    const sizes = { areaMinWidth: 224, areaMinHeight: 144, divider: 2, tabStrip: 32 };
    const drawn = { geometry: viewGeometry(layout.root as ViewNode, { x: 0, y: 0, width: 1000, height: 600 }, sizes), sizes };
    const commands = searchEntries(withViews, true, drawn).filter((entry) => entry.subtitle === "View areas");
    expect(commands.map((entry) => entry.title)).toEqual([
      "Split right",
      "Split down",
      "Move to the next area",
      "Focus next view area",
      "Focus previous view area",
      "Close view",
      "Grow view area",
      "Shrink view area",
      "Open file to the side",
    ]);
    expect(commands.find((entry) => entry.title === "Split right")?.unavailable).toBe("This is the only view in its area.");
    expect(commands.find((entry) => entry.title === "Close view")?.unavailable).toBeNull();
  });
});
