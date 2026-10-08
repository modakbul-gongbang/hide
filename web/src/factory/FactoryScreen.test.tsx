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
import { english } from "../i18n/catalogs";
import { TooltipProvider } from "../components/ui/tooltip";
import { useShellStore } from "../store";
import type { SnapshotRest } from "../snapshot";
import { FACTORY_ENTRY, useUiStore, type FactoryPlace } from "../ui";
import type { DispatchFn } from "../ws";
import { InitFailure } from "./CreateSheet";
import { FactoryScreen } from "./FactoryScreen";
import { REQUEST_ANSWER_TIMEOUT_MS } from "./request";
import type { CardView, FactorySummary, FactoryView, InboxItem, TaskDetail, TaskState } from "./model";

const NOW = Date.now();

function card(task: string, state: TaskState, patch: Partial<CardView> = {}): CardView {
  return { task, display_id: task, column: null, title: `${task} 제목`, summary: "작업 요약", issue: null, issue_url: null, pr: null, worker_runtime: null, resume_at: null, waiting_group: null, stage: 1, state, state_label: state, needs_person: false, waiting_for: null, waiting_code: null, waiting_on: [], env_hold: null, stop: null, priority: 0, since: NOW, unread: false, folded: false, archived: false, failures: 0, external: [], revive_until: null, worker_pane: null, worker_label: null, pause_reason: null, ...patch };
}

function factory(patch: Partial<FactoryView> = {}): FactoryView {
  return {
    id: "f1", project: "/fixture", project_name: "fixture", source: "github", verification: "ci", closed: false,
    flow: { before: 0, stuck: 0, moving: 1, done_today: 0 }, my_turn: 1,
    columns: [{ column: "moving", label: "running", cards: [card("T-1", "running", { column: "moving" })] }],
    cancelled: [], graph: { nodes: ["T-1"], edges: [], unrelated: ["T-1"] }, dependencies: [],
    outside_read_at: NOW - 600_000, stale: false, main_broken: false, auto_merge_available: true, merge_mode: "manual", paused: false, notices: 0, observer_mode: "assist", observer_today: 0, observer_limit: 100, factory_ai: null, workers: [{ agent: "claude", description: "" }], macos_notifications: false, ...patch,
  };
}

const MERGE: InboxItem = {
  group: "merge", kind: "merge", rank: 0, factory: "f1", task: "T-1", display_id: "#12", title: "T-1 제목", project: "fixture", question: null,
  text: "병합할까요?", suggestion: "merge", result: "", default_action: null, choices: ["merge", "request-changes", "cancel"], deadline: null, remaining: null, remaining_hours: null, waiting_since: NOW, waiting_days: 0,
  result_code: "merge", unblocks: [], gates: ["manual_mode"], stop: null, notice: null, refers_to: null, decision_kind: null, observer_reason: null, overridable: false,
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
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [factory({ stale: true })], inbox: [] });
  const read = container.querySelector("[data-factory-outside-read]")!;
  expect(read.getAttribute("data-factory-outside-read")).toBe("stale");
  expect(read.className).toContain("text-warning");
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, actions: [] } }));
  expect(container.querySelector("[data-factory-outside-read]")!.getAttribute("data-factory-outside-read")).toBe("fresh");
});

it("keeps a refused answer's item in place with its next action in the screen's language (B10, B24)", async () => {
  const { container, events } = await mount({ my_turn: 1, notices: 0, factories: [factory()], inbox: [MERGE] });
  const send = container.querySelector<HTMLButtonElement>("[data-factory-send]")!;
  await act(async () => send.click());
  const sent = events.at(-1) as unknown as { kind: string; payload: { request_id: string; command: { verb: string; task: string } } };
  expect(sent.kind).toBe("factory_action");
  expect(sent.payload.command).toEqual({ verb: "merge", task: "f1/T-1" });
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 1, notices: 0, factories: [factory()], inbox: [MERGE] }, actions: [{ request_id: sent.payload.request_id, answer: { ok: false, reason: "main_dirty", next_action: "Commit or stash the changes in the main checkout, then merge" } }] } }));
  const refused = container.querySelector("[data-factory-refused]")!;
  expect(refused.getAttribute("data-factory-refused")).toBe("main_dirty");
  expect(refused.textContent).toBe(english["factory.refusal.main_dirty"]);
  expect(container.querySelectorAll("[data-factory-item]")).toHaveLength(1);
});

