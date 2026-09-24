import { describe, expect, it } from "vitest";
import { agentSections, allAgents, liveDescendants, mainSections, startupScreen } from "./navigation";
import type { SnapshotRest } from "./snapshot";

const project = (id: string, device: string, pinned = false) => ({
  id,
  label: id,
  path: `/${id}`,
  device_id: device,
  pinned,
  checkouts: [{ id: `${id}-main`, tabs: [{ panes: [{ id: `${id}-pane` }] }] }],
});

describe("Main", () => {
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
    expect(mini?.availability).toEqual({ state: "unavailable", text: "mini is unreachable", retry: true });
    expect(mini?.projects.map((row) => [row.id, row.counts, row.workspaceCount])).toEqual([["r", null, null]]);
  });
});

describe("Main while this machine's Herdr does not answer", () => {
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
    expect(liveDescendants(agents[0]!, agents)).toBe(2);
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
    const listed = allAgents(rest, [{ pane_id: "w:p", group: "seen" }] as never);
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
    expect(startupScreen({ ...local, status: { herdr: { state: "connecting" } } } as unknown as SnapshotRest, true)).toBe("workspace");
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
