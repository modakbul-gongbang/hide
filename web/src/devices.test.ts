import { describe, expect, it } from "vitest";
import { deviceBadge, deviceConnected, frontTitle, homeOf, homeProjectCount, inboxBadge, railVisible, remoteDevices, tileName } from "./devices";
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
        { id: "home", label: "hide", path: "/Users/me/hide", device_id: "local", pinned: false, home: true },
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

describe("the device rail's facts (PRD home-device-rail)", () => {
  it("shows the rail only while at least one remote device is registered, connected or not (B1, B11, B12)", () => {
    expect(railVisible(rest([]))).toBe(false);
    expect(railVisible(rest(["build-box"]))).toBe(true);
    expect(railVisible(rest())).toBe(true);
    expect(railVisible(null)).toBe(false);
    expect(remoteDevices(rest()).map((device) => device.id)).toEqual(["mini", "build-box"]);
  });

  it("badges each device with its own Needs You count and nothing at zero (B3)", () => {
    expect(deviceBadge(rest(), LOCAL_AGENTS, "local")).toBe(2);
    expect(deviceBadge(rest(), LOCAL_AGENTS, "mini")).toBe(1);
    expect(deviceBadge(rest(), [agent("x", "working")], "local")).toBeNull();
  });

  it("gives a device that is not connected no badge, and the Inbox counts only connected devices (B3, B8, D-27)", () => {
    expect(deviceConnected(rest(), "build-box")).toBe(false);
    expect(deviceBadge(rest(), LOCAL_AGENTS, "build-box")).toBeNull();
    // 2 on this Mac + 1 on mini; the stale device's 1 is not added.
    expect(inboxBadge(rest(), LOCAL_AGENTS)).toBe(3);
  });

  it("names a tile for assistive technology by device, state and count (B41)", () => {
    expect(tileName("mini", true, 1)).toBe("mini, Needs You 1");
    expect(tileName("build-box", false, null)).toBe("build-box, 연결 안 됨");
    expect(tileName("This Mac", true, null)).toBe("This Mac");
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

  it("titles the sidebar's top line by what is in front (B7)", () => {
    const devices = rest().navigator!.devices;
    expect(frontTitle(devices, "local", false)).toEqual({ name: "This Mac", note: null });
    expect(frontTitle(devices, "mini", false)).toEqual({ name: "mini", note: "Remote" });
    expect(frontTitle(devices, "mini", true)).toEqual({ name: "Inbox", note: "모든 기기" });
  });
});
