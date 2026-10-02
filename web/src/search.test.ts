import { describe, expect, it } from "vitest";
import { CHILD, RICH } from "./gallery/cmdkSceneData";
import { filterEntries, fuzzyScore, groupEntries, numberQuery, searchEntries, type SearchEntry } from "./search";
import type { SnapshotRest } from "./snapshot";

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
      { id: "a1", pane_id: "p1", identity_label: "Agent one", agent_kind: "claude", symbol: "●", group: "working", status_label: "Working", changed_at_unix_ms: null, emphasized: false, unread: false },
      { id: "a2", pane_id: "p9", identity_label: "Agent elsewhere", agent_kind: "codex", symbol: "○", group: "seen", status_label: "Idle", detail: "Waiting for review", changed_at_unix_ms: null, emphasized: false, unread: false },
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

/** The one command every screen has; the tests below read the rows around it. */
const START_AGENT = "command:start-agent";
const without = (entries: SearchEntry[]) => entries.filter((entry) => entry.id !== START_AGENT && entry.kind !== "device");

describe("search entries", () => {
  it("lists agents, projects and checkouts with the ids that activate them", () => {
    const entries = without(searchEntries(REST));
    expect(entries.map((entry) => entry.kind)).toEqual(["agent", "agent", "project", "checkout"]);
    expect(entries[0]).toMatchObject({ kind: "agent", paneId: "p1" });
    expect(entries[3]).toMatchObject({ kind: "checkout", workspaceId: "w1", checkoutId: "c1" });
  });

  it("heads each entry under its kind: Agents, Projects, Checkouts (PRD cmdk-navigation B11)", () => {
    const heads = Object.fromEntries(without(searchEntries(REST)).map((entry) => [entry.id, entry.group.label]));
    expect(heads).toEqual({
      "agent:p1": "Agents",
      "agent:p9": "Agents",
      "project:w1": "Projects",
      "checkout:c1": "Checkouts",
    });
  });

  it("offers 에이전트 시작… as the only command, on every snapshot (B22)", () => {
    expect(searchEntries(REST).filter((entry) => entry.kind === "command")).toEqual([expect.objectContaining({ id: START_AGENT, title: "에이전트 시작…", command: "start_agent" })]);
  });

  it("gives an agent row its mark's kind, where it works and its state sentence, else the status word", () => {
    const [one, elsewhere] = without(searchEntries(REST));
    expect(one).toMatchObject({ agentKind: "claude", subtitle: "fixture · Working" });
    // A pane no checkout holds has no place, and none is made up.
    expect(elsewhere).toMatchObject({ agentKind: "codex", subtitle: "Waiting for review" });
  });

  it("filters by the fuzzy score and keeps the best first", () => {
    const group = { id: "projects", label: "Projects" };
    const entries: SearchEntry[] = [
      { id: "1", title: "Alpha", subtitle: "/a", kind: "project", group },
      { id: "2", title: "Beta", subtitle: "/b", kind: "project", group },
    ];
    expect(filterEntries(entries, "beta").map((entry) => entry.id)).toEqual(["2"]);
    expect(filterEntries(entries, "").map((entry) => entry.id)).toEqual(["1", "2"]);
  });
});

/** This Mac and `mini` (connected, with a Home and a project) and `build-box` (registered, not connected). */
const TWO_DEVICES = {
  navigator: {
    focused_device_id: "local",
    devices: [
      { id: "local", label: "This Mac", kind: "local", state: "local", message: null },
      { id: "mini", label: "mini", kind: "remote", state: "ready", message: null },
      { id: "build-box", label: "build-box", kind: "remote", state: "unavailable", message: null },
    ],
    agents: REST.navigator!.agents,
    workspaces: REST.navigator!.workspaces,
  },
  status: {
    remote: [
      {
        target_id: "mini",
        state: "connected",
        session: {
          agents: [{ id: "m1", pane_id: "remote:mini:pane:1", identity_label: "배치 감시", agent_kind: "codex", symbol: "●", group: "working", status_label: "Working", changed_at_unix_ms: null, emphasized: false, unread: false }],
          workspaces: [
            {
              id: "remote:mini:workspace:web",
              label: "web",
              path: "/srv/web",
              device_id: "mini",
              inactive_checkouts: { expanded: false, checkout_ids: [] },
              checkouts: [{ id: "remote:mini:checkout:web", workspace_id: "remote:mini:workspace:web", label: "main", path: "/srv/web", tabs: [{ id: "remote:mini:tab:1", panes: [{ id: "remote:mini:pane:1" }] }] }],
            },
            { id: "remote:mini:workspace:home", label: "hide", path: "/Users/example/hide", device_id: "mini", is_home: true, inactive_checkouts: { expanded: false, checkout_ids: [] }, checkouts: [] },
          ],
        },
      },
      { target_id: "build-box", state: "not_connected", session: { agents: [{ id: "old", pane_id: "remote:build-box:pane:1", identity_label: "stale", group: "seen" }], workspaces: [] } },
    ],
  },
} as unknown as SnapshotRest;