function detail(state: TaskState, allowed: string[]): TaskDetail {
  return {
    card: card("T-1", state), factory: "f1", project: "/fixture", goal: "목표", criteria: [], out_of_scope: [], before: [], after: [], attachments: [], pr: null,
    verification: "1/3", attempts: [], decisions: [], questions: [], discoveries: [], gates: [], gate_codes: [], allowed, stop: null, stop_code: null, merge_sha: null, worker_name: null, worktree: null, branch: null,
    worker: null, diagnosis: null, auto_restarts: 0, resting_since: null, pinned_worker: null, ai_picked_worker: null, ai_pick_reason: null, woke_at: null, diagnosed_at: null, diagnosed_from: null,
  };
}

it.each([
  ["running", ["pause", "cancel"], "pause cancel"],
  ["paused", ["resume", "cancel"], "resume cancel"],
  ["merge_waiting", ["merge", "request-changes", "cancel"], "merge request-changes cancel"],
  ["stopped", ["retry", "cancel"], "retry cancel"],
  ["verifying", ["cancel"], "cancel"],
] as const)("draws only the %s state's actions the engine allows (B19)", async (state, allowed, drawn) => {
  const { container, events } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: detail(state, [...allowed]) } }));
  expect(container.querySelector("[data-factory-actions]")!.getAttribute("data-factory-actions")).toBe(drawn);
  expect(events.some((event) => (event as unknown as { kind: string }).kind === "factory_task_open")).toBe(true);
});

it("draws no action for a blocked Task, which takes only an answer (B19)", async () => {
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: detail("blocked", []) } }));
  expect(container.querySelector("[data-factory-task-state]")).not.toBeNull();
  expect(container.querySelector("[data-factory-actions]")).toBeNull();
});

const CONFIG = {
  verification: { kind: "ci", checks: ["ci"] }, merge_mode: "manual", merge_method: "merge", quick_check: null, question_deadline_ms: 86_400_000, stall_ms: 1_800_000,
  no_report_ms: 3_600_000, watch_interval_ms: 1_800_000, watch_daily_limit: 4, outside_read_ms: 120_000, cancel_keep_ms: 604_800_000, done_fold_ms: 259_200_000,
  archive_fold_ms: 7_776_000_000, new_task_limit: 10, verify_failure_limit: 3, verify_timeout_ms: 3_600_000, disk_floor_bytes: 10_737_418_240, default_runtime: "claude",
  harness: null, autonomy: [], autonomy_diff_limit: 400, recovery: [], risk_paths: [], checks: [], prd_in_issue: false, macos_notifications: false, worker_args: {},
  workers: [], observer_mode: "assist", observer_daily_limit: 100, factory_ai: null,
};

/** Answers the settings tab's config read the way the engine does. */
async function answerConfig(summary: FactorySummary, events: Parameters<DispatchFn>[0][]) {
  const read = events.find((event) => (event as unknown as { kind: string; payload: { command: { verb: string } } }).payload?.command?.verb === "config") as unknown as { payload: { request_id: string } };
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: read.payload.request_id, answer: { ok: true, config: CONFIG, machine: { max_workers: 5 } } }] } }));
}

it("keeps Close disabled while the Running column holds a Task in any of its states, as the engine refuses then (B22)", async () => {
  const waiting = factory({ columns: [{ column: "moving", label: "running", cards: [card("T-1", "merge_waiting", { column: "moving" })] }] });
  const summary = { my_turn: 0, notices: 0, factories: [waiting], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings", factory: "f1" });
  await answerConfig(summary, events);
  expect(container.querySelector<HTMLButtonElement>("[data-factory-close]")!.disabled).toBe(true);
  const done = { my_turn: 0, notices: 0, factories: [factory({ columns: [{ column: "done", label: "done", cards: [card("T-1", "done", { column: "done" })] }] })], inbox: [] };
  await answerConfig(done, events);
  expect(container.querySelector<HTMLButtonElement>("[data-factory-close]")!.disabled).toBe(false);
});

it("takes an answer that comes after the wait ran out, since the engine may still finish the work (B10)", async () => {
  vi.useFakeTimers();
  try {
    const { container, events } = await mount({ my_turn: 1, notices: 0, factories: [factory()], inbox: [MERGE] });
    await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-send]")!.click());
    const sent = events.at(-1) as unknown as { payload: { request_id: string } };
    await act(async () => vi.advanceTimersByTime(REQUEST_ANSWER_TIMEOUT_MS + 1));
    expect(container.querySelector("[data-factory-refused]")!.getAttribute("data-factory-refused")).toBe("no_answer");
    await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 1, notices: 0, factories: [factory()], inbox: [MERGE] }, actions: [{ request_id: sent.payload.request_id, answer: { ok: true } }] } }));
    expect(container.querySelector("[data-factory-refused]")).toBeNull();
    expect(container.querySelector("[data-factory-send]")!.getAttribute("data-factory-send")).toBe("taken");
  } finally {
    vi.useRealTimers();
  }
});

