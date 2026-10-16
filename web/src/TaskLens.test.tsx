// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, afterEach, expect, it, vi } from "vitest";
import { emptyScope } from "../test/legacyAgentScope";
import { legacyAgentRow } from "../test/legacyAgentRow";
import { createActions } from "./actions";
import type { AgentScope, TaskCardScope, TaskState } from "./agentScope";
import { TooltipProvider } from "./components/ui/tooltip";
import { clientI18n } from "./i18n/translator";
import type { AgentRow, Checkout, SnapshotRest, TaskView, Workspace } from "./snapshot";
import { useShellStore } from "./store";
import { TaskLens } from "./TaskLens";
import { useUiStore } from "./ui";

// The sidebar's 태스크 list over a device scope the core already decided
// (PRD sidebar-task-lens B4-B18). Expected groups, order and lines come from
// the PRD, not from the list's own code.

const browserCanvas = vi.hoisted(() => {
  const original = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = () => null;
  return { restore: () => { HTMLCanvasElement.prototype.getContext = original; } };
});
afterAll(() => browserCanvas.restore());

type Sent = { kind: string; payload: Record<string, unknown> };

function row(id: string, over: Partial<AgentRow> = {}, session: Partial<AgentRow["state"]["session"]> = {}): AgentRow {
  const base = legacyAgentRow({ id, pane_id: id, identity_label: `작업 ${id}`, agent_kind: "claude", symbol: "●", group: "working", demand: "none", unread: false, status_code: "working", emphasized: false, changed_at_unix_ms: 1, session_id: `s-${id}`, ...over });
  return { ...base, state: { ...base.state, session: { group: "working", line: null, unfinished: false, ...session } } } as AgentRow;
}

function card(root: string, state: TaskState, checkout: string, members: string[] = [root]): TaskCardScope {
  return { root, state, members, checkouts: [checkout], recency: "" };
}

/** Two projects on this Mac, each with one checkout, and the device's tasks in the core's order. */
function snapshot(agents: AgentRow[], cards: TaskCardScope[], view: TaskView = "status", folds: string[] = []): SnapshotRest {
  const scope: AgentScope = { ...emptyScope(), tasks: { cards, needs_you: cards.filter((entry) => entry.state === "needs_you").length, recency: "" } };
  const checkout = (id: string, project: string) => ({ id, workspace_id: project, label: "main", branch: "main", path: `/${project}`, agent_scope: emptyScope(), tabs: [{ id: `${id}-tab`, panes: agents.map((agent) => ({ id: agent.pane_id })) }] }) as unknown as Checkout;
  const project = (id: string, label: string) => ({ id, device_id: "local", label, path: `/${id}`, is_git: true, agent_scope: emptyScope(), checkouts: [checkout(`${id}-main`, id)] }) as unknown as Workspace;
  return {
    navigator: { devices: [{ id: "local", kind: "local", label: "Local", agent_scope: scope }], focused_device_id: "local", focused_checkout_id: "hide-main", workspaces: [project("hide", "herdr-ide"), project("site", "site")], agents },
    ui_state: { task_view: view, sessions_expanded_agent_pane_ids: folds, left_sidebar_visible: true },
  } as unknown as SnapshotRest;
}

let cleanup: (() => Promise<void>) | null = null;
afterEach(async () => {
  await cleanup?.();
  cleanup = null;
});

async function render(rest: SnapshotRest, agents: AgentRow[], focused: string | null = null) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  await clientI18n.changeLanguage("ko");
  const shell = useShellStore.getState();
  const ui = useUiStore.getState();
  useShellStore.setState({ rest, agents, connection: "live", focusedPaneId: focused });
  useUiStore.setState({ screen: { kind: "workspace" }, overviewOpen: false });
  const sent: Sent[] = [];
  const actions = createActions((event) => { sent.push(event as Sent); return true; });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(<TooltipProvider><TaskLens actions={actions} /></TooltipProvider>));
  cleanup = async () => {
    await act(async () => root.unmount());
    container.remove();
    useShellStore.setState(shell, true);
    useUiStore.setState(ui, true);
    await clientI18n.changeLanguage("en");
    vi.unstubAllGlobals();
  };
  return { container, sent };
}

const heads = (container: HTMLElement) => [...container.querySelectorAll("[data-task-group-head]")].map((head) => head.textContent);
const rows = (container: HTMLElement) => [...container.querySelectorAll("[data-task-row]")].map((element) => element.getAttribute("data-task-row"));

