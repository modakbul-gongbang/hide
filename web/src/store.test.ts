import { beforeEach, describe, expect, it } from "vitest";
import type { EditorDocumentSnapshot } from "./snapshot";
import { DIAGNOSTIC_CAP, LISTING_CAP, useShellStore } from "./store";

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
      listings: {},
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

  it("drops the front Workspace's view when a delta no longer carries one", () => {
    const store = useShellStore.getState();
    const view = { device_id: "local", path: "/h/hide", mode: "together", explorer: true, changes: false, agent_share: 0.5 };
    store.applyFrame({ type: "snapshot", payload: { revision: 1, rest: { workspace_view: view } as never } });
    expect(useShellStore.getState().rest?.workspace_view?.path).toBe("/h/hide");
    store.applyFrame({ type: "delta", payload: { revision: 2, rest: { focused: { pane_id: "p2" } } } });
    expect(useShellStore.getState().rest?.workspace_view).toBeUndefined();
  });

  it("keeps the named Project's sessions across a delta that does not carry them", () => {
    const store = useShellStore.getState();
    const sessions = { device_id: "local", workspace_id: "workspace:a", unavailable_reason: null, loading: false, failure: null, rows: [], detail: null };
    store.applyFrame({ type: "snapshot", payload: { revision: 1, rest: {} } });
    expect(useShellStore.getState().projectSessions).toBeNull();
    store.applyFrame({ type: "delta", payload: { revision: 2, project_sessions: sessions } });
    store.applyFrame({ type: "delta", payload: { revision: 3, rest: { focused: { pane_id: "p2" } } } });
    expect(useShellStore.getState().projectSessions).toBe(sessions);
    // A daemon that restarted names no Project.
    store.applyFrame({ type: "snapshot", payload: { revision: 1, rest: {} } });
    expect(useShellStore.getState().projectSessions).toBeNull();
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
        payload: {
          kind: "remote_file_list",
          root_path: "/h",
          entries: [{ name: "a", path: "/h/a", is_directory: true }],
          truncated: false,
        },
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

  it("re-reads a watched folder only for the device it shows", () => {
    const store = useShellStore.getState();
    useShellStore.setState({
      rest: { navigator: { focused_device_id: "mac" } } as never,
      listings: { "/r/src": { kind: "file_list", root_path: "/r/src", entries: [], truncated: false } as never },
      folderChanges: {},
    });
    store.applyFrame({ type: "directory_changed", payload: { path: "/r/src", device_id: "local" } });
    expect(useShellStore.getState().listings["/r/src"]).toBeDefined();
    store.applyFrame({ type: "directory_changed", payload: { path: "/r/src", device_id: "mac" } });
    expect(useShellStore.getState().listings["/r/src"]).toBeUndefined();
    expect(useShellStore.getState().folderChanges["/r/src"]).toBe(1);
  });

  it("keeps the Explorer's listing apart from the registration input", () => {
    const store = useShellStore.getState();
    store.applyFrame({
      type: "directory_list",
      payload: {
        kind: "file_list",
        root_path: "/repo/src",
        entries: [{ name: "main.rs", path: "/repo/src/main.rs", is_directory: false }],
        truncated: false,
      },
    });
    expect(useShellStore.getState().listings["/repo/src"]?.entries[0]?.name).toBe("main.rs");
    expect(useShellStore.getState().directoryList).toBeNull();
    store.applyFrame({
      type: "directory_list",
      payload: {
        kind: "remote_file_list",
        root_path: "/h",
        entries: [{ name: "a", path: "/h/a", is_directory: true }],
        truncated: false,
      },
    });
    expect(useShellStore.getState().directoryList?.root_path).toBe("/h");
    expect(Object.keys(useShellStore.getState().listings)).toEqual(["/repo/src"]);
  });

  it("keeps the newest LISTING_CAP folders and drops the oldest", () => {
    const store = useShellStore.getState();
    for (let i = 0; i < LISTING_CAP + 2; i += 1) {
      store.applyFrame({
        type: "directory_list",
        payload: { kind: "file_list", root_path: `/repo/f${i}`, entries: [], truncated: false },
      });
    }
    const paths = Object.keys(useShellStore.getState().listings);
    expect(paths).toHaveLength(LISTING_CAP);
    expect(paths[0]).toBe("/repo/f2");
    expect(paths.at(-1)).toBe(`/repo/f${LISTING_CAP + 1}`);
  });

  it("records a listing whose kind it does not know instead of guessing a flow", () => {
    const store = useShellStore.getState();
    store.applyFrame({
      type: "directory_list",
      payload: { root_path: "/h", entries: [], truncated: false },
    });
    const state = useShellStore.getState();
    expect(state.directoryList).toBeNull();
    expect(state.listings).toEqual({});
    expect(state.diagnostics.at(-1)).toContain("directory_list without a known kind=none");
  });
});