it("says what acknowledging a notice does, though a notice has no suggestion (B9)", async () => {
  const notice: InboxItem = { ...MERGE, group: "notice", kind: "notice", question: "q-9", text: "무관한 발견", suggestion: "", choices: ["ok"], result_code: "acknowledge", gates: [] };
  const { container } = await mount({ my_turn: 1, notices: 0, factories: [factory()], inbox: [notice] });
  expect(container.querySelector("[data-factory-result]")!.getAttribute("data-factory-result")).toBe("acknowledge");
  expect(container.querySelector("[data-factory-result]")!.textContent).toBe(english["factory.result.acknowledge"]);
});

it("shows where a project check failed and the engine's next action, a logged-out gh's included (B5)", async () => {
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  const state = { phase: "refused", answer: { ok: false, reason: "init_failed", next_action: "Run gh auth login, then retry", detail: { stage: "probe" } } } as const;
  await act(async () => root!.render(<InitFailure state={state} />));
  expect(container.querySelector("[data-factory-create-stage]")!.textContent).toBe(english["factory.create.failedStage"].replace("{{stage}}", english["factory.create.stage.probe"]));
  expect(container.querySelector("[data-factory-create-next]")!.textContent).toContain("gh auth login");
});

function sentVerbs(events: Parameters<DispatchFn>[0][]): string[] {
  return events.flatMap((event) => {
    const sent = event as unknown as { kind: string; payload: { command: { verb: string } } };
    return sent.kind === "factory_action" ? [sent.payload.command.verb] : [];
  });
}

function press(target: Element, init: KeyboardEventInit) {
  target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
}

it("sends the picked answer on Enter, but leaves Enter on 자세히 to open the Task and Enter mid-composition alone (B9)", async () => {
  const { container, events } = await mount({ my_turn: 1, notices: 0, factories: [factory()], inbox: [MERGE] });
  const details = container.querySelector<HTMLButtonElement>("[data-factory-details]")!;
  await act(async () => press(details, { key: "Enter" }));
  expect(sentVerbs(events)).toEqual([]);
  const choice = container.querySelector("[data-factory-choice='1']")!;
  await act(async () => press(choice, { key: "Enter", isComposing: true }));
  expect(sentVerbs(events)).toEqual([]);
  await act(async () => press(choice, { key: "Enter" }));
  expect(sentVerbs(events)).toEqual(["merge"]);
});

it("sends a setting once though Enter and leaving the field both commit, and shows the saved value again when the engine refuses it (B22)", async () => {
  const summary = { my_turn: 0, notices: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings", factory: "f1" });
  await answerConfig(summary, events);
  const field = container.querySelector<HTMLInputElement>("[data-factory-setting='watch_daily_limit']")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, "0");
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => press(field, { key: "Enter" }));
  await act(async () => field.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
  const writes = events.filter((event) => (event as unknown as { payload: { command: { set?: unknown[] } } }).payload?.command?.set?.length);
  expect(writes).toHaveLength(1);
  const write = writes[0] as unknown as { payload: { request_id: string } };
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: write.payload.request_id, answer: { ok: false, reason: "config_invalid", next_action: "Check the value" } }] } }));
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='watch_daily_limit']")!.value).toBe(String(CONFIG.watch_daily_limit));
});

it("treats a Factory whose cards are all archived as empty, not as a filter that matches nothing (B15)", async () => {
  const archived = factory({ columns: [{ column: "done", label: "done", cards: [card("T-1", "done", { column: "done", archived: true, folded: true })] }] });
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [archived], inbox: [] }, { tab: "board" });
  expect(container.querySelector("[data-factory-board-empty='intake']")).not.toBeNull();
});