it("groups the device's tasks as Needs You, 막힘, 끝남, 일하는 중 and 기다림, drops empty groups, names each row's project, and folds the resting and cleanup ones (B4, B5, B9)", async () => {
  const agents = [
    row("ask", { status_code: "question", state: undefined as never }, { group: "needs_you" }),
    row("stuck", {}, { line: "API 서버에 닿지 못해 멈춤" }),
    row("waits"), row("works", {}, { line: "테스트 돌리는 중" }), row("done"), row("rest"), row("clean"),
  ];
  agents[0] = { ...agents[0]!, state: { ...agents[0]!.state, ask: { verb: "answer", what: "새 언어로 다시 쓸까요?", more: 0 } } };
  const cards = [card("ask", "needs_you", "hide-main"), card("stuck", "blocked", "hide-main"), card("waits", "waiting", "site-main"), card("works", "working", "hide-main"), card("done", "done", "site-main"), card("rest", "resting", "hide-main"), card("clean", "cleanup", "hide-main")];
  const { container } = await render(snapshot(agents, cards), agents);
  expect(heads(container)).toEqual(["Needs You1", "막힘1", "끝남1", "일하는 중1", "기다림1"]);
  // A waiting task stands in its own group under the working one, whatever the core's order, and 상태별 draws no band labels.
  expect(rows(container)).toEqual(["ask", "stuck", "done", "works", "waits"]);
  expect(container.querySelector("[data-task-band]")).toBeNull();
  expect(container.querySelector("[data-task-count]")?.textContent).toBe("태스크 5");
  const line = (pane: string) => container.querySelector(`[data-task-row="${pane}"] [data-task-line]`);
  expect(line("ask")?.textContent).toBe("herdr-ide새 언어로 다시 쓸까요?");
  expect(line("ask")?.querySelector(".text-warning")).not.toBeNull();
  expect(line("stuck")?.querySelector(".text-destructive")?.textContent).toBe("API 서버에 닿지 못해 멈춤");
  expect(line("waits")?.textContent).toBe("site");
  // The folds open in place and close with 접기.
  const folds = [...container.querySelectorAll<HTMLElement>("[data-task-fold] > button")];
  expect(folds.map((fold) => fold.textContent)).toEqual(["쉬는 중 1", "정리할 것 1"]);
  await act(async () => folds[0]!.click());
  expect(rows(container)).toContain("rest");
  await act(async () => container.querySelector<HTMLElement>('[data-task-focus="fold-close"]')!.click());
  expect(rows(container)).not.toContain("rest");
});

it("in 프로젝트별 draws one group per project, the asking project first, each state's rows under a band label, with no project name on the rows (B6)", async () => {
  const agents = [row("a"), row("b", {}, { group: "needs_you" }), row("c"), row("d")];
  const cards = [card("a", "working", "hide-main"), card("b", "needs_you", "site-main"), card("c", "done", "hide-main"), card("d", "waiting", "hide-main")];
  const { container } = await render(snapshot(agents, cards, "project"), agents);
  expect([...container.querySelectorAll("[data-task-group]")].map((group) => group.getAttribute("data-task-group"))).toEqual(["site", "hide"]);
  expect(container.querySelector("[data-task-project]")).toBeNull();
  expect(rows(container)).toEqual(["b", "c", "a", "d"]);
  expect(heads(container)).toEqual(["siteNeeds You 1", "herdr-ide끝남 1일하는 중 1기다림 1"]);
  const bands = (project: string) => [...container.querySelectorAll(`[data-task-group="${project}"] [data-task-band]`)].map((band) => band.textContent);
  expect(bands("hide")).toEqual(["끝남", "일하는 중", "기다림"]);
  expect(bands("site")).toEqual(["Needs You"]);
});

it("opens a task's pane on a press, its record on Space, and resolves it from the ⋯ menu (B12, B15, B18)", async () => {
  const agents = [row("a", {}, { line: "일하는 중" })];
  const rest = snapshot(agents, [card("a", "working", "hide-main")]);
  (rest as { workspace_view?: unknown }).workspace_view = { device_id: "local", path: "/hide", tool: null, last_tool: null, prs: { only_checkout: false }, pinned: false, tree: false, lane_width: null, tree_width: null, lane_called: 0, lane_calls: 0 };
  const { container, sent } = await render(rest, agents);
  const button = container.querySelector<HTMLElement>('[data-task-row="a"] [data-task-focus="row"]')!;
  await act(async () => button.click());
  expect(sent.at(-1)?.kind).toBe("focus_pane");
  sent.length = 0;
  await act(async () => button.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true })));
  expect(sent.map((event) => event.kind)).toEqual(["detail_open"]);
  expect(sent[0]!.payload).toEqual({ item: { kind: "record", session: { device_id: "local", agent: "claude", session_id: "s-a" } } });
});

it("selects the row of the pane with the keyboard, and its nearest drawn ancestor while it is folded away (B13)", async () => {
  const agents = [row("parent", { lineage_child_pane_ids: ["child"] }), row("child", { lineage_parent_pane_id: "parent", delegated: true }), row("other")];
  const cards = [card("parent", "working", "hide-main", ["parent", "child"]), card("other", "working", "hide-main")];
  const { container } = await render(snapshot(agents, cards), agents, "child");
  expect(container.querySelector('[data-task-child="child"]')).toBeNull();
  expect(container.querySelector('[data-task-row="parent"]')?.getAttribute("data-selected")).toBe("true");
  expect(container.querySelector('[data-task-row="other"]')?.getAttribute("data-selected")).toBeNull();
  // Folded, the parent shows one mark per child.
  expect(container.querySelector('[data-task-row="parent"] [data-task-children]')?.getAttribute("data-task-children")).toBe("1");
});

it("draws the first step when the device has no project, and New agent when it has no task (B20, B42)", async () => {
  const empty = { ...snapshot([], []), navigator: { ...snapshot([], []).navigator, workspaces: [] } } as SnapshotRest;
  const none = await render(empty, []);
  expect(none.container.querySelector("[data-tasks-no-project]")?.textContent).toContain("Add a project to start an agent.");
  await cleanup?.();
  cleanup = null;
  const idle = await render(snapshot([], []), []);
  expect(idle.container.querySelector("[data-tasks-empty]")?.textContent).toContain("진행 중인 일감 없음");
});
