import { describe, expect, it } from "vitest";
import { badgeText, deviceConnected, frontTitle, homeOf, homeProjectCount, railShown, tileCounts, tileMonogram, tileName } from "./devices";
import type { AgentRow, SnapshotRest } from "./snapshot";

function agent(paneId: string, group: string): AgentRow {
  return { id: paneId, pane_id: paneId, identity_label: paneId, agent_kind: "claude", symbol: "?", group, status_label: group, elapsed: "1m", emphasized: false, unread: false, demand: "none", activity: "idle" } as AgentRow;
}

const LOCAL_AGENTS = [agent("l1", "needs_you"), agent("l2", "needs_you"), agent("l3", "working")];

/** This Mac, a connected `mini` with one Needs You, and an unreachable `build-box` whose stale session still holds one. */
function rest(devices: string[] = ["mini", "build-box"]): SnapshotRest {
  const all = [
    { id: "local", label: "This Mac", kind: "local", state: "local", message: null },
    { id: "mini", label: "mini", kind: "remote", state: "ready", message: null },
    { id: "build-box", label: "build-box", kind: "remote", state: "unavailable", message: null },
  ].filter((device) => device.kind === "local" || devices.includes(device.id));
  return {
    navigator: { focused_device_id: "local", devices: all, workspaces: [{ id: "home", label: "hide", is_home: true, device_id: "local", checkouts: [] }] },
    ui_state: {
      workspace_registrations: [
        { id: "a", label: "a", path: "/a", device_id: "local", pinned: false },
        { id: "b", label: "b", path: "/b", device_id: "local", pinned: false },
        { id: "home", label: "hide", path: "/Users/example/hide", device_id: "local", pinned: false, home: true },
        { id: "m", label: "m", path: "/m", device_id: "mini", pinned: false },
      ],
    },
    status: {
      remote: [
        { target_id: "mini", state: "connected", session: { agents: [agent("remote:mini:pane:1", "needs_you"), agent("remote:mini:pane:2", "done")], workspaces: [] } },
        { target_id: "build-box", state: "stale", session: { agents: [agent("remote:build-box:pane:1", "needs_you")], workspaces: [] } },
      ],
    },
  } as unknown as SnapshotRest;
}

describe("the device rail's facts (quick device-rail-badges)", () => {
  it("shows the rail with one device alone, and hides it only when the core says so (B1, B6)", () => {
    expect(railShown(rest([]))).toBe(true);
    expect(railShown(rest())).toBe(true);
    expect(railShown(null)).toBe(true);
    const hidden = rest();
    hidden.ui_state = { ...hidden.ui_state, device_rail_visible: false };
    expect(railShown(hidden)).toBe(false);
  });

  it("counts each device's own Needs You and unseen Done, and leaves Working to the Agents tab", () => {
    const local = [...LOCAL_AGENTS, agent("l4", "done"), agent("l5", "seen")];
    expect(tileCounts(rest(), local, "local")).toEqual({ needs_you: 2, done: 1 });
    // Working and Seen alone mark nothing.
    expect(tileCounts(rest(), [agent("x", "working"), agent("y", "seen")], "local")).toEqual({ needs_you: 0, done: 0 });
    expect(tileCounts(rest(), LOCAL_AGENTS, "mini")).toEqual({ needs_you: 1, done: 1 });
  });

  it("gives a device that is not connected no count, its stale session ignored (B4, B8)", () => {
    expect(deviceConnected(rest(), "build-box")).toBe(false);
    expect(tileCounts(rest(), LOCAL_AGENTS, "build-box")).toEqual({ needs_you: 0, done: 0 });
  });

  it("draws `9+` from ten and the count below it (B4)", () => {
    expect([1, 9, 10, 250].map(badgeText)).toEqual(["1", "9", "9+", "9+"]);
  });

  it("draws a device's monogram from the first letters of its first two words", () => {
    expect(["Mac mini", "Mac Studio", "mini", "build-box", "연구실 빌드 서버", "  spaced  out "].map(tileMonogram)).toEqual(["Mm", "MS", "M", "Bb", "연빌", "So"]);
  });

  it("names a tile for assistive technology by device, connection and each count it marks (B4)", () => {
    expect(tileName("mini", true, { needs_you: 1, done: 0 })).toBe("mini, Needs You 1");
    expect(tileName("This Mac", true, { needs_you: 2, done: 1 })).toBe("This Mac, Needs You 2, Done 1");
    expect(tileName("build-box", false, { needs_you: 0, done: 0 })).toBe("build-box, 연결 안 됨");
    expect(tileName("This Mac", true, { needs_you: 0, done: 0 })).toBe("This Mac");
  });

  it("counts a device's Home row from its registrations, its Home excluded (B16, D-04)", () => {
    expect(homeProjectCount(rest(), "local")).toBe(2);
    expect(homeProjectCount(rest(), "mini")).toBe(1);
    expect(homeProjectCount(rest(), "build-box")).toBe(0);
  });

  it("finds a device's Home once it exists, and none before", () => {
    expect(homeOf(rest(), "local")?.id).toBe("home");
    expect(homeOf(rest(), "mini")).toBeNull();
  });

  it("titles the sidebar's top line by the device in front (B3)", () => {
    const devices = rest().navigator!.devices;
    expect(frontTitle(devices, "local")).toEqual({ name: "This Mac", note: null });
    expect(frontTitle(devices, "mini")).toEqual({ name: "mini", note: "Remote" });
  });
});