it("waits behind Settings or a close confirmation instead of swapping the screen under it (B2)", () => {
  const actions = createActions(() => true);
  useUiStore.setState({ screen: { kind: "main" }, overlay: "settings", pendingClose: null });
  actions.openFactory();
  expect(useUiStore.getState().screen?.kind).toBe("main");
  useUiStore.setState({ overlay: "none", pendingClose: { paneId: "p1" } as never });
  actions.openFactory();
  expect(useUiStore.getState().screen?.kind).toBe("main");
  useUiStore.setState({ pendingClose: null });
  actions.openFactory();
  expect(useUiStore.getState().screen?.kind).toBe("factory");
});

function type(field: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, value);
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

function lastAction(events: Parameters<DispatchFn>[0][]) {
  return events.filter((event) => (event as unknown as { kind: string }).kind === "factory_action").at(-1) as unknown as { payload: { request_id: string; command: Record<string, unknown> } };
}

it("puts an emptied number back to the saved value instead of sending 0 (B22)", async () => {
  const summary = { my_turn: 0, notices: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings", factory: "f1" });
  await answerConfig(summary, events);
  const field = container.querySelector<HTMLInputElement>("[data-factory-setting='question_deadline_hours']")!;
  await act(async () => type(field, ""));
  await act(async () => field.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
  expect(events.some((event) => ((event as unknown as { payload: { command?: { set?: unknown[] } } }).payload?.command?.set?.length ?? 0) > 0)).toBe(false);
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='question_deadline_hours']")!.value).toBe("24");
});

it("keeps a change request's comment while the engine refuses it, and closes the form once taken (B19)", async () => {
  const { container, events } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: detail("merge_waiting", ["merge", "request-changes", "cancel"]) } }));
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-action='request-changes']")!.click());
  await act(async () => type(container.querySelector<HTMLInputElement>("[data-factory-comment]")!, "테스트를 더 써 주세요"));
  await act(async () => container.querySelector("[data-factory-comment]")!.closest("form")!.requestSubmit());
  const sent = lastAction(events);
  expect(sent.payload.command).toMatchObject({ verb: "request_changes", comment: "테스트를 더 써 주세요" });
  const summary = { my_turn: 0, notices: 0, factories: [factory()], inbox: [] };
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: sent.payload.request_id, answer: { ok: false, reason: "factory_busy", next_action: "Try again in a moment" } }] } }));
  expect(container.querySelector<HTMLInputElement>("[data-factory-comment]")!.value).toBe("테스트를 더 써 주세요");
  await act(async () => container.querySelector("[data-factory-comment]")!.closest("form")!.requestSubmit());
  const again = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: again.payload.request_id, answer: { ok: true } }] } }));
  expect(container.querySelector("[data-factory-comment]")).toBeNull();
});

it("says a Task is gone when its Factory has left the summary, instead of loading forever", async () => {
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { task: { factory: "gone", task: "T-1" } });
  expect(container.querySelector("[data-factory-task-missing]")).not.toBeNull();
  expect(container.querySelector("[data-factory-task-loading]")).toBeNull();
});

it("does not send a taken answer again while its item is still on screen (B10)", async () => {
  const { container, events } = await mount({ my_turn: 1, notices: 0, factories: [factory()], inbox: [MERGE] });
  const send = container.querySelector<HTMLButtonElement>("[data-factory-send]")!;
  await act(async () => send.click());
  const sent = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 1, notices: 0, factories: [factory()], inbox: [MERGE] }, actions: [{ request_id: sent.payload.request_id, answer: { ok: true } }] } }));
  expect(container.querySelector<HTMLButtonElement>("[data-factory-send]")!.disabled).toBe(true);
  await act(async () => press(container.querySelector("[data-factory-choice='1']")!, { key: "Enter" }));
  expect(sentVerbs(events)).toEqual(["merge"]);
});

it("puts a number the field cannot take back to the saved value instead of leaving it as if saved (B22)", async () => {
  const summary = { my_turn: 0, notices: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings", factory: "f1" });
  await answerConfig(summary, events);
  const field = container.querySelector<HTMLInputElement>("[data-factory-setting='watch_daily_limit']")!;
  await act(async () => type(field, "2.5"));
  await act(async () => field.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='watch_daily_limit']")!.value).toBe(String(CONFIG.watch_daily_limit));
});

it("leaves an open priority form alone when another action on the page is taken (B19)", async () => {
  const { container, events } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: detail("waiting", ["priority", "cancel"]) } }));
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-action='priority']")!.click());
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-action='cancel']")!.click());
  const cancel = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, actions: [{ request_id: cancel.payload.request_id, answer: { ok: true } }] } }));
  expect(container.querySelector("[data-factory-priority]")).not.toBeNull();
});

