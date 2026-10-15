// @vitest-environment jsdom
import { act, Profiler } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { initializeInterfaceI18n } from "./i18n/instance";
import { RELATION_ANSWER_TIMEOUT_MS } from "./lineage";
import { PaneHeaderBand, PanePrChip } from "./PaneHeaderBand";
import type { AgentPullRequest, AgentRow, PaneHeader, PaneFocusRequest, SnapshotRest, Workspace } from "./snapshot";
import { legacyAgentRow } from "../test/legacyAgentRow";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import type { DispatchFn } from "./ws";

vi.hoisted(() => { HTMLCanvasElement.prototype.getContext = () => null; });
const savedShell = useShellStore.getState();
const savedUi = useUiStore.getState();
const savedScrollIntoView = Element.prototype.scrollIntoView;
afterEach(() => {
  useShellStore.setState(savedShell, true);
  useUiStore.setState(savedUi, true);
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  Element.prototype.scrollIntoView = savedScrollIntoView;
  document.body.innerHTML = "";
});

const header: PaneHeader = { working: false, band: { kind: "raised", tone: "warning", reason: null, since_unix_ms: null, action: {kind: "child", pane_id: "child", label: "자식 검증"}, more: 0, exit_code: null,
  raised: { verb: "approval", what: "하위 검증 권한이 필요합니다", raised_pane_id: "child", pane_id: "child", title: "자식 검증", agent_kind: "codex", open_pane_id: "child", since_unix_ms: null, path: ["부모", "자식 검증"], checkout: "feature", human_notice: true } } };

async function mount(value = header, agents: AgentRow[] = [], options: { rest?: SnapshotRest; paneId?: string } = {}) {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  Element.prototype.scrollIntoView = () => {};
  initializeInterfaceI18n("en");
  useUiStore.setState({relation: null});
  useShellStore.setState({agents, rest: options.rest ?? {status: {pane_focus_request: null}} as SnapshotRest});
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions(event => {events.push(event); return true;});
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(<TooltipProvider><PaneHeaderBand paneId={options.paneId ?? "parent"} header={value} actions={actions}/></TooltipProvider>));
  const answer = async (outcome: PaneFocusRequest) => {
    await act(async () => useShellStore.setState({rest: {status: {pane_focus_request: outcome}} as SnapshotRest}));
  };
  return {container, actions, events, answer, unmount: async () => { await act(async () => root.unmount()); }};
}

it("keeps pending, correlated refusal, retry and retired-target feedback in the source band", async () => {
  const {container, actions, events, answer, unmount} = await mount();
  try {
    const button = () => container.querySelector<HTMLButtonElement>("[data-pane-band-open]")!;
    await act(async () => button().click());
    expect(events).toHaveLength(1);
    const first = useUiStore.getState().relation!;
    expect(button().disabled).toBe(true);
    expect(container.querySelector('[role="status"]')?.textContent).toContain("자식 검증");
    await act(async () => actions.followRelation("parent", "child", "자식 검증"));
    expect(events).toHaveLength(1);
    await answer({request_id: "older", target_pane_id: "child", phase: "failed", message: "Old refusal", retryable: true});
    expect(button().disabled).toBe(true);
    await answer({request_id: first.requestId, target_pane_id: "child", phase: "failed", message: "Herdr refused this move", retryable: true});
    expect(container.querySelector('[role="alert"]')?.textContent).toBe("Herdr refused this move");
    expect(button().textContent).toBe("Retry");
    await act(async () => button().click());
    expect(events).toHaveLength(2);
    const second = useUiStore.getState().relation!;
    expect(second.requestId).not.toBe(first.requestId);
    await answer({request_id: first.requestId, target_pane_id: "child", phase: "succeeded", message: null, retryable: false});
    expect(button().disabled).toBe(true);
    await answer({request_id: second.requestId, target_pane_id: "child", phase: "failed", message: "Child no longer exists", retryable: false});
    expect(button().disabled).toBe(true);
    expect(container.querySelector('[role="alert"]')?.textContent).toBe("Child no longer exists");
  } finally { await unmount(); }
});

