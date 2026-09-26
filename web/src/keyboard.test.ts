import { describe, expect, it } from "vitest";
import { panelCycle } from "./keyboard";
import type { SnapshotRest } from "./snapshot";

describe("Recent Panels with a device in front", () => {
  it("walks that device's visible checkout's tabs, the shown one first, and commits them on the device", () => {
    const checkout = {
      id: "remote:mini:checkout:c1",
      workspace_id: "remote:mini:workspace:w1",
      label: "api",
      path: "/srv/api",
      tabs: ["t1", "t2", "t3"].map((id) => ({ id: `remote:mini:tab:${id}`, label: id, panes: [] })),
      active_tab_id: "remote:mini:tab:t1",
    };
    const rest = {
      navigator: { focused_device_id: "mini", devices: [{ id: "mini", kind: "remote", label: "mini" }], workspaces: [] },
      status: {
        remote: [
          {
            target_id: "mini",
            state: "connected",
            session: {
              workspaces: [{ id: checkout.workspace_id, label: "api", checkouts: [checkout] }],
              agents: [],
              active_tab_ids: {},
              focused_workspace_id: checkout.workspace_id,
              focused_checkout_id: checkout.id,
              focused_tab_id: "remote:mini:tab:t2",
              focused_pane_id: null,
              pane_layouts: [],
            },
          },
        ],
      },
    } as unknown as SnapshotRest;
    const cycle = panelCycle(rest)!;
    expect(cycle.items.map((item) => item.deviceTabId)).toEqual(["remote:mini:tab:t2", "remote:mini:tab:t1", "remote:mini:tab:t3"]);
    expect(cycle.items[0]).toMatchObject({ title: "t2", detail: "api · Terminal", surface: null });
  });
});