it("shows a done Task's criteria met, names what the verification count counts, and cues a shortened decision (B19)", async () => {
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  const done = { ...detail("done", []), verification: "0/3", criteria: ["README가 최신"], decisions: [{ at: NOW, by: "engine", text: "is al [cut 15 bytes] -> 머지 결정" }] } as unknown as TaskDetail;
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: done } }));
  expect(container.querySelector("[data-factory-criterion]")!.getAttribute("data-factory-criterion")).toBe("met");
  expect(container.querySelector("[data-factory-verification]")!.textContent).toContain(english["factory.task.verification"].replace("{{value}}", "0/3"));
  const decisions = container.querySelector("[data-factory-decisions]")!;
  expect(decisions.textContent).not.toContain("[cut");
  expect(decisions.querySelector("[data-factory-cut]")).not.toBeNull();
});

it("keeps the graph in columns and logs why when the layout worker cannot start (D-08)", async () => {
  // jsdom has no Worker, so the layered layout refuses the way a failed worker does.
  const graphed = factory({
    columns: [{ column: "before", label: "waiting", cards: [card("T-1", "waiting", { column: "before" }), card("T-2", "waiting", { column: "before" })] }],
    graph: { nodes: ["T-1", "T-2"], edges: [["T-1", "T-2"]], unrelated: [] },
    dependencies: [["T-1", "T-2"]],
  });
  useShellStore.setState({ diagnostics: [] });
  // jsdom has no CSS.escape; the ids here need no escaping.
  vi.stubGlobal("CSS", { escape: (value: string) => value });
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [graphed], inbox: [] }, { tab: "graph" });
  await act(async () => {
    await vi.waitFor(() => expect(useShellStore.getState().diagnostics.some((line) => line.includes("dependency graph stays in columns"))).toBe(true));
  });
  expect(container.querySelector("[data-dependency-graph]")!.getAttribute("data-dependency-layout")).toBe("columns");
  expect(container.querySelectorAll("[data-dependency-layer]")).toHaveLength(2);
});

// These fail if a card sends another command, navigates on an action, or forgets its taken request.
it.each([
  ["blocked", { ...MERGE, group: "answer", kind: "blocking", question: "q1", suggestion: "WS", choices: ["REST"], text: "Which endpoint?" }, { verb: "answer", task: "f1/T-1", question: "q1", choice: "suggestion", text: null }],
  ["merge_waiting", MERGE, { verb: "merge", task: "f1/T-1" }],
  ["stopped", { ...MERGE, group: "stopped", kind: "stopped", suggestion: "retry", stop: "verify_failed" }, { verb: "retry", task: "f1/T-1" }],
  ["stopped", { ...MERGE, group: "stopped", kind: "action", question: "q1", suggestion: "retry", stop: "verify_failed" }, { verb: "retry", task: "f1/T-1" }],
  ["stopped", { ...MERGE, group: "answer", kind: "new_task_cap", question: "q1", suggestion: "raise cap", stop: "verify_failed" }, { verb: "retry", task: "f1/T-1" }],
] as const)("sends the %s card's canonical command once, preserves a refusal and allows retry", async (state, item, expected) => {
  const task = card("T-1", state, { column: "stuck", waiting_group: "person", needs_person: true, stop: state === "stopped" ? "verify_failed" : null });
  const summary = { my_turn: 1, notices: 0, factories: [factory({ columns: [{ column: "stuck", label: "", cards: [task] }] })], inbox: [{ ...item, choices: [...item.choices], gates: [...item.gates], unblocks: [...item.unblocks] } as InboxItem] };
  if (state === "merge_waiting" || item.kind === "stopped") summary.inbox.unshift({ ...MERGE, kind: "action", question: "older-question", suggestion: "approve", gates: [] });
  const { container, events } = await mount(summary, { tab: "board" });
  const send = container.querySelector<HTMLButtonElement>("[data-factory-card-send]")!;
  expect(send).not.toBeNull();
  await act(async () => send.click());
  expect((events.at(-1)!.payload as { command: unknown }).command).toEqual(expected);
  expect(useUiStore.getState().screen).toMatchObject({ place: { task: null } });
  expect(send.disabled).toBe(true);
  const sent = events.at(-1)!;
  await act(async () => send.click());
  expect(events.at(-1)).toBe(sent);
  const request = sent.payload as { request_id: string };
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: request.request_id, answer: { ok: false, reason: "action_not_allowed_in_state", next_action: "Read current task" } }] } }));
  expect(container.querySelector("[data-factory-refused]")).not.toBeNull();
  expect(send.disabled).toBe(false);
  await act(async () => send.click());
  const retry = events.at(-1)!.payload as { request_id: string };
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: retry.request_id, answer: { ok: true } }] } }));
  expect(send.disabled).toBe(true);
});

