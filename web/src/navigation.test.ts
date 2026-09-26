import { describe, expect, it } from "vitest";
import { agentSections, allAgents, allProjectsCount, liveDescendantCounts, mainSections, openingProgress, overviewProject, startupScreen } from "./navigation";
import type { SnapshotRest } from "./snapshot";

const project = (id: string, device: string, pinned = false) => ({
  id,
  label: id,
  path: `/${id}`,
  device_id: device,
  pinned,
  checkouts: [{ id: `${id}-main`, tabs: [{ panes: [{ id: `${id}-pane` }] }] }],
});

describe("All projects", () => {
  it("lists this machine first and a device that cannot answer without counts", () => {
    const rest = {
      navigator: {
        devices: [
          { id: "mini", kind: "remote", label: "mini" },
          { id: "local", kind: "local", label: "This Mac" },
        ],
        workspaces: [project("a", "local"), project("b", "local", true)],
      },
      ui_state: { workspace_registrations: [{ id: "r", label: "r", path: "/r", device_id: "mini", pinned: false }] },
      status: { remote: [{ target_id: "mini", state: "unreachable", message: null }] },
    } as unknown as SnapshotRest;
    const agents = [{ pane_id: "a-pane", group: "needs_you" }] as never;
    const [local, mini] = mainSections(rest, agents);
    expect(local?.projects.map((row) => row.id)).toEqual(["b", "a"]);
    expect(local?.projects[1]?.counts?.needs_you).toBe(1);
    expect(local?.projects[1]?.workspaceCount).toBe(1);
    expect(mini?.availability).toEqual({ state: "unavailable", text: "mini is unreachable", retry: "connect" });
    expect(mini?.projects.map((row) => [row.id, row.counts, row.workspaceCount])).toEqual([["r", null, null]]);
    // The sidebar's All projects row counts the same list without building it.
    expect(allProjectsCount(rest)).toBe(3);
  });
});

describe("All projects while this machine's Herdr does not answer", () => {
  it("shows why instead of counting no agents", () => {
    const rest = {
      navigator: { devices: [{ id: "local", kind: "local", label: "This Mac" }], workspaces: [project("a", "local")] },
      status: { herdr: { state: "unreachable", message: "Herdr did not answer" } },
    } as unknown as SnapshotRest;
    const [local] = mainSections(rest, []);
    expect(local?.availability.state).toBe("unavailable");
    expect(local?.projects[0]?.counts).toBeNull();
    expect(local?.projects[0]?.workspaceCount).toBe(1);
  });
});

describe("a Project's Overview", () => {
  it("keeps the rows its device last reported and says it cannot answer now", () => {
    const down = {
      navigator: { devices: [{ id: "local", kind: "local", label: "This Mac" }], workspaces: [project("a", "local")] },
      status: { herdr: { state: "unreachable", message: "Herdr did not answer" } },
    } as unknown as SnapshotRest;
    const local = overviewProject(down, [{ pane_id: "a-pane", group: "working" }] as never, "a");
    expect(local?.deviceAgents.map((row) => row.pane_id)).toEqual(["a-pane"]);
    expect(local?.availability).toMatchObject({ state: "unavailable", retry: null });

    const stale = {
      navigator: { devices: [{ id: "mini", kind: "remote", label: "mini" }], workspaces: [] },
      status: { remote: [{ target_id: "mini", state: "stale", message: null, session: { workspaces: [project("m", "mini")], agents: [{ pane_id: "m-pane", group: "working" }] } }] },
    } as unknown as SnapshotRest;
    const device = overviewProject(stale, [], "m");
    expect(device?.deviceAgents.map((row) => row.pane_id)).toEqual(["m-pane"]);
    expect(device?.availability).toMatchObject({ state: "unavailable", retry: "connect" });

    const catalog = {
      navigator: { devices: [{ id: "mini", kind: "remote", label: "mini" }], workspaces: [] },
      status: { remote: [{ target_id: "mini", state: "connected", catalog: { state: "unavailable", message: "helper refused" }, session: { workspaces: [project("m", "mini")], agents: [] } }] },
    } as unknown as SnapshotRest;
    expect(overviewProject(catalog, [], "m")?.availability).toEqual({ state: "unavailable", text: "helper refused", retry: "helper" });
  });

  it("hands the board every agent of the device, since a descendant may work in another project", () => {
    const up = {
      navigator: { devices: [{ id: "local", kind: "local", label: "This Mac" }], workspaces: [project("a", "local")] },
      status: { herdr: { state: "connected" } },
    } as unknown as SnapshotRest;
    expect(overviewProject(up, [{ pane_id: "a-pane", group: "working" }, { pane_id: "x", group: "seen" }] as never, "a")?.deviceAgents).toHaveLength(2);
  });
});

describe("Main's order", () => {
  it("puts pinned Projects first, then sorts by name", () => {
    const rest = {
      navigator: { devices: [{ id: "local", kind: "local", label: "This Mac" }], workspaces: [project("c", "local"), project("a", "local"), project("b", "local", true)] },
      status: { herdr: { state: "connected" } },
    } as unknown as SnapshotRest;
    expect(mainSections(rest, [])[0]?.projects.map((row) => row.id)).toEqual(["b", "a", "c"]);
  });
});

