import { describe, expect, it } from "vitest";
import { mainSections } from "./navigation";
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
