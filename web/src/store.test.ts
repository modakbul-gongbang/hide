import { beforeEach, describe, expect, it } from "vitest";
import { DIAGNOSTIC_CAP, useShellStore } from "./store";

describe("snapshot merge", () => {
  beforeEach(() => {
    useShellStore.setState({
      connection: "connecting",
      revision: 0,
      terminalSequence: 0,
      rest: null,
      agents: [],
      focusedPaneId: null,
      herdrState: null,
      find: null,
      directoryList: null,
      pathRefusal: null,
      diagnostics: [],
      diagnosticsDropped: 0,
      viewGeneration: 0,
      refused: false,
    });
  });

  it("advances the view generation only on a self-contained snapshot", () => {
    const store = useShellStore.getState();
    store.applyFrame({ type: "snapshot", payload: { revision: 1, rest: {} } });
    store.applyFrame({ type: "delta", payload: { revision: 2 } });
    store.applyFrame({ type: "delta", payload: { revision: 3, rest: { focused: { pane_id: "p2" } } } });
    expect(useShellStore.getState().viewGeneration).toBe(1);
    store.applyFrame({ type: "snapshot", payload: { revision: 4, rest: {} } });
    expect(useShellStore.getState().viewGeneration).toBe(2);
  });

  it("keeps the newest diagnostics under the cap and counts the rest", () => {
    const store = useShellStore.getState();
    for (let i = 0; i < DIAGNOSTIC_CAP + 5; i += 1) store.noteDiagnostic(`d${i}`);
    const state = useShellStore.getState();
    expect(state.diagnostics).toHaveLength(DIAGNOSTIC_CAP);
    expect(state.diagnostics[0]).toBe("d5");
    expect(state.diagnosticsDropped).toBe(5);
  });

  it("replaces the store on a full snapshot", () => {
    useShellStore.getState().applyFrame({
      type: "snapshot",
      payload: {
        revision: 3,
        rest: {
          navigator: {
            agents: [
              {
                id: "a",
                pane_id: "p1",
                identity_label: "codex",
                agent_kind: "codex",
                symbol: "?",
                group: "needs_you",
                status_label: "Needs You",
                elapsed: "1m",
                emphasized: true,
                unread: true,
              },
            ],
          },
          focused: { pane_id: "p1" },
          status: { herdr: { state: "unconfigured" } },
        },
      },
    });
    const state = useShellStore.getState();
    expect(state.agents).toHaveLength(1);
    expect(state.focusedPaneId).toBe("p1");
    expect(state.herdrState).toBe("unconfigured");
  });

  it("draws unknown enum rows without stalling", () => {
    useShellStore.getState().applyFrame({
      type: "snapshot",
      payload: {
        rest: {
          navigator: {
            agents: [
              {
                id: "a",
                pane_id: "p1",
                identity_label: "x",
                agent_kind: "x",
                symbol: "?",
                group: "brand_new",
                status_label: "??",
                elapsed: "",
                emphasized: false,
                unread: false,
              },
            ],
          },
        },
      },
    });
    const state = useShellStore.getState();
    expect(state.agents[0]?.unknown).toBe(true);
    expect(state.diagnostics[0]).toContain("brand_new");
  });

  it("returns terminal chunks without storing them", () => {
    const chunks = useShellStore.getState().applyFrame({
      type: "delta",
      payload: {
        revision: 4,
        chunks: [{ pane_id: "p1", sequence: 1, bytes_base64: "YQ==" }],
      },
    });
    expect(chunks).toHaveLength(1);
    expect(useShellStore.getState().agents).toEqual([]);
  });

  it("keeps untouched workspace rows by reference across a delta", () => {
    const workspace = {
      id: "w1",
      label: "hide",
      path: "/h/hide",
      device_id: "local",
      registered: true,
      temporary: false,
      pinned: false,
      checkouts: [],
      inactive_checkouts: { expanded: false, checkout_ids: [] },
    };
    const store = useShellStore.getState();
    store.applyFrame({
      type: "snapshot",
      payload: { revision: 1, rest: { navigator: { workspaces: [workspace], agents: [] } } },
    });
    const before = useShellStore.getState().rest?.navigator?.workspaces?.[0];
    store.applyFrame({
      type: "delta",
      payload: {
        revision: 2,
        rest: {
          navigator: {
            workspaces: [{ ...workspace }],
            agents: [
              {
                id: "a",
                pane_id: "p1",
                identity_label: "codex",
                agent_kind: "codex",
                symbol: "?",
                group: "working",
                status_label: "Working",
                elapsed: "2m",
                emphasized: false,
                unread: false,
              },
            ],
          },
        },
      },
    });
    const after = useShellStore.getState().rest?.navigator?.workspaces?.[0];
    expect(after).toBe(before);
    expect(useShellStore.getState().agents).toHaveLength(1);
  });

  it("stores hided's directory listing and path refusal frames", () => {
    const store = useShellStore.getState();
    expect(
      store.applyFrame({
        type: "directory_list",
        payload: { root_path: "/h", entries: [{ name: "a", path: "/h/a" }], truncated: false },
      }),
    ).toEqual([]);
    expect(useShellStore.getState().directoryList?.entries[0]?.path).toBe("/h/a");
    store.applyFrame({
      type: "path_refused",
      payload: { kind: "create_workspace", path: "/etc", reason: "outside_home" },
    });
    expect(useShellStore.getState().pathRefusal?.reason).toBe("outside_home");
    store.clearPathRefusal();
    expect(useShellStore.getState().pathRefusal).toBeNull();
    store.applyFrame({ type: "error", message: "client json: boom" });
    expect(useShellStore.getState().diagnostics.at(-1)).toContain("boom");
  });
});
