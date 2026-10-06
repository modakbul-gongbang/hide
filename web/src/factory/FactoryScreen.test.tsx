// @vitest-environment jsdom
// The Factory screen's states a browser run cannot reach without GitHub or a
// running worker (PRD software-factory-ui B10, B19, B20): the outside read
// gone stale, an action the engine refused, and the page actions of states a
// held fixture never enters. The summary is the engine's shape; the screen,
// stores and actions are the real ones, and only dispatch is recorded.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "../actions";
import { TooltipProvider } from "../components/ui/tooltip";
import { useShellStore } from "../store";
import { FACTORY_ENTRY, useUiStore, type FactoryPlace } from "../ui";
import type { DispatchFn } from "../ws";
import { FactoryScreen } from "./FactoryScreen";
import type { CardView, FactorySummary, FactoryView, InboxItem, TaskDetail, TaskState } from "./model";

const NOW = Date.now();

function card(task: string, state: TaskState, patch: Partial<CardView> = {}): CardView {
  return { task, display_id: task, column: null, title: `${task} 제목`, state, state_label: state, needs_person: false, waiting_for: null, priority: 0, since: NOW, unread: false, folded: false, archived: false, failures: 0, external: [], revive_until: null, worker_pane: null, ...patch };
}

function factory(patch: Partial<FactoryView> = {}): FactoryView {
  return {
    id: "f1", project: "/fixture", project_name: "fixture", source: "github", verification: "ci", closed: false,
    flow: { drafting: 0, waiting: 0, running: 1, done_today: 0 }, my_turn: 1,
    columns: [{ column: "running", label: "running", cards: [card("T-1", "running", { column: "running" })] }],
    cancelled: [], graph: { nodes: ["T-1"], edges: [], unrelated: ["T-1"] }, dependencies: [],
    outside_read_at: NOW - 600_000, stale: false, main_broken: false, auto_merge_available: true, merge_mode: "manual", ...patch,
  };
}

const MERGE: InboxItem = {
  group: "merge", kind: "merge", rank: 0, factory: "f1", task: "T-1", display_id: "#12", title: "T-1 제목", project: "fixture", question: null,
  text: "병합할까요?", suggestion: "merge", result: "", default_action: null, choices: ["merge", "request-changes", "cancel"], deadline: null, remaining: null, waiting_since: NOW, waiting_days: 0,
};

let root: Root | null = null;

async function mount(summary: FactorySummary, place: Partial<FactoryPlace> = {}) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  // jsdom lays nothing out, so it has no scrolling to bring the open item into view.
  Element.prototype.scrollIntoView = () => {};
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const events: Parameters<DispatchFn>[0][] = [];
  const actions = createActions((event) => { events.push(event); return true; });
  useShellStore.setState({ connection: "live", factory: { summary, actions: [] }, factoryTask: null });
  useUiStore.setState({ screen: { kind: "factory", place: { ...FACTORY_ENTRY, ...place } } });
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await act(async () => root!.render(<TooltipProvider><FactoryScreen actions={actions} /></TooltipProvider>));
  return { container, events };
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  delete (Element.prototype as Partial<Element>).scrollIntoView;
});

it("turns the outside read the warning colour after three failed reads, and keeps it plain while reads succeed (B20)", async () => {
  const { container } = await mount({ my_turn: 0, factories: [factory({ stale: true })], inbox: [] });
  const read = container.querySelector("[data-factory-outside-read]")!;
  expect(read.getAttribute("data-factory-outside-read")).toBe("stale");
  expect(read.className).toContain("text-warning");
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 0, factories: [factory()], inbox: [] }, actions: [] } }));
  expect(container.querySelector("[data-factory-outside-read]")!.getAttribute("data-factory-outside-read")).toBe("fresh");
});

it("keeps a refused answer's item in place with the engine's next action (B10)", async () => {
  const { container, events } = await mount({ my_turn: 1, factories: [factory()], inbox: [MERGE] });
  const send = container.querySelector<HTMLButtonElement>("[data-factory-send]")!;
  await act(async () => send.click());
  const sent = events.at(-1) as unknown as { kind: string; payload: { request_id: string; command: { verb: string; task: string } } };
  expect(sent.kind).toBe("factory_action");
  expect(sent.payload.command).toEqual({ verb: "merge", task: "f1/T-1" });
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 1, factories: [factory()], inbox: [MERGE] }, actions: [{ request_id: sent.payload.request_id, answer: { ok: false, reason: "merge_refused", next_action: "충돌을 해결한 뒤 다시 병합하세요" } }] } }));
  const refused = container.querySelector("[data-factory-refused]")!;
  expect(refused.getAttribute("data-factory-refused")).toBe("merge_refused");
  expect(refused.textContent).toContain("충돌을 해결한 뒤 다시 병합하세요");
  expect(container.querySelectorAll("[data-factory-item]")).toHaveLength(1);
});

function detail(state: TaskState, allowed: string[]): TaskDetail {
  return {
    card: card("T-1", state), factory: "f1", project: "/fixture", goal: "목표", criteria: [], out_of_scope: [], before: [], after: [], attachments: [], pr: null,
    verification: "1/3", attempts: [], decisions: [], questions: [], discoveries: [], gates: [], allowed, stop: null, merge_sha: null, worker_name: null, worktree: null, branch: null,
  };
}

it.each([
  ["running", ["pause", "cancel"], "pause cancel"],
  ["paused", ["resume", "cancel"], "resume cancel"],
  ["merge_waiting", ["merge", "request-changes", "cancel"], "merge request-changes cancel"],
  ["stopped", ["retry", "cancel"], "retry cancel"],
  ["verifying", ["cancel"], "cancel"],
] as const)("draws only the %s state's actions the engine allows (B19)", async (state, allowed, drawn) => {
  const { container, events } = await mount({ my_turn: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: detail(state, [...allowed]) } }));
  expect(container.querySelector("[data-factory-actions]")!.getAttribute("data-factory-actions")).toBe(drawn);
  expect(events.some((event) => (event as unknown as { kind: string }).kind === "factory_task_open")).toBe(true);
});

it("draws no action for a blocked Task, which takes only an answer (B19)", async () => {
  const { container } = await mount({ my_turn: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: detail("blocked", []) } }));
  expect(container.querySelector("[data-factory-task-state]")).not.toBeNull();
  expect(container.querySelector("[data-factory-actions]")).toBeNull();
});
