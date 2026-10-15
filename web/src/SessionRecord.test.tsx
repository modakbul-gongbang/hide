// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createActions } from "./actions";
import { TooltipProvider } from "./components/ui/tooltip";
import { initializeInterfaceI18n } from "./i18n/instance";
import { LaneSessionRecord } from "./SessionRecord";
import type { MissedWork, SessionRecord as RecordValue, SnapshotRest, WorkOutput, WorkRequest, WorkSession, WorkTask } from "./snapshot";
import { useShellStore } from "./store";
import { useUiStore } from "./ui";

const savedShell = useShellStore.getState();
const savedUi = useUiStore.getState();
afterEach(() => {
  useShellStore.setState(savedShell, true);
  useUiStore.setState(savedUi, true);
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

const AT = 1_790_000_000_000;
const SESSION: WorkSession = { device_id: "local", agent: "claude", session_id: "root" };
const CHILD: WorkSession = { device_id: "local", agent: "codex", session_id: "child" };
const task = (id: string, name: string, at: number, done: boolean, end: WorkTask["end"] = "done"): WorkTask => ({ id, chapter_id: "c", name, at_unix_ms: at, last_at_unix_ms: at, end, done });
const request = (offset: number, text: string, task_ids: string[], author: WorkRequest["author"] = "operator", judged = true): WorkRequest => ({ offset, at_unix_ms: AT + offset, text, author, task_ids, chapter_id: "c", judged });
const missed = (id: string, text: string, turn_offset: number, status: MissedWork["status"] = "open"): MissedWork => ({ id, kind: "problem", text, status, at_unix_ms: AT, turn_offset, issue_repository: null, issue_number: null, creating: false, backlog_key: null });
const output = (number: number, state: string, task_id: string | null, source: WorkSession): WorkOutput => ({ kind: "pr", repository: "acme/app", number, title: `PR title ${number}`, url: `https://example.invalid/pull/${number}`, state, stale: false, last_read_at_unix_ms: AT, created_at_unix_ms: AT, task_id, source, source_name: source.session_id });
const child: RecordValue = { session: CHILD, title: "Child work", fallback_title: "Codex", native_title: null, manual_title: null, state: "idle", last_end: "done", cwd: null, path: null, started_at_unix_ms: AT, ended_at_unix_ms: null, chapters: [], tasks: [], requests: [], missed: [], outputs: [], children: [] };
const BASE: RecordValue = {
  session: SESSION, title: "Session goal", fallback_title: "Claude", native_title: null, manual_title: null, state: "idle", last_end: "unfinished", cwd: null, path: null, started_at_unix_ms: AT, ended_at_unix_ms: null,
  chapters: [{ id: "c", goal: "Only goal", at_unix_ms: AT }],
  tasks: [task("t1", "Write the record", AT + 1, true), task("t2", "Check the search", AT + 2, false, "unfinished")],
  requests: [request(1, "Please write the record", ["t1"]), request(2, "Check the search too", ["t2"]), request(3, "How is it going?", [])],
  missed: [missed("m-open", "Retry the failed draft", 2), missed("m-closed", "Spacing of the heading", 2, "resolved")],
  outputs: [output(1100, "review", "t2", SESSION), output(1093, "merged", null, CHILD)],
  children: [child],
};

async function mount(record: RecordValue, options: { summaries?: boolean; words?: boolean } = {}) {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  initializeInterfaceI18n("en");
  const rest = { status: { background_ai: { provider: "claude", providers: [], unavailable_reason: null, enabled: options.summaries !== false, chosen: true, agent_summary: options.summaries !== false } } } as unknown as SnapshotRest;
  useShellStore.setState({ rest, sessionRecord: { session: SESSION, loading: false, failure: null, record } });
  useUiStore.setState({ sessionRecordWords: options.words ?? false });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(<TooltipProvider><LaneSessionRecord actions={createActions(() => true)} target={SESSION} /></TooltipProvider>));
  return { container, unmount: async () => { await act(async () => root.unmount()); } };
}

const text = (element: Element | null | undefined) => element?.textContent ?? "";

it("draws the PRs first, then the checklist with its count, a task's PR on its line and no status words", async () => {
  const { container, unmount } = await mount(BASE);
  try {
    const outputs = Array.from(container.querySelectorAll('[data-record-outputs] [data-record-output="pr"]'));
    // Only the child's PR names its maker, at the row's end.
    expect(outputs.map(text)).toEqual(["#" + "1100PR title 1100", "#" + "1093PR title 1093Child work"]);
    expect(text(container.querySelector("[data-record-outputs] h3"))).toBe("PR 2");
    expect(text(container.querySelector("[data-record-work] h3"))).toBe("Assigned work 1/2");
    const tasks = Array.from(container.querySelectorAll("[data-record-task]"));
    expect(tasks.map((row) => row.getAttribute("data-record-task"))).toEqual(["t1", "t2"]);
    expect(tasks.map((row) => row.querySelector("[data-task-state]")?.getAttribute("aria-label"))).toEqual(["Done", "Review pending"]);
    expect(text(tasks[1]!.querySelector("[data-record-task-pr]"))).toBe("#" + "1100");
    // The state is an icon and its name, never a word on the line.
    expect(text(tasks[1]!)).not.toContain("Review pending");
    expect(container.querySelector("[data-record-verdict]")).toBeNull();
  } finally { await unmount(); }
});

it("keeps the operator's words behind the words button and puts a missed item under its task", async () => {
  const off = await mount(BASE);
  try {
    const button = off.container.querySelector<HTMLButtonElement>("[data-record-words]")!;
    expect(button.getAttribute("aria-label")).toBe("Show my words");
    expect(button.getAttribute("aria-pressed")).toBe("false");
    expect(off.container.querySelector("[data-record-words-bubble]")).toBeNull();
    const second = off.container.querySelector('[data-record-task="t2"]')!;
    expect(second.querySelector('[data-missed-work="m-open"][data-missed-status="open"]')).not.toBeNull();
    expect(second.querySelector('[data-missed-work="m-closed"]')?.textContent).toContain("Spacing of the heading · Handled");
    await act(async () => button.click());
    expect(useUiStore.getState().sessionRecordWords).toBe(true);
  } finally { await off.unmount(); }
  const on = await mount(BASE, { words: true });
  try {
    expect(on.container.querySelector("[data-record-words]")?.getAttribute("aria-pressed")).toBe("true");
    const bubbles = (scope: Element | null) => Array.from(scope?.querySelectorAll("[data-record-words-bubble]") ?? []).map(text);
    expect(bubbles(on.container.querySelector('[data-record-task="t1"]'))).toEqual(["Please write the record"]);
    expect(bubbles(on.container.querySelector("[data-record-other]"))).toEqual(["How is it going?"]);
    expect(text(on.container.querySelector("[data-record-other] h4"))).toBe("Other words");
  } finally { await on.unmount(); }
});

it("names the button for requests received when a parent made them, and draws no person mark for them", async () => {
  const delegated = { ...BASE, requests: BASE.requests.map((one) => ({ ...one, author: "parent" as const })) };
  const { container, unmount } = await mount(delegated, { words: true });
  try {
    expect(container.querySelector("[data-record-words]")?.getAttribute("aria-label")).toBe("Show requests received");
    expect(container.querySelector("[data-record-words-bubble] svg")).toBeNull();
  } finally { await unmount(); }
});

it("shows the newest unjudged request as Now whatever the words button says, and not once the session stopped", async () => {
  const unjudged = { ...BASE, requests: [...BASE.requests, request(4, "Install and measure again", [], "operator", false)] };
  const working = await mount({ ...unjudged, state: "working" });
  try {
    expect(text(working.container.querySelector("[data-record-now]"))).toBe("NowInstall and measure again");
  } finally { await working.unmount(); }
  const idle = await mount({ ...unjudged, state: "idle" });
  try {
    expect(idle.container.querySelector("[data-record-now]")).toBeNull();
  } finally { await idle.unmount(); }
});

it("draws no PR head or list for a session with no outputs", async () => {
  const { container, unmount } = await mount({ ...BASE, outputs: [] });
  try {
    expect(container.querySelector("[data-record-outputs]")).toBeNull();
  } finally { await unmount(); }
});

it("with Hide AI off shows the PRs and the requests oldest first, the newest five open, and no checklist", async () => {
  const requests = Array.from({ length: 7 }, (_, index) => request(index + 1, `request ${index + 1}`, []));
  const { container, unmount } = await mount({ ...BASE, requests }, { summaries: false });
  try {
    expect(container.querySelector("[data-record-words]")).toBeNull();
    expect(container.querySelector("[data-record-task]")).toBeNull();
    expect(container.querySelector("[data-missed-work]")).toBeNull();
    expect(Array.from(container.querySelectorAll("[data-record-request]")).map((row) => text(row).replace(/^\d{2}:\d{2}/, ""))).toEqual(["request 3", "request 4", "request 5", "request 6", "request 7"]);
    const earlier = Array.from(container.querySelectorAll("button")).find((button) => text(button) === "Earlier requests 2")!;
    await act(async () => earlier.click());
    expect(container.querySelectorAll("[data-record-request]")).toHaveLength(7);
  } finally { await unmount(); }
});

it("draws a descendant's missed item once, named, at the end of the top record, where the child is a row to open rather than a second checklist", async () => {
  const withMissed = { ...BASE, children: [{ ...child, tasks: [task("ct", "Child task", AT, true)], missed: [missed("m-child", "Child found this", 99)] }] };
  const { container, unmount } = await mount(withMissed);
  try {
    expect(container.querySelector('[data-record-child="child"]')?.tagName).toBe("BUTTON");
    const items = container.querySelectorAll('[data-missed-work="m-child"]');
    expect(items).toHaveLength(1);
    expect(items[0]!.closest("[data-record-other]")?.textContent).toContain("Child found this · Child work");
  } finally { await unmount(); }
});

it("opens a child's record in place under `← <parent>`, and goes back one level per press (PRD overview-device-tasks B20)", async () => {
  const { container, unmount } = await mount({ ...BASE, children: [child] });
  try {
    await act(async () => container.querySelector<HTMLElement>('[data-record-child="child"]')!.click());
    expect(text(container.querySelector("h2"))).toContain("Child work");
    const up = container.querySelector<HTMLElement>("[data-record-up]")!;
    expect(text(up)).toContain(BASE.title);
    await act(async () => up.click());
    expect(container.querySelector("[data-record-up]")).toBeNull();
    expect(container.querySelector("[data-record-back]")).not.toBeNull();
    expect(text(container.querySelector("h2"))).toContain(BASE.title);
  } finally { await unmount(); }
});

it("gives a task's PR button the last read time while GitHub cannot be read", async () => {
  const stale = { ...BASE, outputs: [{ ...output(1100, "review", "t2", SESSION), stale: true, last_read_at_unix_ms: AT }] };
  const { container, unmount } = await mount(stale);
  try {
    const label = container.querySelector("[data-record-task-pr]")!.getAttribute("aria-label")!;
    expect(label).toContain(new Date(AT).toLocaleString());
  } finally { await unmount(); }
});

it("says so when the only requests were not tasks and the words button is off, and gives a task-less child no count", async () => {
  const noTasks = { ...BASE, tasks: [], missed: [], outputs: [], requests: [request(1, "Just a question", [])], children: [child] };
  const { container, unmount } = await mount(noTasks);
  try {
    expect(text(container.querySelector("[data-record-work]"))).toContain("No recorded requests yet");
    expect(text(container.querySelector("[data-record-child]"))).not.toContain("0/0");
  } finally { await unmount(); }
});

it("ends an idle record with the last thing its agent said, and a working one with its Now line instead (B17)", async () => {
  const { container, unmount } = await mount(BASE);
  try {
    await act(async () => useShellStore.setState({ agents: [{ pane_id: "w1:p1", session_id: "root", message: "Fix the header\nRan the tests\nAll green, ready for review" } as never] }));
    expect(text(container.querySelector("[data-record-last-words]"))).toContain("All green, ready for review");
    await act(async () => useShellStore.setState({ sessionRecord: { session: SESSION, loading: false, failure: null, record: { ...BASE, state: "working", requests: [...BASE.requests, request(99, "Keep going", [], "operator", false)] } } }));
    expect(container.querySelector("[data-record-now]")).not.toBeNull();
    expect(container.querySelector("[data-record-last-words]")).toBeNull();
  } finally { await unmount(); }
});
