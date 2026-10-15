import { localDeviceId, type AgentRow, type MissedWork, type PrState, type SessionRecord, type SessionWorkSummary, type SnapshotRest, type WorkOutput, type WorkRequest, type WorkSession, type WorkTask } from "./snapshot";

export const workKey = (session: WorkSession) => `${session.device_id}\0${session.agent}\0${session.session_id}`;
export function sessionOfAgent(agent: AgentRow, rest: SnapshotRest | null): WorkSession | null {
  return agent.session_id ? { device_id: agent.device_id ?? localDeviceId(rest), agent: agent.agent_kind, session_id: agent.session_id } : null;
}
export function recordTitle(record: Pick<SessionRecord, "title" | "manual_title" | "native_title" | "fallback_title">, summaries: boolean): string {
  return record.manual_title ?? (summaries ? record.title : record.native_title ?? record.fallback_title);
}
export function recordTree(record: SessionRecord): SessionRecord[] {
  return [record, ...record.children.flatMap(recordTree)];
}
export function taskState(task: WorkTask, outputs: readonly WorkOutput[]) {
  const prs = outputs.filter((output) => output.kind === "pr" && output.task_id === task.id);
  if (prs.some((output) => output.state === "ci_failed")) return "ci_failed";
  if (prs.some((output) => ["open", "review"].includes(output.state))) return "review";
  if (task.done) return "done";
  return task.end === "done" ? "unfinished" : task.end === "waiting" ? "working" : task.end;
}
const OUTPUT_ORDER: Record<string, number> = { ci_failed: 0, review: 1, open: 2, merged: 3, closed: 4 };
export function orderedOutputs(outputs: readonly WorkOutput[]): WorkOutput[] {
  return [...outputs].sort((a, b) => (OUTPUT_ORDER[a.state] ?? 5) - (OUTPUT_ORDER[b.state] ?? 5) || b.created_at_unix_ms - a.created_at_unix_ms);
}
/**
 * Every open missed item in the scope the Sessions panel shows, of a live
 * session as much as of an ended one: a session that works again keeps what it
 * missed in the group, and the newest comes first.
 */
export function openMissedWork(work: readonly SessionWorkSummary[], checkout: string | null): { owner: SessionWorkSummary; item: MissedWork }[] {
  return work
    .filter((row) => !checkout || row.cwd === checkout || row.cwd?.startsWith(`${checkout}/`))
    .flatMap((owner) => owner.missed.filter((item) => item.status === "open").map((item) => ({ owner, item })))
    .sort((a, b) => b.item.at_unix_ms - a.item.at_unix_ms);
}

/** The PR mark's state for a recorded output: the record keeps merged, closed, CI failed, and otherwise open for review. */
export function outputPrState(state: string): PrState {
  return state === "merged" ? "merged" : state === "closed" ? "closed" : state === "ci_failed" ? "failed" : "pending";
}

export type TaskState = ReturnType<typeof taskState>;
export type ChecklistTask = { task: WorkTask; state: TaskState; prs: WorkOutput[]; words: WorkRequest[]; missed: MissedWork[] };
/** One chapter's tasks; `goal` is null for tasks outside any chapter and wherever the record has a single goal. */
export type ChecklistGroup = { id: string | null; goal: string | null; at: number; tasks: ChecklistTask[] };
export type OwnedMissed = { owner: Pick<SessionRecord, "session" | "title">; missed: MissedWork; named: boolean };
export type Checklist = {
  groups: ChecklistGroup[];
  done: number;
  total: number;
  /** The operator's words that no task holds, in the order said. */
  otherWords: WorkRequest[];
  /** What the turns that were not tasks found, what no turn could be found for, and what descendants found (named). */
  otherMissed: OwnedMissed[];
  /** The newest request not yet judged while the session works; it shows whatever the words switch says. */
  now: WorkRequest | null;
};

/**
 * One record's assigned work as a checklist, first-asked first: each task with
 * its PRs, the words tied to it and the missed work its turn found (a turn
 * starts at its request's offset, which carries the task ids). `descendants`
 * are the records below this one, whose missed work lands at the end, named.
 */
export function checklist(record: SessionRecord, descendants: readonly SessionRecord[] = []): Checklist {
  const byAt = <T extends { at_unix_ms: number }>(rows: readonly T[]) => [...rows].sort((a, b) => a.at_unix_ms - b.at_unix_ms);
  const tasks = byAt(record.tasks);
  const taskIds = new Set(tasks.map((task) => task.id));
  const ownerOf = (offset: number) => record.requests.find((request) => request.offset === offset)?.task_ids.find((id) => taskIds.has(id));
  const missedBy = new Map<string, MissedWork[]>();
  const otherMissed: OwnedMissed[] = [];
  for (const missed of [...record.missed].sort((a, b) => a.at_unix_ms - b.at_unix_ms)) {
    const id = ownerOf(missed.turn_offset);
    if (id) missedBy.set(id, [...(missedBy.get(id) ?? []), missed]);
    else otherMissed.push({ owner: record, missed, named: false });
  }
  for (const below of descendants) for (const missed of below.missed) otherMissed.push({ owner: below, missed, named: true });
  const view = (task: WorkTask): ChecklistTask => ({
    task,
    state: taskState(task, record.outputs),
    prs: orderedOutputs(record.outputs.filter((output) => output.kind === "pr" && output.task_id === task.id)),
    words: byAt(record.requests.filter((request) => request.task_ids.includes(task.id))),
    missed: missedBy.get(task.id) ?? [],
  });
  const chapters = byAt(record.chapters);
  // A single goal needs no title; with several, a task outside them leads without one.
  const titled = chapters.length >= 2;
  const groups: ChecklistGroup[] = titled
    ? [{ id: null, goal: null, at: 0, tasks: tasks.filter((task) => !chapters.some((chapter) => chapter.id === task.chapter_id)).map(view) }, ...chapters.map((chapter) => ({ id: chapter.id, goal: chapter.goal, at: chapter.at_unix_ms, tasks: tasks.filter((task) => task.chapter_id === chapter.id).map(view) }))].filter((group) => group.tasks.length > 0)
    : tasks.length > 0 ? [{ id: null, goal: null, at: 0, tasks: tasks.map(view) }] : [];
  const latest = record.requests.reduce<WorkRequest | null>((latest, request) => (!latest || request.offset > latest.offset ? request : latest), null);
  const now = record.state === "working" && latest && !latest.judged ? latest : null;
  return {
    groups,
    done: tasks.filter((task) => task.done).length,
    total: tasks.length,
    otherWords: byAt(record.requests.filter((request) => request !== now && !request.task_ids.some((id) => taskIds.has(id)))),
    otherMissed,
    now,
  };
}
