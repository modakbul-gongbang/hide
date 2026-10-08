// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { initializeInterfaceI18n } from "./i18n/instance";
import { RELATION_ANSWER_TIMEOUT_MS } from "./lineage";
import { PaneHeaderBand } from "./PaneHeaderBand";
import type { PaneHeader, PaneFocusRequest, SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";
import type { DispatchFn } from "./ws";

vi.hoisted(() => { HTMLCanvasElement.prototype.getContext = () => null; });
const savedShell = useShellStore.getState();
const savedUi = useUiStore.getState();
afterEach(() => {
  useShellStore.setState(savedShell, true);
  useUiStore.setState(savedUi, true);
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

const header: PaneHeader = { working: false, pull: null, band: { kind: "raised_child", tone: "warning", reason: "하위 검증 권한이 필요합니다", since_unix_ms: null, action: {kind: "child", pane_id: "child", label: "자식 검증"}, more: 0, exit_code: null, child_tag: "approval" } };

async function mount(value = header) {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  initializeInterfaceI18n("en");
  useUiStore.setState({relation: null});
  useShellStore.setState({rest: {status: {pane_focus_request: null}} as SnapshotRest});
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions(event => {events.push(event); return true;});
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(<TooltipProvider><PaneHeaderBand paneId="parent" header={value} actions={actions}/></TooltipProvider>));
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

it("turns a missing core answer into a retryable timeout beside the move", async () => {
  const {container, unmount} = await mount();
  try {
    await act(async () => container.querySelector<HTMLButtonElement>("[data-pane-band-open]")!.click());
    await act(async () => vi.advanceTimersByTime(RELATION_ANSWER_TIMEOUT_MS));
    expect(container.querySelector('[role="alert"]')?.textContent).toContain("did not answer in time");
    expect(container.querySelector<HTMLButtonElement>("[data-pane-band-open]")!.disabled).toBe(false);
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