it("opens the corresponding inbox item for another answer and keeps issue/PR controls separate from card navigation", async () => {
  const task = card("T-1", "blocked", { column: "stuck", waiting_group: "person", needs_person: true, issue: "#12", issue_url: "https://example.invalid/issues/12", pr: { number: 34, url: "https://example.invalid/pull/34", head: "change", by_factory: true, open: true } });
  const item: InboxItem = { ...MERGE, group: "answer", kind: "blocking", question: "q1", suggestion: "WS", choices: ["REST"] };
  const { container } = await mount({ my_turn: 1, notices: 0, factories: [factory({ columns: [{ column: "stuck", label: "", cards: [task] }] })], inbox: [item] }, { tab: "board" });
  expect(container.querySelector("[data-factory-card] a")!.getAttribute("href")).toBe(task.issue_url);
  expect(container.querySelectorAll("button button").length).toBe(0);
  const other = [...container.querySelectorAll<HTMLButtonElement>("[data-factory-card] button")].find((button) => button.textContent === english["factory.card.otherAnswer"])!;
  await act(async () => other.click());
  expect(container.querySelector("[data-factory-item-open='true']")!.getAttribute("data-factory-item")).toBe("f1/T-1/q1");
  expect(useUiStore.getState().screen).toMatchObject({ place: { tab: "turn", task: null } });
});

it("opens a local issue through its catalog identity without opening the Factory task", async () => {
  const rest = { navigator: { focused_device_id: "remote", devices: [{ id: "remote", kind: "remote" }, { id: "local", kind: "local" }], workspaces: [
    { id: "remote-project", path: "/fixture", device_id: "remote", tasks: { tasks: [{ id: "L-7", key: "remote:/fixture#7", source: "local" }] } },
    { id: "project", path: "/fixture", device_id: "local", tasks: { tasks: [{ id: "L-7", key: "local:/fixture#7", source: "local" }] } },
  ] } } as unknown as SnapshotRest;
  useShellStore.setState({ rest });
  const task = card("T-1", "running", { issue: "L-7" });
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [factory({ columns: [{ column: "moving", label: "", cards: [task] }] })], inbox: [] }, { tab: "board" });
  useUiStore.setState({ overviewProjectId: "project" });
  const issue = [...container.querySelectorAll<HTMLButtonElement>("[data-factory-card] button")].find((button) => button.textContent === "L-7")!;
  await act(async () => issue.click());
  expect(useUiStore.getState().overviewLens.panel).toBe("local:/fixture#7");
  expect(useUiStore.getState().screen).toMatchObject({ kind: "main", deviceId: "local" });
  useShellStore.setState({ rest: null });
});

const NOTICE: InboxItem = {
  ...MERGE, group: "notice", kind: "notice", rank: 4, question: "q-9", text: "Which database? → postgres", suggestion: "", choices: ["ok"], result_code: "acknowledge", gates: [],
  notice: "ai_answered", refers_to: "q-1", decision_kind: "B", observer_reason: null, overridable: true,
};

function commands(events: Parameters<DispatchFn>[0][]): Record<string, unknown>[] {
  return events.flatMap((event) => {
    const sent = event as unknown as { kind: string; payload: { command: Record<string, unknown> } };
    return sent.kind === "factory_action" ? [sent.payload.command] : [];
  });
}

