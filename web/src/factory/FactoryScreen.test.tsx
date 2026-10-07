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
import { FACTORY_ENTRY, useUiStore, type FactoryPlace } from "../ui";
import type { DispatchFn } from "../ws";
import { InitFailure } from "./CreateSheet";
import { FactoryScreen } from "./FactoryScreen";
import { REQUEST_ANSWER_TIMEOUT_MS } from "./request";
import type { CardView, FactorySummary, FactoryView, InboxItem, TaskDetail, TaskState } from "./model";

const NOW = Date.now();

function card(task: string, state: TaskState, patch: Partial<CardView> = {}): CardView {
  return { task, display_id: task, column: null, title: `${task} 제목`, state, state_label: state, needs_person: false, waiting_for: null, waiting_code: null, waiting_on: [], env_hold: null, stop: null, priority: 0, since: NOW, unread: false, folded: false, archived: false, failures: 0, external: [], revive_until: null, worker_pane: null, ...patch };
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
  text: "병합할까요?", suggestion: "merge", result: "", default_action: null, choices: ["merge", "request-changes", "cancel"], deadline: null, remaining: null, remaining_hours: null, waiting_since: NOW, waiting_days: 0,
  result_code: "merge", unblocks: [], gates: ["manual_mode"], stop: null,
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

it("keeps a refused answer's item in place with its next action in the screen's language (B10, B24)", async () => {
  const { container, events } = await mount({ my_turn: 1, factories: [factory()], inbox: [MERGE] });
  const send = container.querySelector<HTMLButtonElement>("[data-factory-send]")!;
  await act(async () => send.click());
  const sent = events.at(-1) as unknown as { kind: string; payload: { request_id: string; command: { verb: string; task: string } } };
  expect(sent.kind).toBe("factory_action");
  expect(sent.payload.command).toEqual({ verb: "merge", task: "f1/T-1" });
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 1, factories: [factory()], inbox: [MERGE] }, actions: [{ request_id: sent.payload.request_id, answer: { ok: false, reason: "main_dirty", next_action: "Commit or stash the changes in the main checkout, then merge" } }] } }));
  const refused = container.querySelector("[data-factory-refused]")!;
  expect(refused.getAttribute("data-factory-refused")).toBe("main_dirty");
  expect(refused.textContent).toBe(english["factory.refusal.main_dirty"]);
  expect(container.querySelectorAll("[data-factory-item]")).toHaveLength(1);
});

function detail(state: TaskState, allowed: string[]): TaskDetail {
  return {
    card: card("T-1", state), factory: "f1", project: "/fixture", goal: "목표", criteria: [], out_of_scope: [], before: [], after: [], attachments: [], pr: null,
    verification: "1/3", attempts: [], decisions: [], questions: [], discoveries: [], gates: [], gate_codes: [], allowed, stop: null, stop_code: null, merge_sha: null, worker_name: null, worktree: null, branch: null,
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

const CONFIG = {
  verification: { kind: "ci", checks: ["ci"] }, merge_mode: "manual", merge_method: "merge", quick_check: null, question_deadline_ms: 86_400_000, stall_ms: 1_800_000,
  no_report_ms: 3_600_000, watch_interval_ms: 1_800_000, watch_daily_limit: 4, outside_read_ms: 120_000, cancel_keep_ms: 604_800_000, done_fold_ms: 259_200_000,
  archive_fold_ms: 7_776_000_000, new_task_limit: 10, verify_failure_limit: 3, verify_timeout_ms: 3_600_000, disk_floor_bytes: 10_737_418_240, default_runtime: "claude",
  harness: null, autonomy: [], autonomy_diff_limit: 400, recovery: [], risk_paths: [], checks: [], prd_in_issue: false, macos_notifications: false, worker_args: {},
};

/** Answers the settings tab's config read the way the engine does. */
async function answerConfig(summary: FactorySummary, events: Parameters<DispatchFn>[0][]) {
  const read = events.find((event) => (event as unknown as { kind: string; payload: { command: { verb: string } } }).payload?.command?.verb === "config") as unknown as { payload: { request_id: string } };
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: read.payload.request_id, answer: { ok: true, config: CONFIG, machine: { max_workers: 5 } } }] } }));
}