// Approved B2 fixture: five direct children, one of each state; the question
// belongs to the parent until the core explicitly raises it to the operator.
function childrenFixture(): AgentRow[] {
  const seed = legacyAgentRow({ id: "parent", pane_id: "parent", identity_label: "Parent", agent_kind: "claude", symbol: "●", group: "working", status_code: "working", detail: null, changed_at_unix_ms: null, emphasized: false, unread: false, demand: "none", activity: "working" });
  const children = ["working", "result", "idle", "unknown", "question"].map((id, index) => ({
    ...seed, id, pane_id: id, identity_label: `Child ${id}`,
    status_code: id === "unknown" ? "unknown" as const : id === "question" ? "question" as const : id === "result" ? "done" as const : id === "idle" ? "idle" as const : seed.status_code,
    activity: id === "unknown" ? "unknown" : id === "result" || id === "idle" ? "stopped" : seed.activity, demand: id === "question" ? "question" : "none",
    lineage_parent_pane_id: "parent", lineage_child_pane_ids: [],
    state: { ...seed.state, tree_rank: id === "question" ? 0 : index + 1, line: id === "question" ? { text: "Should these changes share a PR?", mode: "quiet" as const, tone: { kind: "subtle" as const, read: false } } : null },
  }));
  return [{ ...seed, lineage_child_pane_ids: [...children.map((child) => child.pane_id), "closed"], direct_child_counts: { error: 0, approval: 0, question: 1, working: 1, done: 1 } }, ...children];
}

it("shows direct child counts and the core-ranked child line without raising an ordinary child question", async () => {
  const { container, unmount } = await mount({ working: true, band: null }, childrenFixture());
  try {
    const trigger = container.querySelector<HTMLButtonElement>("[data-pane-tree]")!;
    expect(trigger.getAttribute("aria-label")).toBe("Child agents 5 · Working: 1 · Reported results: 1 · Child requests: 1 · Unknown state: 1 · Resting: 1");
    expect(trigger.textContent).not.toContain("!");
    expect(container.textContent).toContain("Should these changes share a PR?");
    expect(container.querySelector("[data-pane-working-line]")).not.toBeNull();
  } finally { await unmount(); }
});

it("keeps an own approval in the same band and shows only raised subtree asks in the compact trigger", async () => {
  const agents = childrenFixture();
  agents[0]!.descendant_mark = { kind: "raised", count: 2 };
  const { container, unmount } = await mount({ working: false, band: { ...header.band!, kind: "approval", reason: "Run the verification command", raised: undefined, action: null, more: 1 } }, agents);
  try {
    const trigger = container.querySelector<HTMLButtonElement>("[data-pane-tree]")!;
    expect(trigger.textContent).toBe("!2");
    expect(trigger.getAttribute("aria-label")).toContain("Descendant requests needing you: 2");
    expect(container.textContent).toContain("Run the verification command");
    expect(container.querySelector("[data-pane-child-summary]")).toBeNull();
    expect(container.querySelector("[data-pane-band-more]")).toBeNull();
  } finally { await unmount(); }
});

it("never counts a core unknown child as resting or a result when the wire omits the unknown count", async () => {
  const [parent, ...all] = childrenFixture();
  const child = all.find((row) => row.pane_id === "unknown")!;
  const { container, unmount } = await mount({ working: false, band: null }, [{ ...parent!, lineage_child_pane_ids: [child.pane_id], direct_child_counts: { error: 0, approval: 0, question: 0, working: 0, done: 0 } }, child]);
  try {
    expect(container.querySelector("[data-pane-tree]")?.getAttribute("aria-label")).toBe("Child agents 1 · Unknown state: 1");
  } finally { await unmount(); }
});

it("removes the child band when its last live child leaves and keeps the no-child working line", async () => {
  const agents = childrenFixture();
  const { container, unmount } = await mount({ working: true, band: null }, agents);
  try {
    await act(async () => useShellStore.setState({ agents: [{ ...agents[0]!, lineage_child_pane_ids: [] }] }));
    expect(container.querySelector("[data-pane-tree]")).toBeNull();
    expect(container.querySelector("[data-pane-header-band]")).toBeNull();
    expect(container.querySelector("[data-pane-working-line]")).not.toBeNull();
  } finally { await unmount(); }
});

