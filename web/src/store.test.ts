import { beforeEach, describe, expect, it } from "vitest";
import { useShellStore } from "./store";

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
      diagnostics: [],
    });
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
});