describe("the documents section", () => {
  const doc = (path: string, contents: string): EditorDocumentSnapshot => ({
    path,
    language: null,
    document_kind: "text",
    contents_utf8: contents,
    revision: null,
    dirty: false,
    readonly_reason: null,
    conflict: null,
    save: null,
  });

  beforeEach(() => {
    useShellStore.setState({ documents: {}, editor: null, changes: null });
  });

  it("replaces the map with a snapshot's section, and empties it when a snapshot has none", () => {
    const store = useShellStore.getState();
    store.applyFrame({ type: "snapshot", payload: { rest: {}, documents: { visible: ["a", "b"], changed: [{ tab_id: "a", document: doc("/a", "1") }, { tab_id: "b", document: doc("/b", "1") }] } } });
    store.applyFrame({ type: "snapshot", payload: { rest: {}, documents: { visible: ["c"], changed: [{ tab_id: "c", document: doc("/c", "1") }] } } });
    expect(Object.keys(useShellStore.getState().documents)).toEqual(["c"]);
    store.applyFrame({ type: "snapshot", payload: { rest: {} } });
    expect(useShellStore.getState().documents).toEqual({});
  });

  it("keeps a still-visible document, overwrites a changed one and drops one no longer visible", () => {
    const store = useShellStore.getState();
    const a = doc("/a", "1");
    store.applyFrame({ type: "snapshot", payload: { rest: {}, documents: { visible: ["a", "b"], changed: [{ tab_id: "a", document: a }, { tab_id: "b", document: doc("/b", "1") }] } } });
    const c = doc("/c", "2");
    store.applyFrame({ type: "delta", payload: { documents: { visible: ["a", "c"], changed: [{ tab_id: "c", document: c }] } } });
    const documents = useShellStore.getState().documents;
    expect(Object.keys(documents).sort()).toEqual(["a", "c"]);
    expect(documents.a).toBe(a);
    expect(documents.c).toBe(c);
    const edited = doc("/a", "typed");
    store.applyFrame({ type: "delta", payload: { documents: { visible: ["a", "c"], changed: [{ tab_id: "a", document: edited }] } } });
    expect(useShellStore.getState().documents.a).toBe(edited);
    expect(useShellStore.getState().documents.c).toBe(c);
  });

  it("keeps the map, object for object, through a delta without the section or one that changes nothing", () => {
    const store = useShellStore.getState();
    store.applyFrame({ type: "snapshot", payload: { rest: {}, documents: { visible: ["a"], changed: [{ tab_id: "a", document: doc("/a", "1") }] } } });
    const before = useShellStore.getState().documents;
    store.applyFrame({ type: "delta", payload: { revision: 2, changes: null, rest: { focused: { pane_id: "p2" } } } });
    expect(useShellStore.getState().documents).toBe(before);
    store.applyFrame({ type: "delta", payload: { documents: { visible: ["a"], changed: [] } } });
    expect(useShellStore.getState().documents).toBe(before);
  });
});