it("opens a nested parent's raised branch using only root-owned asks within that subtree", async () => {
  const [seed] = childrenFixture();
  const leafAsk = { ...header.band!.raised!, raised_pane_id: "leaf", pane_id: "leaf", open_pane_id: "leaf", what: "Allow this nested verification" };
  const agents = [
    { ...seed!, id: "root", pane_id: "root", lineage_child_pane_ids: ["parent", "outside"], raised: [leafAsk, { ...leafAsk, raised_pane_id: "outside", pane_id: "outside", open_pane_id: "outside", what: "Unrelated branch request" }] },
    { ...seed!, lineage_parent_pane_id: "root", lineage_path_pane_ids: ["root", "parent"], lineage_child_pane_ids: ["grand"], raised: [], descendant_mark: { kind: "raised" as const, count: 1 } },
    { ...seed!, id: "grand", pane_id: "grand", lineage_parent_pane_id: "parent", lineage_path_pane_ids: ["root", "parent", "grand"], lineage_child_pane_ids: ["leaf"] },
    { ...seed!, id: "leaf", pane_id: "leaf", lineage_parent_pane_id: "grand", lineage_path_pane_ids: ["root", "parent", "grand", "leaf"], lineage_child_pane_ids: [] },
    { ...seed!, id: "outside", pane_id: "outside", lineage_parent_pane_id: "root", lineage_path_pane_ids: ["root", "outside"], lineage_child_pane_ids: [] },
  ];
  const { container, unmount } = await mount({ working: false, band: null }, agents);
  try {
    await act(async () => container.querySelector<HTMLButtonElement>("[data-pane-tree]")!.click());
    const popup = document.querySelector('[data-agent-tree="parent"]')!;
    expect(popup.querySelector('[data-agent-child="grand"]')).not.toBeNull();
    expect(popup.querySelector('[data-agent-child="leaf"]')?.textContent).toContain("Allow this nested verification");
    expect(popup.textContent).not.toContain("Unrelated branch request");
    await act(async () => popup.querySelector<HTMLButtonElement>('[data-tree-ask-open="leaf"]')!.click());
    expect(useUiStore.getState().relation).toMatchObject({ sourcePaneId: "parent", targetPaneId: "leaf" });
  } finally { await unmount(); }
});

it.each(["parent", "child", "descendant"] as const)("preserves topology but replaces stale child states when the %s device disconnects", async (scope) => {
  const agents = childrenFixture();
  agents[0]!.descendant_mark = { kind: "raised", count: 2 };
  if (scope === "descendant") agents.push({ ...agents[1]!, id: "deep", pane_id: "deep", device_id: "mini", lineage_parent_pane_id: "working", lineage_path_pane_ids: ["parent", "working", "deep"], lineage_child_pane_ids: [] });
  else agents[scope === "parent" ? 0 : 1]!.device_id = "mini";
  const rest = { status: { pane_focus_request: null, remote: [{ target_id: "mini", state: "disconnected", message: "Mac mini is not connected" }] } } as unknown as SnapshotRest;
  const { container, unmount } = await mount({ working: false, band: null }, agents, { rest });
  try {
    const trigger = container.querySelector<HTMLButtonElement>("[data-pane-tree]")!;
    expect(trigger.getAttribute("aria-label")).toBe("Child agents 5 · Mac mini is not connected");
    expect(trigger.textContent).toContain("⊘");
    expect(container.querySelector("[data-pane-child-count]")).toBeNull();
    expect(container.querySelector("[data-pane-child-summary]")?.textContent).toBe("Mac mini is not connected");
    await act(async () => trigger.click());
    expect(document.querySelector('[data-agent-tree="parent"]')).not.toBeNull();
    if (scope === "child") {
      const child = document.querySelector('[data-agent-child="working"]')!;
      expect(child.getAttribute("aria-label")).toContain("Mac mini is not connected");
      expect(child.getAttribute("aria-label")).not.toContain("Working");
    }
  } finally { await unmount(); }
});

it("keeps online raised requests accessible when another descendant's device is unavailable", async () => {
  const agents = childrenFixture();
  const ask = { ...header.band!.raised!, raised_pane_id: "online-ask", pane_id: "online-ask", open_pane_id: "online-ask", what: "Allow the online nested check" };
  agents[0]!.descendant_mark = { kind: "raised", count: 1 };
  agents[0]!.raised = [ask];
  agents[1]!.lineage_child_pane_ids = ["online-ask"];
  agents.find((row) => row.pane_id === "question")!.device_id = "mini";
  agents.push({ ...agents[1]!, id: "online-ask", pane_id: "online-ask", lineage_parent_pane_id: "working", lineage_path_pane_ids: ["parent", "working", "online-ask"], lineage_child_pane_ids: [] });
  const rest = { status: { pane_focus_request: null, remote: [{ target_id: "mini", state: "disconnected", message: "Mac mini is not connected" }] } } as unknown as SnapshotRest;
  const { container, unmount } = await mount({ working: false, band: { ...header.band!, kind: "approval", raised: undefined, action: null, more: 1 } }, agents, { rest });
  try {
    const trigger = container.querySelector<HTMLButtonElement>("[data-pane-tree]")!;
    expect(trigger.getAttribute("aria-label")).not.toContain("Descendant requests needing you");
    expect(container.querySelector("[data-pane-band-more]")).toBeNull();
    await act(async () => trigger.click());
    const popup = document.querySelector('[data-agent-tree="parent"]')!;
    expect(popup.getAttribute("data-agent-tree-mode")).toBe("raised");
    expect(popup.querySelector('[data-agent-child="online-ask"]')?.textContent).toContain("Allow the online nested check");
    expect(popup.querySelector<HTMLButtonElement>('[data-tree-ask-open="online-ask"]')!.disabled).toBe(false);
  } finally { await unmount(); }
});

