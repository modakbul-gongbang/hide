import { emptyScope } from "../test/legacyAgentScope";
// @vitest-environment jsdom
import { act, Profiler } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { translate } from "./i18n/client";
import { issuePlaces, NewIssueDialog, StartIssueDialog } from "./IssueDialogs";
import type { BackgroundAi, Device, Task, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import type { DispatchFn } from "./ws";

// The shell's modules reach xterm, which asks jsdom for a canvas it lacks.
vi.hoisted(() => {
  HTMLCanvasElement.prototype.getContext = () => null;
});

const task = { key: "github:7", source: "github", id: "#7", url: null, title: "Fix the sidebar", open: true } as Task;
const workspace = { agent_scope: emptyScope(), id: "ws-1", path: "/repo", is_git: true, device_id: null, branches: ["main"], default_branch: "main", checkouts: [] } as unknown as Workspace;
const claude = { id: "claude", label: "Claude Code", agent: "claude-code", state: "ready", headline: "", message: null, installed: true, selectable: true, retry_at_ms: null, model: "", models: [], models_fixed: false, cli_default: false, models_unavailable_reason: null };
const ai = (over: Partial<BackgroundAi>): BackgroundAi => ({ enabled: true, provider: "claude", chosen: true, providers: [claude], fallback: [], refusal: null, unavailable_reason: null, ...over }) as BackgroundAi;

afterEach(() => {
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
});

async function open(background: BackgroundAi) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const saved = useShellStore.getState();
  await act(async () => {
    useShellStore.setState({
      connection: "live",
      rest: { status: { background_ai: background }, issue_work: { detail: { task_key: task.key, phase: "ready", body: "Body" } } },
    } as never);
    root.render(
      <TooltipProvider>
        <StartIssueDialog actions={actions} workspace={workspace} task={task} onClose={() => {}} />
      </TooltipProvider>,
    );
  });
  const q = (selector: string) => document.body.querySelector(selector) as HTMLElement | null;
  return { events, q, unmount: async () => { await act(async () => root.unmount()); useShellStore.setState(saved, true); } };
}

const kinds = (events: Parameters<DispatchFn>[0][]) => events.map((event) => event.kind);

it("asks the AI to name the worktree and says so while it works, when Hide AI can answer", async () => {
  const { events, q, unmount } = await open(ai({}));
  expect(kinds(events)).toContain("worktree_name_suggest");
  expect(q("[data-name-state]")?.getAttribute("data-name-state")).toBe("working");
  await unmount();
});

it.each([
  ["Hide AI is off", ai({ enabled: false })],
  ["no agent is chosen", ai({ provider: null, chosen: false })],
  ["the chosen agent cannot answer", ai({ providers: [{ ...claude, state: "needs_login", selectable: false }] })],
])("neither asks nor waits for a name when %s", async (_name, background) => {
  const { events, q, unmount } = await open(background);
  expect(kinds(events)).not.toContain("worktree_name_suggest");
  expect(q("[data-name-state]")).toBeNull();
  await unmount();
});

it("names every row's machine in New issue's list when it spans machines, this window's as This Mac, so two projects at the same name read apart (D-29)", () => {
  // This window runs on a Mac, whatever system runs the test: this machine is named by its system.
  vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "" });
  const devices = [
    { id: "mini-node", kind: "local", state: "local", label: "mini", machine_name: "Mac mini" },
    { id: "macbook-node", kind: "remote", state: "ready", label: "MacBook", dials_in: true },
  ] as unknown as Device[];
  const source = { kind: "local", label: "Local", name: null } as never;
  const own = { id: "ws-own", label: "herdr-ide", device_id: "mini-node", remote_target_id: null, tasks: { source } } as unknown as Workspace;
  const node = { ...own, id: "ws-node", device_id: "macbook-node", remote_target_id: "macbook-node" } as Workspace;
  const rows = () => [...issuePlaces([own, node], devices, translate).values()];
  // The core's window.
  expect(rows()).toEqual(["herdr-ide · Local · This Mac", "herdr-ide · Local · MacBook"]);
  // The MacBook's window after the move: the core's row is the mini's.
  window.location.hash = "#token=t&node=macbook-node";
  expect(rows()).toEqual(["herdr-ide · Local · Mac mini", "herdr-ide · Local · This Mac"]);
  window.location.hash = "";
  // One machine's list names none.
  expect([...issuePlaces([own], devices, translate).values()]).toEqual(["herdr-ide · Local"]);
});

it("draws New issue again for a project change, not for a snapshot change it does not read", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  const own = { ...workspace, label: "herdr-ide", remote_target_id: null } as Workspace;
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const saved = useShellStore.getState();
  let draws = 0;
  await act(async () => {
    useShellStore.setState({ connection: "live", rest: { navigator: { workspaces: [own], devices: [] }, status: {} } } as never);
    root.render(
      <Profiler id="new-issue" onRender={() => { draws += 1; }}>
        <TooltipProvider>
          <NewIssueDialog actions={createActions(() => true)} workspace={own} onClose={() => {}} />
        </TooltipProvider>
      </Profiler>,
    );
  });
  const settled = draws;
  const rest = useShellStore.getState().rest!;
  await act(async () => { useShellStore.setState({ rest: { ...rest, task_operation: { phase: "working" } } } as never); });
  expect(draws).toBe(settled);
  await act(async () => { useShellStore.setState({ rest: { ...rest, navigator: { ...rest.navigator, workspaces: [{ ...own, label: "renamed" }] } } } as never); });
  expect(draws).toBeGreaterThan(settled);
  await act(async () => root.unmount());
  useShellStore.setState(saved, true);
});
