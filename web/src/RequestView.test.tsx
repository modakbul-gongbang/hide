// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import type { HostBridge, ProbedPath } from "./host";
import { lensHandlers } from "./OverviewLenses";
import { RequestView, type RequestViewProps } from "./RequestView";
import type { RequestRow } from "./requestList";
import type { AgentRow, Checkout, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { clearProbeCache } from "./terminalLinkProvider";
import { NO_REQUEST_LENS } from "./ui";
import type { DispatchFn } from "./ws";

// Count calls through the real row's mark, retaining its implementation.
// Native verification independently observes React commits without this seam.
const observation = vi.hoisted(() => {
  const canvas = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { rows: 0, restore: () => { HTMLCanvasElement.prototype.getContext = canvas; } };
});

// Only the operating system's asynchronous path answers are replaced. The
// renderer, snapshot subscription and reveal_path action are the real ones.
async function pathRow() {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  const saved = useShellStore.getState();
  const savedHost = window.hideHost;
  clearProbeCache();
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const checkout = { id: "main", workspace_id: "project", path: "/checkout", exists: true, label: "main", branch: "main", tabs: [] } as unknown as Checkout;
  const project: Workspace = { id: "project", label: "Studio", path: "/checkout", device_id: "local", registered: true, temporary: false, pinned: false, checkouts: [checkout], inactive_checkouts: { expanded: false, checkout_ids: [] } };
  const agent: AgentRow = { id: "agent", pane_id: "pane", identity_label: "결과물", agent_kind: "codex", symbol: "●", group: "working", status_code: "working", changed_at_unix_ms: null, emphasized: false, unread: false,
    request: { verb: "working", verb_since_unix_ms: 0, request: null, later_by: null, reply: { text: "[report](./report.md) https://example.test/result", cut: false, at_unix_ms: 0 }, pull_requests: [] } };
  const row: RequestRow = { lens: { agent, bucket: "working", project, checkout, device: null, task: null }, verb: "working", children: [] };
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  const pending: { paths: string[]; resolve: (answers: ProbedPath[]) => void }[] = [];
  let probes = 0;
  window.hideHost = {
    kind: "electron", platform: "darwin",
    probePaths: async (paths: string[]) => {
      probes++;
      if (paths.every((path) => !path.endsWith("/report.md"))) return paths.map((real) => ({ real, kind: "directory" as const }));
      return new Promise<ProbedPath[]>((resolve) => pending.push({ paths, resolve }));
    },
  } as HostBridge;
  const context = async (cwd: string | null, path: string) => act(async () => {
    // The row and its request/reply remain the same objects throughout.
    const current = { ...checkout, path, tabs: [{ panes: [{ id: "pane", cwd }] }] } as unknown as Checkout;
    useShellStore.setState({ connection: "live", rest: { navigator: { devices: [{ id: "local", kind: "local" }], workspaces: [{ ...project, checkouts: [current] }] } } as never });
  });
  const answer = async (directory: string) => {
    const path = `${directory}/report.md`;
    const index = pending.findIndex((batch) => batch.paths.includes(path));
    expect(index, `host query for ${path}`).toBeGreaterThanOrEqual(0);
    const batch = pending.splice(index, 1)[0]!;
    await act(async () => batch.resolve(batch.paths.map((real) => real === path ? { real, kind: "file" } : null)));
  };
  const chip = () => container.querySelector('[data-request-open="./report.md"]') as HTMLButtonElement | null;
  const open = async () => {
    expect(chip()).not.toBeNull();
    await act(async () => chip()!.click());
    return events.filter((event) => event.kind === "reveal_path").at(-1);
  };
  const render = async () => act(async () => root.render(<TooltipProvider><RequestView rows={[row]} scope="all" lens={NO_REQUEST_LENS} onLens={() => {}} handlers={lensHandlers(actions, { openIssue: () => {}, toggleFold: () => {} })} actions={actions} /></TooltipProvider>));
  const cleanup = async () => {
    await act(async () => root.unmount());
    container.remove();
    if (savedHost) window.hideHost = savedHost;
    else delete window.hideHost;
    useShellStore.setState(saved, true);
    clearProbeCache();
    vi.unstubAllGlobals();
  };
  return { container, context, answer, chip, open, render, cleanup, probeCount: () => probes };
}

it.each(["cwd", "checkout"] as const)("opens the current %s path when only the pane context changes", async (changed) => {
  const view = await pathRow();
  const context = (directory: string) => changed === "cwd" ? view.context(directory, "/checkout") : view.context(null, directory);
  try {
    await context("/checkout/one");
    await view.render();
    await view.answer("/checkout/one");
    expect((await view.open())?.payload.path).toBe("/checkout/one/report.md");
    const probes = view.probeCount();
    const renders = observation.rows;
    await act(async () => useShellStore.setState({ rest: { ...useShellStore.getState().rest, focused: { pane_id: "unrelated" } } }));
    expect(view.probeCount()).toBe(probes);
    expect(observation.rows).toBe(renders);
    await context("/checkout/two");
    expect(view.chip()).toBeNull();
    expect(view.container.querySelector('[data-request-open="https://example.test/result"]')).not.toBeNull();
    await view.answer("/checkout/two");
    expect((await view.open())?.payload.path).toBe("/checkout/two/report.md");
  } finally {
    await view.cleanup();
  }
});

it("ignores a late path answer from the previous pane context", async () => {
  const view = await pathRow();
  try {
    await view.context(null, "/checkout/old");
    await view.render();
    await view.context(null, "/checkout/current");
    await view.answer("/checkout/current");
    expect((await view.open())?.payload.path).toBe("/checkout/current/report.md");
    await view.answer("/checkout/old");
    expect((await view.open())?.payload.path).toBe("/checkout/current/report.md");
  } finally {
    await view.cleanup();
  }
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
    const agent: AgentRow = { id: `a${index}`, pane_id: `a${index}`, identity_label: `결과물 ${index}`, agent_kind: "codex", symbol: "●", group: "working", status_code: "working", changed_at_unix_ms: null, emphasized: false, unread: false,
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