describe("the Agents explorer", () => {
  const row = (pane_id: string, group: string, children: string[] = []) => ({ pane_id, group, lineage_child_pane_ids: children }) as never;

  it("keeps the fixed group order, leaves empty groups out and never drops a row", () => {
    const sections = agentSections([row("a", "seen"), row("b", "needs_you"), row("c", "paused"), row("d", "seen")]);
    expect(sections.map((section) => [section.label, section.agents.length])).toEqual([
      ["Needs You", 1],
      ["Seen", 2],
      ["paused", 1],
    ]);
  });

  it("counts every live descendant once, not only the direct children", () => {
    const agents = [row("root", "working", ["child", "gone"]), row("child", "working", ["grandchild"]), row("grandchild", "seen", ["root"])];
    expect(liveDescendantCounts(agents).get("root")).toBe(2);
  });
});

describe("every current agent", () => {
  it("lists this machine's and each connected device's, naming the device, and nothing a device only last reported", () => {
    const rest = {
      navigator: { devices: [{ id: "mini", kind: "remote", label: "Mac mini" }] },
      status: {
        remote: [
          { target_id: "mini", state: "connected", session: { agents: [{ pane_id: "remote:mini:pane:1", group: "working" }] } },
          { target_id: "old", state: "stale", session: { agents: [{ pane_id: "remote:old:pane:1", group: "working" }] } },
        ],
      },
    } as unknown as SnapshotRest;
    const listed = allAgents(rest.status?.remote, rest.navigator?.devices, [{ pane_id: "w:p", group: "seen" }] as never);
    expect(listed.map((row) => [row.agent.pane_id, row.device])).toEqual([
      ["w:p", null],
      ["remote:mini:pane:1", "Mac mini"],
    ]);
  });
});

describe("the first screen", () => {
  const local = { navigator: { focused_device_id: "local", devices: [{ id: "local", kind: "local", label: "This Mac" }] } };
  const device = { navigator: { focused_device_id: "mini", devices: [{ id: "mini", kind: "remote", label: "Mac mini" }] } };

  it("waits for this machine's Herdr, then opens Main when no Workspace is in front", () => {
    expect(startupScreen({ ...local, status: { herdr: { state: "not_connected" } } } as unknown as SnapshotRest, false)).toBeNull();
    expect(startupScreen({ ...local, status: { herdr: { state: "connected" } } } as unknown as SnapshotRest, false)).toBe("main");
    const resumed = { device_id: "local", path: "/a", resumed: true };
    expect(startupScreen({ ...local, workspace_view: resumed, status: { herdr: { state: "connecting" } } } as unknown as SnapshotRest, true)).toBe("workspace");
  });

  it("opens Main on a first run or when the Workspace in front is not the one used last", () => {
    const fresh = { device_id: "local", path: "/a", resumed: false };
    expect(startupScreen({ ...local, workspace_view: fresh, status: { herdr: { state: "connected" } } } as unknown as SnapshotRest, true)).toBe("main");
    expect(startupScreen({ ...local, status: { herdr: { state: "connected" } } } as unknown as SnapshotRest, true)).toBe("main");
  });

  it("waits for the device in front, not for this machine's Herdr", () => {
    const connecting = { ...device, status: { herdr: { state: "connected" }, remote: [{ target_id: "mini", state: "not_connected" }] } };
    expect(startupScreen(connecting as unknown as SnapshotRest, false)).toBeNull();
    const noSession = { ...device, status: { herdr: { state: "connected" }, remote: [{ target_id: "mini", state: "connected", session: null }] } };
    expect(startupScreen(noSession as unknown as SnapshotRest, false)).toBeNull();
    const gone = { ...device, status: { herdr: { state: "connected" }, remote: [{ target_id: "mini", state: "unreachable" }] } };
    expect(startupScreen(gone as unknown as SnapshotRest, false)).toBe("main");
  });
});

describe("an open from Main or an Overview", () => {
  const rest = (front: string, error: number | null = null) =>
    ({
      navigator: {
        focused_device_id: "local",
        focused_checkout_id: front,
        workspaces: [
          { id: "w", checkouts: [
            { id: "c1", path: "/w", tabs: [{ id: "t1", panes: [{ id: "p1" }] }] },
            { id: "c2", path: "/w2", tabs: [{ id: "t2", panes: [{ id: "p2" }] }] },
          ] },
        ],
      },
      status: { last_error: error === null ? null : { kind: "focus.refused", message: "Herdr refused", occurred_at: error } },
    }) as unknown as SnapshotRest;

  it("lands only once the Workspace or the agent's pane is in front", () => {
    const toCheckout = { target: { checkoutId: "c2", deviceId: "local", path: "/w2" }, errorBefore: null, failure: null };
    expect(openingProgress(rest("c1"), toCheckout)).toBeNull();
    expect(openingProgress(rest("c2"), toCheckout)).toBe("landed");
    const toPane = { target: { paneId: "p2" }, errorBefore: null, failure: null };
    expect(openingProgress(rest("c1"), toPane)).toBeNull();
    expect(openingProgress(rest("c2"), toPane)).toBe("landed");
  });

  it("reads a newer core error as the refusal and ignores the one from before", () => {
    const opening = { target: { checkoutId: "c2", deviceId: "local", path: "/w2" }, errorBefore: 5, failure: null };
    expect(openingProgress(rest("c1", 5), opening)).toBeNull();
    expect(openingProgress(rest("c1", 9), opening)).toBe("Herdr refused");
  });
});
