// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { commitCycle, observeRecent, panelCycle } from "./keyboard";
import { MainScreen } from "./MainScreen";
import { resetRecent } from "./recent";
import type { AgentRow, Checkout, SnapshotRest, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

it("restores Main request expansion after departure and remount through Recent Panels, and offers the scoped empty entry", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  const canvas = vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const shell = useShellStore.getState();
  const ui = useUiStore.getState();
  const full = Array.from({ length: 25 }, (_, i) => `요청 원문 ${i}`).join("\n");
  const agent: AgentRow = { id: "a0", pane_id: "a0", identity_label: "요청 결과", agent_kind: "codex", symbol: "●", group: "working", status_code: "working", changed_at_unix_ms: null, emphasized: false, unread: false,
    request: { verb: "idle", verb_since_unix_ms: 0, request: { text: full, cut: false, images: 0, at_unix_ms: 0, sender: { kind: "operator" } }, later_by: null, reply: { text: "전체 결과", cut: false, at_unix_ms: 1 }, pull_requests: [] } };
  const checkout = { id: "main", workspace_id: "project", label: "main", path: "/fixture", branch: "main", active_tab_id: "tab", tabs: [{ id: "tab", label: "Agent", panes: [{ id: "a0" }] }], strip: [] } as unknown as Checkout;
  const project: Workspace = { id: "project", label: "Studio", path: "/fixture", device_id: "local", is_git: true, registered: true, temporary: false, pinned: false, checkouts: [checkout], inactive_checkouts: { expanded: false, checkout_ids: [] } };
  const rest = { navigator: { devices: [{ id: "local", label: "This Mac", kind: "local", state: "local" }], workspaces: [project], agents: [agent], focused_checkout_id: "main", focused_workspace_id: "project" } } as unknown as SnapshotRest;
  useShellStore.setState({ rest, agents: [agent], connection: "live" });
  useUiStore.setState({ screen: { kind: "main", deviceId: "local" }, mainView: "requests", cycle: null, workspaceDialog: null });
  resetRecent();
  const actions = createActions(() => true);
  function ScreenSlot() {
    const screen = useUiStore((state) => state.screen);
    return screen?.kind === "main" ? <MainScreen actions={actions} /> : <div data-departed="true">Workspace</div>;
  }
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<TooltipProvider><ScreenSlot /></TooltipProvider>));
    await act(async () => (container.querySelector('[data-request-group-head="idle"]') as HTMLButtonElement).click());
    await act(async () => (container.querySelector('[data-request-toggle="a0"]') as HTMLButtonElement).click());
    await act(async () => (container.querySelector('[data-request-more="a0"]') as HTMLButtonElement).click());
    expect(container.querySelector('[data-request-full="a0"]')?.textContent).toBe(full);
    const left = useUiStore.getState().screen;
    observeRecent(rest, true);
    await act(async () => useUiStore.getState().setScreen({ kind: "workspace" }));
    expect(container.querySelector("[data-main-screen]")).toBeNull();
    observeRecent(rest, true);
    const cycle = panelCycle(rest)!;
    const index = cycle.items.findIndex((item) => item.kind === "main");
    expect(index).toBeGreaterThanOrEqual(0);
    await act(async () => { commitCycle({ ...cycle, index }, actions); });
    expect(useUiStore.getState().screen).toEqual(left);
    expect(container.querySelector('[data-request-full="a0"]')?.textContent).toBe(full);
    expect(container.querySelector('[data-request-detail="a0"]')?.textContent).toContain("전체 결과");
    // An ordinary Home entry starts folded; with no agent the existing
    // project-scoped creation dialog is still available from Requests.
    await act(async () => {
      useShellStore.setState({ agents: [] });
      useUiStore.getState().setScreen({ kind: "main", deviceId: "local" });
    });
    await act(async () => (container.querySelector("[data-requests-new-agent]") as HTMLButtonElement).click());
    expect(useUiStore.getState().workspaceDialog).toEqual({ kind: "new_worktree", workspaceId: "project" });
  } finally {
    await act(async () => root.unmount());
    container.remove();
    resetRecent();
    useUiStore.setState(ui, true);
    useShellStore.setState(shell, true);
    canvas.mockRestore();
    vi.unstubAllGlobals();
  }
});
