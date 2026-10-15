import { expect, it } from "vitest";
import type { MissedWork, SessionRecord, SessionWorkSummary, WorkRequest, WorkTask } from "./snapshot";
import { checklist, openMissedWork, outputPrState, recordTitle } from "./sessionWork";

const missed = (id: string, at: number, status: MissedWork["status"] = "open"): MissedWork => ({ id, kind: "problem", text: id, status, at_unix_ms: at, turn_offset: 1, issue_repository: null, issue_number: null, creating: false, backlog_key: null });
const work = (id: string, cwd: string, items: MissedWork[]): SessionWorkSummary => ({
  session: { device_id: "local", agent: "claude", session_id: id }, title: "Current goal", fallback_title: "Claude", native_title: null, manual_title: null,
  cwd, ended_at_unix_ms: null, open_missed: items.filter((item) => item.status === "open").length, subtree_open_missed: 0, missed: items,
});

it("collects every open missed item in scope, of live and ended sessions alike, newest first", () => {
  const rows = [work("live", "/repo", [missed("a", 1), missed("handled", 9, "resolved")]), work("ended", "/repo/sub", [missed("b", 3)]), work("elsewhere", "/other", [missed("c", 5)])];
  expect(openMissedWork(rows, null).map(({ item }) => item.id)).toEqual(["c", "b", "a"]);
  expect(openMissedWork(rows, "/repo").map(({ item }) => item.id)).toEqual(["b", "a"]);
  expect(openMissedWork([work("none", "/repo", [missed("done", 1, "dismissed")])], null)).toEqual([]);
});

it("titles a record by its manual name, else the model's goal or the native name", () => {
  const row = work("s", "/repo", []);
  expect(recordTitle(row, true)).toBe("Current goal");
  expect(recordTitle(row, false)).toBe("Claude");
  expect(recordTitle({ ...row, manual_title: "Mine" }, true)).toBe("Mine");
});

const SESSION = { device_id: "local", agent: "claude", session_id: "root" };
const task = (id: string, chapter: string, at: number, done = false, end: WorkTask["end"] = "done"): WorkTask => ({ id, chapter_id: chapter, name: `task ${id}`, at_unix_ms: at, last_at_unix_ms: at, end, done });
const request = (offset: number, at: number, task_ids: string[], judged = true): WorkRequest => ({ offset, at_unix_ms: at, text: `request ${offset}`, author: "operator", task_ids, chapter_id: null, judged });
const record = (patch: Partial<SessionRecord>): SessionRecord => ({
  session: SESSION, title: "Goal", fallback_title: "Claude", native_title: null, manual_title: null, state: "idle", last_end: null, cwd: null, path: null, started_at_unix_ms: 0, ended_at_unix_ms: null,
  chapters: [], tasks: [], requests: [], missed: [], outputs: [], children: [], ...patch,
});

it("lists tasks first-asked first and counts the finished ones", () => {
  const list = checklist(record({ tasks: [task("late", "c", 30, false, "unfinished"), task("early", "c", 10, true)], chapters: [{ id: "c", goal: "Only goal", at_unix_ms: 0 }] }));
  expect(list.groups.map((group) => group.goal)).toEqual([null]);
  expect(list.groups[0]!.tasks.map((item) => item.task.id)).toEqual(["early", "late"]);
  expect([list.done, list.total]).toEqual([1, 2]);
});

it("titles chapters only when there are several goals, tasks outside them leading without a title", () => {
  const list = checklist(record({
    chapters: [{ id: "b", goal: "Second", at_unix_ms: 20 }, { id: "a", goal: "First", at_unix_ms: 10 }],
    tasks: [task("t-b", "b", 25), task("t-a", "a", 15), task("t-out", "gone", 5)],
  }));
  expect(list.groups.map((group) => [group.goal, group.tasks.map((item) => item.task.id)])).toEqual([[null, ["t-out"]], ["First", ["t-a"]], ["Second", ["t-b"]]]);
});

it("puts a missed item under the task its turn's request belongs to, and the rest at the end", () => {
  const own = (id: string, turn_offset: number): MissedWork => ({ ...missed(id, 1), turn_offset });
  const child = record({ session: { ...SESSION, session_id: "child" }, title: "Child", missed: [missed("from-child", 2)] });
  const list = checklist(record({
    tasks: [task("t1", "c", 10), task("t2", "c", 20)],
    requests: [request(1, 10, ["t1"]), request(2, 20, ["t2"]), request(3, 30, [])],
    missed: [own("under-t2", 2), own("no-task-turn", 3), own("no-turn", 99)],
  }), [child]);
  expect(list.groups[0]!.tasks.map((item) => item.missed.map((one) => one.id))).toEqual([[], ["under-t2"]]);
  expect(list.otherMissed.map(({ missed: one, named }) => [one.id, named])).toEqual([["no-task-turn", false], ["no-turn", false], ["from-child", true]]);
});

it("ties words to their tasks, keeps the rest as other words and shows the newest unjudged request as now only while working", () => {
  const requests = [request(1, 10, ["t1"]), request(2, 20, []), request(3, 30, [], false)];
  const base = { tasks: [task("t1", "c", 10)], requests };
  const working = checklist(record({ ...base, state: "working" }));
  expect(working.groups[0]!.tasks[0]!.words.map((one) => one.offset)).toEqual([1]);
  expect(working.otherWords.map((one) => one.offset)).toEqual([2]);
  expect(working.now?.offset).toBe(3);
  const idle = checklist(record({ ...base, state: "idle" }));
  expect(idle.now).toBeNull();
  expect(idle.otherWords.map((one) => one.offset)).toEqual([2, 3]);
});

it("maps a recorded PR state to the PR mark's state", () => {
  expect(["ci_failed", "review", "merged", "closed"].map(outputPrState)).toEqual(["failed", "pending", "merged", "closed"]);
});