describe("search entries across devices (PRD home-device-rail B40)", () => {
  it("finds every connected device's agents, projects and checkouts and the devices themselves", () => {
    const entries = searchEntries(TWO_DEVICES);
    expect(entries.filter((entry) => entry.kind === "agent").map((entry) => entry.title)).toEqual(["Agent one", "Agent elsewhere", "배치 감시"]);
    expect(entries.filter((entry) => entry.kind === "project").map((entry) => entry.title)).toEqual(["fixture", "web"]);
    expect(entries.filter((entry) => entry.kind === "checkout").map((entry) => entry.checkoutId)).toEqual(["c1", "remote:mini:checkout:web"]);
    expect(entries.filter((entry) => entry.kind === "device").map((entry) => entry.title)).toEqual(["This Mac", "mini", "build-box"]);
  });

  it("chips a result only while its device is not the one in front, and never lists a device's Home as a project", () => {
    const fromLocal = searchEntries(TWO_DEVICES);
    expect(fromLocal.find((entry) => entry.id === "agent:p1")?.chip).toBeUndefined();
    expect(fromLocal.find((entry) => entry.id === "agent:remote:mini:pane:1")?.chip).toEqual({ label: "mini", local: false });
    expect(fromLocal.some((entry) => entry.title === "hide")).toBe(false);
    const fromMini = searchEntries({ ...TWO_DEVICES, navigator: { ...TWO_DEVICES.navigator, focused_device_id: "mini" } } as SnapshotRest);
    expect(fromMini.find((entry) => entry.id === "agent:p1")?.chip).toEqual({ label: "This Mac", local: true });
    expect(fromMini.find((entry) => entry.id === "agent:remote:mini:pane:1")?.chip).toBeUndefined();
  });

  it("lists nothing current from a device that is not connected, beyond the device itself", () => {
    const entries = searchEntries(TWO_DEVICES);
    expect(entries.some((entry) => entry.deviceId === "build-box" && entry.kind !== "device")).toBe(false);
  });
});

describe("grouping (issue 154)", () => {
  const agentsHere = { id: "agents:w1", label: "herdr-ide > AGENTS" };
  const agentsThere = { id: "agents:w2", label: "sasu > AGENTS" };
  const projects = { id: "projects", label: "Projects" };
  const entry = (id: string, group: SearchEntry["group"]): SearchEntry => ({ id, title: id, subtitle: "", kind: "agent", group });

  it("stands each group where its best entry ranked and keeps the rank inside it", () => {
    const ranked = [entry("a", agentsHere), entry("p", projects), entry("b", agentsThere), entry("c", agentsHere), entry("q", projects)];
    const sections = groupEntries(ranked);
    expect(sections.map((section) => section.group.label)).toEqual(["herdr-ide > AGENTS", "Projects", "sasu > AGENTS"]);
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

describe("issues and pull requests (PRD cmdk-navigation B11-B13, D-14)", () => {
  const found = (query: string) => filterEntries(searchEntries(RICH), query).map((entry) => entry.id);

  it("holds this Mac's issues and pull requests, a pull request once however many places name it", () => {
    const entries = searchEntries(RICH);
    expect(entries.filter((entry) => entry.kind === "pr").map((entry) => entry.id)).toEqual(["pr:w1:275", "pr:w1:260"]);
    expect(entries.filter((entry) => entry.kind === "issue").map((entry) => entry.id)).toEqual(["issue:github:acme/herdr-ide#273"]);
  });

  it("finds a pull request by number, title or branch and an issue by number or title, not by words only a body would hold (B13)", () => {
    expect(found("#275")).toContain("pr:w1:275");
    expect(found("sandbox refusals")).toContain("pr:w1:275");
    expect(found("sandbox-letters")).toContain("pr:w1:275");
    expect(found("273")).toContain("issue:github:acme/herdr-ide#273");
    expect(found("internal")).toContain("issue:github:acme/herdr-ide#273");
    expect(found("zzzzz")).toEqual([]);
  });

  it("puts the exact number first, an issue and a pull request of the same number each as its own row (B12)", () => {
    const withTwin = structuredClone(RICH) as typeof RICH;
    withTwin.navigator!.workspaces![0]!.pull_requests = [{ ...(RICH.navigator!.workspaces![0]!.pull_requests![0]!), number: 273, title: "Unrelated 275 title" }];
    const ranked = filterEntries(searchEntries(withTwin), "#273").map((entry) => entry.id);
    expect(ranked.slice(0, 2)).toEqual(["issue:github:acme/herdr-ide#273", "pr:w1:273"]);
    // A bare number holds only rows whose text holds the digits.
    expect(filterEntries(searchEntries(RICH), "273").every((entry) => `${entry.title} ${entry.subtitle}`.includes("273"))).toBe(true);
  });

  it("reads #273 and 273 as numbers and nothing else", () => {
    expect(numberQuery("#273")).toBe(273);
    expect(numberQuery(" 273 ")).toBe(273);
    expect(numberQuery("#273a")).toBeNull();
    expect(numberQuery("sandbox")).toBeNull();
  });

  it("carries the pull request's CI rollup only when it has one", () => {
    const [open, merged] = searchEntries(RICH).filter((entry) => entry.kind === "pr");
    expect(open?.ci).toEqual({ tone: "pending", label: "CI 진행 중" });
    expect(merged?.ci).toEqual({ tone: "done", label: "CI 통과" });
    const unknown = structuredClone(RICH) as typeof RICH;
    unknown.navigator!.workspaces![0]!.pull_requests![0]!.checks = "unknown";
    expect(searchEntries(unknown).find((entry) => entry.id === "pr:w1:275")?.ci).toBeUndefined();
  });

  it("keeps the 80 row cap", () => {
    const many = structuredClone(RICH) as typeof RICH;
    many.navigator!.workspaces![0]!.tasks!.tasks = Array.from({ length: 120 }, (_, index) => ({ key: `github:acme/herdr-ide#${index + 1000}`, source: "github", id: `#${index + 1000}`, url: null, title: `sandbox issue ${index}`, open: true }));
    expect(filterEntries(searchEntries(many), "sandbox").length).toBe(80);
  });

  it("finds an agent by where it works", () => {
    expect(found("sandbox").includes(`agent:${CHILD.pane_id}`)).toBe(true);
  });
});