it("keeps Close disabled while the Running column holds a Task in any of its states, as the engine refuses then (B22)", async () => {
  const waiting = factory({ columns: [{ column: "running", label: "running", cards: [card("T-1", "merge_waiting", { column: "running" })] }] });
  const summary = { my_turn: 0, factories: [waiting], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings" });
  await answerConfig(summary, events);
  expect(container.querySelector<HTMLButtonElement>("[data-factory-close]")!.disabled).toBe(true);
  const done = { my_turn: 0, factories: [factory({ columns: [{ column: "done", label: "done", cards: [card("T-1", "done", { column: "done" })] }] })], inbox: [] };
  await answerConfig(done, events);
  expect(container.querySelector<HTMLButtonElement>("[data-factory-close]")!.disabled).toBe(false);
});

it("takes an answer that comes after the wait ran out, since the engine may still finish the work (B10)", async () => {
  vi.useFakeTimers();
  try {
    const { container, events } = await mount({ my_turn: 1, factories: [factory()], inbox: [MERGE] });
    await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-send]")!.click());
    const sent = events.at(-1) as unknown as { payload: { request_id: string } };
    await act(async () => vi.advanceTimersByTime(REQUEST_ANSWER_TIMEOUT_MS + 1));
    expect(container.querySelector("[data-factory-refused]")!.getAttribute("data-factory-refused")).toBe("no_answer");
    await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 1, factories: [factory()], inbox: [MERGE] }, actions: [{ request_id: sent.payload.request_id, answer: { ok: true } }] } }));
    expect(container.querySelector("[data-factory-refused]")).toBeNull();
    expect(container.querySelector("[data-factory-send]")!.getAttribute("data-factory-send")).toBe("taken");
  } finally {
    vi.useRealTimers();
  }
});

it("says what acknowledging a notice does, though a notice has no suggestion (B9)", async () => {
  const notice: InboxItem = { ...MERGE, group: "notice", kind: "notice", question: "q-9", text: "무관한 발견", suggestion: "", choices: ["ok"], result_code: "acknowledge", gates: [] };
  const { container } = await mount({ my_turn: 1, factories: [factory()], inbox: [notice] });
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
  const { container, events } = await mount({ my_turn: 1, factories: [factory()], inbox: [MERGE] });
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
  const summary = { my_turn: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings" });
  await answerConfig(summary, events);
  const field = container.querySelector<HTMLInputElement>("[data-factory-setting='new_task_limit']")!;
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
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='new_task_limit']")!.value).toBe(String(CONFIG.new_task_limit));
});

it("treats a Factory whose cards are all archived as empty, not as a filter that matches nothing (B15)", async () => {
  const archived = factory({ columns: [{ column: "done", label: "done", cards: [card("T-1", "done", { column: "done", archived: true, folded: true })] }] });
  const { container } = await mount({ my_turn: 0, factories: [archived], inbox: [] }, { tab: "board" });
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

it("keeps a check's instruction until the engine takes it, then reads the config again, since a check answers with no config (B22)", async () => {
  const summary = { my_turn: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings" });
  await answerConfig(summary, events);
  const field = container.querySelector<HTMLInputElement>("[data-factory-setting='check_instruction']")!;
  await act(async () => type(field, "README가 최신인지"));
  const add = [...container.querySelectorAll("button")].find((button) => button.textContent === english["factory.settings.add"])!;
  await act(async () => add.click());
  const check = lastAction(events);
  expect(check.payload.command).toMatchObject({ verb: "check", instruction: "README가 최신인지" });
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: check.payload.request_id, answer: { ok: false, reason: "instruction_required", next_action: "Give --instruction" } }] } }));
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='check_instruction']")!.value).toBe("README가 최신인지");
  await act(async () => add.click());
  const again = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: again.payload.request_id, answer: { ok: true, message: "check added" } }] } }));
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='check_instruction']")!.value).toBe("");
  expect(lastAction(events).payload.command).toEqual({ verb: "config", project: "/fixture", set: [] });
  expect(container.querySelector("[data-factory-close]")).not.toBeNull();
});