it("changes Factory AI's answer only through 다른 답, which answers the question the notice is about (D-19)", async () => {
  const { container, events } = await mount({ my_turn: 0, notices: 1, factories: [factory()], inbox: [NOTICE] });
  expect(container.querySelector("[data-factory-group='notice']")!.textContent).toContain(english["factory.turn.noticesHint"]);
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-override]")!.click());
  await act(async () => type(container.querySelector<HTMLInputElement>("[data-factory-override-text]")!, "sqlite"));
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-send]")!.click());
  expect(commands(events)).toEqual([{ verb: "answer", task: "f1/T-1", question: "q-1", choice: null, text: "sqlite", change: true }]);
});

it("acknowledges the notices of every Factory shown with one action (D-43)", async () => {
  const other = factory({ id: "f2", project: "/other", project_name: "other" });
  const { container, events } = await mount({ my_turn: 0, notices: 2, factories: [factory(), other], inbox: [NOTICE, { ...NOTICE, factory: "f2", question: "q-8" }] });
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-ack-all]")!.click());
  expect(commands(events)).toEqual([{ verb: "ack_notices", project: "/fixture" }, { verb: "ack_notices", project: "/other" }]);
});

it("says why a request Factory AI sorted is still the person's in the Factory's mode (D-14, D-21)", async () => {
  const question: InboxItem = { ...MERGE, group: "answer", kind: "blocking", question: "q-2", text: "빈 열에 무엇을?", suggestion: "없음", choices: ["안내"], result_code: "wake_worker", gates: [], decision_kind: "C", observer_reason: "작업자는 첫 안을 추천합니다." };
  const { container } = await mount({ my_turn: 1, notices: 0, factories: [factory()], inbox: [question] });
  expect(container.querySelector("[data-factory-decision-kind='C']")!.textContent).toBe(`${english["factory.decision.C"]} · ${english["factory.decision.mine.assist"]} 작업자는 첫 안을 추천합니다.`);
});

it("pauses the one Factory the header shows, and says it is paused with a way to resume (D-48)", async () => {
  const { container, events } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { factory: "f1" });
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-pause='pause']")!.click());
  expect(commands(events)).toEqual([{ verb: "pause_factory", project: "/fixture" }]);
  const paused = lastAction(events).payload.request_id;
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 0, notices: 0, factories: [factory({ paused: true })], inbox: [] }, actions: [{ request_id: paused, answer: { ok: true } }] } }));
  expect(container.querySelector("[data-factory-paused-chip]")).not.toBeNull();
  expect(container.querySelector("[data-factory-flow-cell='moving']")!.textContent).toContain(english["factory.flow.asleep"]);
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-pause='resume']")!.click());
  expect(commands(events).at(-1)).toEqual({ verb: "resume_factory", project: "/fixture" });
});

it("lists every Factory when all projects are shown, and pauses one from its row (B36)", async () => {
  const other = factory({ id: "f2", project: "/other", project_name: "other", paused: true, observer_mode: "autonomous" });
  const { container, events } = await mount({ my_turn: 0, notices: 0, factories: [factory(), other], inbox: [] }, { tab: "settings" });
  expect([...container.querySelectorAll("[data-factory-list-row]")].map((row) => row.getAttribute("data-factory-list-row"))).toEqual(["f1", "f2"]);
  expect(container.querySelector("[data-factory-list-row='f2'] [data-factory-list-paused]")!.getAttribute("data-factory-list-paused")).toBe("true");
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-list-pause='f2']")!.click());
  expect(commands(events).at(-1)).toEqual({ verb: "resume_factory", project: "/other" });
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-list-open='f2']")!.click());
  expect(useUiStore.getState().screen).toMatchObject({ place: { factory: "f2" } });
});

it("reads a Factory made before worker candidates as one of its default agent, and sends the whole list on a change (D-41, D-42)", async () => {
  const summary = { my_turn: 0, notices: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings", factory: "f1" });
  await answerConfig(summary, events);
  expect(container.querySelectorAll("[data-factory-worker-candidate]")).toHaveLength(1);
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-worker-add]")!.click());
  expect(lastAction(events).payload.command).toEqual({ verb: "config", project: "/fixture", set: [["workers", JSON.stringify([{ agent: "claude", description: "" }, { agent: "claude", description: "" }])]] });
});

