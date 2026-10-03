// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { lensHandlers } from "./OverviewLenses";
import { RequestView, type RequestViewProps } from "./RequestView";
import type { RequestRow } from "./requestList";
import type { AgentRow, Checkout, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { NO_REQUEST_LENS } from "./ui";

// Count calls through the real row's mark, retaining its implementation.
// Native verification independently observes React commits without this seam.
const observation = vi.hoisted(() => {
  const canvas = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { rows: 0, restore: () => { HTMLCanvasElement.prototype.getContext = canvas; } };
});
vi.mock("./AgentMark", async (original) => {
  const actual = await original<typeof import("./AgentMark")>();
  return { ...actual, AgentMark: (props: Parameters<typeof actual.AgentMark>[0]) => {
    observation.rows++;
    return actual.AgentMark(props);
  } };
});
afterAll(() => observation.restore());

it("keeps twenty unchanged rows asleep, but renders changed facts and uses current actions", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  // jsdom has no layout or resize notifications; the native run proves fitting.
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  const saved = useShellStore.getState();
  useShellStore.setState({ connection: "live", rest: null });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const checkout = { id: "main", label: "main", branch: "main", tabs: [] } as unknown as Checkout;
  const project: Workspace = { id: "project", label: "Studio", path: "/fixture", device_id: "local", registered: true, temporary: false, pinned: false, checkouts: [checkout], inactive_checkouts: { expanded: false, checkout_ids: [] } };
  const rows: RequestRow[] = Array.from({ length: 20 }, (_, index) => {
    const agent: AgentRow = { id: `a${index}`, pane_id: `a${index}`, identity_label: `결과물 ${index}`, agent_kind: "codex", symbol: "●", group: "working", status_label: "Working", changed_at_unix_ms: null, emphasized: false, unread: false,
      request: { verb: "working", verb_since_unix_ms: 0, request: null, later_by: null, reply: { text: `진행 ${index}`, cut: false, at_unix_ms: 0 }, pull_requests: [] } };
    return { lens: { agent, bucket: "working", project, checkout, device: null, task: null }, verb: "working", children: [] };
  });
  const actions = createActions(() => true);
  let props: RequestViewProps = { rows, scope: "all", lens: NO_REQUEST_LENS, onLens: vi.fn(), handlers: lensHandlers(actions, { openIssue: vi.fn(), toggleFold: vi.fn() }), actions };
  const render = async () => act(async () => root.render(<TooltipProvider><RequestView {...props} /></TooltipProvider>));
  try {
    observation.rows = 0;
    await render();
    expect(container.querySelectorAll("[data-request-row]")).toHaveLength(20);
    const initial = observation.rows;
    expect(initial).toBe(20);
    // The enclosing rest/project received facts unrelated to request rows.
    const refreshed = { ...project, last_activity_unix_ms: 100 };
    props = { ...props, rows: rows.map((row) => ({ ...row, lens: { ...row.lens, project: refreshed }, children: [...row.children] })) };
    await render();
    expect(observation.rows).toBe(initial);
    const first = props.rows[0]!;
    const agent = { ...first.lens.agent, identity_label: "새 결과물", request: { ...first.lens.agent.request!, reply: { text: "새 결과", cut: false, at_unix_ms: 1 } } };
    props = { ...props, rows: [{ ...first, lens: { ...first.lens, agent } }, ...props.rows.slice(1)] };
    await render();
    expect(observation.rows).toBeGreaterThan(initial);
    expect(container.querySelector('[data-request-title="a0"]')?.textContent).toBe("새 결과물");
    expect(container.querySelector('[data-request-result="a0"]')?.textContent).toBe("새 결과");
    const onLens = vi.fn();
    props = { ...props, onLens };
    await render();
    await act(async () => (container.querySelector('[data-request-toggle="a0"]') as HTMLButtonElement).click());
    expect(onLens).toHaveBeenCalledWith({ open: ["a0"] });
    const openAgent = vi.fn();
    props = { ...props, lens: { ...NO_REQUEST_LENS, open: ["a0"] }, handlers: { ...props.handlers, openAgent } };
    await render();
    await act(async () => (container.querySelector('[data-request-toggle="a0"]') as HTMLButtonElement).dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
    expect(openAgent).toHaveBeenCalledWith("a0");
    expect(container.querySelectorAll("[data-request-detail]")).toHaveLength(1);
    const completed = { ...agent, request: { ...agent.request, verb: "result" as const, pull_requests: [{ number: 9, title: "전체 과거 PR 제목 ".repeat(20), url: "https://github.com/acme/project/pull/9", badge: "merged" as const, checks: "passing" as const, live: false, head_branch: "feature/history", closing_issues: [], duty: false, created: true, settled_at_unix_ms: 1 }] } };
    const branch = "feature/전체-분기-이름-".repeat(15);
    const result: RequestRow = { ...first, verb: "result", lens: { ...first.lens, checkout: { ...checkout, branch }, agent: completed } };
    const openGitHub = vi.fn();
    const openedResult = vi.spyOn(actions, "openResult");
    props = { ...props, rows: [result], lens: NO_REQUEST_LENS, handlers: { ...props.handlers, openGitHub } };
    await render();
    await act(async () => (container.querySelector('[data-request-toggle="a0"]') as HTMLButtonElement).click());
    expect(onLens).toHaveBeenLastCalledWith({ open: ["a0"], resting: true });
    expect(openedResult).toHaveBeenCalledWith("a0");
    // The acknowledged result's next snapshot regroups it as idle. Its
    // intentionally opened detail must remain usable with default resting folded.
    props = { ...props, lens: { ...NO_REQUEST_LENS, ...onLens.mock.lastCall![0] }, rows: [{ ...result, verb: "idle", lens: { ...result.lens, agent: { ...completed, request: { ...completed.request, verb: "idle" } } } }] };
    await render();
    expect(container.querySelector('[data-request-detail="a0"]')).not.toBeNull();
    expect(container.querySelector('[data-request-place="a0"]')?.textContent).toContain(branch);
    expect(container.querySelector('[data-request-pull="9"]')?.textContent).toContain(completed.request.pull_requests[0]!.title);
    await act(async () => (container.querySelector('[data-request-pr-chip="9"]') as HTMLButtonElement).click());
    expect(openGitHub).toHaveBeenCalledWith("https://github.com/acme/project/pull/9", "local");
  } finally {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(saved, true);
    vi.unstubAllGlobals();
  }
});