it("puts an emptied number back to the saved value instead of sending 0 (B22)", async () => {
  const summary = { my_turn: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings" });
  await answerConfig(summary, events);
  const field = container.querySelector<HTMLInputElement>("[data-factory-setting='question_deadline_hours']")!;
  await act(async () => type(field, ""));
  await act(async () => field.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
  expect(events.some((event) => ((event as unknown as { payload: { command?: { set?: unknown[] } } }).payload?.command?.set?.length ?? 0) > 0)).toBe(false);
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='question_deadline_hours']")!.value).toBe("24");
});

it("keeps a change request's comment while the engine refuses it, and closes the form once taken (B19)", async () => {
  const { container, events } = await mount({ my_turn: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: detail("merge_waiting", ["merge", "request-changes", "cancel"]) } }));
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-action='request-changes']")!.click());
  await act(async () => type(container.querySelector<HTMLInputElement>("[data-factory-comment]")!, "테스트를 더 써 주세요"));
  await act(async () => container.querySelector("[data-factory-comment]")!.closest("form")!.requestSubmit());
  const sent = lastAction(events);
  expect(sent.payload.command).toMatchObject({ verb: "request_changes", comment: "테스트를 더 써 주세요" });
  const summary = { my_turn: 0, factories: [factory()], inbox: [] };
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: sent.payload.request_id, answer: { ok: false, reason: "factory_busy", next_action: "Try again in a moment" } }] } }));
  expect(container.querySelector<HTMLInputElement>("[data-factory-comment]")!.value).toBe("테스트를 더 써 주세요");
  await act(async () => container.querySelector("[data-factory-comment]")!.closest("form")!.requestSubmit());
  const again = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: again.payload.request_id, answer: { ok: true } }] } }));
  expect(container.querySelector("[data-factory-comment]")).toBeNull();
});

it("says a Task is gone when its Factory has left the summary, instead of loading forever", async () => {
  const { container } = await mount({ my_turn: 0, factories: [factory()], inbox: [] }, { task: { factory: "gone", task: "T-1" } });
  expect(container.querySelector("[data-factory-task-missing]")).not.toBeNull();
  expect(container.querySelector("[data-factory-task-loading]")).toBeNull();
});

it("does not send a taken answer again while its item is still on screen (B10)", async () => {
  const { container, events } = await mount({ my_turn: 1, factories: [factory()], inbox: [MERGE] });
  const send = container.querySelector<HTMLButtonElement>("[data-factory-send]")!;
  await act(async () => send.click());
  const sent = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 1, factories: [factory()], inbox: [MERGE] }, actions: [{ request_id: sent.payload.request_id, answer: { ok: true } }] } }));
  expect(container.querySelector<HTMLButtonElement>("[data-factory-send]")!.disabled).toBe(true);
  await act(async () => press(container.querySelector("[data-factory-choice='1']")!, { key: "Enter" }));
  expect(sentVerbs(events)).toEqual(["merge"]);
});

it("holds Add while a check is on its way, and keeps a refused check's text through a later write of another setting (B22)", async () => {
  const summary = { my_turn: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings" });
  await answerConfig(summary, events);
  await act(async () => type(container.querySelector<HTMLInputElement>("[data-factory-setting='check_instruction']")!, "README"));
  const add = [...container.querySelectorAll("button")].find((button) => button.textContent === english["factory.settings.add"])!;
  await act(async () => add.click());
  expect(add.disabled).toBe(true);
  const check = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: check.payload.request_id, answer: { ok: false, reason: "factory_busy", next_action: "Try again in a moment" } }] } }));
  const field = container.querySelector<HTMLInputElement>("[data-factory-setting='new_task_limit']")!;
  await act(async () => type(field, "7"));
  await act(async () => press(field, { key: "Enter" }));
  const write = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary, actions: [{ request_id: write.payload.request_id, answer: { ok: true, config: { ...CONFIG, new_task_limit: 7 }, machine: { max_workers: 5 } } }] } }));
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='check_instruction']")!.value).toBe("README");
});

it("puts a number the field cannot take back to the saved value instead of leaving it as if saved (B22)", async () => {
  const summary = { my_turn: 0, factories: [factory()], inbox: [] };
  const { container, events } = await mount(summary, { tab: "settings" });
  await answerConfig(summary, events);
  const field = container.querySelector<HTMLInputElement>("[data-factory-setting='new_task_limit']")!;
  await act(async () => type(field, "2.5"));
  await act(async () => field.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
  expect(container.querySelector<HTMLInputElement>("[data-factory-setting='new_task_limit']")!.value).toBe(String(CONFIG.new_task_limit));
});

it("leaves an open priority form alone when another action on the page is taken (B19)", async () => {
  const { container, events } = await mount({ my_turn: 0, factories: [factory()], inbox: [] }, { task: { factory: "f1", task: "T-1" } });
  await act(async () => useShellStore.setState({ factoryTask: { factory: "f1", task: "T-1", detail: detail("waiting", ["priority", "cancel"]) } }));
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-action='priority']")!.click());
  await act(async () => container.querySelector<HTMLButtonElement>("[data-factory-action='cancel']")!.click());
  const cancel = lastAction(events);
  await act(async () => useShellStore.setState({ factory: { summary: { my_turn: 0, factories: [factory()], inbox: [] }, actions: [{ request_id: cancel.payload.request_id, answer: { ok: true } }] } }));
  expect(container.querySelector("[data-factory-priority]")).not.toBeNull();
});