it("changes who answers with one of three choices, and dims them while Hide AI is off, when a risk path is the person's again (B33, B10, B37)", async () => {
  const summary = { my_turn: 0, notices: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings", factory: "f1" });
  await answerConfig(summary, events);
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-mode='autonomous'] button")!.click());
  const write = lastAction(events);
  expect(write.payload.command).toEqual({ verb: "config", project: "/fixture", set: [["observer_mode", "autonomous"]] });
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: write.payload.request_id, answer: { ok: true, config: { ...CONFIG, observer_mode: "autonomous" }, machine: { max_workers: 5 } } }] } }));
  expect(container.querySelector("[data-factory-risk-note]")!.textContent).toBe(english["factory.settings.riskAi"]);
  await act(async () => useShellStore.setState((state) => ({ rest: { ...state.rest, status: { ...state.rest?.status, background_ai: { enabled: false, provider: null, chosen: false, providers: [], unavailable_reason: null } } } as never })));
  expect(container.querySelector("[data-factory-ai-off]")!.textContent).toBe(english["factory.settings.aiOff"]);
  expect(container.querySelector("[data-factory-risk-note]")!.textContent).toBe(english["factory.settings.riskMine"]);
});

it("lets a person answer a decision Factory AI made differently from the Task page, until the Task finishes (D-19)", async () => {
  const at = NOW - 60_000;
  const answered: TaskDetail = {
    ...detail("running", ["pause", "cancel"]),
    decisions: [{ text: "정렬은 web 쪽에서", by: "worker:T-1", at: at - 1 }, { text: "정렬 키는 updated_at", by: "observer", at, kind: "B", reason: null }],
    questions: [{ id: "q-1", origin: "worker", kind: { kind: "default" }, text: "정렬 키?", suggestion: "updated_at", default_action: null, deadline: null, asked_at: at - 5, choices: [], answer: { text: "updated_at", chose: null, relayed_by: "observer", at }, letter: null, routing: { kind: "B" } }],
  };
  const { container, events } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: answered } }));
  expect([...container.querySelectorAll("[data-factory-decision-by]")].map((by) => by.getAttribute("data-factory-decision-by"))).toEqual(["observer", "worker"]);
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-decision-override='q-1']")!.click());
  await act(async () => type(container.querySelector<HTMLInputElement>("[data-factory-override-text]")!, "last_activity"));
  await act(async () => container.querySelector("[data-factory-override-text]")!.closest("form")!.requestSubmit());
  expect(commands(events).at(-1)).toEqual({ verb: "answer", task: "f1/T-1", question: "q-1", choice: null, text: "last_activity", change: true });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: { ...answered, card: card("T-1", "done") } } }));
  expect(container.querySelector("[data-factory-decision-override]")).toBeNull();
});

it("says why a worker stopped with Factory AI's reading, and how the engine treated its rest (B23, D-22)", async () => {
  const stopped: TaskDetail = { ...detail("stopped", ["retry", "cancel"]), stop_code: "no_report", diagnosis: "테스트 실행을 기다리다 멈춤", woke_at: NOW - 240_000, diagnosed_at: NOW - 120_000, diagnosed_from: "screen" };
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: stopped } }));
  expect(container.querySelector("[data-factory-stop='no_report']")!.textContent).toBe(`${english["factory.stop.no_report"]} · ${english["factory.card.noReply"]}`);
  expect(container.querySelector("[data-factory-diagnosis]")!.textContent).toBe(`${english["factory.task.diagnosis"].replace("{{text}}", "테스트 실행을 기다리다 멈춤")} · ${english["factory.diagnosisSource.screen"]}`);
  expect(container.querySelector("[data-factory-rest]")!.textContent).toBe(`${english["factory.task.woke"]}${english["factory.task.noReply"]}${english["factory.task.diagnosed"]}`);
});

it("marks Factory AI's pick among the worker candidates before a worker starts (D-41)", async () => {
  const workers = [{ agent: "codex", model: "gpt-6.1-sol", effort: "high", description: "대부분의 Task" }, { agent: "claude", model: "opus", effort: "max", description: "큰 리팩터" }];
  const waiting: TaskDetail = { ...detail("waiting", ["priority", "cancel"]), ai_picked_worker: 2, ai_pick_reason: "큰 변경" };
  const { container } = await mount({ my_turn: 0, notices: 0, factories: [factory({ workers })], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: waiting } }));
  expect(container.querySelector("[data-factory-worker-pick]")!.getAttribute("data-factory-worker-pick")).toBe("2");
  expect(container.querySelector("[data-factory-picked]")!.textContent).toBe(english["factory.task.picked"].replace("{{description}}", "큰 리팩터 · 큰 변경"));
});