it("turns a missing core answer into a retryable timeout beside the move", async () => {
  const {container, unmount} = await mount();
  try {
    await act(async () => container.querySelector<HTMLButtonElement>("[data-pane-band-open]")!.click());
    await act(async () => vi.advanceTimersByTime(RELATION_ANSWER_TIMEOUT_MS));
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("did not answer in time");
    expect(container.querySelector<HTMLButtonElement>("[data-pane-band-open]")!.disabled).toBe(false);
  } finally { await unmount(); }
});

it("draws a raised ask as verb, what and who", async () => {
  const {container, unmount} = await mount();
  try {
    const band = container.querySelector("[data-pane-header-band=raised]")!;
    expect(band.querySelector("[data-verb=approval]")?.textContent).toBe("Approve");
    expect(band.textContent).toContain("하위 검증 권한이 필요합니다");
    expect(band.querySelector("[data-pane-band-who=child]")?.textContent).toBe("자식 검증");
    expect(band.querySelector("[data-pane-band-more]")).toBeNull();
  } finally { await unmount(); }
});

// B8: a raised descendant on a device that is not connected cannot be opened, and the band says why.
it("blocks Open on a raised descendant whose device is not connected and says why", async () => {
  const pane = "remote:mini:pane:w1:p2";
  const band = header.band!;
  const {container, unmount} = await mount({ ...header, band: { ...band, action: {kind: "child", pane_id: pane, label: "자식 검증"}, raised: { ...band.raised!, raised_pane_id: pane, pane_id: pane, open_pane_id: pane } } });
  try {
    await act(async () => useShellStore.setState({rest: {status: {pane_focus_request: null, remote: [{target_id: "mini", state: "disconnected", message: "Mac mini is not connected"}]}} as unknown as SnapshotRest}));
    expect(container.querySelector<HTMLButtonElement>("[data-pane-band-open]")!.disabled).toBe(true);
    expect(container.querySelector("[data-pane-band-unreachable]")?.textContent).toBe("Mac mini is not connected");
  } finally { await unmount(); }
});

it("opens the displayed repository even when another project has the same PR number", async () => {
  const {actions, unmount} = await mount();
  const url = "https://github.com/acme/other/pull/7";
  try {
    await act(async () => useShellStore.setState({rest: {navigator: {focused_device_id: "local", devices: [{id: "local", kind: "local"}], workspaces: [
      {id: "here", device_id: "local", pull_requests: [{number: 7, url: "https://github.com/acme/here/pull/7"}], checkouts: []},
      {id: "other", device_id: "local", pull_requests: [{number: 7, url}], checkouts: []},
    ] as unknown as Workspace[]}} as SnapshotRest}));
    await act(async () => actions.openSessionPullRequest({workspace_id: "here", number: 7, url}));
    expect(useUiStore.getState().overviewProjectId).toBe("other");
    expect(useUiStore.getState().overviewLens).toMatchObject({tab: "prs", prs: {panel: 7, focus: 7}});
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    actions.openSessionPullRequest({workspace_id: "here", number: 7, url: "https://github.com/acme/absent/pull/7"});
    expect(open).toHaveBeenCalledWith("https://github.com/acme/absent/pull/7", "_blank", "noopener,noreferrer");
  } finally { await unmount(); }
});

// The pane header draws the Sessions row's PR mark: a parent counts its
// descendants' pull requests, a pane without children shows only its own.
function pullRow(pane: string, numbers: number[], children: string[] = []): AgentRow {
  const seed = legacyAgentRow({ pane_id: pane, identity_label: `${pane} 세션`, lineage_child_pane_ids: children });
  const pulls: AgentPullRequest[] = numbers.map((number) => ({ number, title: `PR ${number}`, url: `https://github.com/acme/app/pull/${number}`, badge: "open", checks: "pending", head_branch: "feat/work", closing_issues: [], live: true, duty: true, created: true, settled_at_unix_ms: null }));
  return { ...seed, state: { ...seed.state, pr: numbers.length ? { count: numbers.length, worst: "pending", pulls: numbers.map((_, index) => ({ state: "pending", index })) } : null }, request: { verb: "working", verb_since_unix_ms: 0, request: null, later_by: null, reply: null, pull_requests: pulls } };
}

async function mountChip(paneId: string, agents: AgentRow[]) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  initializeInterfaceI18n("en");
  useShellStore.setState({ agents, rest: { status: { pane_focus_request: null } } as SnapshotRest });
  const actions = createActions(() => true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(<TooltipProvider><PanePrChip paneId={paneId} actions={actions} /></TooltipProvider>));
  return { container, unmount: async () => { await act(async () => root.unmount()); } };
}

it("shows a parent's PR mark over its descendants as the Sessions row does, and a childless pane's own", async () => {
  const parent = pullRow("parent", [], ["child"]);
  const child = pullRow("child", [1073]);
  const { container, unmount } = await mountChip("parent", [parent, child]);
  try {
    const mark = container.querySelector<HTMLButtonElement>("[data-agent-pr=parent]")!;
    // One PR in the family is that PR's number; the digits-only form is a count of several.
    expect(mark.textContent).toBe("#" + "1073");
    expect(mark.getAttribute("aria-label")).toBe("#" + "1073 CI running or review pending");
  } finally { await unmount(); }
  const own = await mountChip("child", [parent, child]);
  try {
    expect(own.container.querySelector("[data-agent-pr=child]")?.textContent).toBe("#" + "1073");
  } finally { await own.unmount(); }
  const several = await mountChip("parent", [parent, pullRow("child", [1073, 1074])]);
  try {
    const mark = several.container.querySelector<HTMLButtonElement>("[data-agent-pr=parent]")!;
    expect(mark.textContent).toBe("2");
    expect(mark.getAttribute("aria-label")).toBe("PR 2 CI running or review pending");
  } finally { await several.unmount(); }
  const none = await mountChip("parent", [{ ...parent, lineage_child_pane_ids: [] }]);
  try {
    expect(none.container.querySelector("[data-agent-pr]")).toBeNull();
  } finally { await none.unmount(); }
});

it("leaves a header alone when a snapshot section it does not draw moves, and redraws it when its own agents or device do", async () => {
  const agents = childrenFixture();
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  initializeInterfaceI18n("en");
  const rest = { navigator: { devices: [{ id: "local", kind: "local" }], workspaces: [] }, ui_state: { theme: "dark" }, status: { pane_focus_request: null, remote: [] } } as unknown as SnapshotRest;
  useShellStore.setState({ agents, rest });
  const draws: Record<string, number> = { shell: 0, parent: 0 };
  const container = document.body.appendChild(document.createElement("div"));
  const root = createRoot(container);
  const actions = createActions(() => true);
  const plain: PaneHeader = { working: false, band: null };
  await act(async () => root.render(<TooltipProvider>
    {["shell", "parent"].map((id) => <Profiler key={id} id={id} onRender={() => { draws[id] = (draws[id] ?? 0) + 1; }}><PaneHeaderBand paneId={id} header={plain} actions={actions} /></Profiler>)}
  </TooltipProvider>));
  try {
    expect(container.querySelector('[data-pane-tree="parent"]')).not.toBeNull();
    const settled = { ...draws };
    await act(async () => useShellStore.setState({ rest: { ...useShellStore.getState().rest!, ui_state: { theme: "light" } } as SnapshotRest }));
    await act(async () => useShellStore.setState({ rest: { ...useShellStore.getState().rest!, navigator: { ...useShellStore.getState().rest!.navigator, workspaces: [] } } as unknown as SnapshotRest }));
    expect(draws).toEqual(settled);
    // The device a parent's totals depend on moves: the parent redraws, a plain pane does not.
    await act(async () => useShellStore.setState({ rest: { ...useShellStore.getState().rest!, status: { pane_focus_request: null, remote: [{ target_id: "mini", state: "disconnected", message: "Mac mini is not connected" }] } } as unknown as SnapshotRest }));
    expect(draws.parent).toBeGreaterThan(settled.parent!);
    expect(draws.shell).toBe(settled.shell);
    // An agent row moves: every header that selects the agents is asked again.
    const before = draws.parent!;
    await act(async () => useShellStore.setState({ agents: agents.map((row) => row.pane_id === "parent" ? { ...row, lineage_child_pane_ids: row.lineage_child_pane_ids!.slice(0, 2) } : row) }));
    expect(draws.parent).toBeGreaterThan(before);
  } finally { await act(async () => root.unmount()); }
});
